//! CLI entry — clap-derive command parsing.
//!
//! Two subcommands:
//!
//! - `build` — walk a glob of vimdoc files and produce a full-text index at
//!   the given output path. The index format is opaque to the CLI; the
//!   adapter that owns it (tantivy, in a follow-up) decides shape.
//! - `search` — open the index at `--index=<path>` and answer a query,
//!   printing hits ranked by score.
//!
//! On this PR both subcommands validate args and return a "not yet
//! implemented" error — the parser and search-engine adapters land next.
//! The e2e tests below anchor the CLI *shape* so downstream implementation
//! PRs can slot into stable exit codes and error messages.

use clap::{Parser, Subcommand};
use std::path::PathBuf;

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
        /// The query text. Wrap in quotes for multi-word queries.
        query: String,
    },
}

/// Parse args and dispatch to the appropriate subcommand.
pub fn run() -> anyhow::Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Command::Build { docs, out } => run_build(&docs, &out),
        Command::Search {
            index,
            limit,
            query,
        } => run_search(&index, limit, &query),
    }
}

fn run_build(docs: &str, out: &std::path::Path) -> anyhow::Result<()> {
    // Placeholder body — feat(parser) + feat(indexer) fill this in.
    // The message shape is stable so downstream implementations only
    // need to swap the body, not the surrounding CLI plumbing.
    Err(anyhow::anyhow!(
        "build not yet implemented (docs='{}', out='{}')",
        docs,
        out.display()
    ))
}

fn run_search(index: &std::path::Path, limit: usize, query: &str) -> anyhow::Result<()> {
    let _ = crate::domain::Query::new(query, limit)?;
    Err(anyhow::anyhow!(
        "search not yet implemented (index='{}', limit={}, query='{}')",
        index.display(),
        limit,
        query
    ))
}

// CLI end-to-end tests live in tests/cli.rs (an integration test), NOT here.
// A `cargo test` from within src/cli/mod.rs runs as a library unit test — the
// test harness does not rebuild the binary and CARGO_BIN_EXE_* is unset, so
// assert_cmd's cargo_bin() picks up whatever stale binary is already at
// target/debug/. Placing the tests under tests/ makes cargo treat them as
// binary integration tests and rebuild the bin dep on demand.
