//! Per-file state tracking for incremental builds.
//!
//! `vimhelp-index build --incremental` needs to answer "which files
//! changed since the last build?" without re-reading every file. We
//! persist a small manifest at `<index-dir>/vimhelp-manifest.json` with
//! `{path: {mtime_ns, size}}` per file.
//!
//! Change detection uses (mtime_ns, size). Same signal `make` and `ninja`
//! use — cheap, works in the overwhelming majority of cases, and only
//! misses the pathological "same mtime + same size + different content"
//! case. Users hit by that can force a full rebuild. Content-hashing is
//! a future opt-in.
//!
//! The manifest schema is versioned — bumping `MANIFEST_VERSION`
//! invalidates old manifests cleanly (the loader returns `Ok(None)` so
//! callers fall back to a full rebuild rather than crashing on shape
//! drift).

use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Schema version. Bump when `FileState` or `Manifest` fields change so
/// old manifests fall back to a full rebuild instead of deserializing
/// into an incorrect shape.
pub const MANIFEST_VERSION: u32 = 1;

/// Manifest lives at the top of the index directory next to tantivy's
/// segment files. Filename chosen to be recognisable and unlikely to
/// collide with tantivy's own naming (which is UUID-based).
pub const MANIFEST_FILENAME: &str = "vimhelp-manifest.json";

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Manifest {
    pub version: u32,
    /// Absolute (or repo-relative, matching how the caller supplied
    /// paths) source paths → last-seen state.
    pub files: HashMap<PathBuf, FileState>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct FileState {
    /// mtime as nanoseconds since UNIX epoch. `i128` because SystemTime
    /// arithmetic can produce durations larger than u64 nanoseconds
    /// nominally (though real filesystems don't); i128 avoids ambiguity.
    pub mtime_ns: i128,
    pub size: u64,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("manifest json: {0}")]
    Json(#[from] serde_json::Error),
}

impl Manifest {
    /// A fresh manifest at the current schema version with no files.
    pub fn new() -> Self {
        Self {
            version: MANIFEST_VERSION,
            files: HashMap::new(),
        }
    }

    /// Load a manifest from `dir/vimhelp-manifest.json`.
    ///
    /// Returns `Ok(None)` when the file is absent OR when the on-disk
    /// version doesn't match the current [`MANIFEST_VERSION`]. Both
    /// cases mean the caller should fall back to a full rebuild —
    /// mixing the two into a single "no usable prior state" answer
    /// keeps the CLI logic straightforward.
    pub fn load(dir: &Path) -> Result<Option<Self>, ManifestError> {
        let path = dir.join(MANIFEST_FILENAME);
        if !path.exists() {
            return Ok(None);
        }
        let text = std::fs::read_to_string(&path).map_err(|source| ManifestError::Io {
            path: path.clone(),
            source,
        })?;
        let m: Manifest = serde_json::from_str(&text)?;
        if m.version != MANIFEST_VERSION {
            return Ok(None);
        }
        Ok(Some(m))
    }

    /// Serialize to `dir/vimhelp-manifest.json`.
    pub fn save(&self, dir: &Path) -> Result<(), ManifestError> {
        let path = dir.join(MANIFEST_FILENAME);
        let text = serde_json::to_string_pretty(self)?;
        std::fs::write(&path, text).map_err(|source| ManifestError::Io { path, source })?;
        Ok(())
    }
}

/// Compute the current on-disk state of a file.
pub fn stat_file(path: &Path) -> Result<FileState, ManifestError> {
    let md = std::fs::metadata(path).map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    let mtime = md.modified().map_err(|source| ManifestError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    // Duration since epoch, cast to i128 ns. A file dated before 1970
    // (rare, but possible on restored backups) trips duration_since's
    // Err arm — we fall back to 0, meaning we'll always treat it as
    // "changed" against any non-zero prior state. That's the safe
    // direction: over-index rather than under-index.
    let mtime_ns = mtime
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos() as i128)
        .unwrap_or(0);
    Ok(FileState {
        mtime_ns,
        size: md.len(),
    })
}

/// Stat a batch of paths. Returns a map ordered by insertion; deterministic
/// output ordering is the caller's responsibility.
pub fn stat_all(paths: &[PathBuf]) -> Result<HashMap<PathBuf, FileState>, ManifestError> {
    let mut out = HashMap::with_capacity(paths.len());
    for path in paths {
        out.insert(path.clone(), stat_file(path)?);
    }
    Ok(out)
}

/// Per-file classification produced by [`diff`].
#[derive(Debug, Default, PartialEq, Eq)]
pub struct ManifestDiff {
    /// Files present now that weren't in the prior manifest.
    pub new: Vec<PathBuf>,
    /// Files present in both, with different mtime or size.
    pub changed: Vec<PathBuf>,
    /// Files in the prior manifest that no longer exist in the current corpus.
    pub removed: Vec<PathBuf>,
    /// Files present in both, unchanged.
    pub unchanged: Vec<PathBuf>,
}

impl ManifestDiff {
    /// True when no re-indexing work is required.
    pub fn is_empty(&self) -> bool {
        self.new.is_empty() && self.changed.is_empty() && self.removed.is_empty()
    }
}

/// Compare the current corpus state against a prior manifest.
pub fn diff(current: &HashMap<PathBuf, FileState>, prior: &Manifest) -> ManifestDiff {
    let mut out = ManifestDiff::default();
    for (path, state) in current {
        match prior.files.get(path) {
            None => out.new.push(path.clone()),
            Some(prior_state) if prior_state != state => out.changed.push(path.clone()),
            Some(_) => out.unchanged.push(path.clone()),
        }
    }
    for path in prior.files.keys() {
        if !current.contains_key(path) {
            out.removed.push(path.clone());
        }
    }
    // Sort each list for deterministic output ordering — reports and
    // downstream JSON consumers get a stable shape across runs.
    out.new.sort();
    out.changed.sort();
    out.removed.sort();
    out.unchanged.sort();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fs_of(mtime_ns: i128, size: u64) -> FileState {
        FileState { mtime_ns, size }
    }

    fn path(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    // -- Manifest round-trip --------------------------------------------

    #[test]
    fn new_manifest_has_current_version_and_empty_files() {
        let m = Manifest::new();
        assert_eq!(m.version, MANIFEST_VERSION);
        assert!(m.files.is_empty());
    }

    #[test]
    fn save_and_load_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mut m = Manifest::new();
        m.files.insert(path("doc/a.txt"), fs_of(100, 200));
        m.files.insert(path("doc/b.txt"), fs_of(300, 400));
        m.save(tmp.path()).unwrap();

        let loaded = Manifest::load(tmp.path()).unwrap().unwrap();
        assert_eq!(loaded.version, MANIFEST_VERSION);
        assert_eq!(loaded.files.len(), 2);
        assert_eq!(loaded.files.get(&path("doc/a.txt")), Some(&fs_of(100, 200)));
    }

    #[test]
    fn load_returns_none_when_file_absent() {
        let tmp = tempfile::tempdir().unwrap();
        assert!(Manifest::load(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn load_returns_none_for_version_mismatch() {
        // A future version we don't know how to read. Caller must fall
        // back to a full rebuild rather than misinterpret the shape.
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(
            tmp.path().join(MANIFEST_FILENAME),
            r#"{"version":9999,"files":{}}"#,
        )
        .unwrap();
        assert!(Manifest::load(tmp.path()).unwrap().is_none());
    }

    #[test]
    fn load_returns_err_for_malformed_json() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join(MANIFEST_FILENAME), "not json").unwrap();
        let err = Manifest::load(tmp.path()).unwrap_err();
        assert!(matches!(err, ManifestError::Json(_)));
    }

    // -- stat_file ------------------------------------------------------

    #[test]
    fn stat_file_returns_mtime_and_size() {
        let tmp = tempfile::tempdir().unwrap();
        let p = tmp.path().join("x.txt");
        std::fs::write(&p, b"hello world").unwrap();
        let s = stat_file(&p).unwrap();
        assert_eq!(s.size, 11);
        assert!(s.mtime_ns > 0);
    }

    #[test]
    fn stat_all_batches_stats_per_path() {
        let tmp = tempfile::tempdir().unwrap();
        let paths: Vec<_> = ["a.txt", "b.txt"]
            .iter()
            .map(|n| {
                let p = tmp.path().join(n);
                std::fs::write(&p, n.as_bytes()).unwrap();
                p
            })
            .collect();
        let states = stat_all(&paths).unwrap();
        assert_eq!(states.len(), 2);
    }

    // -- diff -----------------------------------------------------------

    #[test]
    fn diff_flags_files_only_in_current_as_new() {
        let mut current = HashMap::new();
        current.insert(path("a"), fs_of(1, 10));
        let prior = Manifest::new();
        let d = diff(&current, &prior);
        assert_eq!(d.new, vec![path("a")]);
        assert!(d.changed.is_empty());
        assert!(d.removed.is_empty());
        assert!(d.unchanged.is_empty());
    }

    #[test]
    fn diff_flags_mtime_or_size_change_as_changed() {
        let mut current = HashMap::new();
        current.insert(path("a"), fs_of(2, 10)); // mtime bumped
        current.insert(path("b"), fs_of(1, 99)); // size bumped
        current.insert(path("c"), fs_of(3, 30)); // unchanged
        let mut prior = Manifest::new();
        prior.files.insert(path("a"), fs_of(1, 10));
        prior.files.insert(path("b"), fs_of(1, 10));
        prior.files.insert(path("c"), fs_of(3, 30));
        let d = diff(&current, &prior);
        assert_eq!(d.changed, vec![path("a"), path("b")]);
        assert_eq!(d.unchanged, vec![path("c")]);
    }

    #[test]
    fn diff_flags_files_only_in_prior_as_removed() {
        let current = HashMap::new();
        let mut prior = Manifest::new();
        prior.files.insert(path("gone.txt"), fs_of(1, 1));
        let d = diff(&current, &prior);
        assert_eq!(d.removed, vec![path("gone.txt")]);
    }

    #[test]
    fn diff_is_empty_when_no_change() {
        let mut current = HashMap::new();
        current.insert(path("a"), fs_of(1, 10));
        let mut prior = Manifest::new();
        prior.files.insert(path("a"), fs_of(1, 10));
        let d = diff(&current, &prior);
        assert!(d.is_empty());
    }

    #[test]
    fn diff_output_is_sorted_for_deterministic_reports() {
        // HashMap iteration order is randomised — the diff must impose a
        // sort so reports and downstream JSON consumers get a stable shape.
        let mut current = HashMap::new();
        for name in ["c", "a", "b"] {
            current.insert(path(name), fs_of(1, 1));
        }
        let prior = Manifest::new();
        let d = diff(&current, &prior);
        assert_eq!(d.new, vec![path("a"), path("b"), path("c")]);
    }
}
