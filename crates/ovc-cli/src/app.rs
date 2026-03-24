//! CLI argument definitions using `clap`.

use std::path::PathBuf;

use clap::{Args, Parser, Subcommand};

/// OVC (Olib Version Control) — encrypted single-file version control.
#[derive(Debug, Parser)]
#[command(
    name = "ovc",
    version,
    about = "OVC \u{2014} Olib Version Control\nEncrypted single-file version control by Olib AI",
    long_about = "OVC \u{2014} Olib Version Control\n\n\
        Encrypted single-file version control by Olib AI (www.olib.ai).\n\
        Every repository is a single encrypted .ovc file \u{2014} portable, secure, Git-compatible.\n\n\
        Quick start:\n  \
        ovc init --name myproject.ovc        Create a new repository\n  \
        ovc key generate --name mykey        Generate encryption key pair\n  \
        ovc add src/ && ovc commit -m \"msg\"  Stage and commit\n  \
        ovc serve --port 9742                Start web UI\n\n\
        Documentation: https://www.olib.ai/ovc",
    after_help = "Environment variables:\n  \
        OVC_PASSWORD       Repository password\n  \
        OVC_KEY            Key name for key-based auth\n  \
        OVC_KEY_PASSPHRASE Key passphrase\n  \
        OVC_AUTHOR_NAME    Commit author name\n  \
        OVC_AUTHOR_EMAIL   Commit author email\n  \
        OVC_PORT           API server port (default: 9742)\n  \
        OVC_REPOS_DIR      API server repos directory"
)]
pub struct Cli {
    /// Path to the .ovc repository file.
    #[arg(long, env = "OVC_REPO", global = true)]
    pub repo: Option<PathBuf>,

    /// Increase verbosity (-v, -vv, -vvv).
    #[arg(short, long, action = clap::ArgAction::Count, global = true)]
    pub verbose: u8,

    #[command(subcommand)]
    pub command: Command,
}

/// Top-level subcommands.
#[derive(Debug, Subcommand)]
pub enum Command {
    /// Create a new encrypted OVC repository.
    Init(InitArgs),
    /// Manage SSH key pairs for repository encryption.
    Key(KeyArgs),
    /// Stage files for the next commit.
    Add(AddArgs),
    /// Record staged changes as a new commit.
    Commit(CommitArgs),
    /// Show the working tree status.
    Status(StatusArgs),
    /// Show commit history.
    Log(LogArgs),
    /// Show changes between versions.
    Diff(DiffArgs),
    /// Manage branches.
    Branch(BranchArgs),
    /// Switch branches or restore working tree files.
    Checkout(CheckoutArgs),
    /// Manage tags.
    Tag(TagArgs),
    /// Merge a branch into the current branch.
    Merge(MergeArgs),
    /// Manage remote repositories.
    Remote(RemoteArgs),
    /// Import a Git repository into OVC format.
    GitImport(GitImportArgs),
    /// Export an OVC repository to Git format.
    GitExport(GitExportArgs),
    /// Push the local repository to a remote.
    Push(PushArgs),
    /// Pull the latest version from a remote.
    Pull(PullArgs),
    /// Sync local changes with the remote .ovc file (merge other users' work).
    Sync(SyncArgs),
    /// Show sync status with the remote.
    SyncStatus(SyncStatusArgs),
    /// Stash current index state.
    Stash(StashArgs),
    /// Rebase current branch onto another branch.
    Rebase(RebaseCliArgs),
    /// Apply a commit's changes onto HEAD.
    CherryPick(CherryPickArgs),
    /// Binary search for a regression-introducing commit.
    Bisect(BisectCliArgs),
    /// Run garbage collection.
    Gc(GcArgs),
    /// Verify a commit signature.
    Verify(VerifyArgs),
    /// Manage and run actions (lint, format, build, test, audit).
    Actions(ActionsArgs),
    /// Start the API server and web UI.
    Serve(ServeArgs),
    /// Manage the OVC background server daemon (macOS `LaunchAgent`).
    Daemon(DaemonArgs),
    /// Open the OVC web UI in the default browser.
    #[command(alias = "ui", alias = "gui")]
    Web(WebArgs),
    /// Interactive setup wizard for new OVC users.
    Onboard(OnboardArgs),
    /// Revert a commit by creating a new commit that undoes its changes.
    Revert(RevertArgs),
    /// Show line-by-line authorship of a file.
    Blame(BlameArgs),
    /// Reset HEAD to a previous commit.
    Reset(ResetArgs),
    /// Remove untracked files from the working directory.
    Clean(CleanArgs),
    /// Show commit details and diff.
    Show(ShowArgs),
    /// Search file contents in the repository.
    Grep(GrepArgs),
    /// Show the reference log.
    Reflog(ReflogArgs),
    /// List tracked files.
    LsFiles(LsFilesArgs),
    /// Find the nearest tag ancestor of a commit.
    Describe(DescribeArgs),
    /// Summarize commits grouped by author.
    Shortlog(ShortlogArgs),
    /// Manage commit annotations (notes).
    Notes(NotesArgs),
    /// Export the repository tree as an archive.
    Archive(ArchiveArgs),
    /// Manage nested submodule repositories.
    Submodule(SubmoduleArgs),
    /// Manage per-user access control.
    Access(AccessArgs),
    /// Configure branch protection rules.
    BranchProtect(BranchProtectArgs),
}

