//! `ovc onboard` — Interactive setup wizard for new OVC users.
//!
//! Walks the user through identity, key pair generation, repository storage,
//! projects directory, web UI password, and optional daemon installation.
//!
//! In `--non-interactive` mode all prompts are bypassed. The `--name` and
//! `--identity` flags are required; everything else falls back to defaults.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use console::{Style, Term};
use dialoguer::{Input, Password, Select};

use crate::app::OnboardArgs;

/// Collected identity from step 1.
struct IdentityInfo {
    name: String,
    email: String,
}

/// Collected key pair info from step 2.
struct KeyInfo {
    key_name: String,
    fingerprint: String,
    private_key_path: PathBuf,
    /// The passphrase entered during onboarding. Intentionally not written to
    /// shell config — stored here only so the summary step can direct the user
    /// to set it via their keychain.
    #[allow(dead_code)]
    passphrase: String,
}

/// Entry point for `ovc onboard`.
pub fn execute(args: &OnboardArgs) -> Result<()> {
    if args.non_interactive {
        execute_non_interactive(args)
    } else {
        execute_interactive()
    }
}

/// Non-interactive onboard: generates a key and prints env-var configuration
/// without any prompts. Suitable for CI pipelines and scripting.
fn execute_non_interactive(args: &OnboardArgs) -> Result<()> {
    // Both --name and --identity are required in non-interactive mode.
    let key_name = args.name.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "--name is required in non-interactive mode\n\
             Usage: ovc onboard --non-interactive --name <key-name> --identity \"Name <email>\""
        )
    })?;
    let identity_str = args.identity.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "--identity is required in non-interactive mode\n\
             Usage: ovc onboard --non-interactive --name <key-name> --identity \"Name <email>\""
        )
    })?;

    // Parse the identity string ("Name <email>").
    let (author_name, author_email) = parse_identity(identity_str)?;

    // Obtain the passphrase from the environment or error out (no prompts).
    let passphrase = std::env::var("OVC_KEY_PASSPHRASE").map_err(|_| {
        anyhow::anyhow!(
            "OVC_KEY_PASSPHRASE must be set in non-interactive mode\n\
             Example: OVC_KEY_PASSPHRASE=mysecret ovc onboard --non-interactive ..."
        )
    })?;

    let dir = ovc_core::keys::ovc_keys_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

    let priv_path = dir.join(format!("{key_name}.key"));
    let pub_path = dir.join(format!("{key_name}.pub"));

    let fingerprint = if priv_path.exists() || pub_path.exists() {
        // Key already exists — load it to verify passphrase and get fingerprint.
        let keypair = ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
            .context("failed to load existing key — wrong OVC_KEY_PASSPHRASE?")?;
        keypair.fingerprint().to_owned()
    } else {
        // Generate a new key pair.
        let key_identity_str = format!("{author_name} <{author_email}>");
        let key_identity = ovc_core::keys::KeyIdentity::parse(&key_identity_str)
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        let keypair = ovc_core::keys::OvcKeyPair::generate_with_identity(key_identity);

        keypair
            .save_private(&priv_path, passphrase.as_bytes())
            .context("failed to save private key")?;
        keypair
            .save_public(&pub_path)
            .context("failed to save public key")?;

        keypair.fingerprint().to_owned()
    };

    // Print the suggested env-var block for the user's shell config.
    let green = Style::new().green().bold();
    let yellow = Style::new().yellow();

    println!("{}", green.apply_to("Onboard complete (non-interactive)"));
    println!();
    println!("  Key name:    {key_name}");
    println!("  Fingerprint: {fingerprint}");
    println!("  Private key: {}", priv_path.display());
    println!("  Public key:  {}", pub_path.display());
    println!();
    println!("Add the following to your shell configuration file (~/.zshrc, ~/.bashrc, etc.):");
    println!();
    println!("{}", yellow.apply_to("  # ── OVC Configuration ──"));
    println!(
        "{}",
        yellow.apply_to(format!("  export OVC_KEY={key_name}"))
    );
    println!(
        "{}",
        yellow.apply_to("  export OVC_KEY_PASSPHRASE=<your-passphrase>")
    );
    println!(
        "{}",
        yellow.apply_to(format!("  export OVC_AUTHOR_NAME=\"{author_name}\""))
    );
    println!(
        "{}",
        yellow.apply_to(format!("  export OVC_AUTHOR_EMAIL=\"{author_email}\""))
    );
    println!("{}", yellow.apply_to("  export OVC_SIGN_COMMITS=true"));
    println!("{}", yellow.apply_to("  # ── End OVC Configuration ──"));

    Ok(())
}

