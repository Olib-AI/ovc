# Getting Started with OVC

Welcome to OVC (Olib Version Control). This guide will help you install OVC, generate encryption keys, configure your environment, and manage your first repository.

## Installation

### Desktop App

Download the latest desktop installer from the [OVC releases page](https://github.com/Olib-AI/ovc/releases/latest):

| Platform | Package |
|----------|---------|
| macOS Apple Silicon | `ovc-desktop-macos-arm64.dmg` |
| macOS Intel | `ovc-desktop-macos-amd64.dmg` |
| Windows | `ovc-desktop-windows-amd64.msi` |
| Linux | `ovc-desktop-linux-amd64.deb` or `ovc-desktop-linux-amd64.AppImage` |

The desktop app contains the local OVC service and the complete web interface. It does not require the CLI, a background daemon, Node.js, or a separately configured server.

On first launch, OVC opens an onboarding screen where you enter your commit identity and create your first encrypted repository. Later launches open the repository list directly.

See the [Desktop App Guide](desktop-app.md) for storage locations, runtime requirements, and development instructions.

### CLI

#### Linux & macOS

```bash
curl -fsSL https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.sh | bash
```

#### Windows (PowerShell)

```powershell
irm https://raw.githubusercontent.com/Olib-AI/ovc/main/scripts/install.ps1 | iex
```

#### Installer Options

| Option | Description |
|--------|-------------|
| `--version VERSION` | Install a specific version (default: latest) |
| `--update` | Update binary only, preserve keys and config |
| `--uninstall` | Remove OVC completely |
| `--help` | Show help message |

### Building From Source

```bash
git clone https://github.com/Olib-AI/ovc.git
cd ovc
cd frontend && npm install && npm run build && cd ..
cargo build --release
sudo cp target/release/ovc /usr/local/bin/ovc
```

> **macOS Note:** If you see `zsh: killed` when running `ovc`, macOS Gatekeeper is blocking the unsigned binary. Run `sudo xattr -cr /usr/local/bin/ovc && sudo codesign --force --sign - /usr/local/bin/ovc` to ad-hoc sign it.

---

## Key Setup

OVC uses Ed25519+X25519 key pairs for both repository encryption and commit signing.

### Generate Key Pair

```bash
ovc key generate --name mykey --identity "Your Name <you@email.com>"
```

Keys are stored in `~/.ssh/ovc/`. Back them up in your password manager:

```bash
ovc key export mykey
# Copy the output into a Bitwarden/1Password secure note
```

---

## Environment Configuration

Add the following to your `~/.zshrc` or `~/.bashrc`:

```bash
export OVC_KEY=mykey
export OVC_KEY_PASSPHRASE=<your-key-passphrase>
export OVC_AUTHOR_NAME="Your Name"
export OVC_AUTHOR_EMAIL="you@email.com"
export OVC_SIGN_COMMITS=true   # auto-sign every commit
```

---

## Creating Repositories

### Basic Repository

Create an `.ovc` file in your current working directory:

```bash
ovc init --name my-project.ovc --key mykey
```

### Repository with Cloud Sync Location

Store the `.ovc` repository file in a cloud-synced folder (such as iCloud, Dropbox, or Google Drive) while working locally:

```bash
ovc init --name my-project.ovc --key mykey \
  --store ~/Library/Mobile\ Documents/com~apple~CloudDocs/ovc-repos/
```

---

## Basic Workflow

### Staging & Committing

```bash
ovc add .                              # Stage all files
ovc commit -m "add feature X"          # Commit changes
ovc commit --amend -m "better msg"     # Amend the last commit
```

### Checking Status & History

```bash
ovc status                             # Working tree status
ovc log --oneline --graph              # Commit history with branch graph
ovc log --show-signatures               # View signatures
ovc diff                               # View unstaged changes
ovc diff --staged                       # View staged changes
ovc diff main..feature-x               # Diff between branches
```

### Branching & Merging

```bash
ovc branch feature-x                    # Create branch
ovc checkout feature-x                  # Switch branch
ovc merge feature-x                     # Merge into current branch
ovc rebase main                         # Rebase current branch onto main
ovc cherry-pick abc123                  # Apply commit onto HEAD
ovc revert abc123                       # Revert commit changes
```

### Undoing Changes

```bash
ovc checkout -- src/file.rs            # Restore file from HEAD
ovc reset -- src/file.rs               # Unstage file
ovc reset --hard HEAD~1                # Hard reset to previous commit
ovc clean -f                            # Remove untracked files
```

### Inspection Tools

```bash
ovc blame src/main.rs                   # Line-by-line authorship
ovc grep "TODO"                         # Search contents
ovc show HEAD~2                         # Show commit details
ovc reflog                              # Reference update history
```
