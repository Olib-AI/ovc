//! `ovc` -- Command-line interface for OVC (Olib Version Control).

mod app;
mod commands;
mod context;
mod output;

use clap::Parser;

use app::{Cli, Command};
use context::CliContext;

fn main() -> anyhow::Result<()> {
    let cli = Cli::parse();
    let ctx = CliContext::from_cli(&cli)?;

    let result = match &cli.command {
        Command::Init(args) => commands::init::execute(&ctx, args),
        Command::Key(args) => commands::key::execute(&ctx, args),
        Command::Add(args) => commands::add::execute(&ctx, args),
        Command::Commit(args) => commands::commit::execute(&ctx, args),
        Command::Status(args) => commands::status::execute(&ctx, args),
        Command::Log(args) => commands::log::execute(&ctx, args),
        Command::Diff(args) => commands::diff::execute(&ctx, args),
        Command::Branch(args) => commands::branch::execute(&ctx, args),
        Command::Checkout(args) => commands::checkout::execute(&ctx, args),
        Command::Tag(args) => commands::tag::execute(&ctx, args),
        Command::Merge(args) => commands::merge::execute(&ctx, args),
        Command::Remote(args) => commands::remote::execute(&ctx, args),
        Command::GitImport(args) => commands::git_import::execute(&ctx, args),
        Command::GitExport(args) => commands::git_export::execute(&ctx, args),
        Command::Push(args) => commands::push::execute(&ctx, args),
        Command::Pull(args) => commands::pull::execute(&ctx, args),
        Command::Sync(args) => commands::sync::execute(&ctx, args),
        Command::SyncStatus(args) => commands::sync_status::execute(&ctx, args),
        Command::Stash(args) => commands::stash::execute(&ctx, args),
        Command::Rebase(args) => commands::rebase::execute(&ctx, args),
        Command::CherryPick(args) => commands::cherry_pick::execute(&ctx, args),
        Command::Bisect(args) => commands::bisect::execute(&ctx, args),
        Command::Gc(args) => commands::gc::execute(&ctx, args),
        Command::Verify(args) => commands::verify::execute(&ctx, args),
        Command::Actions(args) => commands::actions::execute(&ctx, args),
        Command::Serve(args) => commands::serve::execute(args),
        Command::Web(args) => commands::web::execute(args),
        Command::Daemon(args) => commands::daemon::execute(args),
        Command::Onboard(args) => commands::onboard::execute(args),
        Command::Revert(args) => commands::revert::execute(&ctx, args),
        Command::Blame(args) => commands::blame::execute(&ctx, args),
        Command::Reset(args) => commands::reset::execute(&ctx, args),
        Command::Clean(args) => commands::clean::execute(&ctx, args),
        Command::Show(args) => commands::show::execute(&ctx, args),
        Command::Grep(args) => commands::grep::execute(&ctx, args),
        Command::Reflog(args) => commands::reflog::execute(&ctx, args),
        Command::LsFiles(args) => commands::ls_files::execute(&ctx, args),
        Command::Describe(args) => commands::describe::execute(&ctx, args),
        Command::Shortlog(args) => commands::shortlog::execute(&ctx, args),
        Command::Notes(args) => commands::notes::execute(&ctx, args),
        Command::Archive(args) => commands::archive::execute(&ctx, args),
        Command::Submodule(args) => commands::submodule::execute(&ctx, args),
        Command::Access(args) => commands::access::execute(&ctx, args),
        Command::BranchProtect(args) => commands::branch_protect::execute(&ctx, args),
    };

    if let Err(ref e) = result {
        output::print_error(&format!("{e:#}"));
    }

    result
}