/// Interactive onboard wizard (original behaviour).
fn execute_interactive() -> Result<()> {
    let term = Term::stdout();
    term.clear_screen()?;

    print_welcome_banner();

    let identity = step_identity()?;
    let key = step_key_pair(&identity)?;
    let repos_dir = step_repo_storage()?;
    let projects_dir = step_projects_dir()?;
    let (ui_password, port) = step_web_ui()?;
    let install_daemon = step_daemon()?;

    write_shell_config(
        &identity,
        &key,
        &repos_dir,
        &projects_dir,
        &ui_password,
        port,
    )?;

    if install_daemon {
        install_daemon_service(port)?;
    }

    print_summary(
        &identity,
        &key,
        &repos_dir,
        &projects_dir,
        port,
        install_daemon,
    );

    Ok(())
}

// ── Identity parsing ──────────────────────────────────────────────────

/// Parses "Name <email>" into `(name, email)`.
fn parse_identity(s: &str) -> Result<(String, String)> {
    let s = s.trim();
    if let Some(lt) = s.find('<') {
        let name = s[..lt].trim().to_owned();
        let email = s[lt + 1..].trim_end_matches('>').trim().to_owned();
        if name.is_empty() || email.is_empty() || !email.contains('@') {
            anyhow::bail!("invalid --identity format: expected 'Name <email>', got '{s}'");
        }
        Ok((name, email))
    } else {
        anyhow::bail!("invalid --identity format: expected 'Name <email>', got '{s}'")
    }
}

// ── Banner ───────────────────────────────────────────────────────────

fn print_welcome_banner() {
    let cyan = Style::new().cyan();

    let banner = r"
   ╔═══════════════════════════════════════════╗
   ║                                           ║
   ║     ██████╗ ██╗   ██╗ ██████╗            ║
   ║    ██╔═══██╗██║   ██║██╔════╝            ║
   ║    ██║   ██║██║   ██║██║                 ║
   ║    ██║   ██║╚██╗ ██╔╝██║                 ║
   ║    ╚██████╔╝ ╚████╔╝ ╚██████╗            ║
   ║     ╚═════╝   ╚═══╝   ╚═════╝            ║
   ║                                           ║
   ║    Olib Version Control                   ║
   ║    Encrypted • Portable • Secure          ║
   ║    by Olib AI (www.olib.ai)               ║
   ║                                           ║
   ╚═══════════════════════════════════════════╝
";

    println!("{}", cyan.apply_to(banner));
    println!(
        "   {}",
        Style::new()
            .white()
            .bold()
            .apply_to("Welcome to OVC! Let's get you set up in a few steps.")
    );
    println!();
}

// ── Step 1: Identity ─────────────────────────────────────────────────

