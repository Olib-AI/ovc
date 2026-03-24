//! `ovc archive [--format tar|zip] [-o output] [commit]` — Export tree as archive.

use std::io::Write;

use anyhow::{Context, Result, bail};

use ovc_core::id::ObjectId;
use ovc_core::index::Index;
use ovc_core::object::Object;

use crate::app::ArchiveArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &ArchiveArgs) -> Result<()> {
    let (repo, _workdir) = ctx.open_repo()?;

    let commit_oid = if let Some(ref spec) = args.commit {
        if spec.eq_ignore_ascii_case("HEAD") {
            repo.ref_store().resolve_head().context("no commits yet")?
        } else {
            spec.parse::<ObjectId>()
                .map_err(|e| anyhow::anyhow!("invalid commit id: {e}"))?
        }
    } else {
        repo.ref_store().resolve_head().context("no commits yet")?
    };

    let commit_obj = repo
        .get_object(&commit_oid)?
        .ok_or_else(|| anyhow::anyhow!("commit not found: {commit_oid}"))?;

    let tree_oid = match commit_obj {
        Object::Commit(c) => c.tree,
        _ => bail!("target is not a commit"),
    };

    // Build file list from tree.
    let mut index = Index::new();
    index.read_tree(&tree_oid, repo.object_store())?;

    let format = args.format.as_deref().unwrap_or("tar");

    match format {
        "tar" => write_tar(args, &index, &repo)?,
        "zip" => write_zip(args, &index, &repo)?,
        other => bail!("unsupported archive format: {other} (use tar or zip)"),
    }

    Ok(())
}

fn write_tar(
    args: &ArchiveArgs,
    index: &Index,
    repo: &ovc_core::repository::Repository,
) -> Result<()> {
    let writer: Box<dyn Write> = if let Some(ref output) = args.output {
        Box::new(
            std::fs::File::create(output)
                .with_context(|| format!("failed to create output file: {}", output.display()))?,
        )
    } else {
        Box::new(std::io::stdout().lock())
    };

    let mut builder = tar::Builder::new(writer);

    for entry in index.entries() {
        let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? else {
            continue;
        };

        let mut header = tar::Header::new_gnu();
        header.set_path(&entry.path)?;
        header.set_size(u64::try_from(data.len()).unwrap_or(0));
        header.set_mode(0o644);
        header.set_mtime(0);
        header.set_cksum();

        builder.append(&header, data.as_slice())?;
    }

    builder.finish()?;

    if args.output.is_some() {
        output::print_success("archive created");
    }

    Ok(())
}

fn write_zip(
    args: &ArchiveArgs,
    index: &Index,
    repo: &ovc_core::repository::Repository,
) -> Result<()> {
    let output = args
        .output
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("zip format requires -o/--output"))?;

    let file = std::fs::File::create(output)
        .with_context(|| format!("failed to create output file: {}", output.display()))?;

    let mut zip = zip::ZipWriter::new(file);
    let options = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);

    for entry in index.entries() {
        let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? else {
            continue;
        };

        zip.start_file(&entry.path, options)?;
        zip.write_all(&data)?;
    }

    zip.finish()?;

    output::print_success(&format!("archive created: {}", output.display()));

    Ok(())
}
