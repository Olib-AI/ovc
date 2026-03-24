//! Output formatting helpers for the OVC CLI.

use std::fmt::Write;

use console::Style;
use ovc_core::diff::{Hunk, HunkLine};
use ovc_core::id::ObjectId;
use ovc_core::object::Identity;

/// Prints a success message with a green prefix.
pub fn print_success(msg: &str) {
    let style = Style::new().green();
    println!("{} {msg}", style.apply_to("[ok]"));
}

/// Prints an error message with a red prefix.
pub fn print_error(msg: &str) {
    let style = Style::new().red();
    eprintln!("{} {msg}", style.apply_to("[error]"));
}

/// Prints a warning message with a yellow prefix.
pub fn print_warning(msg: &str) {
    let style = Style::new().yellow();
    eprintln!("{} {msg}", style.apply_to("[warn]"));
}

/// Formats and prints a full commit display with optional signature info.
pub fn print_commit_with_signature(
    oid: &ObjectId,
    message: &str,
    author: &Identity,
    refs: &[&str],
    sig_status: Option<&ovc_core::keys::VerifyResult>,
    show_details: bool,
) {
    let yellow = Style::new().yellow();
    let green = Style::new().green().bold();

    let hex = oid.to_string();

    let mut header = format!("commit {}", yellow.apply_to(&hex));
    if !refs.is_empty() {
        let ref_str = refs.join(", ");
        let _ = write!(header, " ({})", green.apply_to(ref_str));
    }
    println!("{header}");

    if let Some(status) = sig_status {
        print_signature_line(status, show_details);
    }

    println!("Author: {} <{}>", author.name, author.email);

    let ts = chrono::DateTime::from_timestamp(author.timestamp, 0);
    if let Some(dt) = ts {
        println!("Date:   {}", dt.format("%a %b %d %H:%M:%S %Y %z"));
    }

    println!();
    for line in message.lines() {
        println!("    {line}");
    }
    println!();
}

/// Prints a signature verification line for full commit display.
fn print_signature_line(status: &ovc_core::keys::VerifyResult, show_details: bool) {
    use ovc_core::keys::VerifyResult;

    let green_style = Style::new().green();
    let red_style = Style::new().red();

    match status {
        VerifyResult::Verified {
            fingerprint,
            identity,
        } => {
            print!("{}", green_style.apply_to("Sig:    Verified"));
            if show_details {
                if let Some(id) = identity {
                    print!(" by {id}");
                }
                print!(" (key {fingerprint})");
            }
            println!();
        }
        VerifyResult::Unverified { reason } => {
            print!("{}", red_style.apply_to("Sig:    Unverified"));
            if show_details {
                print!(" ({reason})");
            }
            println!();
        }
        VerifyResult::NotSigned => {
            // Only show if details requested.
            if show_details {
                println!("Sig:    (unsigned)");
            }
        }
    }
}

/// Prints an inline signature indicator (for oneline mode).
pub fn print_signature_inline(status: &ovc_core::keys::VerifyResult) {
    use ovc_core::keys::VerifyResult;

    let green_style = Style::new().green();
    let red_style = Style::new().red();

    match status {
        VerifyResult::Verified { .. } => {
            // In oneline mode, prefix was already printed. Just note it.
            eprint!("  {}", green_style.apply_to("[verified]"));
        }
        VerifyResult::Unverified { .. } => {
            eprint!("  {}", red_style.apply_to("[unverified]"));
        }
        VerifyResult::NotSigned => {}
    }
}

/// Formats and prints a oneline commit display.
pub fn print_commit_oneline(oid: &ObjectId, message: &str, refs: &[&str]) {
    let yellow = Style::new().yellow();
    let green = Style::new().green().bold();

    let hex = oid.to_string();
    let short_hash = &hex[..12];

    let first_line = message.lines().next().unwrap_or("");
    let mut line = format!("{} ", yellow.apply_to(short_hash));
    if !refs.is_empty() {
        let ref_str = refs.join(", ");
        let _ = write!(line, "({}) ", green.apply_to(ref_str));
    }
    line.push_str(first_line);
    println!("{line}");
}

/// Formats and prints a colored unified diff hunk.
pub fn print_diff_hunk(hunk: &Hunk) {
    let cyan = Style::new().cyan();
    let green = Style::new().green();
    let red = Style::new().red();

    println!(
        "{}",
        cyan.apply_to(format!(
            "@@ -{},{} +{},{} @@",
            hunk.old_start, hunk.old_count, hunk.new_start, hunk.new_count
        ))
    );

    for line in &hunk.lines {
        match line {
            HunkLine::Context(data) => {
                let s = String::from_utf8_lossy(data);
                let s = s.trim_end_matches('\n');
                println!(" {s}");
            }
            HunkLine::Addition(data) => {
                let s = String::from_utf8_lossy(data);
                let s = s.trim_end_matches('\n');
                println!("{}", green.apply_to(format!("+{s}")));
            }
            HunkLine::Deletion(data) => {
                let s = String::from_utf8_lossy(data);
                let s = s.trim_end_matches('\n');
                println!("{}", red.apply_to(format!("-{s}")));
            }
        }
    }
}

/// Prints a colored diff header (file names).
pub fn print_diff_header(old_name: &str, new_name: &str) {
    let bold = Style::new().bold();
    println!("{}", bold.apply_to(format!("--- {old_name}")));
    println!("{}", bold.apply_to(format!("+++ {new_name}")));
}
