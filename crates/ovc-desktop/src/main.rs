//! Native Slint shell for the existing OVC web application.

#![cfg_attr(target_os = "windows", windows_subsystem = "windows")]

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc;
#[cfg(target_os = "linux")]
use std::time::Duration;

use anyhow::{Context, Result};
use slint::ComponentHandle;
use slint::winit_030::WinitWindowAccessor;
use wry::dpi::{PhysicalPosition, PhysicalSize};
use wry::{Rect, WebView, WebViewBuilder};

slint::include_modules!();

fn main() -> Result<()> {
    slint::BackendSelector::new()
        .backend_name("winit".into())
        .select()
        .context("failed to select Slint's winit backend")?;
    let _platform_event_loop = start_platform_webview_loop()?;

    // The desktop shell owns a private API instance, so create its session in
    // process rather than presenting the server-admin login meant for remote
    // deployments. Repository passwords remain independent and are never
    // injected into the web UI.
    let jwt_secret = desktop_jwt_secret();
    let (desktop_token, _) =
        ovc_api::auth::create_jwt(&jwt_secret, ovc_api::state::AppState::INITIAL_TOKEN_VERSION)
            .map_err(|error| anyhow::anyhow!("failed to create desktop session: {error:?}"))?;
    let (server_tx, server_rx) = mpsc::sync_channel(1);
    std::thread::Builder::new()
        .name("ovc-local-server".to_owned())
        .spawn(move || run_local_server(&server_tx, jwt_secret))
        .context("failed to start local server thread")?;
    let url = server_rx
        .recv()
        .context("local server stopped during startup")??;

    let app = AppWindow::new().context("failed to create the Slint window")?;
    // Materialize the platform window before asking for its raw handle. The
    // handle becomes available on the first event-loop turn after `show()`.
    app.show().context("failed to show the Slint window")?;
    let app_weak = app.as_weak();
    let webview: Rc<RefCell<Option<WebView>>> = Rc::new(RefCell::new(None));
    let webview_for_startup = Rc::clone(&webview);
    let webview_for_resize = Rc::clone(&webview);

    app.window().on_winit_window_event(move |_window, event| {
        if let slint::winit_030::winit::event::WindowEvent::Resized(size) = event
            && let Some(view) = webview_for_resize.borrow().as_ref()
        {
            let _ = view.set_bounds(bounds_for_size(*size));
        }
        slint::winit_030::EventResult::Propagate
    });

    // Slint creates the native winit window asynchronously after `show()`. Its
    // readiness future eliminates the race that a zero-delay timer still has
    // on macOS and some Linux window managers.
    slint::spawn_local(async move {
        let Some(app) = app_weak.upgrade() else {
            return;
        };
        app.set_status_message("Loading interface…".into());

        let result = async {
            let parent = app
                .window()
                .winit_window()
                .await
                .context("Slint's native winit window is not available")?;
            // winit initializes NSApplication while creating the first native
            // window, so set the development/Dock icon only after that point.
            install_app_icon()?;
            create_webview(parent.as_ref(), &url, &desktop_token)
        }
        .await;

        match result {
            Ok(view) => *webview_for_startup.borrow_mut() = Some(view),
            Err(error) => show_error(&app, &error),
        }
    })
    .context("failed to schedule webview startup")?;

    app.run().context("desktop event loop failed")?;
    drop(webview);
    Ok(())
}

#[cfg(target_os = "macos")]
fn install_app_icon() -> Result<()> {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let main_thread = MainThreadMarker::new().context("OVC must start on the macOS main thread")?;
    let icon_data = NSData::with_bytes(include_bytes!("../icons/1024x1024.png"));
    let icon = NSImage::initWithData(NSImage::alloc(), &icon_data)
        .context("failed to decode the OVC application icon")?;
    let application = NSApplication::sharedApplication(main_thread);

    // SAFETY: AppKit documents a non-null NSImage as valid for the lifetime of
    // the application. The setter retains `icon`; only passing `None` has the
    // undocumented behavior that makes objc2 mark this method unsafe.
    unsafe { application.setApplicationIconImage(Some(&icon)) };
    Ok(())
}