fn step_identity() -> Result<IdentityInfo> {
    print_step_header(1, 6, "Your Identity");

    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("Your name and email will appear on your commits.")
    );
    println!();

    let name: String = Input::new()
        .with_prompt("  Full name")
        .validate_with(|input: &String| -> std::result::Result<(), &str> {
            if input.trim().is_empty() {
                Err("Name cannot be empty")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .context("failed to read name")?;

    let email: String = Input::new()
        .with_prompt("  Email")
        .validate_with(|input: &String| -> std::result::Result<(), &str> {
            let trimmed = input.trim();
            if trimmed.is_empty() {
                Err("Email cannot be empty")
            } else if !trimmed.contains('@') || !trimmed.contains('.') {
                Err("Please enter a valid email address")
            } else {
                Ok(())
            }
        })
        .interact_text()
        .context("failed to read email")?;

    let green = Style::new().green();
    println!(
        "\n  {} Identity set: {} <{}>",
        green.apply_to("✓"),
        name.trim(),
        email.trim(),
    );
    println!();

    Ok(IdentityInfo {
        name: name.trim().to_owned(),
        email: email.trim().to_owned(),
    })
}

// ── Step 2: Key Pair ─────────────────────────────────────────────────

fn step_key_pair(identity: &IdentityInfo) -> Result<KeyInfo> {
    print_step_header(2, 6, "Encryption Key Pair");

    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("OVC uses Ed25519+X25519 keys to encrypt your repos and sign commits.")
    );
    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("Your key will be stored at ~/.ssh/ovc/")
    );
    println!();

    let key_name: String = Input::new()
        .with_prompt("  Key name")
        .default("default".to_owned())
        .interact_text()
        .context("failed to read key name")?;

    let key_name = key_name.trim().to_owned();

    let dir = ovc_core::keys::ovc_keys_dir()
        .ok_or_else(|| anyhow::anyhow!("cannot determine home directory"))?;

    let priv_path = dir.join(format!("{key_name}.key"));
    let pub_path = dir.join(format!("{key_name}.pub"));

    if priv_path.exists() || pub_path.exists() {
        let yellow = Style::new().yellow();
        println!(
            "\n  {} Key '{}' already exists at {}",
            yellow.apply_to("!"),
            key_name,
            dir.display()
        );

        // Load existing key to get fingerprint.
        let passphrase = Password::new()
            .with_prompt("  Key passphrase (existing)")
            .interact()
            .context("failed to read passphrase")?;

        let keypair = ovc_core::keys::OvcKeyPair::load_private(&priv_path, passphrase.as_bytes())
            .context("failed to load existing key — wrong passphrase?")?;

        let fingerprint = keypair.fingerprint().to_owned();

        let green = Style::new().green();
        println!("\n  {} Using existing key pair", green.apply_to("✓"),);
        println!("    Fingerprint: {fingerprint}");
        println!("    Private key: {}", priv_path.display());
        println!("    Public key:  {}", pub_path.display());
        println!();

        return Ok(KeyInfo {
            key_name,
            fingerprint,
            private_key_path: priv_path,
            passphrase,
        });
    }

    let passphrase = Password::new()
        .with_prompt("  Key passphrase")
        .with_confirmation("  Confirm passphrase", "Passphrases do not match")
        .interact()
        .context("failed to read passphrase")?;

    let id_str = format!("{} <{}>", identity.name, identity.email);
    let key_identity =
        ovc_core::keys::KeyIdentity::parse(&id_str).map_err(|e| anyhow::anyhow!("{e}"))?;
    let keypair = ovc_core::keys::OvcKeyPair::generate_with_identity(key_identity);

    keypair
        .save_private(&priv_path, passphrase.as_bytes())
        .context("failed to save private key")?;
    keypair
        .save_public(&pub_path)
        .context("failed to save public key")?;

    let fingerprint = keypair.fingerprint().to_owned();

    let green = Style::new().green();
    println!("\n  {} Key pair generated", green.apply_to("✓"));
    println!("    Fingerprint: {fingerprint}");
    println!("    Private key: {}", priv_path.display());
    println!("    Public key:  {}", pub_path.display());
    println!();
    println!(
        "  {} Export this key to your password manager:",
        Style::new().dim().apply_to("Tip:")
    );
    println!(
        "     {}",
        Style::new()
            .yellow()
            .apply_to(format!("ovc key export {key_name}"))
    );
    println!();

    Ok(KeyInfo {
        key_name,
        fingerprint,
        private_key_path: priv_path,
        passphrase,
    })
}

// ── Step 3: Repository Storage ───────────────────────────────────────

