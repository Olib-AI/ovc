//! `ovc reflog` — Show reference log.

use anyhow::Result;
use console::Style;

use ovc_core::refs::RefTarget;

use crate::app::ReflogArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, _args: &ReflogArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    // Determine the current ref name to filter the reflog.
    let ref_name = match repo.ref_store().head() {
        RefTarget::Symbolic(name) => name.clone(),
        RefTarget::Direct(_) => "HEAD".to_owned(),
    };

    let entries = repo.ref_store().get_reflog(&ref_name);

    if entries.is_empty() {
        println!("reflog is empty");
        return Ok(());
    }

    let yellow = Style::new().yellow();
    let cyan = Style::new().cyan();

    // Display in reverse order (most recent first).
    for (i, entry) in entries.iter().rev().enumerate() {
        let hex = entry.new_value.to_string();
        let short = &hex[..12.min(hex.len())];

        println!(
            "{} {} {}",
            yellow.apply_to(short),
            cyan.apply_to(format!("HEAD@{{{i}}}")),
            entry.message,
        );
    }

    Ok(())
}
