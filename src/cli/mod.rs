//! CLI entry — clap-derive command parsing.
//!
//! Scaffold shape: exposes `run()` and a `--help`-renderable command.
//! Real subcommand implementations (`build`, `search`) land in feat(cli).

use clap::Parser;

/// Full-text index and search over vimdoc.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    // No subcommands wired yet — `feat(cli)` adds the derive enum with
    // `build` and `search` variants. Keeping the struct present so
    // `--help` and `--version` render against the scaffold.
}

/// Parse args and dispatch. Returns anyhow::Result so the binary can propagate
/// errors uniformly; adapter layers use thiserror types the CLI converts here.
pub fn run() -> anyhow::Result<()> {
    let _ = Cli::parse();
    println!("vimhelp-index: scaffold. Subcommands land in feat(cli).");
    Ok(())
}