fn step_repo_storage() -> Result<PathBuf> {
    print_step_header(3, 6, "Repository Storage");

    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("Where should OVC store encrypted repository files?")
    );
    println!(
        "  {}",
        Style::new().dim().apply_to(
            "This can be a local folder or a cloud-synced folder (iCloud, Dropbox, etc.)"
        )
    );
    println!();

    let home = dirs::home_dir().context("cannot determine home directory")?;
    let icloud_path = home.join("Library/Mobile Documents/com~apple~CloudDocs/ovc-repos");
    let local_path = home.join(".ovc-repos");

    let mut options = Vec::new();
    let mut paths = Vec::new();

    // Only show iCloud option on macOS when iCloud Drive exists.
    let icloud_parent = home.join("Library/Mobile Documents/com~apple~CloudDocs");
    if icloud_parent.is_dir() {
        options.push(format!(
            "iCloud: {}",
            display_with_tilde(&icloud_path, &home)
        ));
        paths.push(icloud_path);
    }

    options.push(format!(
        "Local:  {}",
        display_with_tilde(&local_path, &home)
    ));
    paths.push(local_path);

    options.push("Custom path".to_owned());

    let selection = Select::new()
        .with_prompt("  Select storage location")
        .items(&options)
        .default(0)
        .interact()
        .context("failed to read storage selection")?;

    let chosen = if selection == options.len() - 1 {
        // Custom path.
        let custom: String = Input::new()
            .with_prompt("  Custom path")
            .interact_text()
            .context("failed to read custom path")?;
        let expanded = expand_tilde(custom.trim());
        PathBuf::from(expanded)
    } else {
        paths[selection].clone()
    };

    // Create the directory.
    std::fs::create_dir_all(&chosen)
        .with_context(|| format!("failed to create directory: {}", chosen.display()))?;

    let green = Style::new().green();
    println!(
        "\n  {} Repository storage: {}",
        green.apply_to("✓"),
        Style::new()
            .yellow()
            .apply_to(display_with_tilde(&chosen, &home))
    );
    println!();

    Ok(chosen)
}

// ── Step 4: Projects Directory ───────────────────────────────────────

fn step_projects_dir() -> Result<PathBuf> {
    print_step_header(4, 6, "Projects Directory");

    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("Where do your source code projects live?")
    );
    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("OVC will scan this directory for linked repositories.")
    );
    println!();

    let home = dirs::home_dir().context("cannot determine home directory")?;
    let default_dir = home.join("GitHub");
    let default_display = display_with_tilde(&default_dir, &home);

    let input: String = Input::new()
        .with_prompt("  Projects directory")
        .default(default_display)
        .interact_text()
        .context("failed to read projects directory")?;

    let expanded = expand_tilde(input.trim());
    let projects_path = PathBuf::from(expanded);

    if !projects_path.exists() {
        std::fs::create_dir_all(&projects_path)
            .with_context(|| format!("failed to create directory: {}", projects_path.display()))?;
    }

    let green = Style::new().green();
    println!(
        "\n  {} Will scan: {} for OVC-linked projects",
        green.apply_to("✓"),
        Style::new()
            .yellow()
            .apply_to(display_with_tilde(&projects_path, &home))
    );
    println!();

    Ok(projects_path)
}

// ── Step 5: Web UI Password ──────────────────────────────────────────

fn step_web_ui() -> Result<(String, u16)> {
    print_step_header(5, 6, "Web UI Access");

    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("Set a password for the web dashboard (http://localhost:9742).")
    );
    println!();

    let password = Password::new()
        .with_prompt("  Web UI password")
        .with_confirmation("  Confirm password", "Passwords do not match")
        .interact()
        .context("failed to read web UI password")?;

    let port_str: String = Input::new()
        .with_prompt("  Port")
        .default("9742".to_owned())
        .validate_with(|input: &String| -> std::result::Result<(), String> {
            input
                .trim()
                .parse::<u16>()
                .map(|_| ())
                .map_err(|_| "Please enter a valid port number (1-65535)".to_owned())
        })
        .interact_text()
        .context("failed to read port")?;

    let port: u16 = port_str.trim().parse().unwrap_or(9742);

    let green = Style::new().green();
    println!(
        "\n  {} Web UI will be available at {}",
        green.apply_to("✓"),
        Style::new()
            .yellow()
            .apply_to(format!("http://localhost:{port}"))
    );
    println!();

    Ok((password, port))
}

// ── Step 6: Daemon ───────────────────────────────────────────────────

