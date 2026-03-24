//! Git reference reading and writing.
//!
//! Reads loose refs from `.git/refs/`, packed refs from `.git/packed-refs`,
//! and HEAD from `.git/HEAD`. Writes refs back in the same format.

use std::collections::BTreeMap;
use std::path::Path;

use crate::error::GitResult;

/// Reads all git references from a `.git` directory.
///
/// Returns a map of `ref_name -> sha1_hex`. HEAD is included as `"HEAD"`.
pub fn read_git_refs(git_dir: &Path) -> GitResult<BTreeMap<String, String>> {
    let mut refs = BTreeMap::new();

    // Read packed-refs first; loose refs override them.
    let packed_refs_path = git_dir.join("packed-refs");
    if packed_refs_path.is_file() {
        let content = std::fs::read_to_string(&packed_refs_path)?;
        for line in content.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') || line.starts_with('^') {
                continue;
            }
            let mut parts = line.splitn(2, ' ');
            if let (Some(sha1), Some(ref_name)) = (parts.next(), parts.next())
                && sha1.len() == 40
            {
                refs.insert(ref_name.to_owned(), sha1.to_owned());
            }
        }
    }

    // Read loose refs, which override packed refs.
    let refs_dir = git_dir.join("refs");
    if refs_dir.is_dir() {
        read_refs_recursive(&refs_dir, "refs", &mut refs)?;
    }

    // Read HEAD.
    let head_path = git_dir.join("HEAD");
    if head_path.is_file() {
        let head_content = std::fs::read_to_string(&head_path)?;
        let head_content = head_content.trim();
        if let Some(sym_ref) = head_content.strip_prefix("ref: ") {
            // Symbolic HEAD — resolve through refs map.
            if let Some(sha1) = refs.get(sym_ref) {
                refs.insert("HEAD".to_owned(), sha1.clone());
            }
            // Store the symbolic target too, for later HEAD reconstruction.
            refs.insert("HEAD_SYMBOLIC".to_owned(), sym_ref.to_owned());
        } else if head_content.len() == 40 {
            // Detached HEAD.
            refs.insert("HEAD".to_owned(), head_content.to_owned());
        }
    }

    Ok(refs)
}

/// Recursively reads loose refs from a directory.
fn read_refs_recursive(
    dir: &Path,
    prefix: &str,
    refs: &mut BTreeMap<String, String>,
) -> GitResult<()> {
    let entries = std::fs::read_dir(dir)?;
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        let ref_name = format!("{prefix}/{name_str}");

        if path.is_dir() {
            read_refs_recursive(&path, &ref_name, refs)?;
        } else if path.is_file() {
            let content = std::fs::read_to_string(&path)?;
            let sha1 = content.trim();
            if sha1.len() == 40 && sha1.chars().all(|c| c.is_ascii_hexdigit()) {
                refs.insert(ref_name, sha1.to_owned());
            }
        }
    }
    Ok(())
}

/// Writes git references to a `.git` directory.
///
/// Creates loose refs under `.git/refs/heads/` and `.git/refs/tags/`, and
/// writes `.git/HEAD`.
pub fn write_git_refs(
    git_dir: &Path,
    refs: &BTreeMap<String, String>,
    head_ref: &str,
) -> GitResult<()> {
    for (ref_name, sha1) in refs {
        if ref_name == "HEAD" || ref_name == "HEAD_SYMBOLIC" {
            continue;
        }
        let ref_path = git_dir.join(ref_name);
        if let Some(parent) = ref_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&ref_path, format!("{sha1}\n"))?;
    }

    // Write HEAD.
    let head_path = git_dir.join("HEAD");
    if head_ref.starts_with("refs/") {
        std::fs::write(head_path, format!("ref: {head_ref}\n"))?;
    } else if head_ref.len() == 40 {
        std::fs::write(head_path, format!("{head_ref}\n"))?;
    } else {
        // Default symbolic HEAD.
        std::fs::write(head_path, format!("ref: refs/heads/{head_ref}\n"))?;
    }

    Ok(())
}

/// Extracts the default branch name from a `HEAD_SYMBOLIC` entry, falling
/// back to `"main"`.
#[must_use]
pub fn head_branch_from_refs(refs: &BTreeMap<String, String>) -> String {
    refs.get("HEAD_SYMBOLIC")
        .and_then(|s| s.strip_prefix("refs/heads/"))
        .unwrap_or("main")
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_read_roundtrip() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(git_dir.join("refs/heads")).unwrap();
        std::fs::create_dir_all(git_dir.join("refs/tags")).unwrap();

        let mut refs = BTreeMap::new();
        refs.insert(
            "refs/heads/main".to_owned(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa".to_owned(),
        );
        refs.insert(
            "refs/tags/v1.0".to_owned(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".to_owned(),
        );

        write_git_refs(&git_dir, &refs, "refs/heads/main").unwrap();

        let read_back = read_git_refs(&git_dir).unwrap();
        assert_eq!(
            read_back.get("refs/heads/main").unwrap(),
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(
            read_back.get("refs/tags/v1.0").unwrap(),
            "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb"
        );
        assert_eq!(read_back.get("HEAD_SYMBOLIC").unwrap(), "refs/heads/main");
    }
}
