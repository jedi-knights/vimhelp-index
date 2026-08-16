//! `build` subcommand — resolve one or more globs of vimdoc files, parse
//! each, and write a tantivy index at `--out`.
//!
//! Two modes:
//!
//! - **Full** (default): wipes `--out` and rebuilds from scratch.
//! - **Incremental** (`--incremental`): reads the manifest at
//!   `<out>/vimhelp-manifest.json`, classifies each current file as
//!   new/changed/removed/unchanged, and only re-indexes the changed +
//!   new subset. If the manifest is absent or on a different version,
//!   falls back to a full rebuild.
//!
//! `--docs` is repeatable — every occurrence adds one glob, and the
//! union of resolved paths (sorted + deduplicated) becomes the corpus.
//! Individual globs that match zero files are silently ignored; only
//! an empty union is an error. This handles the LazyVim shape where
//! `plugins/*/doc/*.txt` sits alongside `$VIMRUNTIME/doc/*.txt` and
//! the plugin dir may be empty on a fresh install.

use crate::adapters::manifest::{self, FileState, Manifest, ManifestDiff};
use crate::adapters::parser::vimdoc;
use crate::adapters::tantivy::TantivyIndex;
use crate::domain::Document;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Execute the `build` subcommand.
pub fn run(docs_globs: &[String], out_dir: &Path, incremental: bool) -> anyhow::Result<()> {
    let paths = resolve_globs(docs_globs)?;
    if paths.is_empty() {
        anyhow::bail!(
            "no files matched any --docs pattern (checked {} glob(s): {:?})",
            docs_globs.len(),
            docs_globs
        );
    }

    let current_states = manifest::stat_all(&paths)?;

    if incremental {
        let prior = Manifest::load(out_dir)?;
        if let Some(prior) = prior {
            return run_incremental(&current_states, prior, out_dir);
        }
        // No usable manifest — full build. Report so users understand
        // why the first --incremental after a wipe took full-build time.
        println!(
            "no prior manifest at {} — running full build",
            out_dir.display()
        );
    }

    run_full(paths, current_states, out_dir)
}

fn run_full(
    paths: Vec<PathBuf>,
    current_states: HashMap<PathBuf, FileState>,
    out_dir: &Path,
) -> anyhow::Result<()> {
    let mut docs: Vec<Document> = Vec::with_capacity(paths.len());
    for path in &paths {
        docs.push(vimdoc::parse_file(path)?);
    }

    let section_count: usize = docs.iter().map(|d| d.sections.len()).sum();
    TantivyIndex::build_from(out_dir, docs)?;

    // Manifest is written AFTER build_from because build_from wipes the
    // directory (including any old manifest) before writing tantivy files.
    write_manifest(out_dir, current_states)?;

    println!(
        "Indexed {section_count} section(s) from {file_count} file(s) → {path}",
        file_count = paths.len(),
        path = out_dir.display()
    );
    Ok(())
}

fn run_incremental(
    current_states: &HashMap<PathBuf, FileState>,
    prior: Manifest,
    out_dir: &Path,
) -> anyhow::Result<()> {
    let diff = manifest::diff(current_states, &prior);

    if diff.is_empty() {
        println!(
            "Incremental update at {}: no changes ({} unchanged)",
            out_dir.display(),
            diff.unchanged.len()
        );
        // No writes at all — the existing manifest is already correct.
        return Ok(());
    }

    // Parse changed + new files up front so a parse error rolls back
    // before we touch the index.
    let changed_docs = parse_batch(&diff.changed)?;
    let new_docs = parse_batch(&diff.new)?;

    let idx = TantivyIndex::open(out_dir)?;
    idx.update(&diff.removed, changed_docs, new_docs)?;

    // Manifest replaced only after commit — if update fails we keep the
    // prior manifest so a retry sees the same diff.
    write_manifest(out_dir, current_states.clone())?;

    print_incremental_summary(out_dir, &diff);
    Ok(())
}

fn parse_batch(paths: &[PathBuf]) -> anyhow::Result<Vec<Document>> {
    let mut out = Vec::with_capacity(paths.len());
    for p in paths {
        out.push(vimdoc::parse_file(p)?);
    }
    Ok(out)
}

fn write_manifest(out_dir: &Path, files: HashMap<PathBuf, FileState>) -> anyhow::Result<()> {
    let mut m = Manifest::new();
    m.files = files;
    m.save(out_dir)?;
    Ok(())
}