fn step_daemon() -> Result<bool> {
    print_step_header(6, 6, "Background Service");

    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("Would you like OVC to start automatically on login?")
    );
    println!(
        "  {}",
        Style::new()
            .dim()
            .apply_to("The web UI will always be available at http://localhost:9742.")
    );
    println!();

    let items = &["Yes", "No"];
    let selection = Select::new()
        .with_prompt("  Install launch daemon?")
        .items(items)
        .default(0)
        .interact()
        .context("failed to read daemon preference")?;

    let install = selection == 0;

    let green = Style::new().green();
    if install {
        println!(
            "\n  {} Daemon will be installed -- OVC server will start on login",
            green.apply_to("✓"),
        );
    } else {
        println!(
            "\n  {} Daemon skipped -- run {} to start manually",
            green.apply_to("✓"),
            Style::new().yellow().apply_to("ovc serve"),
        );
    }
    println!();

    Ok(install)
}

// ── Shell Config ─────────────────────────────────────────────────────

const SHELL_CONFIG_BEGIN: &str = "# ── OVC Configuration ──";
const SHELL_CONFIG_END: &str = "# ── End OVC Configuration ──";

fn write_shell_config(
    identity: &IdentityInfo,
    key: &KeyInfo,
    repos_dir: &Path,
    projects_dir: &Path,
    _ui_password: &str,
    port: u16,
) -> Result<()> {
    let home = dirs::home_dir().context("cannot determine home directory")?;

    // Determine which shell config file to write to.
    let shell_config = detect_shell_config(&home);

    let repos_str = repos_dir.to_string_lossy();
    let projects_str = projects_dir.to_string_lossy();

    // SECURITY: Do NOT write OVC_KEY_PASSPHRASE or OVC_JWT_SECRET to the
    // shell config. The passphrase should be stored in the system keychain
    // or entered interactively. The JWT secret is auto-generated and
    // persisted by the server via load_or_create_persisted_secret — it
    // must never appear in plaintext in a shell rc file.
    let config_block = format!(
        "{SHELL_CONFIG_BEGIN}\n\
         export OVC_KEY={key_name}\n\
         # OVC_KEY_PASSPHRASE — set in your keychain or enter interactively when prompted\n\
         export OVC_REPOS_DIR={repos}\n\
         export OVC_WORKDIR_SCAN={projects}\n\
         export OVC_AUTHOR_NAME=\"{author_name}\"\n\
         export OVC_AUTHOR_EMAIL=\"{author_email}\"\n\
         export OVC_SIGN_COMMITS=true\n\
         export OVC_PORT={port}\n\
         {SHELL_CONFIG_END}",
        key_name = shell_escape(&key.key_name),
        repos = shell_escape(&repos_str),
        projects = shell_escape(&projects_str),
        author_name = identity.name,
        author_email = identity.email,
    );

    // Inform the user how to set their passphrase securely.
    println!(
        "\n  \u{26a0}  OVC_KEY_PASSPHRASE was NOT written to your shell config.\n\
         \n  To set it securely, use your system keychain:\n\
         \n    macOS:   security add-generic-password -s ovc -a \"{}\" -w\n\
         \n  Or enter it interactively each time ovc prompts for it.\n\
         \n  OVC_JWT_SECRET is auto-generated and persisted by the server — no manual setup needed.",
        key.key_name,
    );

    let config_path = &shell_config;

    if config_path.exists() {
        let content = std::fs::read_to_string(config_path)
            .with_context(|| format!("failed to read {}", config_path.display()))?;

        if let (Some(start), Some(end)) = (
            content.find(SHELL_CONFIG_BEGIN),
            content.find(SHELL_CONFIG_END),
        ) {
            // Replace existing delimited block.
            let end_of_block = end + SHELL_CONFIG_END.len();
            let mut new_content = String::with_capacity(content.len());
            new_content.push_str(&content[..start]);
            new_content.push_str(&config_block);
            if end_of_block < content.len() {
                new_content.push_str(&content[end_of_block..]);
            }
            std::fs::write(config_path, new_content)
                .with_context(|| format!("failed to write {}", config_path.display()))?;
        } else {
            // No delimited block found. Check for bare `export OVC_` lines
            // and remove them before appending the new delimited block.
            let filtered: Vec<&str> = content
                .lines()
                .filter(|line| {
                    let trimmed = line.trim();
                    !trimmed.starts_with("export OVC_")
                })
                .collect();

            let mut new_content = filtered.join("\n");
            // Remove trailing blank lines that result from filtering.
            while new_content.ends_with("\n\n") {
                new_content.pop();
            }
            if !new_content.ends_with('\n') {
                new_content.push('\n');
            }
            new_content.push('\n');
            new_content.push_str(&config_block);
            new_content.push('\n');
            std::fs::write(config_path, new_content)
                .with_context(|| format!("failed to write {}", config_path.display()))?;
        }
    } else {
        // Create new file.
        let content = format!("{config_block}\n");
        std::fs::write(config_path, content)
            .with_context(|| format!("failed to write {}", config_path.display()))?;
    }

    Ok(())
}

