//! Git-to-OVC repository import.
//!
//! Walks a git repository's object graph (BFS from all refs), converts each
//! object to its OVC equivalent, and produces an encrypted `.ovc` file.

use std::collections::{HashSet, VecDeque};
use std::path::Path;

use ovc_core::id::ObjectId;
use ovc_core::keys::OvcKeyPair;
use ovc_core::object::{Commit, FileMode, Identity, Object, ObjectType, Tag, Tree, TreeEntry};
use ovc_core::refs::RefTarget;
use ovc_core::repository::Repository;

use crate::error::{GitError, GitResult};
use crate::git_objects::{self, GitObject, GitTreeEntry, parse_git_identity};
use crate::git_refs;
use crate::oid_map::OidMap;

/// Statistics returned after a successful import.
pub struct ImportResult {
    /// Number of blob objects imported.
    pub blobs_imported: u64,
    /// Number of tree objects imported.
    pub trees_imported: u64,
    /// Number of commit objects imported.
    pub commits_imported: u64,
    /// Number of tag objects imported.
    pub tags_imported: u64,
    /// Number of references imported.
    pub refs_imported: u64,
    /// The bidirectional OID mapping produced during import.
    pub oid_map: OidMap,
}

/// Imports a git repository into a new OVC repository (password-based encryption).
///
/// `git_repo_path` should be the root of a git working tree (containing `.git`)
/// or a bare repository (the `.git` directory itself).
///
/// `ovc_path` is where the `.ovc` file will be created.
pub fn import_git_repo(
    git_repo_path: &Path,
    ovc_path: &Path,
    password: &[u8],
) -> GitResult<ImportResult> {
    let git_dir = locate_git_dir(git_repo_path)?;
    let (refs, ordered) = prepare_import(&git_dir)?;
    let repo = Repository::init(ovc_path, password)?;
    finish_import(repo, refs, ordered)
}

/// Imports a git repository into a new OVC repository (key-based encryption).
///
/// Uses the provided `keypair` to encrypt the new `.ovc` file instead of a
/// password. All other semantics are identical to [`import_git_repo`].
pub fn import_git_repo_with_key(
    git_repo_path: &Path,
    ovc_path: &Path,
    keypair: &OvcKeyPair,
) -> GitResult<ImportResult> {
    let git_dir = locate_git_dir(git_repo_path)?;
    let (refs, ordered) = prepare_import(&git_dir)?;
    let repo = Repository::init_with_key(ovc_path, keypair)?;
    finish_import(repo, refs, ordered)
}

type RefMap = std::collections::BTreeMap<String, String>;
type OrderedObjects = Vec<(String, GitObject)>;

/// Shared preparation step: locate the git dir, read refs, and BFS objects.
fn prepare_import(git_dir: &Path) -> GitResult<(RefMap, OrderedObjects)> {
    let refs = git_refs::read_git_refs(git_dir)?;

    let ref_targets: Vec<String> = refs
        .iter()
        .filter(|(k, _)| *k != "HEAD_SYMBOLIC")
        .map(|(_, v)| v.clone())
        .collect();

    let ordered = bfs_objects(git_dir, &ref_targets)?;
    Ok((refs, ordered))
}