#[cfg(not(target_os = "macos"))]
fn install_app_icon() -> Result<()> {
    Ok(())
}

fn run_local_server(sender: &mpsc::SyncSender<Result<String>>, jwt_secret: String) {
    let result = (|| -> Result<()> {
        let runtime = tokio::runtime::Runtime::new().context("failed to create API runtime")?;
        runtime.block_on(async {
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .context("failed to bind the local API server")?;
            let port = listener.local_addr()?.port();
            sender
                .send(Ok(format!("http://127.0.0.1:{port}")))
                .map_err(|_| anyhow::anyhow!("desktop window closed during startup"))?;

            ovc_api::start_server_on_listener(server_config(port, jwt_secret), listener)
                .await
                .map_err(|error| anyhow::anyhow!(error))
        })
    })();

    if let Err(error) = result {
        let _ = sender.send(Err(error));
    }
}

fn server_config(port: u16, jwt_secret: String) -> ovc_api::ServerConfig {
    let repos_dir = std::env::var_os("OVC_REPOS_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| dirs::data_dir().map(|dir| dir.join("ovc").join("repos")))
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    ovc_api::ServerConfig {
        bind: "127.0.0.1".to_owned(),
        port,
        repos_dir,
        jwt_secret: Some(jwt_secret),
        cors_origins: Vec::new(),
        workdir_map: Vec::new(),
        workdir_scan: Vec::new(),
        llm_base_url: std::env::var("OVC_LLM_BASE_URL").ok(),
        llm_model: std::env::var("OVC_LLM_MODEL").ok(),
        llm_api_key: std::env::var("OVC_LLM_API_KEY").ok(),
        llm_enabled: env_flag("OVC_LLM_ENABLED"),
        llm_max_tokens: env_parse("OVC_LLM_MAX_TOKENS", 32_768),
        llm_timeout_secs: env_parse("OVC_LLM_TIMEOUT", 120),
    }
}

fn create_webview(
    parent: &slint::winit_030::winit::window::Window,
    url: &str,
    desktop_token: &str,
) -> Result<WebView> {
    let physical_size = parent.inner_size();
    let allowed_origin = url.to_owned();
    let token_literal = serde_json::to_string(desktop_token)
        .context("failed to encode the desktop session token")?;
    let builder = WebViewBuilder::new()
        .with_initialization_script(format!(
            "localStorage.setItem('ovc_token', {token_literal});"
        ))
        .with_url(url)
        .with_bounds(bounds_for_size(physical_size))
        .with_background_color((10, 14, 26, 255))
        .with_devtools(cfg!(debug_assertions))
        .with_navigation_handler(move |target| target.starts_with(&allowed_origin));

    builder
        .build_as_child(parent)
        .context("failed to attach the system webview")
}

fn desktop_jwt_secret() -> String {
    let mut secret = [0_u8; 32];
    getrandom::fill(&mut secret).expect("operating system random source unavailable");
    hex::encode(secret)
}

fn bounds_for_size(size: PhysicalSize<u32>) -> Rect {
    Rect {
        position: PhysicalPosition::new(0, 0).into(),
        size: size.into(),
    }
}

#[cfg(target_os = "linux")]
fn start_platform_webview_loop() -> Result<Option<slint::Timer>> {
    gtk::init().context("failed to initialize WebKitGTK")?;
    let timer = slint::Timer::default();
    timer.start(
        slint::TimerMode::Repeated,
        Duration::from_millis(16),
        || {
            while gtk::events_pending() {
                gtk::main_iteration_do(false);
            }
        },
    );
    Ok(Some(timer))
}

#[cfg(not(target_os = "linux"))]
fn start_platform_webview_loop() -> Result<Option<slint::Timer>> {
    Ok(None)
}

fn show_error(app: &AppWindow, error: &anyhow::Error) {
    app.set_failed(true);
    app.set_status_message(format!("{error:#}").into());
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "yes"))
}

fn env_parse<T>(name: &str, default: T) -> T
where
    T: std::str::FromStr,
{
    std::env::var(name)
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(default)
}
