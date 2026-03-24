//! `ovc daemon` — Manage the OVC background server daemon (macOS `LaunchAgent`).

use std::fmt::Write as _;
use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::Command;

use anyhow::{Context, Result, bail};

use crate::app::{DaemonAction, DaemonArgs};
use crate::output;

const PLIST_LABEL: &str = "ai.olib.ovc-server";
const LOG_FILENAME: &str = ".ovc-server.log";
const DEFAULT_PORT: u16 = 9742;

/// Environment variables to bake into the `LaunchAgent` plist.
const ENV_VARS: &[&str] = &[
    "OVC_KEY",
    "OVC_KEY_PASSPHRASE",
    "OVC_REPOS_DIR",
    "OVC_WORKDIR_MAP",
    "OVC_WORKDIR_SCAN",
    "OVC_AUTHOR_NAME",
    "OVC_AUTHOR_EMAIL",
    "OVC_SIGN_COMMITS",
    "OVC_JWT_SECRET",
    "OVC_CORS_ORIGINS",
    "HOME",
];

fn home_dir() -> Result<PathBuf> {
    dirs::home_dir().context("cannot determine home directory")
}

fn plist_path() -> Result<PathBuf> {
    Ok(home_dir()?.join("Library/LaunchAgents/ai.olib.ovc-server.plist"))
}

fn log_path() -> Result<PathBuf> {
    Ok(home_dir()?.join(LOG_FILENAME))
}

/// Entry point for `ovc daemon <action>`.
pub fn execute(args: &DaemonArgs) -> Result<()> {
    if cfg!(not(target_os = "macos")) {
        bail!("daemon commands are only supported on macOS");
    }

    match &args.action {
        DaemonAction::Install { port } => install(*port),
        DaemonAction::Uninstall => uninstall(),
        DaemonAction::Start => start(),
        DaemonAction::Stop => stop(),
        DaemonAction::Status => status(),
        DaemonAction::Logs { follow } => logs(*follow),
    }
}

/// Generates the `LaunchAgent` plist XML.
fn generate_plist(binary_path: &str, port: u16, log_file: &str) -> String {
    let mut env_section = String::new();
    for &var in ENV_VARS {
        if let Ok(val) = std::env::var(var) {
            let escaped_val = xml_escape(&val);
            let _ = write!(
                env_section,
                "            <key>{var}</key>\n            <string>{escaped_val}</string>\n"
            );
        }
    }

    let env_dict = if env_section.is_empty() {
        String::new()
    } else {
        format!(
            "        <key>EnvironmentVariables</key>\n        <dict>\n{env_section}        </dict>\n"
        )
    };

    let escaped_binary = xml_escape(binary_path);
    let escaped_log = xml_escape(log_file);
    let port_str = port.to_string();

    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
        <key>Label</key>
        <string>{PLIST_LABEL}</string>
        <key>ProgramArguments</key>
        <array>
            <string>{escaped_binary}</string>
            <string>serve</string>
            <string>--port</string>
            <string>{port_str}</string>
        </array>
{env_dict}        <key>RunAtLoad</key>
        <true/>
        <key>KeepAlive</key>
        <true/>
        <key>StandardOutPath</key>
        <string>{escaped_log}</string>
        <key>StandardErrorPath</key>
        <string>{escaped_log}</string>
</dict>
</plist>
"#
    )
}

/// Escapes special XML characters in a string value.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

fn install(port: u16) -> Result<()> {
    let binary_path = std::env::current_exe()
        .context("failed to determine current executable path")?
        .to_string_lossy()
        .into_owned();

    let log_file = log_path()?;
    let log_file_str = log_file.to_string_lossy().into_owned();

    let plist = plist_path()?;

    // Ensure the LaunchAgents directory exists.
    if let Some(parent) = plist.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create ~/Library/LaunchAgents directory")?;
    }

    // If already loaded, unload first to avoid errors on re-install.
    if plist.exists() {
        let _ = Command::new("launchctl")
            .args(["unload", &plist.to_string_lossy()])
            .output();
    }

    let plist_content = generate_plist(&binary_path, port, &log_file_str);
    std::fs::write(&plist, &plist_content)
        .with_context(|| format!("failed to write plist to {}", plist.display()))?;

    let load_output = Command::new("launchctl")
        .args(["load", &plist.to_string_lossy()])
        .output()
        .context("failed to run launchctl load")?;

    if !load_output.status.success() {
        let stderr = String::from_utf8_lossy(&load_output.stderr);
        bail!("launchctl load failed: {stderr}");
    }

    output::print_success(&format!(
        "daemon installed and loaded (port {port})\n  plist: {}\n  log:   {}\n  binary: {binary_path}",
        plist.display(),
        log_file.display()
    ));
    Ok(())
}