/// Shared completion step: populate a freshly-created `repo` from the
/// already-parsed git objects and refs, then save.
#[allow(clippy::needless_pass_by_value)]
fn finish_import(
    mut repo: Repository,
    refs: RefMap,
    ordered: OrderedObjects,
) -> GitResult<ImportResult> {
    let mut oid_map = OidMap::new();
    let mut stats = ImportResult {
        blobs_imported: 0,
        trees_imported: 0,
        commits_imported: 0,
        tags_imported: 0,
        refs_imported: 0,
        oid_map: OidMap::new(),
    };

    // Process objects in topological order (blobs first, then trees, then commits/tags).
    for (sha1, obj) in &ordered {
        match obj {
            GitObject::Blob(data) => {
                let ovc_obj = Object::Blob(data.clone());
                let ovc_id = repo.insert_object(&ovc_obj)?;
                oid_map.insert(sha1, ovc_id);
                stats.blobs_imported += 1;
            }
            GitObject::Tree(entries) => {
                let ovc_tree = convert_tree(entries, &oid_map);
                let ovc_obj = Object::Tree(ovc_tree);
                let ovc_id = repo.insert_object(&ovc_obj)?;
                oid_map.insert(sha1, ovc_id);
                stats.trees_imported += 1;
            }
            GitObject::Commit(gc) => {
                let ovc_commit = convert_commit(gc, &oid_map);
                let ovc_obj = Object::Commit(ovc_commit);
                let ovc_id = repo.insert_object(&ovc_obj)?;
                oid_map.insert(sha1, ovc_id);
                stats.commits_imported += 1;
            }
            GitObject::Tag(gt) => {
                let ovc_tag = convert_tag(gt, &oid_map);
                let ovc_obj = Object::Tag(ovc_tag);
                let ovc_id = repo.insert_object(&ovc_obj)?;
                oid_map.insert(sha1, ovc_id);
                stats.tags_imported += 1;
            }
        }
    }

    // Convert refs.
    let head_branch = git_refs::head_branch_from_refs(&refs);

    // Create a default identity for reflog entries.
    let default_identity = Identity {
        name: "git-import".into(),
        email: "git-import@ovc".into(),
        timestamp: 0,
        tz_offset_minutes: 0,
    };

    for (ref_name, sha1) in &refs {
        if ref_name == "HEAD" || ref_name == "HEAD_SYMBOLIC" {
            continue;
        }
        if let Some(ovc_id) = oid_map.get_ovc(sha1) {
            if let Some(branch) = ref_name.strip_prefix("refs/heads/") {
                repo.ref_store_mut().set_branch(
                    branch,
                    *ovc_id,
                    &default_identity,
                    &format!("import from git: {ref_name}"),
                )?;
                stats.refs_imported += 1;
            } else if let Some(tag) = ref_name.strip_prefix("refs/tags/") {
                // Lightweight tags — ignore error if already exists.
                let _ = repo.ref_store_mut().create_tag(tag, *ovc_id, None);
                stats.refs_imported += 1;
            }
        }
    }

    // Set HEAD to point to the default branch.
    repo.ref_store_mut()
        .set_head(RefTarget::Symbolic(format!("refs/heads/{head_branch}")));

    repo.save()?;

    stats.oid_map = oid_map;
    Ok(stats)
}

/// Locates the `.git` directory from a repository path.
fn locate_git_dir(path: &Path) -> GitResult<std::path::PathBuf> {
    // If the path itself contains HEAD, it might be a bare repo.
    if path.join("HEAD").is_file() && path.join("objects").is_dir() {
        return Ok(path.to_path_buf());
    }
    let dot_git = path.join(".git");
    if dot_git.is_dir() && dot_git.join("HEAD").is_file() {
        return Ok(dot_git);
    }
    Err(GitError::NotAGitRepo(path.to_path_buf()))
}

/// BFS from all ref targets, collecting objects in dependency order.
///
/// Returns objects ordered such that dependencies come before dependents:
/// blobs first, then trees, then commits/tags.
fn bfs_objects(git_dir: &Path, roots: &[String]) -> GitResult<Vec<(String, GitObject)>> {
    let mut visited = HashSet::new();
    let mut queue = VecDeque::new();

    let mut blobs = Vec::new();
    let mut trees = Vec::new();
    let mut commits = Vec::new();
    let mut tags = Vec::new();

    for root in roots {
        if visited.insert(root.clone()) {
            queue.push_back(root.clone());
        }
    }

    while let Some(sha1) = queue.pop_front() {
        let obj = match git_objects::read_git_object(git_dir, &sha1) {
            Ok(o) => o,
            Err(GitError::ObjectNotFound(_)) => continue,
            Err(e) => return Err(e),
        };

        match &obj {
            GitObject::Blob(_) => {
                blobs.push((sha1, obj));
            }
            GitObject::Tree(entries) => {
                for entry in entries {
                    let child_sha1 = hex::encode(entry.sha1);
                    if visited.insert(child_sha1.clone()) {
                        queue.push_back(child_sha1);
                    }
                }
                trees.push((sha1, obj));
            }
            GitObject::Commit(gc) => {
                // Enqueue tree.
                if visited.insert(gc.tree.clone()) {
                    queue.push_back(gc.tree.clone());
                }
                // Enqueue parents.
                for parent in &gc.parents {
                    if visited.insert(parent.clone()) {
                        queue.push_back(parent.clone());
                    }
                }
                commits.push((sha1, obj));
            }
            GitObject::Tag(gt) => {
                if visited.insert(gt.object.clone()) {
                    queue.push_back(gt.object.clone());
                }
                tags.push((sha1, obj));
            }
        }
    }

    // Dependency order: blobs -> trees -> commits -> tags.
    // BFS discovers objects from tips toward roots, so children may appear
    // before their dependencies. Topologically sort commits so parents are
    // always processed before children.
    topo_sort_commits(&mut commits);
    // Trees with subtrees also need dependency ordering.
    topo_sort_trees(&mut trees);
    let mut result = Vec::with_capacity(blobs.len() + trees.len() + commits.len() + tags.len());
    result.extend(blobs);
    result.extend(trees);
    result.extend(commits);
    result.extend(tags);
    Ok(result)
}

