//! `ovc tag` — Manage tags.

use anyhow::{Context, Result};

use ovc_core::object::{Identity, Object, ObjectType, Tag};

use crate::app::TagArgs;
use crate::context::CliContext;
use crate::output;

pub fn execute(ctx: &CliContext, args: &TagArgs) -> Result<()> {
    let (mut repo, _workdir) = ctx.open_repo()?;

    if let Some(ref name) = args.delete {
        repo.ref_store_mut()
            .delete_tag(name)
            .with_context(|| format!("failed to delete tag '{name}'"))?;
        repo.save()?;
        output::print_success(&format!("deleted tag '{name}'"));
        return Ok(());
    }

    if let Some(ref name) = args.name {
        let head_oid = repo
            .ref_store()
            .resolve_head()
            .context("cannot tag: no commits yet")?;

        if let Some(ref message) = args.message {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX));

            let tagger = CliContext::resolve_author(None, &repo).unwrap_or_else(|_| Identity {
                name: String::from("OVC User"),
                email: String::from("user@ovc"),
                timestamp: now,
                tz_offset_minutes: 0,
            });

            let tag_obj = Tag {
                target: head_oid,
                target_type: ObjectType::Commit,
                tag_name: name.clone(),
                tagger,
                message: message.clone(),
                signature: None,
            };

            // Persist the full Tag object in the object store for tooling
            // that reads tag objects directly, then point the ref to the
            // commit so that the ref store's commit_id is always correct.
            let _tag_oid = repo.insert_object(&Object::Tag(tag_obj))?;
            repo.ref_store_mut()
                .create_tag(name, head_oid, Some(message))?;
        } else {
            repo.ref_store_mut().create_tag(name, head_oid, None)?;
        }

        repo.save()?;
        output::print_success(&format!("created tag '{name}'"));
        return Ok(());
    }

    let tags = repo.ref_store().list_tags();
    if tags.is_empty() {
        println!("no tags");
    } else {
        for (name, _oid, msg) in &tags {
            if let Some(m) = msg {
                println!("{name}  {m}");
            } else {
                println!("{name}");
            }
        }
    }

    Ok(())
}
