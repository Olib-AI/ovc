//! OVC-to-Git repository export.
//!
//! Walks all OVC objects, converts them to git format, writes them as loose
//! objects, sets up refs, and optionally checks out the working directory.

use std::collections::{BTreeMap, HashSet};
use std::path::Path;

use ovc_core::id::ObjectId;
use ovc_core::keys::OvcKeyPair;
use ovc_core::object::{FileMode, Object, ObjectType, TreeEntry};
use ovc_core::refs::RefTarget;
use ovc_core::repository::Repository;

use crate::error::GitResult;
use crate::git_objects::{GitCommit, GitTag, format_git_identity};
use crate::git_refs;
use crate::oid_map::OidMap;
use crate::write_git;

/// Statistics returned after a successful export.
pub struct ExportResult {
    /// Number of blob objects exported.
    pub blobs_exported: u64,
    /// Number of tree objects exported.
    pub trees_exported: u64,
    /// Number of commit objects exported.
    pub commits_exported: u64,
    /// Number of tag objects exported.
    pub tags_exported: u64,
    /// Number of references exported.
    pub refs_exported: u64,
    /// The bidirectional OID mapping produced during export.
    pub oid_map: OidMap,
}

/// Exports an OVC repository to a new git repository directory (password-based).
///
/// Creates a `.git` directory structure with loose objects and refs, then
/// checks out the HEAD tree into the working directory.
pub fn export_to_git(
    ovc_path: &Path,
    output_dir: &Path,
    password: &[u8],
) -> GitResult<ExportResult> {
    let repo = Repository::open(ovc_path, password)?;
    export_repo(&repo, output_dir)
}

/// Exports an OVC repository to a new git repository directory (key-based).
///
/// Opens the `.ovc` file using the provided `keypair` instead of a password.
/// All other semantics are identical to [`export_to_git`].
pub fn export_to_git_with_key(
    ovc_path: &Path,
    output_dir: &Path,
    keypair: &OvcKeyPair,
) -> GitResult<ExportResult> {
    let repo = Repository::open_with_key(ovc_path, keypair)?;
    export_repo(&repo, output_dir)
}

/// Performs the actual export given an already-opened `repo`.
fn export_repo(repo: &Repository, output_dir: &Path) -> GitResult<ExportResult> {
    let git_dir = output_dir.join(".git");
    init_git_dir_structure(&git_dir)?;

    let mut oid_map = OidMap::new();
    let mut stats = ExportResult {
        blobs_exported: 0,
        trees_exported: 0,
        commits_exported: 0,
        tags_exported: 0,
        refs_exported: 0,
        oid_map: OidMap::new(),
    };

    let all_oids: Vec<ObjectId> = repo.object_store().ids().copied().collect();

    export_blobs(repo, &all_oids, &git_dir, &mut oid_map, &mut stats)?;
    export_trees(repo, &all_oids, &git_dir, &mut oid_map, &mut stats)?;
    export_commits(repo, &all_oids, &git_dir, &mut oid_map, &mut stats)?;
    export_tags(repo, &all_oids, &git_dir, &mut oid_map, &mut stats)?;
    export_refs(repo, &git_dir, &oid_map, &mut stats)?;
    checkout_head(repo, output_dir)?;

    stats.oid_map = oid_map;
    Ok(stats)
}

/// Creates the minimal `.git` directory structure.
fn init_git_dir_structure(git_dir: &Path) -> GitResult<()> {
    std::fs::create_dir_all(git_dir.join("objects"))?;
    std::fs::create_dir_all(git_dir.join("refs/heads"))?;
    std::fs::create_dir_all(git_dir.join("refs/tags"))?;
    std::fs::write(
        git_dir.join("config"),
        "[core]\n\trepositoryformatversion = 0\n\tfilemode = true\n\tbare = false\n",
    )?;
    Ok(())
}

/// Exports all blobs from the OVC store.
fn export_blobs(
    repo: &Repository,
    oids: &[ObjectId],
    git_dir: &Path,
    oid_map: &mut OidMap,
    stats: &mut ExportResult,
) -> GitResult<()> {
    for oid in oids {
        if let Some(Object::Blob(data)) = repo.get_object(oid)? {
            let git_sha1 = write_git::write_git_loose_object(git_dir, "blob", &data)?;
            oid_map.insert(&git_sha1, *oid);
            stats.blobs_exported += 1;
        }
    }
    Ok(())
}

