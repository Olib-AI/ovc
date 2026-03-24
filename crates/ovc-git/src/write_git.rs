//! Git object writing utilities.
//!
//! Writes loose git objects (zlib-deflated) and serializes trees, commits,
//! and tags into git's on-disk format.

use std::fmt::Write as FmtWrite;
use std::io::Write;
use std::path::Path;

use flate2::Compression;
use flate2::write::ZlibEncoder;
use sha1::{Digest, Sha1};

use crate::error::GitResult;
use crate::git_objects::{GitCommit, GitTag};

/// Writes a loose git object and returns its SHA1 hex string.
///
/// The object is stored at `<git_dir>/objects/<xx>/<yy...>` as zlib-compressed
/// `"<type> <size>\0<data>"`.
pub fn write_git_loose_object(git_dir: &Path, object_type: &str, data: &[u8]) -> GitResult<String> {
    let header = format!("{object_type} {}\0", data.len());

    // Compute SHA1.
    let mut hasher = Sha1::new();
    hasher.update(header.as_bytes());
    hasher.update(data);
    let sha1_hex = hex::encode(hasher.finalize());

    let (prefix, suffix) = sha1_hex.split_at(2);
    let obj_dir = git_dir.join("objects").join(prefix);
    std::fs::create_dir_all(&obj_dir)?;

    let obj_path = obj_dir.join(suffix);
    if obj_path.exists() {
        // Object already exists (content-addressable dedup).
        return Ok(sha1_hex);
    }

    // Zlib compress header + data.
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(header.as_bytes())?;
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;

    std::fs::write(&obj_path, &compressed)?;
    Ok(sha1_hex)
}

/// Serializes tree entries into git's binary tree format.
///
/// Each entry tuple is `(mode, sha1_raw_20_bytes, name_bytes)`.
#[must_use]
pub fn serialize_git_tree(entries: &[(u32, &[u8], &[u8])]) -> Vec<u8> {
    let mut buf = Vec::new();
    for &(mode, sha1_raw, name) in entries {
        // Git uses the minimal octal representation.
        let mode_str = format!("{mode:o}");
        buf.extend_from_slice(mode_str.as_bytes());
        buf.push(b' ');
        buf.extend_from_slice(name);
        buf.push(0);
        buf.extend_from_slice(sha1_raw);
    }
    buf
}

/// Serializes a `GitCommit` into git's text commit format.
#[must_use]
pub fn serialize_git_commit(commit: &GitCommit) -> Vec<u8> {
    let mut buf = String::new();
    let _ = writeln!(buf, "tree {}", commit.tree);
    for parent in &commit.parents {
        let _ = writeln!(buf, "parent {parent}");
    }
    let _ = writeln!(buf, "author {}", commit.author);
    let _ = writeln!(buf, "committer {}", commit.committer);
    buf.push('\n');
    buf.push_str(&commit.message);
    buf.push('\n');
    buf.into_bytes()
}

/// Serializes a `GitTag` into git's text tag format.
#[must_use]
pub fn serialize_git_tag(tag: &GitTag) -> Vec<u8> {
    let mut buf = String::new();
    let _ = writeln!(buf, "object {}", tag.object);
    let _ = writeln!(buf, "type {}", tag.target_type);
    let _ = writeln!(buf, "tag {}", tag.tag_name);
    if !tag.tagger.is_empty() {
        let _ = writeln!(buf, "tagger {}", tag.tagger);
    }
    buf.push('\n');
    buf.push_str(&tag.message);
    buf.push('\n');
    buf.into_bytes()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_and_verify_blob() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(git_dir.join("objects")).unwrap();

        let sha1 = write_git_loose_object(&git_dir, "blob", b"hello").unwrap();
        assert_eq!(sha1, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");

        // Verify the object exists on disk.
        let obj_path = git_dir.join("objects/b6/fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
        assert!(obj_path.exists());

        // Read it back.
        let obj = crate::git_objects::read_git_object(&git_dir, &sha1).unwrap();
        match obj {
            crate::git_objects::GitObject::Blob(data) => assert_eq!(data, b"hello"),
            _ => panic!("expected blob"),
        }
    }

    #[test]
    fn serialize_tree_format() {
        let sha1_bytes: [u8; 20] = [
            0xb6, 0xfc, 0x4c, 0x62, 0x0b, 0x67, 0xd9, 0x5f, 0x95, 0x3a, 0x5c, 0x1c, 0x12, 0x30,
            0xaa, 0xab, 0x5d, 0xb5, 0xa1, 0xb0,
        ];
        let entries = vec![(0o100_644, sha1_bytes.as_slice(), b"hello.txt".as_slice())];
        let data = serialize_git_tree(&entries);

        // Parse it back.
        let parsed = crate::git_objects::parse_git_tree(&data).unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].mode, 0o100_644);
        assert_eq!(parsed[0].name, b"hello.txt");
        assert_eq!(parsed[0].sha1, sha1_bytes);
    }

    #[test]
    fn serialize_commit_format() {
        let commit = GitCommit {
            tree: "4b825dc642cb6eb9a060e54bf899d69f7cb46a00".into(),
            parents: vec![],
            author: "Test <test@example.com> 1700000000 +0000".into(),
            committer: "Test <test@example.com> 1700000000 +0000".into(),
            message: "Initial commit".into(),
        };
        let data = serialize_git_commit(&commit);
        let parsed = crate::git_objects::parse_git_commit(&data).unwrap();
        assert_eq!(parsed.tree, commit.tree);
        assert_eq!(parsed.message, commit.message);
    }

    #[test]
    fn dedup_existing_object() {
        let dir = tempfile::tempdir().unwrap();
        let git_dir = dir.path().join(".git");
        std::fs::create_dir_all(git_dir.join("objects")).unwrap();

        let sha1a = write_git_loose_object(&git_dir, "blob", b"dup").unwrap();
        let sha1b = write_git_loose_object(&git_dir, "blob", b"dup").unwrap();
        assert_eq!(sha1a, sha1b);
    }
}
