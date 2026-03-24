//! `ovc web` — Open the OVC web UI in the default browser.
//!
//! Aliases: `ovc ui`, `ovc gui`.

use anyhow::{Context, Result};
use console::Style;

use crate::app::WebArgs;

pub fn execute(args: &WebArgs) -> Result<()> {
    let port = args.port;
    let url = format!("http://127.0.0.1:{port}");

    // Check if the server is already running by hitting the health endpoint.
    let health_url = format!("{url}/api/v1/health");
    let server_running = std::process::Command::new("curl")
        .args(["-s", "-o", "/dev/null", "-w", "%{http_code}", &health_url])
        .output()
        .ok()
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .is_some_and(|code| code.trim() == "200");

    let cyan = Style::new().cyan().bold();
    let dim = Style::new().dim();

    println!();
    if server_running {
        println!("  {} OVC Web UI", cyan.apply_to("Opening"));
        println!();
        println!("  {}  {}", cyan.apply_to("URL:"), cyan.apply_to(&url));
        println!("  {}  port {port}", dim.apply_to("Server running on"));
        println!();
        open_browser(&url).context("failed to open browser")?;
    } else {
        println!(
            "  {} No OVC server detected on port {port}.",
            dim.apply_to("!")
        );
        println!();
        println!("  Start the server first:");
        println!("    {} ovc serve --port {port}", dim.apply_to("$"));
        println!();
        println!("  Or install the daemon:");
        println!("    {} ovc daemon install --port {port}", dim.apply_to("$"));
        println!();
        println!("  Then run:");
        println!("    {} ovc web", dim.apply_to("$"));
    }
    println!();

    Ok(())
}

/// Open a URL in the default browser.
fn open_browser(url: &str) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(url)
            .spawn()
            .context("failed to run 'open'")?;
    }

    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(url)
            .spawn()
            .context("failed to run 'xdg-open'")?;
    }

    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/c", "start", url])
            .spawn()
            .context("failed to run 'start'")?;
    }

    Ok(())
}