/// Exports all trees, iterating until all dependencies are resolved.
fn export_trees(
    repo: &Repository,
    oids: &[ObjectId],
    git_dir: &Path,
    oid_map: &mut OidMap,
    stats: &mut ExportResult,
) -> GitResult<()> {
    let mut exported = HashSet::new();
    let mut progress = true;
    while progress {
        progress = false;
        for oid in oids {
            if exported.contains(oid) || oid_map.get_git(oid).is_some() {
                continue;
            }
            let Some(Object::Tree(tree)) = repo.get_object(oid)? else {
                continue;
            };
            let all_children_ready = tree
                .entries
                .iter()
                .all(|e| oid_map.get_git(&e.oid).is_some() || e.oid.is_zero());
            if !all_children_ready {
                continue;
            }
            let git_sha1 = write_tree_object(git_dir, &tree.entries, oid_map)?;
            oid_map.insert(&git_sha1, *oid);
            exported.insert(*oid);
            stats.trees_exported += 1;
            progress = true;
        }
    }
    Ok(())
}

/// Exports all commits, iterating until all dependencies are resolved.
fn export_commits(
    repo: &Repository,
    oids: &[ObjectId],
    git_dir: &Path,
    oid_map: &mut OidMap,
    stats: &mut ExportResult,
) -> GitResult<()> {
    let zero_sha1 = "0000000000000000000000000000000000000000";
    let mut exported = HashSet::new();
    let mut progress = true;
    while progress {
        progress = false;
        for oid in oids {
            if exported.contains(oid) || oid_map.get_git(oid).is_some() {
                continue;
            }
            let Some(Object::Commit(commit)) = repo.get_object(oid)? else {
                continue;
            };
            let tree_ready = oid_map.get_git(&commit.tree).is_some() || commit.tree.is_zero();
            let parents_ready = commit
                .parents
                .iter()
                .all(|p| oid_map.get_git(p).is_some() || p.is_zero());
            if !tree_ready || !parents_ready {
                continue;
            }
            let git_tree = oid_map
                .get_git(&commit.tree)
                .unwrap_or(zero_sha1)
                .to_owned();
            let git_parents: Vec<String> = commit
                .parents
                .iter()
                .filter_map(|p| oid_map.get_git(p).map(str::to_owned))
                .collect();
            let gc = GitCommit {
                tree: git_tree,
                parents: git_parents,
                author: format_git_identity(&commit.author),
                committer: format_git_identity(&commit.committer),
                message: commit.message.clone(),
            };
            let commit_data = write_git::serialize_git_commit(&gc);
            let git_sha1 = write_git::write_git_loose_object(git_dir, "commit", &commit_data)?;
            oid_map.insert(&git_sha1, *oid);
            exported.insert(*oid);
            stats.commits_exported += 1;
            progress = true;
        }
    }
    Ok(())
}

/// Exports all tag objects.
fn export_tags(
    repo: &Repository,
    oids: &[ObjectId],
    git_dir: &Path,
    oid_map: &mut OidMap,
    stats: &mut ExportResult,
) -> GitResult<()> {
    let zero_sha1 = "0000000000000000000000000000000000000000";
    for oid in oids {
        if oid_map.get_git(oid).is_some() {
            continue;
        }
        let Some(Object::Tag(tag)) = repo.get_object(oid)? else {
            continue;
        };
        let git_target = oid_map.get_git(&tag.target).unwrap_or(zero_sha1).to_owned();
        let target_type_str = match tag.target_type {
            ObjectType::Blob => "blob",
            ObjectType::Tree => "tree",
            ObjectType::Commit => "commit",
            ObjectType::Tag => "tag",
        };
        let gt = GitTag {
            object: git_target,
            target_type: target_type_str.to_owned(),
            tag_name: tag.tag_name.clone(),
            tagger: format_git_identity(&tag.tagger),
            message: tag.message.clone(),
        };
        let tag_data = write_git::serialize_git_tag(&gt);
        let git_sha1 = write_git::write_git_loose_object(git_dir, "tag", &tag_data)?;
        oid_map.insert(&git_sha1, *oid);
        stats.tags_exported += 1;
    }
    Ok(())
}

