//! Per-language action template generation.

use std::collections::BTreeMap;

use crate::config::{
    ActionCategory, ActionCondition, ActionDefinition, ActionsConfig, BuiltinAction, Trigger,
};
use crate::detect::DetectedLanguage;

/// Generate a starter `ActionsConfig` based on detected languages.
#[must_use]
pub fn generate_template(languages: &[DetectedLanguage]) -> ActionsConfig {
    let mut config = ActionsConfig::default();

    add_builtin_actions(&mut config);

    for lang in languages {
        match lang.language.as_str() {
            "Rust" => add_rust_actions(&mut config),
            "JavaScript" | "TypeScript" => add_js_actions(&mut config),
            "Go" => add_go_actions(&mut config),
            "Python" => add_python_actions(&mut config),
            "Ruby" => add_ruby_actions(&mut config),
            "Java" => {
                if lang.marker_file == "build.gradle" {
                    add_java_gradle_actions(&mut config);
                } else {
                    add_java_maven_actions(&mut config);
                }
            }
            "C++" => add_cpp_actions(&mut config),
            "Kotlin" => add_kotlin_actions(&mut config),
            "Swift" => add_swift_actions(&mut config),
            "Dart" => add_dart_actions(&mut config),
            "C#" => add_csharp_actions(&mut config),
            "Deno" => add_deno_actions(&mut config),
            "PHP" => add_php_actions(&mut config),
            "Elixir" => add_elixir_actions(&mut config),
            _ => {}
        }
    }

    config
}

fn add_builtin_actions(config: &mut ActionsConfig) {
    add_pre_commit_builtins(config);
    add_audit_builtins(config);
}

fn add_pre_commit_builtins(config: &mut ActionsConfig) {
    config.actions.insert(
        "secret-scan".to_owned(),
        ActionDefinition {
            category: ActionCategory::Audit,
            display_name: Some("Secret Scanner".to_owned()),
            builtin: Some(BuiltinAction::SecretScan),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "trailing-whitespace".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Trailing Whitespace".to_owned()),
            builtin: Some(BuiltinAction::TrailingWhitespace),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "merge-conflict-check".to_owned(),
        ActionDefinition {
            category: ActionCategory::Quality,
            display_name: Some("Merge Conflict Check".to_owned()),
            builtin: Some(BuiltinAction::MergeConflictCheck),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "debug-statements".to_owned(),
        ActionDefinition {
            category: ActionCategory::Quality,
            display_name: Some("Debug Statements".to_owned()),
            builtin: Some(BuiltinAction::DebugStatements),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "encoding-check".to_owned(),
        ActionDefinition {
            category: ActionCategory::Quality,
            display_name: Some("Encoding Check".to_owned()),
            builtin: Some(BuiltinAction::EncodingCheck),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "eof-newline".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("EOF Newline".to_owned()),
            builtin: Some(BuiltinAction::EofNewline),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "yaml-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("YAML Lint".to_owned()),
            builtin: Some(BuiltinAction::YamlLint),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "json-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("JSON Lint".to_owned()),
            builtin: Some(BuiltinAction::JsonLint),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "commit-message-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Quality,
            display_name: Some("Commit Message Lint".to_owned()),
            builtin: Some(BuiltinAction::CommitMessageLint),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
    config.actions.insert(
        "mixed-indentation".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Mixed Indentation".to_owned()),
            builtin: Some(BuiltinAction::MixedIndentation),
            trigger: Trigger::PreCommit,
            ..default_def()
        },
    );
}

fn add_audit_builtins(config: &mut ActionsConfig) {
    config.actions.insert(
        "dependency-update-check".to_owned(),
        ActionDefinition {
            category: ActionCategory::Audit,
            display_name: Some("Dependency Update Check".to_owned()),
            builtin: Some(BuiltinAction::DependencyUpdateCheck),
            // Runs on-demand — network requests are not appropriate for every commit.
            trigger: Trigger::Manual,
            ..default_def()
        },
    );
}

fn add_rust_actions(config: &mut ActionsConfig) {
    let rs_condition = Some(ActionCondition {
        paths: vec!["**/*.rs".to_owned(), "Cargo.toml".to_owned()],
    });
    config.actions.insert(
        "rust-check".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("Cargo Check".to_owned()),
            language: Some("Rust".to_owned()),
            tool: Some("cargo".to_owned()),
            command: Some("cargo check --workspace".to_owned()),
            trigger: Trigger::PreCommit,
            condition: rs_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "rust-clippy".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Clippy".to_owned()),
            language: Some("Rust".to_owned()),
            tool: Some("clippy".to_owned()),
            command: Some("cargo clippy --workspace -- -D warnings".to_owned()),
            trigger: Trigger::PreCommit,
            condition: rs_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "rust-fmt".to_owned(),
        ActionDefinition {
            category: ActionCategory::Format,
            display_name: Some("Rustfmt".to_owned()),
            language: Some("Rust".to_owned()),
            tool: Some("rustfmt".to_owned()),
            command: Some("cargo fmt --check".to_owned()),
            fix_command: Some("cargo fmt".to_owned()),
            trigger: Trigger::PreCommit,
            condition: rs_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "rust-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Cargo Test".to_owned()),
            language: Some("Rust".to_owned()),
            tool: Some("cargo".to_owned()),
            command: Some("cargo test --workspace".to_owned()),
            trigger: Trigger::PrePush,
            condition: rs_condition,
            ..default_def()
        },
    );
}

fn add_js_actions(config: &mut ActionsConfig) {
    let js_condition = Some(ActionCondition {
        paths: vec![
            "**/*.js".to_owned(),
            "**/*.ts".to_owned(),
            "**/*.jsx".to_owned(),
            "**/*.tsx".to_owned(),
        ],
    });
    config.actions.insert(
        "js-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("ESLint".to_owned()),
            language: Some("JavaScript".to_owned()),
            tool: Some("eslint".to_owned()),
            command: Some("npx eslint .".to_owned()),
            fix_command: Some("npx eslint . --fix".to_owned()),
            trigger: Trigger::PreCommit,
            condition: js_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "js-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("JS Tests".to_owned()),
            language: Some("JavaScript".to_owned()),
            tool: Some("npm".to_owned()),
            command: Some("npm test".to_owned()),
            trigger: Trigger::PrePush,
            condition: js_condition,
            ..default_def()
        },
    );
}

fn add_go_actions(config: &mut ActionsConfig) {
    let go_condition = Some(ActionCondition {
        paths: vec!["**/*.go".to_owned()],
    });
    config.actions.insert(
        "go-vet".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Go Vet".to_owned()),
            language: Some("Go".to_owned()),
            tool: Some("go".to_owned()),
            command: Some("go vet ./...".to_owned()),
            trigger: Trigger::PreCommit,
            condition: go_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "go-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Go Test".to_owned()),
            language: Some("Go".to_owned()),
            tool: Some("go".to_owned()),
            command: Some("go test ./...".to_owned()),
            trigger: Trigger::PrePush,
            condition: go_condition,
            ..default_def()
        },
    );
}

