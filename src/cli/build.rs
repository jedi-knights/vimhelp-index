//! `build` subcommand — resolve a glob of vimdoc files, parse each, and
//! write a fresh tantivy index at `--out`.

use crate::adapters::parser::vimdoc;
use crate::adapters::tantivy::TantivyIndex;
use crate::domain::Document;
use std::path::Path;

/// Execute the `build` subcommand.
pub fn run(docs_glob: &str, out_dir: &Path) -> anyhow::Result<()> {
    let paths = resolve_glob(docs_glob)?;
    if paths.is_empty() {
        anyhow::bail!(
            "no files matched --docs pattern {:?} — glob resolved to zero paths",
            docs_glob
        );
    }

    let mut docs: Vec<Document> = Vec::with_capacity(paths.len());
    for path in &paths {
        docs.push(vimdoc::parse_file(path)?);
    }

    let section_count: usize = docs.iter().map(|d| d.sections.len()).sum();
    TantivyIndex::build_from(out_dir, docs)?;

    println!(
        "Indexed {section_count} section(s) from {file_count} file(s) → {path}",
        file_count = paths.len(),
        path = out_dir.display()
    );
    Ok(())
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
        let err = run(&pattern, tmp.path()).unwrap_err();
        assert!(err.to_string().contains("no files matched"));
    }
}