/// Converts OVC refs to git refs and writes them.
fn export_refs(
    repo: &Repository,
    git_dir: &Path,
    oid_map: &OidMap,
    stats: &mut ExportResult,
) -> GitResult<()> {
    let mut git_refs = BTreeMap::new();

    for (branch_name, ovc_id) in repo.ref_store().list_branches() {
        if let Some(git_sha1) = oid_map.get_git(ovc_id) {
            git_refs.insert(format!("refs/heads/{branch_name}"), git_sha1.to_owned());
            stats.refs_exported += 1;
        }
    }

    for (tag_name, ovc_id, _msg) in repo.ref_store().list_tags() {
        if let Some(git_sha1) = oid_map.get_git(ovc_id) {
            git_refs.insert(format!("refs/tags/{tag_name}"), git_sha1.to_owned());
            stats.refs_exported += 1;
        }
    }

    let head_ref = match repo.ref_store().head() {
        RefTarget::Symbolic(sym) => sym.clone(),
        RefTarget::Direct(oid) => oid_map
            .get_git(oid)
            .unwrap_or("0000000000000000000000000000000000000000")
            .to_owned(),
    };

    git_refs::write_git_refs(git_dir, &git_refs, &head_ref)?;
    Ok(())
}

/// Writes a single OVC tree as a git tree object, returning the git SHA1.
fn write_tree_object(git_dir: &Path, entries: &[TreeEntry], oid_map: &OidMap) -> GitResult<String> {
    let zero_sha1 = "0000000000000000000000000000000000000000";
    let mut git_entries: Vec<(u32, Vec<u8>, Vec<u8>)> = Vec::with_capacity(entries.len());

    for entry in entries {
        let mode = ovc_mode_to_git(entry.mode);
        let git_sha1_hex = oid_map.get_git(&entry.oid).unwrap_or(zero_sha1);
        let sha1_bytes = hex::decode(git_sha1_hex).unwrap_or_else(|_| vec![0u8; 20]);
        git_entries.push((mode, sha1_bytes, entry.name.clone()));
    }

    // Sort by name using git's tree entry ordering: directory names are compared
    // as if they have a trailing '/' appended (see git tree-object format spec).
    git_entries.sort_by(|a, b| {
        let name_a = if a.0 == 0o40_000 {
            [a.2.as_slice(), b"/"].concat()
        } else {
            a.2.clone()
        };
        let name_b = if b.0 == 0o40_000 {
            [b.2.as_slice(), b"/"].concat()
        } else {
            b.2.clone()
        };
        name_a.cmp(&name_b)
    });

    let refs: Vec<(u32, &[u8], &[u8])> = git_entries
        .iter()
        .map(|(m, s, n)| (*m, s.as_slice(), n.as_slice()))
        .collect();

    let tree_data = write_git::serialize_git_tree(&refs);
    write_git::write_git_loose_object(git_dir, "tree", &tree_data)
}

/// Maps an OVC `FileMode` to a git mode integer.
const fn ovc_mode_to_git(mode: FileMode) -> u32 {
    match mode {
        FileMode::Regular => 0o100_644,
        FileMode::Executable => 0o100_755,
        FileMode::Symlink => 0o120_000,
        FileMode::Directory => 0o40_000,
        FileMode::Subrepository => 0o160_000,
    }
}

/// Checks out the HEAD tree into the working directory and synchronises the git
/// index so that `git status` reports a clean tree.
fn checkout_head(repo: &Repository, output_dir: &Path) -> GitResult<()> {
    let Ok(head_oid) = repo.ref_store().resolve_head() else {
        return Ok(());
    };

    let Some(Object::Commit(commit)) = repo.get_object(&head_oid)? else {
        return Ok(());
    };

    checkout_tree(repo, &commit.tree, output_dir)?;

    // Synchronise the git index with the HEAD tree so that `git status` is clean.
    // We use `git read-tree HEAD` to populate the index from the tree, then
    // `git checkout-index -a` would normally write files, but we already wrote
    // them above, so `read-tree` alone suffices to make the index consistent.
    let git_dir = output_dir.join(".git");
    if git_dir.is_dir() {
        let _ = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(output_dir)
            .args(["read-tree", "HEAD"])
            .output();
        // Also run checkout-index to ensure file timestamps match the index.
        let _ = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&git_dir)
            .arg("--work-tree")
            .arg(output_dir)
            .args(["checkout-index", "-a", "-f"])
            .output();
    }

    Ok(())
}

