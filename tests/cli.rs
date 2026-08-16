//! End-to-end tests for the `vimhelp-index` binary. Integration test position
//! (tests/) so cargo sets `CARGO_BIN_EXE_vimhelp-index` and rebuilds the bin
//! on demand — a src/-position unit test would run against a stale binary.

use assert_cmd::Command;
use predicates::prelude::*;
use std::path::PathBuf;

fn bin() -> Command {
    Command::cargo_bin("vimhelp-index").unwrap()
}

/// Write a small realistic vimdoc file into `dir`. Returns the file path
/// so callers can build a `--docs` glob against it.
fn write_fixture(dir: &std::path::Path, name: &str, body: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, body).unwrap();
    path
}

// -- --help tests -------------------------------------------------------

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
        .stdout(predicate::str::contains("--format"))
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

// -- build behaviour ----------------------------------------------------

#[test]
fn build_errors_when_glob_matches_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let pattern = format!("{}/*.nonexistent", tmp.path().display());
    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(tmp.path().join("idx"))
        .assert()
        .failure()
        .stderr(predicate::str::contains("no files matched"));
}

#[test]
fn build_indexes_files_and_prints_summary() {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* one file\n\n==============================================================================\nSection A *a-section*\n\nfloating window text\n",
    );
    write_fixture(
        &docs_dir,
        "b.txt",
        "*b.txt* second file\n\n==============================================================================\nSection B *b-section*\n\nquick jumps and marks\n",
    );

    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());

    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("Indexed"))
        .stdout(predicate::str::contains("2 file(s)"));

    // Tantivy writes segment files into the output directory.
    assert!(idx.is_dir(), "expected index directory to exist");
    let entries: Vec<_> = std::fs::read_dir(&idx).unwrap().collect();
    assert!(
        !entries.is_empty(),
        "expected tantivy to write segment files into --out"
    );
}

// -- search behaviour ---------------------------------------------------

/// Build a small index into a tempdir and return its path (kept alive by
/// the returned `TempDir` handle). Fixtures cover two files so
/// per-section attribution has room to differ across hits.
fn build_test_index() -> (tempfile::TempDir, PathBuf) {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "telescope.txt",
        "*telescope.txt* fuzzy pickers\n\n==============================================================================\nFind files *telescope-find-files*\n\nfuzzy find files inside a floating window with previews\n",
    );
    write_fixture(
        &docs_dir,
        "harpoon.txt",
        "*harpoon.txt* marks\n\n==============================================================================\nQuick jumps *harpoon-jump*\n\nquick jumps to marked files\n",
    );

    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());
    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success();
    (tmp, idx)
}

#[test]
fn search_console_finds_and_ranks_relevant_hit() {
    let (_tmp, idx) = build_test_index();
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .args(["--format", "console", "floating window"])
        .assert()
        .success()
        .stdout(predicate::str::contains("1. telescope-find-files"))
        .stdout(predicate::str::contains("floating window"));
}

#[test]
fn search_console_prints_no_hits_message_for_missing_term() {
    let (_tmp, idx) = build_test_index();
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("nonexistent-xyz-term")
        .assert()
        .success()
        .stdout(predicate::str::contains("no hits"));
}

#[test]
fn search_json_output_is_parseable_and_has_expected_shape() {
    let (_tmp, idx) = build_test_index();
    let output = bin()
        .args(["search", "--index"])
        .arg(&idx)
        .args(["--format", "json", "floating"])
        .assert()
        .success()
        .get_output()
        .stdout
        .clone();

    let text = String::from_utf8(output).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(text.trim()).unwrap();
    assert_eq!(parsed["query"], "floating");
    let hits = parsed["hits"].as_array().expect("hits array");
    assert!(!hits.is_empty(), "expected at least one hit");
    assert_eq!(hits[0]["tag"], "telescope-find-files");
    assert_eq!(hits[0]["section_header"], "Find files");
    assert!(hits[0]["score"].as_f64().unwrap() > 0.0);
}

#[test]
fn search_rejects_empty_query_via_domain_validator() {
    let (_tmp, idx) = build_test_index();
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("   ")
        .assert()
        .failure()
        .stderr(predicate::str::contains("query text must not be empty"));
}

#[test]
fn search_rejects_invalid_format_value() {
    let (_tmp, idx) = build_test_index();
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .args(["--format", "yaml", "any"])
        .assert()
        .failure()
        .stderr(predicate::str::contains("invalid --format"));
}

#[test]
fn search_against_missing_index_dir_fails_with_actionable_error() {
    let tmp = tempfile::tempdir().unwrap();
    let missing = tmp.path().join("does-not-exist");
    bin()
        .args(["search", "--index"])
        .arg(&missing)
        .arg("anything")
        .assert()
        .failure()
        .stderr(predicate::str::contains("tantivy"));
}