fn add_python_actions(config: &mut ActionsConfig) {
    let py_condition = Some(ActionCondition {
        paths: vec!["**/*.py".to_owned()],
    });
    config.actions.insert(
        "python-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Ruff".to_owned()),
            language: Some("Python".to_owned()),
            tool: Some("ruff".to_owned()),
            command: Some("ruff check .".to_owned()),
            fix_command: Some("ruff check . --fix".to_owned()),
            trigger: Trigger::PreCommit,
            condition: py_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "python-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Pytest".to_owned()),
            language: Some("Python".to_owned()),
            tool: Some("pytest".to_owned()),
            command: Some("pytest".to_owned()),
            trigger: Trigger::PrePush,
            condition: py_condition,
            ..default_def()
        },
    );
}

fn add_ruby_actions(config: &mut ActionsConfig) {
    config.actions.insert(
        "ruby-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("RuboCop".to_owned()),
            language: Some("Ruby".to_owned()),
            tool: Some("rubocop".to_owned()),
            command: Some("rubocop".to_owned()),
            fix_command: Some("rubocop -a".to_owned()),
            trigger: Trigger::PreCommit,
            condition: Some(ActionCondition {
                paths: vec!["**/*.rb".to_owned()],
            }),
            ..default_def()
        },
    );
}

