//! Git object parsing and SHA1 computation.
//!
//! Reads loose objects from `.git/objects/` (zlib-deflated) and parses
//! blob, tree, commit, and tag formats.

use std::io::Read;
use std::path::Path;

use flate2::read::ZlibDecoder;
use sha1::{Digest, Sha1};

use crate::error::{GitError, GitResult};

/// Maximum allowed decompressed git object size (256 MiB).
///
/// Prevents a malicious git repository from triggering unbounded memory
/// allocation during zlib decompression of a crafted loose object.
const MAX_GIT_OBJECT_SIZE: u64 = 256 * 1024 * 1024;

// ── Parsed git object types ──────────────────────────────────────────────

/// A single entry in a git tree object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTreeEntry {
    /// The Unix file mode as an octal integer (e.g. 100644, 40000).
    pub mode: u32,
    /// The entry name (file or directory name).
    pub name: Vec<u8>,
    /// The raw 20-byte SHA1 of the referenced object.
    pub sha1: [u8; 20],
}

/// A parsed git commit object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitCommit {
    /// SHA1 hex of the root tree.
    pub tree: String,
    /// SHA1 hex strings of parent commits.
    pub parents: Vec<String>,
    /// The author line (e.g. `"Name <email> timestamp tz"`).
    pub author: String,
    /// The committer line.
    pub committer: String,
    /// The commit message (everything after the blank line).
    pub message: String,
}

/// A parsed git tag object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitTag {
    /// SHA1 hex of the target object.
    pub object: String,
    /// Object type of the target (e.g. `"commit"`).
    pub target_type: String,
    /// Tag name.
    pub tag_name: String,
    /// Tagger line.
    pub tagger: String,
    /// Tag message.
    pub message: String,
}

/// A fully parsed git object.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitObject {
    /// Raw file content.
    Blob(Vec<u8>),
    /// Directory listing.
    Tree(Vec<GitTreeEntry>),
    /// Commit snapshot.
    Commit(GitCommit),
    /// Annotated tag.
    Tag(GitTag),
}

// ── Reading loose objects ────────────────────────────────────────────────

/// Reads and parses a git loose object from the object store.
///
/// The object is located at `<git_dir>/objects/<xx>/<yy...>` where `xx` is
/// the first two hex characters of the SHA1 and `yy...` is the remainder.
pub fn read_git_object(git_dir: &Path, sha1_hex: &str) -> GitResult<GitObject> {
    if sha1_hex.len() != 40 {
        return Err(GitError::ObjectNotFound(sha1_hex.to_owned()));
    }

    let (prefix, suffix) = sha1_hex.split_at(2);
    let object_path = git_dir.join("objects").join(prefix).join(suffix);

    if !object_path.exists() {
        return Err(GitError::ObjectNotFound(sha1_hex.to_owned()));
    }

    let compressed = std::fs::read(&object_path)?;
    let decoder = ZlibDecoder::new(&compressed[..]);
    let mut limited = decoder.take(MAX_GIT_OBJECT_SIZE + 1);
    let mut raw = Vec::new();
    limited
        .read_to_end(&mut raw)
        .map_err(|e| GitError::CorruptObject(format!("zlib decompress failed: {e}")))?;
    if raw.len() as u64 > MAX_GIT_OBJECT_SIZE {
        return Err(GitError::CorruptObject(format!(
            "decompressed git object exceeds {} MiB limit",
            MAX_GIT_OBJECT_SIZE / (1024 * 1024)
        )));
    }

    parse_raw_git_object(&raw)
}