/// Topologically sorts commits so that parent commits appear before children.
///
/// Uses iterative Kahn's algorithm: commits with no unresolved parents are
/// emitted first, then their children become eligible.
fn topo_sort_commits(commits: &mut Vec<(String, GitObject)>) {
    use std::collections::{HashMap, VecDeque};

    let sha1_set: HashSet<String> = commits.iter().map(|(s, _)| s.clone()).collect();
    // For each commit, count how many parents are in our set.
    let mut in_degree: Vec<usize> = vec![0; commits.len()];
    // Map from parent SHA1 -> list of child indices.
    let mut children: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, (_, obj)) in commits.iter().enumerate() {
        if let GitObject::Commit(gc) = obj {
            for parent in &gc.parents {
                if sha1_set.contains(parent) {
                    in_degree[i] += 1;
                    children.entry(parent.clone()).or_default().push(i);
                }
            }
        }
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut sorted_indices = Vec::with_capacity(commits.len());
    while let Some(idx) = queue.pop_front() {
        sorted_indices.push(idx);
        let sha1 = &commits[idx].0;
        if let Some(child_indices) = children.get(sha1) {
            for &child_idx in child_indices {
                in_degree[child_idx] -= 1;
                if in_degree[child_idx] == 0 {
                    queue.push_back(child_idx);
                }
            }
        }
    }

    // If there's a cycle or disconnected nodes, append any remaining.
    if sorted_indices.len() < commits.len() {
        let in_sorted: HashSet<usize> = sorted_indices.iter().copied().collect();
        for i in 0..commits.len() {
            if !in_sorted.contains(&i) {
                sorted_indices.push(i);
            }
        }
    }

    // Rearrange commits according to sorted order.
    let taken: Vec<(String, GitObject)> = std::mem::take(commits);
    let mut indexed: Vec<Option<(String, GitObject)>> = taken.into_iter().map(Some).collect();
    for i in sorted_indices {
        if let Some(item) = indexed[i].take() {
            commits.push(item);
        }
    }
}

/// Topologically sorts trees so that leaf trees (no subtree references in
/// our set) appear before parent trees.
fn topo_sort_trees(trees: &mut Vec<(String, GitObject)>) {
    use std::collections::{HashMap, VecDeque};

    let sha1_set: HashSet<String> = trees.iter().map(|(s, _)| s.clone()).collect();
    let mut in_degree: Vec<usize> = vec![0; trees.len()];
    let mut children: HashMap<String, Vec<usize>> = HashMap::new();

    for (i, (_, obj)) in trees.iter().enumerate() {
        if let GitObject::Tree(entries) = obj {
            for entry in entries {
                let child_sha1 = hex::encode(entry.sha1);
                if sha1_set.contains(&child_sha1) {
                    in_degree[i] += 1;
                    children.entry(child_sha1).or_default().push(i);
                }
            }
        }
    }

    let mut queue: VecDeque<usize> = VecDeque::new();
    for (i, &deg) in in_degree.iter().enumerate() {
        if deg == 0 {
            queue.push_back(i);
        }
    }

    let mut sorted_indices = Vec::with_capacity(trees.len());
    while let Some(idx) = queue.pop_front() {
        sorted_indices.push(idx);
        let sha1 = &trees[idx].0;
        if let Some(child_indices) = children.get(sha1) {
            for &child_idx in child_indices {
                in_degree[child_idx] -= 1;
                if in_degree[child_idx] == 0 {
                    queue.push_back(child_idx);
                }
            }
        }
    }

    if sorted_indices.len() < trees.len() {
        let in_sorted: HashSet<usize> = sorted_indices.iter().copied().collect();
        for i in 0..trees.len() {
            if !in_sorted.contains(&i) {
                sorted_indices.push(i);
            }
        }
    }

    let taken: Vec<(String, GitObject)> = std::mem::take(trees);
    let mut indexed: Vec<Option<(String, GitObject)>> = taken.into_iter().map(Some).collect();
    for i in sorted_indices {
        if let Some(item) = indexed[i].take() {
            trees.push(item);
        }
    }
}

/// Converts git tree entries to an OVC `Tree`.
fn convert_tree(entries: &[GitTreeEntry], oid_map: &OidMap) -> Tree {
    let mut ovc_entries = Vec::with_capacity(entries.len());
    for entry in entries {
        let git_sha1 = hex::encode(entry.sha1);
        let ovc_id = oid_map
            .get_ovc(&git_sha1)
            .copied()
            .unwrap_or(ObjectId::ZERO);

        let mode = git_mode_to_ovc(entry.mode);

        ovc_entries.push(TreeEntry {
            mode,
            name: entry.name.clone(),
            oid: ovc_id,
        });
    }

    let mut tree = Tree {
        entries: ovc_entries,
    };
    tree.canonicalize();
    tree
}

