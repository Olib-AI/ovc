//! Ignore pattern matching for working directory scanning.
//!
//! [`IgnoreRules`] supports glob patterns from `.ovcignore` and `.gitignore`
//! files, including wildcards, double-star directory matching, negation, and
//! directory-only patterns.

use std::path::Path;

/// A compiled ignore pattern.
#[derive(Debug, Clone)]
struct IgnorePattern {
    /// The raw pattern text (without leading `!`).
    pattern: String,
    /// Whether this is a negation pattern (starts with `!`).
    negated: bool,
    /// Whether this pattern only matches directories (ends with `/`).
    dir_only: bool,
    /// Whether this pattern is anchored (contains `/` before the last char).
    anchored: bool,
}

/// A set of ignore rules loaded from ignore files.
#[derive(Debug, Clone)]
pub struct IgnoreRules {
    patterns: Vec<IgnorePattern>,
}

impl IgnoreRules {
    /// Loads ignore rules from `.ovcignore` and `.gitignore` in the given directory.
    ///
    /// Missing files are silently skipped. Built-in rules for `.ovc` and `.ovc.lock`
    /// are always included.
    #[must_use]
    pub fn load(workdir: &Path) -> Self {
        // Built-in rules: always ignore .ovc directory, lock file, link file, and *.ovc repo files.
        let mut patterns = vec![
            IgnorePattern {
                pattern: ".ovc".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
            IgnorePattern {
                pattern: ".ovc.lock".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
            IgnorePattern {
                pattern: ".ovc-link".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
            IgnorePattern {
                pattern: "*.ovc".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
        ];

        // Load .ovcignore.
        let ovcignore = workdir.join(".ovcignore");
        if let Ok(content) = std::fs::read_to_string(&ovcignore) {
            Self::parse_patterns(&content, &mut patterns);
        }

        // Load .gitignore.
        let gitignore = workdir.join(".gitignore");
        if let Ok(content) = std::fs::read_to_string(&gitignore) {
            Self::parse_patterns(&content, &mut patterns);
        }

        Self { patterns }
    }

    /// Creates an empty rule set with only built-in patterns.
    #[must_use]
    pub fn empty() -> Self {
        let patterns = vec![
            IgnorePattern {
                pattern: ".ovc".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
            IgnorePattern {
                pattern: ".ovc.lock".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
            IgnorePattern {
                pattern: ".ovc-link".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
            IgnorePattern {
                pattern: "*.ovc".into(),
                negated: false,
                dir_only: false,
                anchored: false,
            },
        ];
        Self { patterns }
    }

    /// Returns `true` if the given path should be ignored.
    ///
    /// The path should be relative to the repository root, using forward slashes.
    #[must_use]
    pub fn is_ignored(&self, path: &str) -> bool {
        self.check_ignored(path, false)
    }

    /// Returns `true` if the given directory path should be ignored.
    #[must_use]
    pub fn is_ignored_dir(&self, path: &str) -> bool {
        self.check_ignored(path, true)
    }

    /// Internal implementation of ignore checking.
    fn check_ignored(&self, path: &str, is_directory: bool) -> bool {
        let mut ignored = false;

        for pat in &self.patterns {
            // Directory-only patterns only match directories, but still match
            // path components that might be directory names. An anchored
            // directory pattern like `src/` must also prune everything under
            // that directory (e.g. `src/foo.rs`).
            if pat.dir_only && !is_directory {
                let component_match = if pat.anchored {
                    // Anchored: match the full path as a prefix as well.
                    glob_match(&pat.pattern, path)
                        || path.starts_with(&format!("{}/", pat.pattern))
                        || glob_match(&format!("{}/**", pat.pattern), path)
                } else {
                    path.split('/')
                        .any(|component| glob_match(&pat.pattern, component))
                };
                if component_match {
                    ignored = !pat.negated;
                }
                continue;
            }

            let matches = if pat.anchored {
                // Exact match against the full path.
                glob_match(&pat.pattern, path)
                // Prefix match: anchored pattern "src/foo" should also match
                // "src/foo/bar.rs" — i.e. anything under that directory.
                || {
                    let prefix = format!("{}/", pat.pattern);
                    path.starts_with(prefix.as_str())
                        || glob_match(&format!("{}/**", pat.pattern), path)
                }
            } else {
                glob_match(&pat.pattern, path)
                    || path
                        .split('/')
                        .any(|component| glob_match(&pat.pattern, component))
            };

            if matches {
                ignored = !pat.negated;
            }
        }

        ignored
    }

    /// Parses pattern lines from an ignore file.
    fn parse_patterns(content: &str, patterns: &mut Vec<IgnorePattern>) {
        for line in content.lines() {
            let trimmed = line.trim();

            // Skip empty lines and comments.
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }

            let (negated, raw) = trimmed
                .strip_prefix('!')
                .map_or((false, trimmed), |rest| (true, rest));

            let (dir_only, pattern_str) = raw
                .strip_suffix('/')
                .map_or((false, raw), |stripped| (true, stripped));

            // A pattern is anchored if it contains a `/` (other than trailing).
            let anchored = pattern_str.contains('/');

            patterns.push(IgnorePattern {
                pattern: pattern_str.to_owned(),
                negated,
                dir_only,
                anchored,
            });
        }
    }
}

/// Matches a glob pattern against a string.
///
/// Supports: `*` (any chars except `/`), `**` (any chars including `/`),
/// `?` (single char except `/`).
fn glob_match(pattern: &str, text: &str) -> bool {
    glob_match_bytes(pattern.as_bytes(), text.as_bytes())
}

/// Recursive glob matching implementation.
fn glob_match_bytes(pattern: &[u8], text: &[u8]) -> bool {
    let pat_len = pattern.len();

    // Check for `**` pattern (match everything including `/`).
    if pattern == b"**" {
        return true;
    }

    // Handle `**/` prefix: matches any leading path components.
    if pattern.starts_with(b"**/") {
        if glob_match_bytes(&pattern[3..], text) {
            return true;
        }
        for (pos, &byte) in text.iter().enumerate() {
            if byte == b'/' && glob_match_bytes(&pattern[3..], &text[pos + 1..]) {
                return true;
            }
        }
        return false;
    }

    // Handle `/**` suffix: matches any trailing path components.
    if pattern.ends_with(b"/**") {
        let prefix = &pattern[..pat_len - 3];
        if glob_match_bytes(prefix, text) {
            return true;
        }
        for (pos, &byte) in text.iter().enumerate() {
            if byte == b'/' && glob_match_bytes(prefix, &text[..pos]) {
                return true;
            }
        }
        return false;
    }

    // Handle `/**/` in the middle.
    if let Some(dstar_pos) = find_double_star(pattern) {
        let before = &pattern[..dstar_pos];
        let after = &pattern[dstar_pos + 4..]; // skip `/**/`

        for (pos, &byte) in text.iter().enumerate() {
            if byte == b'/'
                && glob_match_bytes(before, &text[..pos])
                && glob_match_bytes(after, &text[pos + 1..])
            {
                return true;
            }
        }
        return glob_match_bytes(before, text) && glob_match_bytes(after, b"");
    }

    // Simple wildcard matching with backtracking for `*`.
    simple_glob_match(pattern, text)
}

/// Simple glob match without `**` handling: `*` matches non-slash, `?` matches one non-slash.
fn simple_glob_match(pattern: &[u8], text: &[u8]) -> bool {
    let mut pat_pos = 0usize;
    let mut txt_pos = 0usize;
    let mut saved_pat = usize::MAX;
    let mut saved_txt = usize::MAX;
    let pat_len = pattern.len();
    let txt_len = text.len();

    while txt_pos < txt_len {
        if pat_pos < pat_len && pattern[pat_pos] == b'*' {
            saved_pat = pat_pos;
            saved_txt = txt_pos;
            pat_pos += 1;
        } else if pat_pos < pat_len
            && ((pattern[pat_pos] == b'?' && text[txt_pos] != b'/')
                || pattern[pat_pos] == text[txt_pos])
        {
            pat_pos += 1;
            txt_pos += 1;
        } else if saved_pat != usize::MAX && text[saved_txt] != b'/' {
            saved_txt += 1;
            txt_pos = saved_txt;
            pat_pos = saved_pat + 1;
        } else {
            return false;
        }
    }

    // Consume trailing `*` in pattern.
    while pat_pos < pat_len && pattern[pat_pos] == b'*' {
        pat_pos += 1;
    }

    pat_pos == pat_len
}

/// Finds the position of `/**/` in a pattern.
fn find_double_star(pattern: &[u8]) -> Option<usize> {
    pattern.windows(4).position(|w| w == b"/**/")
}

impl Default for IgnoreRules {
    fn default() -> Self {
        Self::empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn always_ignores_ovc_files() {
        let rules = IgnoreRules::empty();
        assert!(rules.is_ignored(".ovc"));
        assert!(rules.is_ignored(".ovc.lock"));
        assert!(rules.is_ignored("subdir/.ovc"));
        assert!(rules.is_ignored("repo.ovc"));
        assert!(rules.is_ignored("subdir/repo.ovc"));
    }

    #[test]
    fn simple_file_pattern() {
        let mut rules = IgnoreRules::empty();
        IgnoreRules::parse_patterns("*.log\n", &mut rules.patterns);
        assert!(rules.is_ignored("debug.log"));
        assert!(rules.is_ignored("subdir/error.log"));
        assert!(!rules.is_ignored("readme.txt"));
    }

    #[test]
    fn negation_pattern() {
        let mut rules = IgnoreRules::empty();
        IgnoreRules::parse_patterns("*.log\n!important.log\n", &mut rules.patterns);
        assert!(rules.is_ignored("debug.log"));
        assert!(!rules.is_ignored("important.log"));
    }

    #[test]
    fn directory_pattern() {
        let mut rules = IgnoreRules::empty();
        IgnoreRules::parse_patterns("build/\n", &mut rules.patterns);
        assert!(rules.is_ignored("build"));
        assert!(rules.is_ignored("project/build"));
    }

    #[test]
    fn question_mark_pattern() {
        let mut rules = IgnoreRules::empty();
        IgnoreRules::parse_patterns("?.txt\n", &mut rules.patterns);
        assert!(rules.is_ignored("a.txt"));
        assert!(!rules.is_ignored("ab.txt"));
    }

    #[test]
    fn comments_and_blanks_skipped() {
        let mut rules = IgnoreRules::empty();
        IgnoreRules::parse_patterns("# comment\n\n*.tmp\n", &mut rules.patterns);
        assert!(rules.is_ignored("test.tmp"));
    }

    #[test]
    fn double_star_pattern() {
        assert!(glob_match("**", "anything/at/all"));
        assert!(glob_match("**/foo", "bar/baz/foo"));
        assert!(glob_match("foo/**", "foo/bar/baz"));
    }

    #[test]
    fn load_from_nonexistent_dir() {
        let rules = IgnoreRules::load(Path::new("/nonexistent/path/12345"));
        // Should still have built-in rules.
        assert!(rules.is_ignored(".ovc"));
    }
}