fn add_java_maven_actions(config: &mut ActionsConfig) {
    let java_condition = Some(ActionCondition {
        paths: vec!["**/*.java".to_owned(), "pom.xml".to_owned()],
    });
    config.actions.insert(
        "java-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("Maven Build".to_owned()),
            language: Some("Java".to_owned()),
            tool: Some("mvn".to_owned()),
            command: Some("mvn compile -q".to_owned()),
            trigger: Trigger::PreCommit,
            condition: java_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "java-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Maven Test".to_owned()),
            language: Some("Java".to_owned()),
            tool: Some("mvn".to_owned()),
            command: Some("mvn test -q".to_owned()),
            trigger: Trigger::PrePush,
            condition: java_condition,
            ..default_def()
        },
    );
}

fn add_java_gradle_actions(config: &mut ActionsConfig) {
    let java_condition = Some(ActionCondition {
        paths: vec!["**/*.java".to_owned(), "build.gradle".to_owned()],
    });
    config.actions.insert(
        "java-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("Gradle Build".to_owned()),
            language: Some("Java".to_owned()),
            tool: Some("gradle".to_owned()),
            command: Some("./gradlew build".to_owned()),
            trigger: Trigger::PreCommit,
            condition: java_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "java-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Gradle Test".to_owned()),
            language: Some("Java".to_owned()),
            tool: Some("gradle".to_owned()),
            command: Some("./gradlew test".to_owned()),
            trigger: Trigger::PrePush,
            condition: java_condition,
            ..default_def()
        },
    );
}

fn add_cpp_actions(config: &mut ActionsConfig) {
    config.actions.insert(
        "cpp-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("CMake Build".to_owned()),
            language: Some("C++".to_owned()),
            tool: Some("cmake".to_owned()),
            command: Some("cmake --build build".to_owned()),
            trigger: Trigger::PreCommit,
            condition: Some(ActionCondition {
                paths: vec![
                    "**/*.cpp".to_owned(),
                    "**/*.h".to_owned(),
                    "CMakeLists.txt".to_owned(),
                ],
            }),
            ..default_def()
        },
    );
}

const fn default_def() -> ActionDefinition {
    ActionDefinition {
        category: ActionCategory::Custom,
        display_name: None,
        language: None,
        tool: None,
        command: None,
        fix_command: None,
        trigger: Trigger::Manual,
        timeout: None,
        working_dir: None,
        env: BTreeMap::new(),
        continue_on_error: false,
        condition: None,
        schedule: None,
        builtin: None,
        config: serde_yaml::Value::Null,
        depends_on: Vec::new(),
        matrix: None,
        retry: None,
        if_condition: None,
        outputs: Vec::new(),
        cache: None,
        auto_fix: false,
        docker_override: None,
    }
}

fn add_kotlin_actions(config: &mut ActionsConfig) {
    let kt_condition = Some(ActionCondition {
        paths: vec![
            "**/*.kt".to_owned(),
            "**/*.kts".to_owned(),
            "build.gradle.kts".to_owned(),
        ],
    });
    config.actions.insert(
        "kotlin-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("Gradle Build".to_owned()),
            language: Some("Kotlin".to_owned()),
            tool: Some("gradle".to_owned()),
            command: Some("./gradlew build".to_owned()),
            trigger: Trigger::PreCommit,
            condition: kt_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "kotlin-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("ktlint".to_owned()),
            language: Some("Kotlin".to_owned()),
            tool: Some("ktlint".to_owned()),
            command: Some("./gradlew ktlintCheck".to_owned()),
            fix_command: Some("./gradlew ktlintFormat".to_owned()),
            trigger: Trigger::PreCommit,
            condition: kt_condition,
            ..default_def()
        },
    );
}

fn add_swift_actions(config: &mut ActionsConfig) {
    let swift_condition = Some(ActionCondition {
        paths: vec!["**/*.swift".to_owned(), "Package.swift".to_owned()],
    });
    config.actions.insert(
        "swift-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("Swift Build".to_owned()),
            language: Some("Swift".to_owned()),
            tool: Some("swift".to_owned()),
            command: Some("swift build".to_owned()),
            trigger: Trigger::PreCommit,
            condition: swift_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "swift-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Swift Test".to_owned()),
            language: Some("Swift".to_owned()),
            tool: Some("swift".to_owned()),
            command: Some("swift test".to_owned()),
            trigger: Trigger::PrePush,
            condition: swift_condition,
            ..default_def()
        },
    );
}