/// Arguments for `ovc verify`.
#[derive(Debug, Parser)]
#[command(long_about = "Verify the Ed25519 signature on a commit.\n\n\
        Examples:\n  \
        ovc verify HEAD                    Verify the current commit\n  \
        ovc verify abc123def456...         Verify a specific commit")]
pub struct VerifyArgs {
    /// Commit id (hex) or 'HEAD' to verify.
    #[arg(default_value = "HEAD")]
    pub commit: String,
}

/// Arguments for `ovc revert`.
#[derive(Debug, Parser)]
pub struct RevertArgs {
    /// Commit to revert (hex id, branch name, HEAD~N, etc.).
    pub commit: String,
}

/// Arguments for `ovc init`.
#[derive(Debug, Parser)]
#[command(long_about = "Create a new encrypted OVC repository.\n\n\
        Examples:\n  \
        ovc init                                    Create repo.ovc in current dir\n  \
        ovc init --name myproject.ovc               Custom filename\n  \
        ovc init --key alice                        Use key pair instead of password\n  \
        ovc init --store ~/iCloud/ovc/myproject.ovc Store in iCloud (auto-sync)")]
pub struct InitArgs {
    /// Directory to initialize the repository in.
    #[arg(default_value = ".")]
    pub path: PathBuf,

    /// Name of the .ovc file.
    #[arg(long, default_value = "repo.ovc")]
    pub name: String,

    /// Name of the default branch.
    #[arg(long, default_value = "main")]
    pub default_branch: String,

    /// Initialize using an SSH key pair instead of a password.
    /// Specify a key fingerprint or name from ~/.ssh/ovc/.
    #[arg(long)]
    pub key: Option<String>,

    /// Store the .ovc file at this path instead of the current directory.
    /// Creates a .ovc-link file pointing to the actual location.
    /// Useful for storing repos in iCloud, Google Drive, etc.
    #[arg(long)]
    pub store: Option<PathBuf>,
}

/// Arguments for `ovc add`.
#[derive(Debug, Parser)]
pub struct AddArgs {
    /// Files to stage.
    #[arg(required_unless_present = "all")]
    pub paths: Vec<String>,

    /// Stage all modified and untracked files.
    #[arg(short, long)]
    pub all: bool,

    /// Stage even ignored files.
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `ovc commit`.
#[derive(Debug, Parser)]
#[allow(clippy::struct_excessive_bools)]
#[command(long_about = "Record staged changes as a new commit.\n\n\
        Examples:\n  \
        ovc commit -m \"initial commit\"              Commit staged files\n  \
        ovc commit -a -m \"fix typo\"                 Auto-stage modified files\n  \
        ovc commit --author \"Alice <a@b.com>\" -m x  Set commit author")]
pub struct CommitArgs {
    /// Commit message.
    #[arg(short, long)]
    pub message: Option<String>,

    /// Author identity ("Name <email>").
    #[arg(long)]
    pub author: Option<String>,

    /// Automatically stage all modified tracked files before committing.
    #[arg(short, long)]
    pub all: bool,

    /// Sign the commit with your Ed25519 key.
    #[arg(short = 'S', long)]
    pub sign: bool,

    /// Skip pre-commit hooks.
    #[arg(long)]
    pub no_verify: bool,

    /// Amend the previous commit instead of creating a new one.
    #[arg(long)]
    pub amend: bool,
}

