//! Search file contents within a commit tree.
//!
//! Walks the tree structure and applies a regex pattern to each blob,
//! returning all matching lines with their file paths and line numbers.

use crate::error::{CoreError, CoreResult};
use crate::id::ObjectId;
use crate::object::{FileMode, Object};
use crate::store::ObjectStore;

/// A single match found by `grep_tree`.
#[derive(Debug, Clone)]
pub struct GrepMatch {
    /// File path relative to the repository root.
    pub path: String,
    /// 1-based line number within the file.
    pub line_number: usize,
    /// The full text of the matching line.
    pub line: String,
}

/// Maximum number of grep matches returned to prevent unbounded response sizes.
///
/// A common pattern (e.g. `"e"`) could match millions of lines in a large
/// repository. Capping the result set protects against memory exhaustion and
/// ensures the API response stays reasonably sized.
const MAX_GREP_MATCHES: usize = 10_000;

/// Searches all blobs in the given tree for lines matching `pattern`.
///
/// When `is_regex` is false, `pattern` is treated as a literal string and
/// special regex characters are escaped automatically. When `true`, it is
/// compiled as a regular expression (returning an error if invalid).
///
/// When `case_insensitive` is true, the pattern is compiled with the `(?i)` flag.
///
/// When `file_pattern` is provided, only files matching the glob are searched.
///
/// Results are capped at [`MAX_GREP_MATCHES`] to prevent unbounded memory use.
pub fn grep_tree(
    pattern: &str,
    tree_oid: &ObjectId,
    store: &ObjectStore,
    case_insensitive: bool,
) -> CoreResult<Vec<GrepMatch>> {
    grep_tree_filtered(pattern, tree_oid, store, case_insensitive, true, None)
}

/// Extended grep with regex mode toggle and file pattern filter.
pub fn grep_tree_filtered(
    pattern: &str,
    tree_oid: &ObjectId,
    store: &ObjectStore,
    case_insensitive: bool,
    is_regex: bool,
    file_pattern: Option<&str>,
) -> CoreResult<Vec<GrepMatch>> {
    let escaped;
    let effective_pattern = if is_regex {
        pattern
    } else {
        escaped = regex::escape(pattern);
        &escaped
    };

    let re_pattern = if case_insensitive {
        format!("(?i){effective_pattern}")
    } else {
        effective_pattern.to_owned()
    };

    let re = regex::Regex::new(&re_pattern).map_err(|e| CoreError::FormatError {
        reason: format!("invalid regex pattern: {e}"),
    })?;

    // Compile glob filter if provided.
    let glob_matcher = match file_pattern {
        Some(pat) if !pat.is_empty() => {
            let glob = globset::GlobBuilder::new(pat)
                .literal_separator(false)
                .build()
                .map_err(|e| CoreError::FormatError {
                    reason: format!("invalid file pattern: {e}"),
                })?
                .compile_matcher();
            Some(glob)
        }
        _ => None,
    };

    let mut matches = Vec::new();
    grep_tree_recursive(
        tree_oid,
        "",
        store,
        &re,
        glob_matcher.as_ref(),
        &mut matches,
    )?;
    Ok(matches)
}

/// Recursively walks tree entries and searches blobs.
///
/// Stops early once [`MAX_GREP_MATCHES`] results have been collected.
/// When `glob_matcher` is provided, only files matching the glob are searched.
fn grep_tree_recursive(
    tree_oid: &ObjectId,
    prefix: &str,
    store: &ObjectStore,
    re: &regex::Regex,
    glob_matcher: Option<&globset::GlobMatcher>,
    matches: &mut Vec<GrepMatch>,
) -> CoreResult<()> {
    if matches.len() >= MAX_GREP_MATCHES {
        return Ok(());
    }

    let obj = store
        .get(tree_oid)?
        .ok_or(CoreError::ObjectNotFound(*tree_oid))?;

    let Object::Tree(tree) = obj else {
        return Err(CoreError::CorruptObject {
            reason: format!("expected tree object at {tree_oid}"),
        });
    };

    for entry in &tree.entries {
        if matches.len() >= MAX_GREP_MATCHES {
            return Ok(());
        }

        let name = String::from_utf8_lossy(&entry.name);
        let full_path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix}/{name}")
        };

        if entry.mode == FileMode::Directory {
            grep_tree_recursive(&entry.oid, &full_path, store, re, glob_matcher, matches)?;
        } else {
            // Skip files that don't match the glob filter.
            if let Some(matcher) = glob_matcher
                && !matcher.is_match(&full_path)
            {
                continue;
            }

            let Some(Object::Blob(data)) = store.get(&entry.oid)? else {
                continue;
            };

            // Skip binary files.
            if crate::diff::is_binary(&data) {
                continue;
            }

            let text = String::from_utf8_lossy(&data);
            for (line_num, line) in text.lines().enumerate() {
                if matches.len() >= MAX_GREP_MATCHES {
                    return Ok(());
                }
                if re.is_match(line) {
                    matches.push(GrepMatch {
                        path: full_path.clone(),
                        line_number: line_num + 1,
                        line: line.to_owned(),
                    });
                }
            }
        }
    }

    Ok(())
}