fn add_dart_actions(config: &mut ActionsConfig) {
    let dart_condition = Some(ActionCondition {
        paths: vec!["**/*.dart".to_owned(), "pubspec.yaml".to_owned()],
    });
    config.actions.insert(
        "dart-analyze".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Dart Analyze".to_owned()),
            language: Some("Dart".to_owned()),
            tool: Some("dart".to_owned()),
            command: Some("dart analyze".to_owned()),
            trigger: Trigger::PreCommit,
            condition: dart_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "dart-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Dart Test".to_owned()),
            language: Some("Dart".to_owned()),
            tool: Some("dart".to_owned()),
            command: Some("dart test".to_owned()),
            trigger: Trigger::PrePush,
            condition: dart_condition,
            ..default_def()
        },
    );
}

fn add_csharp_actions(config: &mut ActionsConfig) {
    let cs_condition = Some(ActionCondition {
        paths: vec!["**/*.cs".to_owned(), "**/*.csproj".to_owned()],
    });
    config.actions.insert(
        "csharp-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("dotnet build".to_owned()),
            language: Some("C#".to_owned()),
            tool: Some("dotnet".to_owned()),
            command: Some("dotnet build".to_owned()),
            trigger: Trigger::PreCommit,
            condition: cs_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "csharp-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("dotnet test".to_owned()),
            language: Some("C#".to_owned()),
            tool: Some("dotnet".to_owned()),
            command: Some("dotnet test".to_owned()),
            trigger: Trigger::PrePush,
            condition: cs_condition,
            ..default_def()
        },
    );
}

fn add_php_actions(config: &mut ActionsConfig) {
    let php_condition = Some(ActionCondition {
        paths: vec!["**/*.php".to_owned(), "composer.json".to_owned()],
    });
    config.actions.insert(
        "php-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("PHPStan".to_owned()),
            language: Some("PHP".to_owned()),
            tool: Some("phpstan".to_owned()),
            command: Some("composer install && php vendor/bin/phpstan analyze".to_owned()),
            trigger: Trigger::PreCommit,
            condition: php_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "php-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("PHPUnit".to_owned()),
            language: Some("PHP".to_owned()),
            tool: Some("phpunit".to_owned()),
            command: Some("php vendor/bin/phpunit".to_owned()),
            trigger: Trigger::PrePush,
            condition: php_condition,
            ..default_def()
        },
    );
}

fn add_elixir_actions(config: &mut ActionsConfig) {
    let ex_condition = Some(ActionCondition {
        paths: vec![
            "**/*.ex".to_owned(),
            "**/*.exs".to_owned(),
            "mix.exs".to_owned(),
        ],
    });
    config.actions.insert(
        "elixir-build".to_owned(),
        ActionDefinition {
            category: ActionCategory::Build,
            display_name: Some("Mix Compile".to_owned()),
            language: Some("Elixir".to_owned()),
            tool: Some("mix".to_owned()),
            command: Some("mix compile --warnings-as-errors".to_owned()),
            trigger: Trigger::PreCommit,
            condition: ex_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "elixir-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Credo".to_owned()),
            language: Some("Elixir".to_owned()),
            tool: Some("credo".to_owned()),
            command: Some("mix credo".to_owned()),
            trigger: Trigger::PreCommit,
            condition: ex_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "elixir-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Mix Test".to_owned()),
            language: Some("Elixir".to_owned()),
            tool: Some("mix".to_owned()),
            command: Some("mix test".to_owned()),
            trigger: Trigger::PrePush,
            condition: ex_condition,
            ..default_def()
        },
    );
}

fn add_deno_actions(config: &mut ActionsConfig) {
    let deno_condition = Some(ActionCondition {
        paths: vec![
            "**/*.ts".to_owned(),
            "**/*.js".to_owned(),
            "deno.json".to_owned(),
        ],
    });
    config.actions.insert(
        "deno-lint".to_owned(),
        ActionDefinition {
            category: ActionCategory::Lint,
            display_name: Some("Deno Lint".to_owned()),
            language: Some("Deno".to_owned()),
            tool: Some("deno".to_owned()),
            command: Some("deno lint".to_owned()),
            trigger: Trigger::PreCommit,
            condition: deno_condition.clone(),
            ..default_def()
        },
    );
    config.actions.insert(
        "deno-test".to_owned(),
        ActionDefinition {
            category: ActionCategory::Test,
            display_name: Some("Deno Test".to_owned()),
            language: Some("Deno".to_owned()),
            tool: Some("deno".to_owned()),
            command: Some("deno test".to_owned()),
            trigger: Trigger::PrePush,
            condition: deno_condition,
            ..default_def()
        },
    );
}