fn print_incremental_summary(out_dir: &Path, diff: &ManifestDiff) {
    println!("Incremental update at {}:", out_dir.display());
    println!("  new:       {}", diff.new.len());
    println!("  changed:   {}", diff.changed.len());
    println!("  removed:   {}", diff.removed.len());
    println!("  unchanged: {}", diff.unchanged.len());
}

/// Resolve one --docs glob to a list of concrete file paths.
/// Directories that happen to match the glob are silently skipped;
/// per-entry errors (e.g. permission denied) propagate.
fn resolve_glob(pattern: &str) -> anyhow::Result<Vec<PathBuf>> {
    let entries = glob::glob(pattern)
        .map_err(|e| anyhow::anyhow!("invalid --docs pattern {pattern:?}: {e}"))?;
    let mut out = Vec::new();
    for entry in entries {
        match entry {
            Ok(path) if path.is_file() => out.push(path),
            Ok(_) => {} // skip directories that happen to match the glob
            Err(e) => return Err(anyhow::anyhow!("reading glob entry: {e}")),
        }
    }
    Ok(out)
}

/// Resolve every --docs pattern and union the results into a single
/// sorted + deduplicated path list. Sorting keeps build output stable
/// across filesystems that walk in different orders; deduplication
/// handles overlapping globs (`a/*.txt` + `a/foo.txt`).
///
/// Invalid glob syntax on any pattern fails the whole call. Empty
/// individual globs (zero matches) are silently ignored — the caller
/// (`run`) surfaces the "empty union" error message.
fn resolve_globs(patterns: &[String]) -> anyhow::Result<Vec<PathBuf>> {
    let mut out = Vec::new();
    for pattern in patterns {
        out.extend(resolve_glob(pattern)?);
    }
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_globs_returns_sorted_deduped_files_for_a_single_glob() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["b.txt", "a.txt", "c.txt"] {
            std::fs::write(tmp.path().join(name), b"body\n").unwrap();
        }
        let pattern = format!("{}/*.txt", tmp.path().display());
        let paths = resolve_globs(&[pattern]).unwrap();
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt", "c.txt"]);
    }

    #[test]
    fn resolve_globs_ignores_directories() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.txt"), b"body").unwrap();
        std::fs::create_dir(tmp.path().join("dir.txt")).unwrap(); // matches *.txt as a dir
        let pattern = format!("{}/*.txt", tmp.path().display());
        let paths = resolve_globs(&[pattern]).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("real.txt"));
    }

    #[test]
    fn resolve_globs_unions_results_across_multiple_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        let dir_a = tmp.path().join("a");
        let dir_b = tmp.path().join("b");
        std::fs::create_dir(&dir_a).unwrap();
        std::fs::create_dir(&dir_b).unwrap();
        std::fs::write(dir_a.join("one.txt"), b"1").unwrap();
        std::fs::write(dir_a.join("two.txt"), b"2").unwrap();
        std::fs::write(dir_b.join("three.txt"), b"3").unwrap();

        let paths = resolve_globs(&[
            format!("{}/*.txt", dir_a.display()),
            format!("{}/*.txt", dir_b.display()),
        ])
        .unwrap();

        // Sort is by FULL path (grouped by directory: a/ before b/, then
        // filename within each dir), not just filename.
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["one.txt", "two.txt", "three.txt"]);
    }

    #[test]
    fn resolve_globs_deduplicates_files_matched_by_overlapping_patterns() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("shared.txt"), b"body").unwrap();
        // Same file resolved by two different globs. Union must dedup.
        let paths = resolve_globs(&[
            format!("{}/*.txt", tmp.path().display()),
            format!("{}/shared.txt", tmp.path().display()),
        ])
        .unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("shared.txt"));
    }

    #[test]
    fn resolve_globs_silently_accepts_individual_zero_match_globs() {
        // Real world: `plugins/*/doc/*.txt` on a fresh install matches
        // nothing, but the user also passed `$VIMRUNTIME/doc/*.txt` which
        // does. The empty individual glob must not error — only an empty
        // UNION should.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.txt"), b"body").unwrap();
        let paths = resolve_globs(&[
            format!("{}/no-such-dir/*.txt", tmp.path().display()),
            format!("{}/*.txt", tmp.path().display()),
        ])
        .unwrap();
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn build_errors_when_all_globs_match_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let a = format!("{}/*.nonexistent-a", tmp.path().display());
        let b = format!("{}/*.nonexistent-b", tmp.path().display());
        let err = run(&[a, b], tmp.path(), false).unwrap_err();
        let msg = err.to_string();
        // Message names the glob count so users see how many patterns were
        // tried; helpful when a multi-glob build produces a surprise empty.
        assert!(msg.contains("no files matched"));
        assert!(msg.contains("2 glob"));
    }
}
