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
    let server_running = {
        let rt = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok();
        rt.and_then(|rt| {
            rt.block_on(async {
                reqwest::Client::builder()
                    .timeout(std::time::Duration::from_secs(2))
                    .build()
                    .ok()?
                    .get(&health_url)
                    .send()
                    .await
                    .ok()
                    .filter(|r| r.status().is_success())
            })
        })
        .is_some()
    };

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
