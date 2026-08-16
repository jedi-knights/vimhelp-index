//! CLI entry — clap-derive command parsing.
//!
//! Two subcommands:
//!
//! - `build` — resolve a glob of `doc/*.txt` files, parse each, and write
//!   a fresh tantivy index at `--out`.
//! - `search` — open a previously-built index and print hits ranked by
//!   score. `--format=console` (default) or `--format=json`.
//!
//! Dispatch below is thin — each subcommand's real implementation lives in
//! its own module (build.rs / search.rs) so this file stays about
//! argument shape and nothing else.
//!
//! Note: CLI end-to-end tests live in `tests/cli.rs`, not here — the
//! `assert_cmd::Command::cargo_bin("...")` helper reads `CARGO_BIN_EXE_*`
//! which is only populated for integration-position tests. A
//! src/-position unit test runs against a stale `target/debug/` binary
//! because the harness doesn't wire cargo_bin to a rebuild trigger.

use clap::{Parser, Subcommand};
use std::path::PathBuf;
use std::str::FromStr;

mod build;
mod search;

/// Full-text index and search over vimdoc.
#[derive(Parser, Debug)]
#[command(version, about, long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand, Debug)]
enum Command {
    /// Build a full-text index from a glob of vimdoc files.
    Build {
        /// Glob resolving to one or more `doc/*.txt` files.
        /// Examples:
        ///   --docs='/path/to/plugin/**/doc/*.txt'
        ///   --docs='$VIMRUNTIME/doc/*.txt'
        #[arg(long, value_name = "GLOB")]
        docs: String,
        /// Directory to write the index into. Created if missing.
        #[arg(long, value_name = "DIR")]
        out: PathBuf,
        /// Re-index only files that changed since the last build.
        /// Detects change via (mtime, size) recorded in a manifest at
        /// <out>/vimhelp-manifest.json. Falls back to a full build when
        /// the manifest is absent or on an incompatible version.
        #[arg(long)]
        incremental: bool,
    },
    /// Search a previously-built index.
    Search {
        /// Path to the index directory produced by `build`.
        #[arg(long, value_name = "DIR")]
        index: PathBuf,
        /// Maximum number of hits to return.
        /// Zero means the searcher's default; not "unbounded".
        #[arg(long, default_value_t = 20)]
        limit: usize,
        /// Output format: `console` (default) or `json`.
        #[arg(long, default_value = "console")]
        format: String,
        /// The query text. Wrap in quotes for multi-word queries.
        query: String,
    },
}

/// Parse args and dispatch to the appropriate subcommand.
pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build {
            docs,
            out,
            incremental,
        } => build::run(&docs, &out, incremental),
        Command::Search {
            index,
            limit,
            format,
            query,
        } => {
            let fmt = search::OutputFormat::from_str(&format)?;
            search::run(&index, &query, limit, fmt)
        }
    }
}