/// Detects the appropriate shell configuration file.
fn detect_shell_config(home: &Path) -> PathBuf {
    // Check SHELL env var first.
    if let Ok(shell) = std::env::var("SHELL") {
        if shell.contains("zsh") {
            return home.join(".zshrc");
        }
        if shell.contains("bash") {
            let bashrc = home.join(".bashrc");
            if bashrc.exists() {
                return bashrc;
            }
            return home.join(".bash_profile");
        }
        if shell.contains("fish") {
            return home.join(".config/fish/config.fish");
        }
    }

    // Default to .zshrc on macOS, .bashrc elsewhere.
    if cfg!(target_os = "macos") {
        home.join(".zshrc")
    } else {
        home.join(".bashrc")
    }
}

// ── Daemon Install ───────────────────────────────────────────────────

/// Installs the daemon using the same mechanism as `ovc daemon install`.
fn install_daemon_service(port: u16) -> Result<()> {
    let binary_path = std::env::current_exe()
        .context("failed to determine current executable path")?
        .to_string_lossy()
        .into_owned();

    let home = dirs::home_dir().context("cannot determine home directory")?;
    let log_file = home.join(".ovc-server.log");
    let plist_path = home.join("Library/LaunchAgents/ai.olib.ovc-server.plist");

    if let Some(parent) = plist_path.parent() {
        std::fs::create_dir_all(parent)
            .context("failed to create ~/Library/LaunchAgents directory")?;
    }

    // Unload existing if present.
    if plist_path.exists() {
        let _ = std::process::Command::new("launchctl")
            .args(["unload", &plist_path.to_string_lossy()])
            .output();
    }

    let plist_content = generate_plist(&binary_path, port, &log_file.to_string_lossy());
    std::fs::write(&plist_path, &plist_content)
        .with_context(|| format!("failed to write plist to {}", plist_path.display()))?;

    let load_output = std::process::Command::new("launchctl")
        .args(["load", &plist_path.to_string_lossy()])
        .output()
        .context("failed to run launchctl load")?;

    if !load_output.status.success() {
        let stderr = String::from_utf8_lossy(&load_output.stderr);
        anyhow::bail!("launchctl load failed: {stderr}");
    }

    Ok(())
}

