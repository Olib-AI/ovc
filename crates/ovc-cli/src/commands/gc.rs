//! `ovc gc` — Run garbage collection.

use anyhow::{Context, Result};

use crate::app::GcArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &GcArgs) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    if args.dry_run {
        // For dry-run, we clone the store to avoid mutating the real one.
        let mut store_clone = repo.object_store().clone();
        let result =
            ovc_core::gc::garbage_collect(&mut store_clone, repo.ref_store(), repo.stash())
                .context("garbage collection failed")?;

        println!("Dry run results:");
        println!("  Objects before: {}", result.objects_before);
        println!("  Objects after:  {}", result.objects_after);
        println!(
            "  Would remove:   {} objects",
            result.objects_before - result.objects_after
        );
        println!("  Bytes before:   {}", format_bytes(result.bytes_before));
        println!("  Bytes after:    {}", format_bytes(result.bytes_after));
        println!(
            "  Would free:     {}",
            format_bytes(result.bytes_before - result.bytes_after)
        );
    } else {
        let result = repo.gc().context("garbage collection failed")?;
        repo.save().context("failed to save repository")?;

        let removed = result.objects_before - result.objects_after;
        let freed = result.bytes_before - result.bytes_after;

        println!("Garbage collection complete:");
        println!("  Objects before: {}", result.objects_before);
        println!("  Objects after:  {}", result.objects_after);
        println!("  Removed:        {removed} objects");
        println!("  Bytes freed:    {}", format_bytes(freed));

        output::print_success(&format!("Removed {removed} unreachable objects"));
    }

    Ok(())
}

/// Formats a byte count as a human-readable string.
///
/// Precision loss from u64->f64 is acceptable here as this is purely
/// for display purposes and exact byte counts are shown for small values.
#[allow(clippy::cast_precision_loss)]
fn format_bytes(bytes: u64) -> String {
    const KIB: u64 = 1024;
    const MIB: u64 = KIB * 1024;
    const GIB: u64 = MIB * 1024;

    if bytes >= GIB {
        format!("{:.2} GiB", bytes as f64 / GIB as f64)
    } else if bytes >= MIB {
        format!("{:.2} MiB", bytes as f64 / MIB as f64)
    } else if bytes >= KIB {
        format!("{:.2} KiB", bytes as f64 / KIB as f64)
    } else {
        format!("{bytes} B")
    }
}
