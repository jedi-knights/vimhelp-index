//! End-to-end tests for the `vimhelp-index` binary. Integration test position
//! (tests/) so cargo sets `CARGO_BIN_EXE_vimhelp-index` and rebuilds the bin
//! on demand — a src/-position unit test would run against a stale binary.

use assert_cmd::Command;
use predicates::prelude::*;

fn bin() -> Command {
    Command::cargo_bin("vimhelp-index").unwrap()
}

#[test]
fn top_level_help_lists_both_subcommands() {
    bin()
        .arg("--help")
        .assert()
        .success()
        .stdout(predicate::str::contains("build"))
        .stdout(predicate::str::contains("search"));
}

#[test]
fn build_help_names_required_flags() {
    bin()
        .args(["build", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--docs"))
        .stdout(predicate::str::contains("--out"));
}

#[test]
fn search_help_names_flags_and_positional() {
    bin()
        .args(["search", "--help"])
        .assert()
        .success()
        .stdout(predicate::str::contains("--index"))
        .stdout(predicate::str::contains("--limit"))
        .stdout(predicate::str::contains("<QUERY>"));
}

#[test]
fn build_missing_required_args_exits_non_zero_with_actionable_message() {
    bin()
        .arg("build")
        .assert()
        .failure()
        .stderr(predicate::str::contains("--docs"));
}

#[test]
fn build_reports_not_yet_implemented_when_args_are_valid() {
    // Anchor the placeholder shape so downstream PRs replacing the body
    // notice they're changing user-visible behaviour.
    bin()
        .args([
            "build",
            "--docs",
            "/nonexistent/**/*.txt",
            "--out",
            "/tmp/vh-scratch-idx",
        ])
        .assert()
        .failure()
        .stderr(predicate::str::contains("not yet implemented"));
}

#[test]
fn search_rejects_empty_query_via_domain_validator() {
    // Query::new returns an error for whitespace-only input; the CLI
    // must propagate it (not swallow it as "not yet implemented").
    bin()
        .args(["search", "--index", "./idx", "   "])
        .assert()
        .failure()
        .stderr(predicate::str::contains("query text must not be empty"));
}