/// Parses the decompressed content of a git object.
///
/// Format: `"<type> <size>\0<data>"`.
fn parse_raw_git_object(raw: &[u8]) -> GitResult<GitObject> {
    let null_pos = raw
        .iter()
        .position(|&b| b == 0)
        .ok_or_else(|| GitError::CorruptObject("missing null byte in header".into()))?;

    let header = std::str::from_utf8(&raw[..null_pos])
        .map_err(|e| GitError::CorruptObject(format!("non-UTF8 header: {e}")))?;

    let space_pos = header
        .find(' ')
        .ok_or_else(|| GitError::CorruptObject("missing space in header".into()))?;

    let obj_type = &header[..space_pos];
    let size_str = &header[space_pos + 1..];
    let declared_size: usize = size_str
        .parse()
        .map_err(|e| GitError::CorruptObject(format!("invalid size: {e}")))?;

    let data = &raw[null_pos + 1..];

    // Validate that the actual data length matches the declared size.
    // A mismatch indicates a corrupt or tampered object.
    if data.len() != declared_size {
        return Err(GitError::CorruptObject(format!(
            "object size mismatch: header declares {declared_size} bytes but data is {} bytes",
            data.len()
        )));
    }

    match obj_type {
        "blob" => Ok(GitObject::Blob(data.to_vec())),
        "tree" => parse_git_tree(data).map(GitObject::Tree),
        "commit" => parse_git_commit(data).map(GitObject::Commit),
        "tag" => parse_git_tag(data).map(GitObject::Tag),
        other => Err(GitError::UnsupportedObjectType(other.to_owned())),
    }
}

// ── Tree parsing ─────────────────────────────────────────────────────────

/// Parses git's binary tree format.
///
/// Each entry is `<mode_ascii> <name>\0<20-byte-sha1>`, repeated.
pub fn parse_git_tree(data: &[u8]) -> GitResult<Vec<GitTreeEntry>> {
    let mut entries = Vec::new();
    let mut pos = 0;

    while pos < data.len() {
        // Find the space between mode and name.
        let space = data[pos..]
            .iter()
            .position(|&b| b == b' ')
            .ok_or_else(|| GitError::CorruptObject("tree entry missing space".into()))?;

        let mode_str = std::str::from_utf8(&data[pos..pos + space])
            .map_err(|e| GitError::CorruptObject(format!("non-UTF8 mode: {e}")))?;

        let mode = u32::from_str_radix(mode_str, 8)
            .map_err(|e| GitError::CorruptObject(format!("invalid mode '{mode_str}': {e}")))?;

        pos += space + 1; // skip past the space

        // Find the null terminator after the name.
        let null = data[pos..]
            .iter()
            .position(|&b| b == 0)
            .ok_or_else(|| GitError::CorruptObject("tree entry missing null".into()))?;

        let name = data[pos..pos + null].to_vec();
        pos += null + 1; // skip past the null

        // Read 20-byte SHA1.
        if pos + 20 > data.len() {
            return Err(GitError::CorruptObject(
                "tree entry truncated at SHA1".into(),
            ));
        }
        let mut sha1 = [0u8; 20];
        sha1.copy_from_slice(&data[pos..pos + 20]);
        pos += 20;

        entries.push(GitTreeEntry { mode, name, sha1 });
    }

    Ok(entries)
}

// ── Commit parsing ───────────────────────────────────────────────────────