/// Arguments for `ovc status`.
#[derive(Debug, Parser)]
pub struct StatusArgs {
    /// Show output in short format.
    #[arg(short, long)]
    pub short: bool,
}

/// Arguments for `ovc log`.
#[derive(Debug, Parser)]
#[allow(clippy::struct_excessive_bools)]
pub struct LogArgs {
    /// Maximum number of commits to show.
    #[arg(short = 'n', long)]
    pub max_count: Option<usize>,

    /// Show each commit on a single line.
    #[arg(long)]
    pub oneline: bool,

    /// Show commits from all branches.
    #[arg(long)]
    pub all: bool,

    /// Show signature verification details.
    #[arg(long)]
    pub show_signatures: bool,

    /// Show branch topology graph.
    #[arg(long)]
    pub graph: bool,
}

/// Arguments for `ovc diff`.
#[derive(Debug, Parser)]
pub struct DiffArgs {
    /// Show staged changes (index vs HEAD).
    #[arg(long, alias = "cached")]
    pub staged: bool,

    /// Show only a summary of changes (files changed, insertions, deletions).
    #[arg(long)]
    pub stat: bool,

    /// Show only names of changed files.
    #[arg(long)]
    pub name_only: bool,

    /// Paths to restrict diff output (or branch-a..branch-b range).
    pub paths: Vec<String>,
}

/// Arguments for `ovc branch`.
#[derive(Debug, Parser)]
pub struct BranchArgs {
    /// Name of the branch to create.
    pub name: Option<String>,

    /// Commit to create the branch at (default: HEAD).
    ///
    /// Accepts a full or short commit hash, branch name, tag name, or HEAD~N.
    pub start_point: Option<String>,

    /// Delete the specified branch.
    #[arg(short = 'd', long)]
    pub delete: Option<String>,

    /// Force delete a branch even if not fully merged.
    #[arg(short = 'D', long = "force-delete")]
    pub force_delete: Option<String>,

    /// List branches.
    #[arg(short, long)]
    pub list: bool,

    /// Show all branches (local and remote).
    #[arg(short, long)]
    pub all: bool,

    /// Rename a branch: `ovc branch -m <old-name> <new-name>`.
    #[arg(short = 'm', long = "move", num_args = 2, value_names = ["OLD", "NEW"])]
    pub rename: Option<Vec<String>>,
}

/// Arguments for `ovc checkout`.
#[derive(Debug, Parser)]
pub struct CheckoutArgs {
    /// Branch or commit to switch to (omit when restoring paths).
    pub target: Option<String>,

    /// Create a new branch and switch to it.
    #[arg(short = 'b')]
    pub new_branch: Option<String>,

    /// Force checkout, discarding local modifications.
    #[arg(short = 'f', long)]
    pub force: bool,

    /// Paths to restore from HEAD (discard working directory changes).
    #[arg(last = true)]
    pub paths: Vec<String>,
}

/// Arguments for `ovc tag`.
#[derive(Debug, Parser)]
pub struct TagArgs {
    /// Name of the tag to create.
    pub name: Option<String>,

    /// Delete the specified tag.
    #[arg(short = 'd', long)]
    pub delete: Option<String>,

    /// List tags.
    #[arg(short, long)]
    pub list: bool,

    /// Tag annotation message (creates an annotated tag).
    #[arg(short, long)]
    pub message: Option<String>,
}

/// Arguments for `ovc merge`.
#[derive(Debug, Parser)]
pub struct MergeArgs {
    /// Branch to merge into the current branch.
    pub branch: String,

    /// Skip pre-merge and post-merge hooks.
    #[arg(long)]
    pub no_verify: bool,
}

/// Arguments for `ovc remote`.
#[derive(Debug, Parser)]
pub struct RemoteArgs {
    #[command(subcommand)]
    pub action: Option<RemoteAction>,
}

/// Arguments for `ovc git-import`.
#[derive(Debug, Parser)]
#[command(long_about = "Import a Git repository into OVC format.\n\n\
        Converts all branches, tags, and history into a single encrypted .ovc file.\n\n\
        Examples:\n  \
        ovc git-import ./my-project                  Import from local Git repo\n  \
        ovc git-import ./my-project -o project.ovc   Custom output path")]
pub struct GitImportArgs {
    /// Path to the Git repository to import.
    pub git_repo: PathBuf,