/// Converts a git commit to an OVC `Commit`.
fn convert_commit(gc: &git_objects::GitCommit, oid_map: &OidMap) -> Commit {
    let tree_ovc = oid_map.get_ovc(&gc.tree).copied().unwrap_or(ObjectId::ZERO);

    let parents: Vec<ObjectId> = gc
        .parents
        .iter()
        .filter_map(|p| oid_map.get_ovc(p).copied())
        .collect();

    let author = parse_git_identity(&gc.author).unwrap_or_else(|_| Identity {
        name: gc.author.clone(),
        email: String::new(),
        timestamp: 0,
        tz_offset_minutes: 0,
    });

    let committer = parse_git_identity(&gc.committer).unwrap_or_else(|_| Identity {
        name: gc.committer.clone(),
        email: String::new(),
        timestamp: 0,
        tz_offset_minutes: 0,
    });

    Commit {
        tree: tree_ovc,
        parents,
        author,
        committer,
        message: gc.message.clone(),
        signature: None,
        sequence: 0,
    }
}

/// Converts a git tag to an OVC `Tag`.
fn convert_tag(gt: &git_objects::GitTag, oid_map: &OidMap) -> Tag {
    let target = oid_map
        .get_ovc(&gt.object)
        .copied()
        .unwrap_or(ObjectId::ZERO);

    let target_type = match gt.target_type.as_str() {
        "blob" => ObjectType::Blob,
        "tree" => ObjectType::Tree,
        "tag" => ObjectType::Tag,
        // "commit" and any unrecognized type default to Commit.
        _ => ObjectType::Commit,
    };

    let tagger = parse_git_identity(&gt.tagger).unwrap_or_else(|_| Identity {
        name: gt.tagger.clone(),
        email: String::new(),
        timestamp: 0,
        tz_offset_minutes: 0,
    });

    Tag {
        target,
        target_type,
        tag_name: gt.tag_name.clone(),
        tagger,
        message: gt.message.clone(),
        signature: None,
    }
}

/// Maps a git mode integer to an OVC `FileMode`.
const fn git_mode_to_ovc(mode: u32) -> FileMode {
    match mode {
        0o100_755 => FileMode::Executable,
        0o120_000 => FileMode::Symlink,
        0o40_000 => FileMode::Directory,
        0o160_000 => FileMode::Subrepository,
        _ => FileMode::Regular, // 100644 and others
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::write_git;

    /// Creates a minimal git repository in a temp directory and imports it.
    #[test]
    fn import_minimal_git_repo() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(git_dir.join("objects")).unwrap();
        std::fs::create_dir_all(git_dir.join("refs/heads")).unwrap();

        // Write a blob.
        let blob_sha1 =
            write_git::write_git_loose_object(&git_dir, "blob", b"hello world\n").unwrap();

        // Write a tree containing the blob.
        let blob_sha1_bytes = hex::decode(&blob_sha1).unwrap();
        let tree_data =
            write_git::serialize_git_tree(&[(0o100_644, &blob_sha1_bytes, b"hello.txt".as_slice())]);
        let tree_sha1 = write_git::write_git_loose_object(&git_dir, "tree", &tree_data).unwrap();

        // Write a commit.
        let commit = git_objects::GitCommit {
            tree: tree_sha1,
            parents: vec![],
            author: "Test User <test@example.com> 1700000000 +0000".into(),
            committer: "Test User <test@example.com> 1700000000 +0000".into(),
            message: "Initial commit".into(),
        };
        let commit_data = write_git::serialize_git_commit(&commit);
        let commit_sha1 =
            write_git::write_git_loose_object(&git_dir, "commit", &commit_data).unwrap();

        // Write refs.
        let mut refs = std::collections::BTreeMap::new();
        refs.insert("refs/heads/main".to_owned(), commit_sha1.clone());
        git_refs::write_git_refs(&git_dir, &refs, "refs/heads/main").unwrap();

        // Import.
        let ovc_path = dir.path().join("test.ovc");
        let result = import_git_repo(dir.path(), &ovc_path, b"testpw").unwrap();

        assert_eq!(result.blobs_imported, 1);
        assert_eq!(result.trees_imported, 1);
        assert_eq!(result.commits_imported, 1);
        assert_eq!(result.refs_imported, 1);

        // Verify the OVC repo can be opened and contains the objects.
        let repo = Repository::open(&ovc_path, b"testpw").unwrap();
        assert!(repo.object_count() >= 3); // blob + tree + commit

        // Verify ref was set.
        let head_oid = repo.ref_store().resolve_head().unwrap();
        let expected_ovc_commit = result.oid_map.get_ovc(&commit_sha1).unwrap();
        assert_eq!(head_oid, *expected_ovc_commit);
    }
}