/// Parses a git commit object body.
///
/// Handles multi-line headers (such as `gpgsig`) where continuation
/// lines start with a space character.
pub fn parse_git_commit(data: &[u8]) -> GitResult<GitCommit> {
    let text = std::str::from_utf8(data)
        .map_err(|e| GitError::CorruptObject(format!("non-UTF8 commit: {e}")))?;

    let mut tree = String::new();
    let mut parents = Vec::new();
    let mut author = String::new();
    let mut committer = String::new();
    let mut message = String::new();

    let mut in_message = false;
    let mut in_multiline_header = false;

    for line in text.split('\n') {
        if in_message {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(line);
            continue;
        }

        // Continuation lines of multi-line headers (e.g. gpgsig) start with
        // a space. Skip them until a non-continuation line is found.
        if in_multiline_header {
            if line.starts_with(' ') {
                continue;
            }
            in_multiline_header = false;
        }

        if line.is_empty() {
            in_message = true;
            continue;
        }

        if let Some(rest) = line.strip_prefix("tree ") {
            tree = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("parent ") {
            parents.push(rest.to_owned());
        } else if let Some(rest) = line.strip_prefix("author ") {
            author = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("committer ") {
            committer = rest.to_string();
        } else {
            // Unknown header (gpgsig, mergetag, encoding, etc.).
            // These may have multi-line values with continuation lines
            // starting with a space on subsequent lines.
            in_multiline_header = true;
        }
    }

    // Strip trailing newline from message if present (git adds one).
    if message.ends_with('\n') {
        message.pop();
    }

    if tree.is_empty() {
        return Err(GitError::CorruptObject("commit missing tree header".into()));
    }

    Ok(GitCommit {
        tree,
        parents,
        author,
        committer,
        message,
    })
}

// ── Tag parsing ──────────────────────────────────────────────────────────

/// Parses a git tag object body.
pub fn parse_git_tag(data: &[u8]) -> GitResult<GitTag> {
    let text = std::str::from_utf8(data)
        .map_err(|e| GitError::CorruptObject(format!("non-UTF8 tag: {e}")))?;

    let mut object = String::new();
    let mut target_type = String::new();
    let mut tag_name = String::new();
    let mut tagger = String::new();
    let mut message = String::new();

    let mut in_message = false;

    for line in text.split('\n') {
        if in_message {
            if !message.is_empty() {
                message.push('\n');
            }
            message.push_str(line);
            continue;
        }

        if line.is_empty() {
            in_message = true;
            continue;
        }

        if let Some(rest) = line.strip_prefix("object ") {
            object = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("type ") {
            target_type = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("tag ") {
            tag_name = rest.to_string();
        } else if let Some(rest) = line.strip_prefix("tagger ") {
            tagger = rest.to_string();
        }
    }

    if message.ends_with('\n') {
        message.pop();
    }

    if object.is_empty() {
        return Err(GitError::CorruptObject("tag missing object header".into()));
    }

    Ok(GitTag {
        object,
        target_type,
        tag_name,
        tagger,
        message,
    })
}

// ── SHA1 computation ─────────────────────────────────────────────────────

/// Computes the git SHA1 for an object: `SHA1("<type> <size>\0<data>")`.
#[must_use]
pub fn compute_git_sha1(object_type: &str, data: &[u8]) -> String {
    let header = format!("{object_type} {}\0", data.len());
    let mut hasher = Sha1::new();
    hasher.update(header.as_bytes());
    hasher.update(data);
    hex::encode(hasher.finalize())
}

// ── Identity parsing helper ──────────────────────────────────────────────

/// Parses a git identity line like `"Name <email> 1700000000 +0000"` into
/// an OVC `Identity`.
pub fn parse_git_identity(line: &str) -> GitResult<ovc_core::object::Identity> {
    // Format: "Name With Spaces <email@example.com> 1700000000 +0000"
    let lt_pos = line
        .find('<')
        .ok_or_else(|| GitError::Encoding(format!("missing '<' in identity: {line}")))?;
    let gt_pos = line
        .find('>')
        .ok_or_else(|| GitError::Encoding(format!("missing '>' in identity: {line}")))?;

    let name = line[..lt_pos].trim().to_owned();
    let email = line[lt_pos + 1..gt_pos].to_owned();

    let remainder = line[gt_pos + 1..].trim();
    let mut parts = remainder.split_whitespace();

    let timestamp: i64 = parts.next().unwrap_or("0").parse().unwrap_or(0);

    let tz_str = parts.next().unwrap_or("+0000");
    let tz_offset_minutes = parse_tz_offset(tz_str);

    Ok(ovc_core::object::Identity {
        name,
        email,
        timestamp,
        tz_offset_minutes,
    })
}

/// Formats an OVC `Identity` into a git identity line.
#[must_use]
pub fn format_git_identity(id: &ovc_core::object::Identity) -> String {
    let sign = if id.tz_offset_minutes >= 0 { '+' } else { '-' };
    let abs_minutes = id.tz_offset_minutes.unsigned_abs();
    let hours = abs_minutes / 60;
    let mins = abs_minutes % 60;
    format!(
        "{} <{}> {} {sign}{hours:02}{mins:02}",
        id.name, id.email, id.timestamp
    )
}

/// Parses a timezone offset string like `"+0530"` or `"-0800"` into minutes.
fn parse_tz_offset(s: &str) -> i16 {
    if s.len() < 5 {
        return 0;
    }
    let sign: i16 = if s.starts_with('-') { -1 } else { 1 };
    let hours: i16 = s[1..3].parse().unwrap_or(0);
    let mins: i16 = s[3..5].parse().unwrap_or(0);
    sign * (hours * 60 + mins)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compute_sha1_known_value() {
        // `echo -n "hello" | git hash-object --stdin` yields
        // "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0"
        let sha = compute_git_sha1("blob", b"hello");
        assert_eq!(sha, "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0");
    }

    #[test]
    fn parse_tree_single_entry() {
        // Construct a single tree entry: "100644 hello.txt\0<20 bytes>"
        let mut data = Vec::new();
        data.extend_from_slice(b"100644 hello.txt\0");
        data.extend_from_slice(&[
            0xb6, 0xfc, 0x4c, 0x62, 0x0b, 0x67, 0xd9, 0x5f, 0x95, 0x3a, 0x5c, 0x1c, 0x12, 0x30,
            0xaa, 0xab, 0x5d, 0xb5, 0xa1, 0xb0,
        ]);

        let entries = parse_git_tree(&data).unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].mode, 0o100_644);
        assert_eq!(entries[0].name, b"hello.txt");
        assert_eq!(
            hex::encode(entries[0].sha1),
            "b6fc4c620b67d95f953a5c1c1230aaab5db5a1b0"
        );
    }

    #[test]
    fn parse_commit_basic() {
        let commit_data = b"tree 4b825dc642cb6eb9a060e54bf899d69f7cb46a00\n\
            parent aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
            author Test User <test@example.com> 1700000000 +0000\n\
            committer Test User <test@example.com> 1700000000 +0000\n\
            \n\
            Initial commit\n";

        let commit = parse_git_commit(commit_data).unwrap();
        assert_eq!(commit.tree, "4b825dc642cb6eb9a060e54bf899d69f7cb46a00");
        assert_eq!(commit.parents.len(), 1);
        assert_eq!(
            commit.parents[0],
            "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"
        );
        assert_eq!(commit.message, "Initial commit");
    }

    #[test]
    fn parse_commit_no_parent() {
        let commit_data = b"tree 4b825dc642cb6eb9a060e54bf899d69f7cb46a00\n\
            author Test User <test@example.com> 1700000000 +0000\n\
            committer Test User <test@example.com> 1700000000 +0000\n\
            \n\
            Root commit\n";

        let commit = parse_git_commit(commit_data).unwrap();
        assert!(commit.parents.is_empty());
        assert_eq!(commit.message, "Root commit");
    }

    #[test]
    fn parse_tag_basic() {
        let tag_data = b"object aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\n\
            type commit\n\
            tag v1.0\n\
            tagger Test User <test@example.com> 1700000000 +0000\n\
            \n\
            Release v1.0\n";

        let tag = parse_git_tag(tag_data).unwrap();
        assert_eq!(tag.object, "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");
        assert_eq!(tag.target_type, "commit");
        assert_eq!(tag.tag_name, "v1.0");
        assert_eq!(tag.message, "Release v1.0");
    }

    #[test]
    fn identity_roundtrip() {
        let id = ovc_core::object::Identity {
            name: "Alice Smith".into(),
            email: "alice@example.com".into(),
            timestamp: 1_700_000_000,
            tz_offset_minutes: -480,
        };
        let formatted = format_git_identity(&id);
        let parsed = parse_git_identity(&formatted).unwrap();
        assert_eq!(parsed.name, id.name);
        assert_eq!(parsed.email, id.email);
        assert_eq!(parsed.timestamp, id.timestamp);
        assert_eq!(parsed.tz_offset_minutes, id.tz_offset_minutes);
    }

    #[test]
    fn tz_offset_parsing() {
        assert_eq!(parse_tz_offset("+0000"), 0);
        assert_eq!(parse_tz_offset("+0530"), 330);
        assert_eq!(parse_tz_offset("-0800"), -480);
    }
}