    /// Output path for the `.ovc` file (default: `<repo-name>.ovc` in current directory).
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Arguments for `ovc git-export`.
#[derive(Debug, Parser)]
#[command(
    long_about = "Export an OVC repository to a standard Git repository.\n\n\
        Reconstructs the full working tree and Git history from the .ovc file.\n\n\
        Examples:\n  \
        ovc git-export project.ovc                   Export to derived directory\n  \
        ovc git-export project.ovc -o ./exported     Custom output directory"
)]
pub struct GitExportArgs {
    /// Path to the `.ovc` file to export.
    pub ovc_file: PathBuf,

    /// Output directory for the Git repository (default: derived from `.ovc` filename).
    #[arg(short, long)]
    pub output: Option<PathBuf>,
}

/// Arguments for `ovc push`.
#[derive(Debug, Parser)]
#[command(long_about = "Push the local repository to a configured remote.\n\n\
        Examples:\n  \
        ovc push                    Push to origin\n  \
        ovc push --remote backup    Push to a named remote\n  \
        ovc push --force            Force push (overwrites remote)")]
pub struct PushArgs {
    /// Name of the remote to push to.
    #[arg(long, default_value = "origin")]
    pub remote: String,

    /// Force push even if the remote has diverged.
    #[arg(short, long)]
    pub force: bool,

    /// Skip pre-push hooks.
    #[arg(long)]
    pub no_verify: bool,
}

/// Arguments for `ovc pull`.
#[derive(Debug, Parser)]
#[command(long_about = "Pull the latest version from a configured remote.\n\n\
        Examples:\n  \
        ovc pull                    Pull from origin\n  \
        ovc pull --remote backup    Pull from a named remote")]
pub struct PullArgs {
    /// Name of the remote to pull from.
    #[arg(long, default_value = "origin")]
    pub remote: String,
}

/// Arguments for `ovc sync`.
#[derive(Debug, Parser)]
#[command(long_about = "Merge remote changes from the shared .ovc file.\n\n\
        When multiple users share a single .ovc file (e.g., via iCloud), each\n\
        working on their own branch, 'ovc sync' imports remote branches, tags,\n\
        objects, and notes into the local repository and saves the merged result.\n\n\
        Example:\n  \
        ovc sync    Merge and save")]
pub struct SyncArgs;

/// Arguments for `ovc sync-status`.
#[derive(Debug, Parser)]
pub struct SyncStatusArgs {
    /// Name of the remote to check against.
    #[arg(long, default_value = "origin")]
    pub remote: String,
}

/// Remote sub-actions.
#[derive(Debug, Subcommand)]
pub enum RemoteAction {
    /// Add a remote.
    Add {
        /// Remote name.
        name: String,
        /// Remote URL or path (for local: a filesystem path; for gcs: bucket/prefix).
        url: String,
        /// Backend type: "local" or "gcs".
        #[arg(long, default_value = "local")]
        backend: String,
    },
    /// Remove a remote.
    Remove {
        /// Remote name.
        name: String,
    },
    /// List all remotes.
    List,
}

/// Arguments for `ovc stash`.
#[derive(Debug, Parser)]
pub struct StashArgs {
    #[command(subcommand)]
    pub action: Option<StashAction>,
}

/// Stash sub-actions.
#[derive(Debug, Subcommand)]
pub enum StashAction {
    /// Save the current index state (default if no subcommand).
    Push {
        /// Stash description message.
        #[arg(short, long, default_value = "WIP")]
        message: String,
    },
    /// Restore and remove the most recent (or specified) stash entry.
    Pop {
        /// Stash index to pop (default: 0).
        #[arg(default_value = "0")]
        index: usize,
    },
    /// Restore a stash entry without removing it.
    Apply {
        /// Stash index to apply (default: 0).
        #[arg(default_value = "0")]
        index: usize,
    },
    /// Remove a stash entry without restoring it.
    Drop {
        /// Stash index to drop (default: 0).
        #[arg(default_value = "0")]
        index: usize,
    },
    /// List all stash entries.
    List,
    /// Remove all stash entries.
    Clear,
}

/// Arguments for `ovc rebase`.
#[derive(Debug, Parser)]
pub struct RebaseCliArgs {
    /// Target branch to rebase onto.
    pub onto: String,
}

/// Arguments for `ovc cherry-pick`.
#[derive(Debug, Parser)]
pub struct CherryPickArgs {
    /// Full commit id (hex) to cherry-pick.
    pub commit: String,
}