/// Recursively writes a tree's files to disk.
fn checkout_tree(repo: &Repository, tree_oid: &ObjectId, dir: &Path) -> GitResult<()> {
    let Some(Object::Tree(tree)) = repo.get_object(tree_oid)? else {
        return Ok(());
    };

    for entry in &tree.entries {
        let name = String::from_utf8_lossy(&entry.name);
        let path = dir.join(name.as_ref());

        match entry.mode {
            FileMode::Directory => {
                std::fs::create_dir_all(&path)?;
                checkout_tree(repo, &entry.oid, &path)?;
            }
            FileMode::Regular | FileMode::Executable => {
                if let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? {
                    if let Some(parent) = path.parent() {
                        std::fs::create_dir_all(parent)?;
                    }
                    std::fs::write(&path, &data)?;
                    #[cfg(unix)]
                    if entry.mode == FileMode::Executable {
                        use std::os::unix::fs::PermissionsExt;
                        let perms = std::fs::Permissions::from_mode(0o755);
                        std::fs::set_permissions(&path, perms)?;
                    }
                }
            }
            FileMode::Symlink => {
                if let Some(Object::Blob(data)) = repo.get_object(&entry.oid)? {
                    let target = String::from_utf8_lossy(&data);
                    #[cfg(unix)]
                    {
                        let _ = std::os::unix::fs::symlink(target.as_ref(), &path);
                    }
                    #[cfg(not(unix))]
                    {
                        std::fs::write(&path, &data)?;
                    }
                }
            }
            FileMode::Subrepository => {
                // Skip submodules.
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::import;

    /// Round-trip test: create git repo -> import to OVC -> export back to git.
    #[test]
    fn roundtrip_import_export() {
        let dir = tempfile::tempdir().unwrap();

        // Create a git repo.
        let git_root = dir.path().join("git-repo");
        let git_dir = git_root.join(".git");
        std::fs::create_dir_all(git_dir.join("objects")).unwrap();
        std::fs::create_dir_all(git_dir.join("refs/heads")).unwrap();

        // Write a blob.
        let blob_sha1 =
            write_git::write_git_loose_object(&git_dir, "blob", b"hello world\n").unwrap();

        // Write a tree.
        let blob_bytes = hex::decode(&blob_sha1).unwrap();
        let tree_data =
            write_git::serialize_git_tree(&[(0o100_644, &blob_bytes, b"hello.txt".as_slice())]);
        let tree_sha1 = write_git::write_git_loose_object(&git_dir, "tree", &tree_data).unwrap();

        // Write a commit.
        let gc = crate::git_objects::GitCommit {
            tree: tree_sha1,
            parents: vec![],
            author: "Test <test@example.com> 1700000000 +0000".into(),
            committer: "Test <test@example.com> 1700000000 +0000".into(),
            message: "Initial commit".into(),
        };
        let commit_data = write_git::serialize_git_commit(&gc);
        let commit_sha1 =
            write_git::write_git_loose_object(&git_dir, "commit", &commit_data).unwrap();

        // Set refs.
        let mut refs = BTreeMap::new();
        refs.insert("refs/heads/main".to_owned(), commit_sha1.clone());
        git_refs::write_git_refs(&git_dir, &refs, "refs/heads/main").unwrap();

        // Import to OVC.
        let ovc_path = dir.path().join("test.ovc");
        let import_result = import::import_git_repo(&git_root, &ovc_path, b"pw").unwrap();
        assert_eq!(import_result.blobs_imported, 1);

        // Export back to git.
        let export_dir = dir.path().join("exported");
        std::fs::create_dir_all(&export_dir).unwrap();
        let export_result = export_to_git(&ovc_path, &export_dir, b"pw").unwrap();
        assert_eq!(export_result.blobs_exported, 1);
        assert_eq!(export_result.trees_exported, 1);
        assert_eq!(export_result.commits_exported, 1);

        // Verify the blob SHA1 matches the original.
        let exported_blob_sha1 = export_result
            .oid_map
            .get_git(import_result.oid_map.get_ovc(&blob_sha1).unwrap())
            .unwrap();
        assert_eq!(exported_blob_sha1, &blob_sha1);

        // Verify working directory was checked out.
        let hello_path = export_dir.join("hello.txt");
        assert!(hello_path.exists());
        assert_eq!(
            std::fs::read_to_string(&hello_path).unwrap(),
            "hello world\n"
        );

        // Verify the exported git repo has valid refs.
        let export_git_dir = export_dir.join(".git");
        let exported_refs = git_refs::read_git_refs(&export_git_dir).unwrap();
        assert!(exported_refs.contains_key("refs/heads/main"));
    }
}