fn uninstall() -> Result<()> {
    let plist = plist_path()?;

    if plist.exists() {
        let unload_output = Command::new("launchctl")
            .args(["unload", &plist.to_string_lossy()])
            .output()
            .context("failed to run launchctl unload")?;

        if !unload_output.status.success() {
            let stderr = String::from_utf8_lossy(&unload_output.stderr);
            output::print_warning(&format!("launchctl unload warning: {stderr}"));
        }

        std::fs::remove_file(&plist)
            .with_context(|| format!("failed to remove plist at {}", plist.display()))?;
    } else {
        output::print_warning("plist not found; daemon may not be installed");
    }

    output::print_success("daemon uninstalled");
    Ok(())
}

fn start() -> Result<()> {
    let output = Command::new("launchctl")
        .args(["start", PLIST_LABEL])
        .output()
        .context("failed to run launchctl start")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("launchctl start failed: {stderr}");
    }

    output::print_success("daemon started");
    Ok(())
}

fn stop() -> Result<()> {
    let output = Command::new("launchctl")
        .args(["stop", PLIST_LABEL])
        .output()
        .context("failed to run launchctl stop")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("launchctl stop failed: {stderr}");
    }

    output::print_success("daemon stopped");
    Ok(())
}

fn status() -> Result<()> {
    let port = read_port_from_plist().unwrap_or(DEFAULT_PORT);
    let log_file = log_path()?;

    let list_output = Command::new("launchctl")
        .args(["list", PLIST_LABEL])
        .output()
        .context("failed to run launchctl list")?;

    if list_output.status.success() {
        let stdout = String::from_utf8_lossy(&list_output.stdout);
        let pid = parse_pid_from_list(&stdout);

        match pid {
            Some(p) => println!("Status:  running (PID {p})"),
            None => println!("Status:  loaded but not running"),
        }
    } else {
        println!("Status:  not loaded (daemon not installed or unloaded)");
    }

    println!("Port:    {port}");
    println!("Log:     {}", log_file.display());

    // Health check.
    let health_url = format!("http://127.0.0.1:{port}/api/v1/health");
    print!("Health:  ");
    match check_health(&health_url) {
        Ok(body) => println!("ok ({body})"),
        Err(e) => println!("unreachable ({e})"),
    }

    Ok(())
}

/// Attempts a blocking HTTP GET to the health endpoint with a short timeout.
fn check_health(url: &str) -> Result<String> {
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create tokio runtime for health check")?;

    rt.block_on(async {
        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(3))
            .build()
            .context("failed to build HTTP client")?;

        let resp = client.get(url).send().await.context("request failed")?;

        let status = resp.status();
        let body = resp.text().await.unwrap_or_default();

        if status.is_success() {
            Ok(body)
        } else {
            bail!("HTTP {status}: {body}")
        }
    })
}

/// Parses the PID from `launchctl list <label>` output.
///
/// The output format includes lines like `"PID" = 12345;` or `"PID" = 0;`.
fn parse_pid_from_list(output: &str) -> Option<u32> {
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("\"PID\"") {
            // Format: "PID" = 12345;
            let value = trimmed
                .split('=')
                .nth(1)?
                .trim()
                .trim_end_matches(';')
                .trim();
            let pid: u32 = value.parse().ok()?;
            if pid > 0 {
                return Some(pid);
            }
            return None;
        }
    }
    None
}

/// Reads the port from the installed plist by parsing the `ProgramArguments`.
fn read_port_from_plist() -> Option<u16> {
    let plist = plist_path().ok()?;
    let content = std::fs::read_to_string(plist).ok()?;

    // Simple XML parsing: find `--port` followed by a port number string.
    let mut lines = content.lines();
    while let Some(line) = lines.next() {
        if line.contains("<string>--port</string>") {
            // Next <string>...</string> should be the port value.
            let next = lines.next()?;
            let trimmed = next.trim();
            let port_str = trimmed
                .strip_prefix("<string>")?
                .strip_suffix("</string>")?;
            return port_str.parse().ok();
        }
    }
    None
}

fn logs(follow: bool) -> Result<()> {
    let log_file = log_path()?;

    if !log_file.exists() {
        bail!(
            "log file not found at {}; is the daemon installed?",
            log_file.display()
        );
    }

    if follow {
        let status = Command::new("tail")
            .args(["-f", &log_file.to_string_lossy()])
            .status()
            .context("failed to run tail -f")?;

        if !status.success() {
            bail!("tail -f exited with status {status}");
        }
    } else {
        let file = std::fs::File::open(&log_file).context("failed to open log file")?;
        let reader = BufReader::new(file);
        let lines: Vec<String> = reader
            .lines()
            .collect::<std::io::Result<Vec<_>>>()
            .context("failed to read log file")?;

        let start = lines.len().saturating_sub(50);
        for line in &lines[start..] {
            println!("{line}");
        }
    }

    Ok(())
}