/// Arguments for `ovc bisect`.
#[derive(Debug, Parser)]
pub struct BisectCliArgs {
    #[command(subcommand)]
    pub action: BisectAction,
}

/// Bisect sub-actions.
#[derive(Debug, Subcommand)]
pub enum BisectAction {
    /// Start a bisect session.
    Start {
        /// The known-good commit id (hex).
        good: String,
        /// The known-bad commit id (hex).
        bad: String,
    },
    /// Mark a commit as good (default: current bisect commit).
    Good {
        /// Commit to mark as good (default: current bisect commit).
        commit: Option<String>,
    },
    /// Mark a commit as bad (default: current bisect commit).
    Bad {
        /// Commit to mark as bad (default: current bisect commit).
        commit: Option<String>,
    },
    /// End the bisect session.
    Reset,
}

/// Arguments for `ovc gc`.
#[derive(Debug, Parser)]
pub struct GcArgs {
    /// Show what would be removed without actually removing.
    #[arg(long)]
    pub dry_run: bool,
}

/// Arguments for `ovc actions`.
#[derive(Debug, Parser)]
#[command(
    long_about = "Manage and run actions (lint, format, build, test, audit).\n\n\
        Actions are configured per-repository in .ovc/actions.yml and can run\n\
        automatically on hooks or manually.\n\n\
        Examples:\n  \
        ovc actions init                 Auto-detect languages and create config\n  \
        ovc actions list                 List all configured actions\n  \
        ovc actions run rust-check       Run a specific action\n  \
        ovc actions run --trigger pre-commit  Run all pre-commit actions"
)]
pub struct ActionsArgs {
    #[command(subcommand)]
    pub action: ActionsAction,
}

/// Arguments for `ovc key`.
#[derive(Debug, Parser)]
#[command(long_about = "Manage SSH key pairs for OVC repository encryption.\n\n\
        Examples:\n  \
        ovc key generate --name mykey    Create a new key pair\n  \
        ovc key list                     List available keys\n  \
        ovc key export mykey             Export key for backup\n  \
        ovc key import backup.json       Import a previously exported key")]
pub struct KeyArgs {
    #[command(subcommand)]
    pub action: KeyAction,
}

/// Key management sub-commands.
#[derive(Debug, Subcommand)]
pub enum KeyAction {
    /// Generate a new SSH key pair for OVC repository encryption.
    #[command(
        long_about = "Generate a new Ed25519 key pair for OVC repository encryption.\n\n\
            Keys are stored in ~/.ssh/ovc/ and protected with a passphrase.\n\n\
            Examples:\n  \
            ovc key generate                  Create key with default name\n  \
            ovc key generate --name deploy    Create key named 'deploy'"
    )]
    Generate {
        /// Name for the key pair (used as filename in ~/.ssh/ovc/).
        #[arg(long, default_value = "default")]
        name: String,
        /// Identity for signing (e.g., "Alice <alice@example.com>").
        #[arg(long)]
        identity: Option<String>,
    },
    /// List all key pairs in ~/.ssh/ovc/.
    List,
    /// Export a key pair for storage in a password manager.
    #[command(
        long_about = "Export a key pair as JSON for safe storage in a password manager.\n\n\
            Example:\n  \
            ovc key export mykey > mykey-backup.json"
    )]
    Export {
        /// Name of the key to export.
        name: String,
    },
    /// Import a key pair from a password manager export.
    Import {
        /// Path to the exported key file (or - for stdin).
        #[arg(default_value = "-")]
        path: String,
        /// Custom name for the imported key (default: derived from fingerprint).
        ///
        /// Must contain only alphanumeric characters, hyphens, underscores, and dots.
        /// No path separators or special characters are allowed.
        #[arg(long)]
        name: Option<String>,
    },
    /// Add a public key to the current repository (grant access).
    Add {
        /// Path to the public key file (.pub).
        public_key_path: PathBuf,
    },
    /// Remove a key from the current repository (revoke access).
    Remove {
        /// Fingerprint of the key to remove.
        fingerprint: String,
    },
    /// List authorized keys for the current repository.
    Authorized,
}

