# OVC Architecture & Security Model

## Repository Architecture

OVC is designed as a standalone, zero-trust version control system where an entire repository lives inside a single encrypted `.ovc` file.

```
crates/
  ovc-core/           Core library: object model, crypto, storage, merge, diff, RBAC
  ovc-git/            Git interoperability: bidirectional import/export
  ovc-cloud/          Cloud sync: FastCDC chunking, storage backends (local, GCS)
  ovc-api/            REST API server: Axum-based, embedded React UI
  ovc-actions/        Actions/CI engine: 28 built-in checks, DAG scheduler, Docker
  ovc-llm/            Local LLM integration: multi-pass context builder, SSE streaming
  ovc-desktop/        Slint desktop shell: embedded API service and system webview
  ovc-cli/            CLI: 47 commands, Clap-based
  ovc-remote-helper/  Git remote helper stub

frontend/             React 19 + TypeScript + Tailwind CSS + Vite (embedded into binary)
docker/               Docker image for actions execution
```

## Desktop Architecture

The desktop app is a self-contained executable. It links the API crate directly, embeds the compiled React assets, and starts the service on a random loopback port. A private per-launch session is injected into the child webview before the UI loads.

Slint owns the native application window. Wry attaches the operating system webview as a child and keeps it sized to the window. The app uses WKWebView on macOS, WebView2 on Windows, and WebKitGTK on Linux.

The CLI remains a separate release artifact for terminal workflows and server deployments. The desktop app does not spawn the CLI or depend on an external daemon.

## Security Model

### Dual-Purpose Keys

Your OVC key pair (`~/.ssh/ovc/mykey.key`) contains:
- **Ed25519**: signs commits (identity verification)
- **X25519**: encrypts repo data (derived via standard SHA-512 conversion)

No GPG, no separate signing key, and no web of trust are required. The repository's authorized key list serves as the trust anchor.

### Encryption Pipeline

```
Your Key Pair (Ed25519 + X25519)
   |
   |--- X25519 ECDH with ephemeral key
   |         |
   |         +--- HKDF-SHA256 -> Sealed Key Encryption Key
   |
   +--- Segment Encryption Key (256-bit)
           |
           +--- XChaCha20-Poly1305 encrypts all data (192-bit nonce per segment)
```

### Security Hardening Features

- Constant-time authentication to prevent timing side-channels
- Secrets zeroed on drop via `Zeroizing<T>` wrappers
- HKDF-SHA256 for key derivation
- Bounded resource allocation (pipe reads, grep results, matrix combos, key slots, reflog, notes)
- Path traversal protection on all file operations
- Input validation at both API and core layers
- Atomic Write-Ahead Logging (WAL) for crash recovery integrity
- Randomized temp file names to prevent symlink attacks
- API error sanitization to prevent internal path leakage
- GCS bucket name validation and request body size limits (16 MiB)

### Encrypted File Structure

Only the 64-byte header (magic bytes, KDF parameters, salt) and 32-byte trailer (offsets, HMAC) are stored in plaintext. Everything else (file names, sizes, commit messages, authors, branch names, tags) is ciphertext.

---

## Collaboration & Concurrency Safety

OVC enables safe multi-user collaboration via shared storage (iCloud, GCS, S3, Dropbox, NAS):

- **Cross-process file locking**: advisory locks prevent concurrent writes; stale locks from crashed processes or remote hosts are automatically detected and cleaned.
- **Conflict detection**: file sequence validation detects if another user modified the repo since you opened it.
- **Write-ahead log**: crash recovery journal prevents data loss from interrupted saves; orphaned temp files are cleaned up on next open.
- **Auto-merge**: when a conflict is detected during save, OVC automatically re-reads the remote state, imports new branches/objects, and saves the combined result.
- **Branch-based workflow**: each user works on their own branch; auto-merge preserves all branches without logical conflicts.

---

## Access Control (RBAC)

OVC includes built-in Role-Based Access Control (RBAC) mapped to public key fingerprints.

### User Roles

- **read**: clone, view content, comment on PRs
- **write**: commit, push, create PRs and branches
- **admin**: manage branches, merge PRs, configure actions
- **owner**: full control including access management

### Branch Protection

Branches can be protected to require specific access levels, code reviews, and passing CI/CD status before merging:

```bash
ovc branch-protect main --required-approvals 2 --require-ci
```
