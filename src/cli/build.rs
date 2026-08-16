//! `build` subcommand — resolve a glob of vimdoc files, parse each, and
//! write a tantivy index at `--out`.
//!
//! Two modes:
//!
//! - **Full** (default): wipes `--out` and rebuilds from scratch.
//! - **Incremental** (`--incremental`): reads the manifest at
//!   `<out>/vimhelp-manifest.json`, classifies each current file as
//!   new/changed/removed/unchanged, and only re-indexes the changed +
//!   new subset. If the manifest is absent or on a different version,
//!   falls back to a full rebuild.

use crate::adapters::manifest::{self, FileState, Manifest, ManifestDiff};
use crate::adapters::parser::vimdoc;
use crate::adapters::tantivy::TantivyIndex;
use crate::domain::Document;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Execute the `build` subcommand.
pub fn run(docs_glob: &str, out_dir: &Path, incremental: bool) -> anyhow::Result<()> {
    let paths = resolve_glob(docs_glob)?;
    if paths.is_empty() {
        anyhow::bail!(
            "no files matched --docs pattern {:?} — glob resolved to zero paths",
            docs_glob
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

/// Resolve the caller's --docs pattern to a sorted, deduplicated list of
/// concrete file paths. Sorted so build output is deterministic across
/// filesystems that walk in different orders.
fn resolve_glob(pattern: &str) -> anyhow::Result<Vec<std::path::PathBuf>> {
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
    out.sort();
    out.dedup();
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolve_glob_returns_sorted_deduped_files() {
        let tmp = tempfile::tempdir().unwrap();
        for name in ["b.txt", "a.txt", "c.txt"] {
            std::fs::write(tmp.path().join(name), b"body\n").unwrap();
        }
        let pattern = format!("{}/*.txt", tmp.path().display());
        let paths = resolve_glob(&pattern).unwrap();
        let names: Vec<_> = paths
            .iter()
            .map(|p| p.file_name().unwrap().to_str().unwrap())
            .collect();
        assert_eq!(names, vec!["a.txt", "b.txt", "c.txt"]);
    }

    #[test]
    fn resolve_glob_ignores_directories() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("real.txt"), b"body").unwrap();
        std::fs::create_dir(tmp.path().join("dir.txt")).unwrap(); // matches *.txt as a dir
        let pattern = format!("{}/*.txt", tmp.path().display());
        let paths = resolve_glob(&pattern).unwrap();
        assert_eq!(paths.len(), 1);
        assert!(paths[0].ends_with("real.txt"));
    }

    #[test]
    fn build_errors_when_glob_matches_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let pattern = format!("{}/*.nonexistent", tmp.path().display());
        let err = run(&pattern, tmp.path(), false).unwrap_err();
        assert!(err.to_string().contains("no files matched"));
    }
}