/// Actions sub-commands.
#[derive(Debug, Subcommand)]
pub enum ActionsAction {
    /// Initialize actions configuration by detecting languages.
    Init {
        /// Overwrite existing configuration.
        #[arg(long)]
        force: bool,
    },
    /// List configured actions.
    List {
        /// Filter by trigger (pre-commit, post-commit, pre-push, manual, schedule).
        #[arg(long)]
        trigger: Option<String>,
        /// Filter by category (lint, format, build, test, audit, builtin, custom).
        #[arg(long)]
        category: Option<String>,
    },
    /// Run one or more actions.
    Run {
        /// Action names to run. If empty, requires --trigger.
        names: Vec<String>,
        /// Run all actions for the given trigger.
        #[arg(long)]
        trigger: Option<String>,
        /// Use fix commands where available.
        #[arg(long)]
        fix: bool,
        /// Skip hook verification (no-op for manual runs).
        #[arg(long)]
        no_verify: bool,
    },
    /// Show action run history.
    History {
        /// Maximum number of runs to show.
        #[arg(short = 'n', long, default_value = "10")]
        limit: usize,
        /// Show details for a specific run.
        #[arg(long)]
        run_id: Option<String>,
    },
    /// Detect languages in the repository.
    Detect,
    /// Manage action secrets.
    Secrets {
        #[command(subcommand)]
        action: SecretsAction,
    },
}

/// Secrets sub-commands for `ovc actions secrets`.
#[derive(Debug, Subcommand)]
pub enum SecretsAction {
    /// List secret names (values are never shown).
    List,
    /// Set a secret value.
    Set {
        /// Secret name.
        name: String,
        /// Secret value.
        value: String,
    },
    /// Remove a secret.
    Remove {
        /// Secret name to remove.
        name: String,
    },
}

/// Arguments for `ovc serve`.
#[derive(Debug, Args)]
#[command(long_about = "Start the OVC API server and embedded web UI.\n\n\
        The server exposes a REST API for repository operations and serves\n\
        the built-in web frontend for browser-based access.\n\n\
        Examples:\n  \
        ovc serve                                  Start on 127.0.0.1:9742\n  \
        ovc serve --port 8080                      Custom port\n  \
        ovc serve --bind 0.0.0.0 --port 9742      Listen on all interfaces\n  \
        ovc serve --repos-dir ~/projects           Serve repos from a directory\n  \
        ovc serve --cors-origin http://app.local   Allow cross-origin requests")]
pub struct ServeArgs {
    /// Port to listen on.
    #[arg(long, default_value = "9742", env = "OVC_PORT")]
    pub port: u16,
    /// Bind address.
    #[arg(long, default_value = "127.0.0.1", env = "OVC_BIND")]
    pub bind: String,
    /// Directory containing .ovc repository files.
    #[arg(long, default_value = ".", env = "OVC_REPOS_DIR")]
    pub repos_dir: std::path::PathBuf,
    /// JWT secret for API authentication.
    #[arg(long, env = "OVC_JWT_SECRET", hide_env_values = true)]
    pub jwt_secret: Option<String>,
    /// Allowed CORS origins (repeatable).
    #[arg(long, env = "OVC_CORS_ORIGINS")]
    pub cors_origin: Vec<String>,
    /// Map repo to its working directory (repeatable).
    /// Format: `repo_id:/path/to/workdir`
    #[arg(long)]
    pub workdir: Vec<String>,
    /// Directories to scan for .ovc-link files (auto-discover workdirs).
    /// Scans one level deep for project directories containing .ovc-link.
    #[arg(long, env = "OVC_WORKDIR_SCAN")]
    pub workdir_scan: Vec<String>,
}

/// Arguments for `ovc web` (aliases: `ovc ui`, `ovc gui`).
#[derive(Debug, Parser)]
#[command(long_about = "Open the OVC web UI in the default browser.\n\n\
        Detects the running OVC daemon port and opens it. If no daemon is running,\n\
        starts a temporary server.\n\n\
        Examples:\n  \
        ovc web                     Open UI on default port (9742)\n  \
        ovc web --port 8080         Open UI on custom port\n  \
        ovc ui                      Alias for ovc web\n  \
        ovc gui                     Alias for ovc web")]
pub struct WebArgs {
    /// Port to connect to (default: 9742).
    #[arg(long, default_value = "9742", env = "OVC_PORT")]
    pub port: u16,
}

/// Arguments for `ovc daemon`.
#[derive(Debug, Args)]
pub struct DaemonArgs {
    #[command(subcommand)]
    pub action: DaemonAction,
}

