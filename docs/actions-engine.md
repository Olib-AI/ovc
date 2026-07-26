# Actions Engine & CI/CD

OVC features a built-in CI/CD engine with 28 built-in checks that run without external dependencies, along with full support for custom shell and Docker actions.

## Quick Start

```bash
ovc actions init                      # Auto-detect project languages and generate config
ovc actions run --trigger pre-commit  # Run pre-commit actions
ovc commit -m "commit message"        # Pre-commit hooks run automatically
ovc commit --no-verify -m "skip"      # Bypass pre-commit hooks
```

## Built-in Actions

| Action | Description |
|--------|-------------|
| `secret_scan` | AWS keys, GitHub tokens, private keys, API keys |
| `supply_chain_scan` | ENV access, system file reads, process execution, network calls |
| `package_scan` | Obfuscated code, encoded payloads, suspicious network calls in dependencies |
| `trailing_whitespace` | Trailing spaces and tabs |
| `line_endings` | Consistent LF or CRLF line endings |
| `file_size` | Files exceeding max size |
| `todo_counter` | TODO/FIXME/HACK occurrences |
| `license_header` | Required copyright header |
| `dependency_audit` | Typosquatting and wildcard versions in manifests |
| `code_complexity` | Excessive nesting depth |
| `dead_code` | Unreferenced functions |
| `duplicate_code` | Duplicate code blocks |
| `commit_message_lint` | Subject length, conventional commit format |
| `encoding_check` | UTF-8 validity |
| `merge_conflict_check` | Unresolved conflict markers |
| `symlink_check` | Broken symlinks |
| `large_diff_warning` | Oversized changesets |
| `branch_naming` | Branch name pattern validation |
| `debug_statements` | `console.log`, `println!`, `dbg!`, etc. |
| `mixed_indentation` | Tabs vs spaces mixing |
| `bom_check` | UTF-8 BOM detection |
| `shell_check` | Shell script best practices |
| `yaml_lint` | YAML syntax validation |
| `json_lint` | JSON syntax validation |
| `xml_lint` | XML well-formedness |
| `hardcoded_ip` | Hardcoded IP addresses |
| `non_ascii_check` | Non-ASCII characters in source |
| `eof_newline` | Files ending with newline |

## Features

- **Parallel Execution**: Independent actions run concurrently using DAG scheduling.
- **Matrix Strategy**: Parameterized runs across variable combinations.
- **Retry Logic**: Configurable retry attempts with custom delays.
- **Secrets Vault**: Encrypted secrets injected as `OVC_SECRET_*` environment variables.
- **Output Capture**: Regex-based variable extraction from action outputs.
- **Dependency Ordering**: Specify prerequisite actions.
- **Path Conditions**: Glob-based filtering for changed files.
- **Fix Commands**: Automatic remediation via the `--fix` flag.

## Docker Container Execution

Actions can run inside isolated Docker containers:

```yaml
# .ovc/actions.yml
defaults:
  docker:
    enabled: true
    image: ghcr.io/olib-ai/ovc-actions:latest
    pull_policy: if-not-present   # always | if-not-present | never

actions:
  rust-check:
    command: cargo check --workspace
    docker_override: false  # force native execution
```

The standard `ovc-actions` container includes toolchains for Rust, Go, Node.js, Python, Ruby, Java, C/C++, Kotlin, C#/.NET, Deno, Dart, Swift, PHP, and Elixir.
