# OVC CLI Reference

Complete command reference for the `ovc` CLI tool.

## Repository Commands

| Command | Description |
|---|---|
| `init` | Create a new encrypted repository (`--key`, `--store`, `--name`) |
| `add <paths...>` | Stage files (`--all`, `--force`) |
| `commit -m <msg>` | Commit staged changes (`--sign`, `--amend`, `--no-verify`) |
| `status` | Working tree status (`--short`) |
| `log` | Commit history (`--oneline`, `--graph`, `--show-signatures`, `-n N`) |
| `diff` | Show changes (`--staged`, `--stat`, `--name-only`, `branch-a..branch-b`) |
| `show [commit]` | Display commit details and diff |

## Branching & History

| Command | Description |
|---|---|
| `branch [name]` | List, create, or delete (`-d`, `-D` force) branches |
| `checkout <target>` | Switch branches (`-b` to create, `-- <files>` to restore) |
| `merge <branch>` | Three-way merge into current branch |
| `rebase <onto>` | Rebase current branch onto target |
| `cherry-pick <commit>` | Apply a single commit onto HEAD (short hashes supported) |
| `revert <commit>` | Create a new commit undoing a previous commit's changes |
| `tag [name]` | Create, list, or delete (`-d`) tags |
| `stash` | `push`, `pop`, `apply`, `drop`, `list`, `clear` |
| `reset [commit]` | Reset HEAD (`--soft`, `--mixed`, `--hard`, `-- <files>` to unstage) |
| `clean` | Remove untracked files (`-n` dry run, `-f` force) |

## Code Investigation

| Command | Description |
|---|---|
| `blame <file>` | Line-by-line authorship (`-L start,end` for ranges) |
| `grep <pattern>` | Search file contents (`-i`, `--count`) |
| `describe [commit]` | Find nearest tag ancestor |
| `shortlog` | Commits grouped by author (`-s` summary, `-n` sort) |
| `reflog` | Show reference update history |
| `ls-files` | List tracked files (`--staged`, `--modified`, `--untracked`) |
| `notes` | `add`, `show`, `remove`: commit annotations |
| `archive` | Export tree as tar or zip (`--format`, `-o`) |
| `bisect` | Binary search for a bug-introducing commit |

## Key Management

| Command | Description |
|---|---|
| `key generate` | Generate Ed25519+X25519 key pair (`--name`, `--identity`) |
| `key list` | List keys in `~/.ssh/ovc/` |
| `key export <name>` | Export for Bitwarden/1Password |
| `key import <file>` | Import from password manager export |
| `key add <pubkey>` | Grant a public key access to the repo |
| `key remove <fingerprint>` | Revoke access |
| `key authorized` | List authorized keys for this repo |
| `verify [commit]` | Verify a commit's Ed25519 signature |

## Access Control & Security

| Command | Description |
|---|---|
| `access list` | List users with access and their roles |
| `access grant <key> --role <role>` | Grant access with a role (`read`, `write`, `admin`, `owner`) |
| `access revoke <fingerprint>` | Revoke a user's access |
| `access set-role <fingerprint> --role <role>` | Change a user's role |
| `branch-protect <branch>` | Set branch protection (`--required-approvals N`, `--require-ci`) |
| `branch-protect <branch> --remove` | Remove branch protection |

## Cloud & Sync

| Command | Description |
|---|---|
| `remote add <name> <url>` | Add remote (`--backend local\|gcs`) |
| `remote list` | List configured remotes |
| `remote remove <name>` | Remove a remote |
| `push` | Push to remote |
| `pull` | Pull from remote |
| `sync` | Merge remote changes and save (for cloud collaboration) |
| `sync-status` | Show sync status |

## Actions (CI/CD)

| Command | Description |
|---|---|
| `actions init` | Auto-detect languages, generate `.ovc/actions.yml` |
| `actions list` | List configured actions |
| `actions run [names...]` | Run actions (`--trigger`, `--fix`) |
| `actions history` | View run history |
| `actions detect` | Detect project languages |
| `actions secrets` | `list`, `set <name> <value>`, `remove <name>`: manage secrets vault |

## Utilities & Management

| Command | Description |
|---|---|
| `git-import <path>` | Import a Git repository into OVC |
| `git-export <file>` | Export an OVC repository to Git |
| `gc` | Garbage collect unreachable objects |
| `submodule` | `add`, `status`, `update`, `remove`: nested repositories |
| `web` / `ui` / `gui` | Open the web UI in your browser |
| `serve` | Start API server + web UI (`--port`, `--bind`, `--repos-dir`) |
| `daemon` | Manage background server (`install`, `uninstall`, `start`, `stop`) |
| `onboard` | Interactive setup wizard for new users |

## Environment Variables Reference

| Variable | Purpose |
|---|---|
| `OVC_KEY` | Default key name (stored in `~/.ssh/ovc/`) |
| `OVC_KEY_PASSPHRASE` | Key passphrase (or omit for interactive prompt) |
| `OVC_SIGN_COMMITS` | Set to `true` to auto-sign all commits |
| `OVC_AUTHOR_NAME` | Default commit author name |
| `OVC_AUTHOR_EMAIL` | Default commit author email |
| `OVC_PASSWORD` | Repository password (for password-based repos) |
| `OVC_PORT` | API server port (default: 9742) |
| `OVC_REPOS_DIR` | API server repos directory |
| `OVC_CORS_ORIGINS` | Allowed CORS origins for API |
| `OVC_WORKDIR_MAP` | Map repo IDs to working directories for the API server |
| `OVC_LLM_ENABLED` | Enable LLM-powered features at the server level |
| `OVC_LLM_BASE_URL` | Base URL of the OpenAI-compatible LLM server |
| `OVC_LLM_MODEL` | Model name for LLM completions |
| `OVC_LLM_API_KEY` | API key for the LLM server |
| `OVC_LLM_MAX_TOKENS` | Maximum context tokens for LLM requests (default: 32768) |
| `OVC_LLM_TIMEOUT` | LLM request timeout in seconds (default: 120) |