/// Daemon management sub-commands.
#[derive(Debug, Subcommand)]
pub enum DaemonAction {
    /// Install and start the daemon (runs `ovc serve` on boot via macOS `LaunchAgent`).
    Install {
        /// Port for the server.
        #[arg(long, default_value = "9742")]
        port: u16,
    },
    /// Uninstall the daemon (unload `LaunchAgent` and remove plist).
    Uninstall,
    /// Start the daemon.
    Start,
    /// Stop the daemon.
    Stop,
    /// Show daemon status and health.
    Status,
    /// View daemon logs.
    Logs {
        /// Follow log output (tail -f).
        #[arg(short, long)]
        follow: bool,
    },
}

// ── New command arguments ───────────────────────────────────────────────

/// Arguments for `ovc blame`.
#[derive(Debug, Parser)]
pub struct BlameArgs {
    /// File path to blame.
    pub file: String,

    /// Line range to blame (e.g., "10,20" or "10,+5").
    #[arg(short = 'L', long)]
    pub lines: Option<String>,
}

/// Arguments for `ovc reset`.
#[derive(Debug, Parser)]
#[command(long_about = "Reset HEAD to a specified commit.\n\n\
        Modes:\n  \
        --soft   Move HEAD only (keep index and working directory)\n  \
        --mixed  Move HEAD and reset index (default)\n  \
        --hard   Move HEAD, reset index, and working directory\n\n\
        Default target: HEAD~1 (parent of current commit)")]
pub struct ResetArgs {
    /// Target commit (hex id, branch name, or HEAD~N).
    pub commit: Option<String>,

    /// Soft reset: move HEAD only.
    #[arg(long)]
    pub soft: bool,

    /// Mixed reset: move HEAD and reset index (default).
    #[arg(long)]
    pub mixed: bool,

    /// Hard reset: reset HEAD, index, and working directory.
    #[arg(long)]
    pub hard: bool,

    /// Paths to unstage (reset to HEAD version in index).
    #[arg(last = true)]
    pub paths: Vec<String>,
}

/// Arguments for `ovc clean`.
#[derive(Debug, Parser)]
pub struct CleanArgs {
    /// Show what would be removed without deleting.
    #[arg(short = 'n', long)]
    pub dry_run: bool,

    /// Actually remove untracked files (required).
    #[arg(short, long)]
    pub force: bool,
}

/// Arguments for `ovc show`.
#[derive(Debug, Parser)]
pub struct ShowArgs {
    /// Commit id or 'HEAD' (default: HEAD).
    pub commit: Option<String>,
}

/// Arguments for `ovc grep`.
#[derive(Debug, Parser)]
pub struct GrepArgs {
    /// Regular expression pattern to search for.
    pub pattern: String,

    /// Case-insensitive search.
    #[arg(short = 'i', long)]
    pub case_insensitive: bool,

    /// Show only match counts per file.
    #[arg(long)]
    pub count: bool,
}

/// Arguments for `ovc reflog`.
#[derive(Debug, Parser)]
pub struct ReflogArgs;

/// Arguments for `ovc ls-files`.
#[derive(Debug, Parser)]
#[allow(clippy::struct_excessive_bools)]
pub struct LsFilesArgs {
    /// Show only staged files (default).
    #[arg(long)]
    pub staged: bool,

    /// Show modified files.
    #[arg(long)]
    pub modified: bool,

    /// Show deleted files.
    #[arg(long)]
    pub deleted: bool,

    /// Show untracked files.
    #[arg(long)]
    pub untracked: bool,
}

/// Arguments for `ovc describe`.
#[derive(Debug, Parser)]
pub struct DescribeArgs {
    /// Commit id or 'HEAD' (default: HEAD).
    pub commit: Option<String>,
}

/// Arguments for `ovc shortlog`.
#[derive(Debug, Parser)]
pub struct ShortlogArgs {
    /// Show only commit count per author.
    #[arg(short, long)]
    pub summary: bool,

    /// Sort by number of commits (descending).
    #[arg(short = 'n', long)]
    pub sort_by_count: bool,
}

/// Arguments for `ovc notes`.
#[derive(Debug, Parser)]
pub struct NotesArgs {
    #[command(subcommand)]
    pub action: Option<NotesAction>,
}

