<p align="center">
  <img src="examples/logo.svg" width="80" height="80" alt="OVC Logo">
</p>
<h1 align="center">OVC: Olib Version Control</h1>
<p align="center">
  <strong>Secure, self-hosted version control: encrypted single-file repositories you can store anywhere.</strong>
</p>
<p align="center">
  Every commit, branch, tag, and file history lives in a single encrypted <code>.ovc</code> blob: Ed25519+X25519 key pair encryption, commit signing & verification, full Git interop, cloud sync via any storage provider, a built-in CI/CD actions engine with 28 built-in checks, local LLM integration for AI-powered commit messages, code review, and diff explanation, plus a premium web UI, all in a <strong>single binary</strong>.
</p>
<p align="center">
  Built by <a href="https://www.olib.ai">Olib AI</a>
</p>

---

## Documentation

- [Getting Started](docs/getting-started.md): Installation, key generation, and basic workflow
- [CLI Reference](docs/cli-reference.md): Full command listing, flags, and environment variables
- [Architecture & Security Model](docs/architecture.md): Repository format, encryption specs, RBAC, and cloud sync safety
- [Actions Engine](docs/actions-engine.md): Built-in checks, DAG scheduler, and Docker integration
- [Web UI & Local LLM Integration](docs/webui-and-llm.md): React web interface and AI features

---

## Why OVC?

Modern teams need version control they fully own without giving up the convenience of cloud infrastructure.

- **Self-hosted, zero trust**: your code never touches a server you don't control; store encrypted repos on any cloud (iCloud, GCS, S3, Dropbox, NAS) while retaining full ownership
- **Encrypted at rest and in transit**: one encrypted file per repo; an attacker who intercepts the file sees only ciphertext
- **One key pair does everything**: encrypt repos, sign commits, verify teammates (no GPG needed)
- **Works like Git**: same mental model, same workflow, zero learning curve
- **Collaboration-safe**: file locking, conflict detection, and auto-merge for shared repos via any cloud storage

---

## Key Features

- **Encrypted at rest**: XChaCha20-Poly1305 encryption with Ed25519+X25519 key pairs
- **Single portable file**: your entire repository lives in one `.ovc` file; store it on iCloud, GCS, S3, Dropbox, USB, NAS, or email it
- **Commit signing & verification**: Ed25519 signatures with verified / unverified badges in CLI and web UI
- **One key, two purposes**: same key encrypts repos AND signs commits (no GPG needed)
- **Multi-user collaboration**: share the `.ovc` file via any cloud storage; cross-process locking, conflict detection, write-ahead log, and auto-merge keep everyone's work safe
- **Cloud sync**: content-defined chunking via FastCDC; only changed parts transfer; supports local, GCS, and extensible backends
- **Git-compatible**: bidirectional import/export with full history fidelity
- **Built-in Actions Engine**: 28 built-in checks + custom shell commands, parallel execution with DAG dependencies, matrix strategy, secrets vault, retry logic
- **Local LLM integration**: AI-powered commit messages, PR review, diff explanation, and PR descriptions via any OpenAI-compatible local model (Ollama, LM Studio); multi-pass map-reduce pipeline handles diffs of any size
- **Premium web UI**: commit graph with SVG lanes, split diff viewer, blame view, code search, command palette, commit actions, toast notifications
- **Single binary**: VCS + crypto + git bridge + cloud sync + actions engine + LLM integration + web server + React UI
- **Access control (RBAC)**: per-user roles (read, write, admin, owner) with branch protection
- **Memory-safe**: `unsafe_code = "forbid"` workspace-wide; keys zeroed on drop via `zeroize`
- **Security-hardened**: constant-time auth, secret zeroization, path traversal protection, bounded resource allocation

---

## Quick Start

### Install

**Linux & macOS:**

```bash
curl -fsSL https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.sh | bash
```

**Windows (PowerShell):**

```powershell
irm https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.ps1 | iex
```

### Create your first repo

```bash
# Basic: .ovc file in current directory
ovc init --name my-project.ovc --key mykey

# With cloud sync: .ovc file on iCloud, code stays local
ovc init --name my-project.ovc --key mykey \
  --store ~/Library/Mobile\ Documents/com~apple~CloudDocs/ovc-repos/
```

### Daily workflow

```bash
ovc add .                              # Stage all files
ovc commit -m "add feature X"          # Commit (auto-signed if OVC_SIGN_COMMITS=true)
ovc status                             # Working tree status
ovc log --oneline --graph              # Commit history with branch graph
ovc diff                               # View changes
```

---

## Building from Source

### Prerequisites
- Rust 1.85+ (edition 2024)
- Node.js 20+ (for frontend build only)

```bash
# Build frontend (embedded into the binary)
cd frontend && npm install && npm run build && cd ..

# Build the binary
cargo build --release

# Install
sudo cp target/release/ovc /usr/local/bin/ovc

# Run tests
cargo test --workspace    # 200+ tests

# Lint
cargo clippy --workspace  # strict: all + pedantic + nursery
```

---

## Contributing

1. Fork and clone the repo
2. Create a feature branch
3. Run `cargo test --workspace` and `cargo clippy --workspace -- -D warnings`
4. Submit a pull request

---

## License

MIT License

```
Copyright (c) 2025 Olib AI <dev@olib.ai>
```

---

*Built with Rust 2024 Edition by [Olib AI](https://www.olib.ai)*
