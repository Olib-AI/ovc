//! `ovc blame <file>` — Show line-by-line authorship.

use anyhow::{Context, Result, bail};
use console::Style;

use ovc_core::blame;

use crate::app::BlameArgs;
use crate::context::CliContext;

pub fn execute(ctx: &CliContext, args: &BlameArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let head_oid = repo
        .ref_store()
        .resolve_head()
        .context("cannot blame: no commits yet")?;

    let lines = blame::blame(&args.file, head_oid, repo.object_store()).context("blame failed")?;

    if lines.is_empty() {
        bail!("file is empty or not found: {}", args.file);
    }

    // Parse -L line range if provided.
    let (start_line, end_line) = if let Some(ref range_str) = args.lines {
        parse_line_range(range_str, lines.len())?
    } else {
        (1, lines.len())
    };

    let yellow = Style::new().yellow();
    let cyan = Style::new().cyan();

    for line in &lines {
        if line.line_number < start_line || line.line_number > end_line {
            continue;
        }

        let hex = line.commit_id.to_string();
        let short = &hex[..12.min(hex.len())];

        let dt = chrono::DateTime::from_timestamp(line.timestamp, 0);
        let date_str = dt.map_or_else(
            || "unknown".to_owned(),
            |d| d.format("%Y-%m-%d").to_string(),
        );

        println!(
            "{} ({} {}) {}",
            yellow.apply_to(short),
            cyan.apply_to(&line.author),
            date_str,
            line.content,
        );
    }

    Ok(())
}

/// Parses a line range specification like "10,20" or "10,+5".
///
/// Returns (start, end) as 1-based inclusive line numbers.
fn parse_line_range(spec: &str, total_lines: usize) -> Result<(usize, usize)> {
    let parts: Vec<&str> = spec.split(',').collect();
    if parts.len() != 2 {
        bail!("invalid line range: '{spec}' (expected 'start,end' or 'start,+count')");
    }

    let start: usize = parts[0]
        .trim()
        .parse()
        .with_context(|| format!("invalid start line: '{}'", parts[0]))?;

    let end_str = parts[1].trim();
    let end = if let Some(offset_str) = end_str.strip_prefix('+') {
        let offset: usize = offset_str
            .parse()
            .with_context(|| format!("invalid line offset: '{end_str}'"))?;
        start.saturating_add(offset).saturating_sub(1)
    } else {
        end_str
            .parse()
            .with_context(|| format!("invalid end line: '{end_str}'"))?
    };

    if start == 0 {
        bail!("line numbers are 1-based; start cannot be 0");
    }
    if start > end {
        bail!("start line ({start}) must not exceed end line ({end})");
    }

    // Clamp end to total lines.
    let end = end.min(total_lines);

    Ok((start, end))
}