/// Notes sub-actions.
#[derive(Debug, Subcommand)]
pub enum NotesAction {
    /// Show the note on a commit.
    Show {
        /// Commit id or 'HEAD' (default: HEAD).
        commit: Option<String>,
    },
    /// Add or replace a note on a commit.
    Add {
        /// Note message.
        #[arg(short, long)]
        message: String,
        /// Commit id or 'HEAD' (default: HEAD).
        commit: Option<String>,
    },
    /// Remove the note from a commit.
    Remove {
        /// Commit id or 'HEAD' (default: HEAD).
        commit: Option<String>,
    },
}

/// Arguments for `ovc archive`.
#[derive(Debug, Parser)]
pub struct ArchiveArgs {
    /// Archive format: tar or zip.
    #[arg(long)]
    pub format: Option<String>,

    /// Output file path (required for zip, optional for tar).
    #[arg(short, long)]
    pub output: Option<std::path::PathBuf>,

    /// Commit to archive (default: HEAD).
    pub commit: Option<String>,
}

/// Arguments for `ovc submodule`.
#[derive(Debug, Parser)]
pub struct SubmoduleArgs {
    #[command(subcommand)]
    pub action: Option<SubmoduleAction>,
}

/// Arguments for `ovc onboard`.
#[derive(Debug, Parser)]
#[command(long_about = "Interactive setup wizard for new OVC users.\n\n\
        In non-interactive mode all prompts are skipped and configuration\n\
        is taken entirely from flags and environment variables.\n\n\
        Examples:\n  \
        ovc onboard                                         Interactive wizard\n  \
        ovc onboard --non-interactive --name mykey --identity \"Alice <a@b.com>\"  CI mode")]
pub struct OnboardArgs {
    /// Skip all prompts and configure from flags/environment variables.
    #[arg(long)]
    pub non_interactive: bool,

    /// Key name to use in non-interactive mode.
    #[arg(long)]
    pub name: Option<String>,

    /// Author identity in non-interactive mode, e.g. "Alice <alice@example.com>".
    #[arg(long)]
    pub identity: Option<String>,
}

/// Submodule sub-actions.
#[derive(Debug, Subcommand)]
pub enum SubmoduleAction {
    /// Add a submodule.
    Add {
        /// Name to register the submodule under.
        name: String,
        /// Remote URL or path of the submodule.
        url: String,
        /// Local path to place the submodule (defaults to name if omitted).
        path: Option<String>,
    },
    /// Show submodule status.
    Status,
    /// Update all submodules.
    Update,
    /// Remove a submodule by name.
    Remove {
        /// Submodule name.
        name: String,
    },
}

// ── Access Control ──────────────────────────────────────────────────────

/// Arguments for `ovc access`.
#[derive(Debug, Parser)]
#[command(
    long_about = "Manage per-user access control for OVC repositories.\n\n\
        Examples:\n  \
        ovc access list                          List who has access\n  \
        ovc access grant teammate --role write   Grant write access\n  \
        ovc access revoke SHA256:abc123          Revoke access\n  \
        ovc access set-role SHA256:abc123 --role admin  Change role"
)]
pub struct AccessArgs {
    #[command(subcommand)]
    pub action: AccessAction,
}

/// Access management sub-commands.
#[derive(Debug, Subcommand)]
pub enum AccessAction {
    /// Grant access to a user by their public key file or key name.
    Grant {
        /// Path to .pub file or key name in ~/.ssh/ovc/.
        key: String,
        /// Role to assign: read, write, admin, or owner.
        #[arg(long, default_value = "write")]
        role: String,
    },
    /// Revoke a user's access by their key fingerprint.
    Revoke {
        /// Key fingerprint (SHA256:...).
        fingerprint: String,
    },
    /// List all users with access and their roles.
    List,
    /// Change a user's role.
    SetRole {
        /// Key fingerprint (SHA256:...).
        fingerprint: String,
        /// New role: read, write, admin, or owner.
        #[arg(long)]
        role: String,
    },
}

/// Arguments for `ovc branch-protect`.
#[derive(Debug, Parser)]
#[command(long_about = "Configure branch protection rules.\n\n\
        Examples:\n  \
        ovc branch-protect main --required-approvals 2 --require-ci\n  \
        ovc branch-protect main --remove")]
pub struct BranchProtectArgs {
    /// Branch name to protect.
    pub branch: String,
    /// Number of required review approvals before merge.
    #[arg(long, default_value = "1")]
    pub required_approvals: u32,
    /// Require CI checks to pass before merge.
    #[arg(long)]
    pub require_ci: bool,
    /// Remove protection from this branch.
    #[arg(long)]
    pub remove: bool,
}