/// Generates the `LaunchAgent` plist XML (mirrors `commands/daemon.rs`).
fn generate_plist(binary_path: &str, port: u16, log_file: &str) -> String {
    use std::fmt::Write as _;

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
        <string>ai.olib.ovc-server</string>
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

fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ── Summary ──────────────────────────────────────────────────────────

fn print_summary(
    identity: &IdentityInfo,
    key: &KeyInfo,
    repos_dir: &Path,
    projects_dir: &Path,
    port: u16,
    daemon_installed: bool,
) {
    let green = Style::new().green();
    let yellow = Style::new().yellow();
    let bold = Style::new().bold();
    let dim = Style::new().dim();

    let home = dirs::home_dir().unwrap_or_default();
    let repos_display = display_with_tilde(repos_dir, &home);
    let projects_display = display_with_tilde(projects_dir, &home);
    let short_fp = truncate_fingerprint(&key.fingerprint);

    println!(
        "{}",
        bold.apply_to("━━━ Setup Complete! ━━━━━━━━━━━━━━━━━━━━━━━━━━")
    );
    println!();
    println!(
        "  {} Identity:    {} <{}>",
        green.apply_to("✓"),
        identity.name,
        identity.email
    );
    println!(
        "  {} Key pair:    {} ({})",
        green.apply_to("✓"),
        short_fp,
        yellow.apply_to(display_with_tilde(&key.private_key_path, &home)),
    );
    println!(
        "  {} Repo store:  {}",
        green.apply_to("✓"),
        yellow.apply_to(&repos_display),
    );
    println!(
        "  {} Projects:    {}",
        green.apply_to("✓"),
        yellow.apply_to(&projects_display),
    );
    println!(
        "  {} Web UI:      {}",
        green.apply_to("✓"),
        yellow.apply_to(format!("http://localhost:{port}")),
    );
    if daemon_installed {
        println!(
            "  {} Daemon:      Installed (starts on login)",
            green.apply_to("✓"),
        );
    } else {
        println!("  {} Daemon:      Skipped", green.apply_to("✓"),);
    }

    let shell_config = detect_shell_config(&home);
    let config_display = display_with_tilde(&shell_config, &home);

    println!();
    println!("  Added to {}:", dim.apply_to(config_display),);
    println!("    export OVC_KEY={}", key.key_name);
    println!("    # OVC_KEY_PASSPHRASE — set in your keychain (not written to shell config)");
    println!("    export OVC_REPOS_DIR={repos_display}");
    println!("    export OVC_WORKDIR_SCAN={projects_display}");
    println!("    export OVC_AUTHOR_NAME=\"{}\"", identity.name);
    println!("    export OVC_AUTHOR_EMAIL=\"{}\"", identity.email);
    println!("    export OVC_SIGN_COMMITS=true");

    println!();
    println!("  {}", bold.apply_to("Quick start:"),);
    println!(
        "    {}",
        dim.apply_to(format!("cd {projects_display}/my-project"))
    );
    println!(
        "    {}",
        dim.apply_to(format!(
            "ovc init --name my-project.ovc --key {} \\",
            key.key_name
        ))
    );
    println!(
        "      {}",
        dim.apply_to(format!("--store {repos_display}/"))
    );
    println!(
        "    {}",
        dim.apply_to("ovc add . && ovc commit -m \"Initial commit\"")
    );

    println!();
    println!(
        "  Open {} to access the web UI.",
        yellow.apply_to(format!("http://localhost:{port}")),
    );
    println!();
    println!(
        "{}",
        bold.apply_to("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━")
    );
}

// ── Helpers ──────────────────────────────────────────────────────────

/// Prints a styled step header.
fn print_step_header(step: u8, total: u8, title: &str) {
    let cyan = Style::new().cyan().bold();
    println!(
        "{}",
        cyan.apply_to(format!("━━━ Step {step} of {total}: {title} ━━━"))
    );
    println!();
}

/// Replaces the home directory prefix with `~` for display.
fn display_with_tilde(path: &Path, home: &Path) -> String {
    let path_str = path.to_string_lossy();
    let home_str = home.to_string_lossy();
    if path_str.starts_with(home_str.as_ref()) {
        format!("~{}", &path_str[home_str.len()..])
    } else {
        path_str.into_owned()
    }
}

/// Expands a leading `~` to the home directory.
fn expand_tilde(path: &str) -> String {
    if let Some(rest) = path.strip_prefix('~')
        && let Some(home) = dirs::home_dir()
    {
        return format!("{}{rest}", home.display());
    }
    path.to_owned()
}

/// Truncates a fingerprint for compact display.
fn truncate_fingerprint(fp: &str) -> String {
    if fp.len() > 18 {
        format!("{}...", &fp[..18])
    } else {
        fp.to_owned()
    }
}

/// Shell-escapes a value by wrapping in single quotes.
fn shell_escape(value: &str) -> String {
    // If the value contains no special characters, return as-is.
    if value
        .chars()
        .all(|c| c.is_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/')
    {
        return value.to_owned();
    }
    // Wrap in single quotes, escaping any embedded single quotes.
    let escaped = value.replace('\'', "'\\''");
    format!("'{escaped}'")
}
