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

#[test]
fn build_unions_files_across_multiple_docs_flags() {
    let tmp = tempfile::tempdir().unwrap();
    let dir_a = tmp.path().join("a");
    let dir_b = tmp.path().join("b");
    std::fs::create_dir(&dir_a).unwrap();
    std::fs::create_dir(&dir_b).unwrap();
    write_fixture(
        &dir_a,
        "alpha.txt",
        "*alpha.txt* alpha\n\n==============================================================================\nSection A *alpha-sec*\n\nalpha content\n",
    );
    write_fixture(
        &dir_b,
        "beta.txt",
        "*beta.txt* beta\n\n==============================================================================\nSection B *beta-sec*\n\nbeta content\n",
    );

    let idx = tmp.path().join("idx");
    let glob_a = format!("{}/*.txt", dir_a.display());
    let glob_b = format!("{}/*.txt", dir_b.display());

    bin()
        .args(["build", "--docs", &glob_a, "--docs", &glob_b, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("2 file(s)"));

    // Both docs are searchable from the same index.
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("alpha")
        .assert()
        .success()
        .stdout(predicate::str::contains("alpha-sec"));
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("beta")
        .assert()
        .success()
        .stdout(predicate::str::contains("beta-sec"));
}

#[test]
fn build_silently_accepts_individual_zero_match_glob_alongside_a_matching_one() {
    // Real-world shape: `plugins/*/doc/*.txt` matches nothing on a fresh
    // install, but `$VIMRUNTIME/doc/*.txt` does. Build must succeed with
    // just the matching glob's files — not error on the empty individual
    // glob.
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* file\n\n==============================================================================\nS *a-sec*\n\ncontent\n",
    );
    let missing_glob = format!("{}/does-not-exist/*.txt", tmp.path().display());
    let real_glob = format!("{}/*.txt", docs_dir.display());
    let idx = tmp.path().join("idx");

    bin()
        .args([
            "build",
            "--docs",
            &missing_glob,
            "--docs",
            &real_glob,
            "--out",
        ])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("1 file(s)"));
}

#[test]
fn build_errors_when_all_docs_globs_match_nothing() {
    let tmp = tempfile::tempdir().unwrap();
    let a = format!("{}/*.nonexistent-a", tmp.path().display());
    let b = format!("{}/*.nonexistent-b", tmp.path().display());
    let idx = tmp.path().join("idx");
    bin()
        .args(["build", "--docs", &a, "--docs", &b, "--out"])
        .arg(&idx)
        .assert()
        .failure()
        .stderr(predicate::str::contains("no files matched"))
        // Names the count so users see how many patterns were tried.
        .stderr(predicate::str::contains("2 glob"));
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

// -- incremental build --------------------------------------------------

/// Advance a file's mtime by 2 seconds so incremental sees a change even on
/// low-precision filesystems (HFS+ rounds to whole seconds).
fn touch_file(path: &std::path::Path) {
    let now = std::time::SystemTime::now();
    let future = now + std::time::Duration::from_secs(2);
    filetime::set_file_mtime(path, filetime::FileTime::from_system_time(future)).unwrap();
}

#[test]
fn incremental_without_prior_manifest_falls_back_to_full_build() {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* file\n\n==============================================================================\nSection *a-sec*\n\nalpha content\n",
    );

    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());

    bin()
        .args(["build", "--incremental", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("no prior manifest"))
        .stdout(predicate::str::contains("Indexed"));

    // Manifest written after the fallback full build.
    assert!(idx.join("vimhelp-manifest.json").exists());
}

#[test]
fn incremental_with_unchanged_files_reports_no_changes() {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* file\n\n==============================================================================\nS *a-sec*\n\nalpha\n",
    );
    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());

    // First build primes the manifest.
    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success();

    // Second run with --incremental sees the same files → no changes.
    bin()
        .args(["build", "--incremental", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("no changes"))
        .stdout(predicate::str::contains("1 unchanged"));
}

#[test]
fn incremental_detects_touched_file_as_changed_and_reindexes_body() {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    let path = write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* file\n\n==============================================================================\nS *a-sec*\n\nfirst-body-term\n",
    );
    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());

    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success();

    // Rewrite the file with a new body term + touch the mtime so
    // (mtime, size) both change and incremental flags it.
    std::fs::write(
        &path,
        "*a.txt* file\n\n==============================================================================\nS *a-sec*\n\nsecond-body-term-different-length\n",
    )
    .unwrap();
    touch_file(&path);

    bin()
        .args(["build", "--incremental", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("changed:   1"));

    // Old term is gone; new term is searchable.
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("first-body-term")
        .assert()
        .success()
        .stdout(predicate::str::contains("no hits"));
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("second-body-term-different-length")
        .assert()
        .success()
        .stdout(predicate::str::contains("1. a-sec"));
}

#[test]
fn incremental_detects_new_file_and_indexes_it() {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* file\n\n==============================================================================\nS *a-sec*\n\nalpha-body\n",
    );
    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());

    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success();

    // Add a second file to the corpus.
    write_fixture(
        &docs_dir,
        "b.txt",
        "*b.txt* file\n\n==============================================================================\nS *b-sec*\n\nbrand-new-beta-body\n",
    );

    bin()
        .args(["build", "--incremental", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("new:       1"));

    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("brand-new-beta-body")
        .assert()
        .success()
        .stdout(predicate::str::contains("1. b-sec"));
}

#[test]
fn incremental_detects_removed_file_and_drops_it_from_the_index() {
    let tmp = tempfile::tempdir().unwrap();
    let docs_dir = tmp.path().join("docs");
    std::fs::create_dir(&docs_dir).unwrap();
    write_fixture(
        &docs_dir,
        "a.txt",
        "*a.txt* file\n\n==============================================================================\nS *a-sec*\n\nkept-body\n",
    );
    let doomed = write_fixture(
        &docs_dir,
        "gone.txt",
        "*gone.txt* file\n\n==============================================================================\nS *gone-sec*\n\ndoomed-body-term\n",
    );
    let idx = tmp.path().join("idx");
    let pattern = format!("{}/*.txt", docs_dir.display());

    bin()
        .args(["build", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success();

    // Delete the file from the corpus.
    std::fs::remove_file(&doomed).unwrap();

    bin()
        .args(["build", "--incremental", "--docs", &pattern, "--out"])
        .arg(&idx)
        .assert()
        .success()
        .stdout(predicate::str::contains("removed:   1"));

    // The removed doc's body term no longer surfaces.
    bin()
        .args(["search", "--index"])
        .arg(&idx)
        .arg("doomed-body-term")
        .assert()
        .success()
        .stdout(predicate::str::contains("no hits"));
}
