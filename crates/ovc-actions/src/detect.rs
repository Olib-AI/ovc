//! Language and toolchain detection for automatic action configuration.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::config::ActionsConfig;
use crate::templates::generate_template;

/// Confidence level for a detection result.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Confidence {
    High,
    Medium,
}

impl std::fmt::Display for Confidence {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::High => write!(f, "high"),
            Self::Medium => write!(f, "medium"),
        }
    }
}

/// A single detected language or toolchain.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedLanguage {
    /// Language name (e.g. "Rust", "JavaScript").
    pub language: String,
    /// Detection confidence.
    pub confidence: Confidence,
    /// Marker file that triggered detection.
    pub marker_file: String,
    /// Directory where the marker was found (relative to repo root).
    pub root_dir: String,
}

/// Aggregated detection result.
#[derive(Debug, Clone)]
pub struct DetectionResult {
    /// All detected languages.
    pub languages: Vec<DetectedLanguage>,
    /// Suggested starter configuration.
    pub suggested_config: ActionsConfig,
}

/// Marker file to language mapping.
struct LangMarker {
    file: &'static str,
    language: &'static str,
    confidence: Confidence,
}

const MARKERS: &[LangMarker] = &[
    LangMarker {
        file: "Cargo.toml",
        language: "Rust",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "package.json",
        language: "JavaScript",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "tsconfig.json",
        language: "TypeScript",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "go.mod",
        language: "Go",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "pyproject.toml",
        language: "Python",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "requirements.txt",
        language: "Python",
        confidence: Confidence::Medium,
    },
    LangMarker {
        file: "Pipfile",
        language: "Python",
        confidence: Confidence::Medium,
    },
    LangMarker {
        file: "Gemfile",
        language: "Ruby",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "pom.xml",
        language: "Java",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "build.gradle",
        language: "Java",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "CMakeLists.txt",
        language: "C++",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "Makefile",
        language: "Make",
        confidence: Confidence::Medium,
    },
    LangMarker {
        file: "Dockerfile",
        language: "Docker",
        confidence: Confidence::Medium,
    },
    LangMarker {
        file: "composer.json",
        language: "PHP",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "mix.exs",
        language: "Elixir",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "build.gradle.kts",
        language: "Kotlin",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "Package.swift",
        language: "Swift",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "pubspec.yaml",
        language: "Dart",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "deno.json",
        language: "Deno",
        confidence: Confidence::High,
    },
    LangMarker {
        file: "bun.lockb",
        language: "Bun",
        confidence: Confidence::Medium,
    },
];

/// Detect languages and toolchains present in the given repository root.
#[must_use]
pub fn detect_languages(repo_root: &Path) -> DetectionResult {
    let mut languages = Vec::new();
    let mut seen = std::collections::HashSet::new();

    for marker in MARKERS {
        let marker_path = repo_root.join(marker.file);
        if marker_path.exists() && seen.insert(marker.language) {
            languages.push(DetectedLanguage {
                language: marker.language.to_owned(),
                confidence: marker.confidence,
                marker_file: marker.file.to_owned(),
                root_dir: ".".to_owned(),
            });
        }
    }

    // Detect C# projects via .csproj or .sln files (requires walkdir scan).
    #[allow(clippy::set_contains_or_insert)]
    if !seen.contains("C#") {
        let has_csharp = walkdir::WalkDir::new(repo_root)
            .max_depth(3)
            .into_iter()
            .filter_map(Result::ok)
            .any(|entry| {
                let name = entry.file_name().to_string_lossy();
                name.ends_with(".csproj") || name.ends_with(".sln")
            });
        if has_csharp {
            seen.insert("C#");
            languages.push(DetectedLanguage {
                language: "C#".to_owned(),
                confidence: Confidence::High,
                marker_file: "*.csproj / *.sln".to_owned(),
                root_dir: ".".to_owned(),
            });
        }
    }

    let suggested_config = generate_template(&languages);

    DetectionResult {
        languages,
        suggested_config,
    }
}
