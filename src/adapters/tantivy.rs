//! Tantivy-backed full-text index and searcher.
//!
//! One tantivy segment per section, not per file — hits attribute to the
//! narrowest addressable unit (a specific header + line inside a doc),
//! which is what the eventual UI needs to jump the user precisely into
//! `:help`.
//!
//! Schema fields:
//!   - `doc_path`   STRING + STORED — exact-match filter + hit attribution
//!   - `header`     TEXT   + STORED — tokenized so partial header words match
//!   - `tags`       TEXT   + STORED — tokenized; user's query often looks like a tag word
//!   - `body`       TEXT   + STORED — the primary search target
//!   - `line_start` U64    + STORED — 1-indexed line for cursor-precise jumps
//!
//! No `--incremental` support yet; `build_from` is single-shot. Callers who
//! need incremental (per the TODO follow-up) will grow a persistent
//! IndexWriter handle. Kept simple here because the CLI's `build` command
//! is inherently one-shot.

use crate::domain::{Document, Query, SearchHit};
use std::path::{Path, PathBuf};
use tantivy::collector::TopDocs;
use tantivy::query::QueryParser;
use tantivy::schema::{Field, STORED, STRING, Schema, TEXT, Value};
use tantivy::{Index, IndexWriter, ReloadPolicy, TantivyDocument, doc};

/// Default writer heap. 50 MB is tantivy's suggested minimum; vimdoc corpora
/// are tiny (single-digit MB total) so this is overkill and fast.
const WRITER_HEAP_BYTES: usize = 50_000_000;

/// Default cap when a caller passes `max_hits = 0` (the "let the searcher
/// decide" sentinel domain::Query documents). Chosen to fit a picker page
/// without truncation being surprising.
const DEFAULT_MAX_HITS: usize = 50;

/// Errors from the tantivy adapter. `Tantivy` wraps engine errors; `Io`
/// covers directory creation and pathing. Kept thin — tantivy's own error
/// enum is rich enough that re-wrapping every variant here is churn.
#[derive(Debug, thiserror::Error)]
pub enum IndexError {
    #[error("tantivy: {0}")]
    Tantivy(#[from] tantivy::TantivyError),
    #[error("tantivy query parse: {0}")]
    QueryParse(#[from] tantivy::query::QueryParserError),
    #[error("io ({path}): {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Strongly-typed handles to the schema fields. Passing `Schema` around
/// requires name lookups per operation; a small struct of Field handles
/// pays for itself the first time we index in a loop.
struct Fields {
    doc_path: Field,
    header: Field,
    tags: Field,
    body: Field,
    line_start: Field,
}

fn build_schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    // STRING (not TEXT) for doc_path: we want exact-match, not tokenized —
    // "doc/telescope.txt" must not tokenize into ["doc", "telescope", "txt"].
    let doc_path = b.add_text_field("doc_path", STRING | STORED);
    let header = b.add_text_field("header", TEXT | STORED);
    let tags = b.add_text_field("tags", TEXT | STORED);
    let body = b.add_text_field("body", TEXT | STORED);
    // U64 STORED (not INDEXED): we don't range-query line numbers, we just
    // read them back on hits to render jump targets.
    let line_start = b.add_u64_field("line_start", STORED);
    let schema = b.build();
    let fields = Fields {
        doc_path,
        header,
        tags,
        body,
        line_start,
    };
    (schema, fields)
}

/// A tantivy index handle. Open one to search; call [`build_from`] to
/// (re)create one on disk in a single shot.
pub struct TantivyIndex {
    index: Index,
    fields: Fields,
}

impl TantivyIndex {
    /// Build a fresh index at `dir` from an iterator of documents. Overwrites
    /// any existing index at that path. Single-shot; commits before returning.
    pub fn build_from<I>(dir: &Path, docs: I) -> Result<(), IndexError>
    where
        I: IntoIterator<Item = Document>,
    {
        std::fs::create_dir_all(dir).map_err(|source| IndexError::Io {
            path: dir.to_path_buf(),
            source,
        })?;
        let (schema, fields) = build_schema();
        // create_in_dir errors if the directory already contains an index;
        // wipe first so build_from is unconditionally "fresh". Callers who
        // want incremental behaviour will use a different entry point.
        for entry in std::fs::read_dir(dir).map_err(|source| IndexError::Io {
            path: dir.to_path_buf(),
            source,
        })? {
            let entry = entry.map_err(|source| IndexError::Io {
                path: dir.to_path_buf(),
                source,
            })?;
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
        let index = Index::create_in_dir(dir, schema)?;
        let mut writer: IndexWriter = index.writer(WRITER_HEAP_BYTES)?;
        for document in docs {
            add_document(&mut writer, &fields, &document)?;
        }
        writer.commit()?;
        Ok(())
    }

    /// Open a previously-built index for reading.
    pub fn open(dir: &Path) -> Result<Self, IndexError> {
        let index = Index::open_in_dir(dir)?;
        // Re-derive the field handles from the on-disk schema; this asserts
        // the schema hasn't drifted from what build_from writes.
        let schema = index.schema();
        let fields = Fields {
            doc_path: schema.get_field("doc_path")?,
            header: schema.get_field("header")?,
            tags: schema.get_field("tags")?,
            body: schema.get_field("body")?,
            line_start: schema.get_field("line_start")?,
        };
        Ok(Self { index, fields })
    }

    /// Search the index for the given query. Returns hits ranked by score,
    /// most relevant first. Bounded by `query.max_hits`, or by
    /// [`DEFAULT_MAX_HITS`] when the caller passes zero.
    pub fn search(&self, query: &Query) -> Result<Vec<SearchHit>, IndexError> {
        let reader = self
            .index
            .reader_builder()
            .reload_policy(ReloadPolicy::OnCommitWithDelay)
            .try_into()?;
        let searcher = reader.searcher();
        // QueryParser searches these fields when the user writes bare terms.
        // Body has the most content; tag/header get scored higher naturally
        // because they're shorter (BM25 favours matches in shorter fields).
        let query_parser = QueryParser::for_index(
            &self.index,
            vec![self.fields.tags, self.fields.header, self.fields.body],
        );
        let parsed = query_parser.parse_query(&query.text)?;

        let limit = if query.max_hits == 0 {
            DEFAULT_MAX_HITS
        } else {
            query.max_hits
        };
        let top = searcher.search(&parsed, &TopDocs::with_limit(limit))?;

        let mut hits = Vec::with_capacity(top.len());
        for (score, addr) in top {
            let doc: TantivyDocument = searcher.doc(addr)?;
            hits.push(shape_hit(&doc, &self.fields, score));
        }
        Ok(hits)
    }
}

/// Index every section of `document` under its own tantivy doc.
fn add_document(
    writer: &mut IndexWriter,
    fields: &Fields,
    document: &Document,
) -> Result<(), IndexError> {
    let path_str = document.path.display().to_string();
    for section in &document.sections {
        let header = section.header.as_deref().unwrap_or("");
        let tags_joined = section.tags.join(" ");
        writer.add_document(doc!(
            fields.doc_path => path_str.as_str(),
            fields.header => header,
            fields.tags => tags_joined.as_str(),
            fields.body => section.body.as_str(),
            fields.line_start => section.line_start as u64,
        ))?;
    }
    Ok(())
}

/// Read one stored tantivy doc back into a domain SearchHit.
fn shape_hit(doc: &TantivyDocument, fields: &Fields, score: f32) -> SearchHit {
    let doc_path = first_text(doc, fields.doc_path).unwrap_or_default();
    let header = first_text(doc, fields.header).filter(|s| !s.is_empty());
    let body = first_text(doc, fields.body).unwrap_or_default();
    let tags = first_text(doc, fields.tags).unwrap_or_default();
    let line = first_u64(doc, fields.line_start).unwrap_or(0) as usize;

    // First tag (if any) — the tag list is space-joined in the stored field,
    // and the first token is the section's primary tag by construction.
    let tag = tags
        .split_whitespace()
        .next()
        .map(|s| s.to_string())
        .filter(|s| !s.is_empty());

    // Snippet: first 240 chars of the body. Later PRs can plug in
    // tantivy::snippet::SnippetGenerator for highlighted excerpts.
    let snippet = if body.len() > 240 {
        format!("{}…", &body[..240])
    } else {
        body
    };

    SearchHit {
        document: PathBuf::from(doc_path),
        tag,
        section_header: header,
        line,
        score,
        snippet,
    }
}

fn first_text(doc: &TantivyDocument, field: Field) -> Option<String> {
    doc.get_first(field)
        .and_then(|v| v.as_str().map(|s| s.to_string()))
}

fn first_u64(doc: &TantivyDocument, field: Field) -> Option<u64> {
    doc.get_first(field).and_then(|v| v.as_u64())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::domain::Section;

    fn sample_doc(path: &str, sections: Vec<Section>) -> Document {
        Document {
            path: PathBuf::from(path),
            tags: sections.iter().flat_map(|s| s.tags.clone()).collect(),
            sections,
        }
    }

    fn section(header: &str, tags: Vec<&str>, body: &str, line: usize) -> Section {
        Section {
            header: if header.is_empty() {
                None
            } else {
                Some(header.to_string())
            },
            tags: tags.into_iter().map(String::from).collect(),
            body: body.to_string(),
            line_start: line,
        }
    }

    #[test]
    fn schema_defines_all_expected_fields() {
        let (schema, _fields) = build_schema();
        for name in ["doc_path", "header", "tags", "body", "line_start"] {
            assert!(
                schema.get_field(name).is_ok(),
                "schema missing field {name}"
            );
        }
    }

    #[test]
    fn round_trip_index_and_search_finds_known_term() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = sample_doc(
            "doc/telescope.txt",
            vec![section(
                "Find files",
                vec!["telescope-find-files"],
                "fuzzy find files inside a floating window with previews",
                42,
            )],
        );
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();

        let idx = TantivyIndex::open(tmp.path()).unwrap();
        let hits = idx
            .search(&Query::new("floating window", 10).unwrap())
            .unwrap();

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].document, PathBuf::from("doc/telescope.txt"));
        assert_eq!(hits[0].section_header.as_deref(), Some("Find files"));
        assert_eq!(hits[0].tag.as_deref(), Some("telescope-find-files"));
        assert_eq!(hits[0].line, 42);
        assert!(hits[0].score > 0.0);
        assert!(hits[0].snippet.contains("floating window"));
    }

    #[test]
    fn search_with_no_match_returns_empty() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = sample_doc(
            "doc/a.txt",
            vec![section("Section", vec!["tag-a"], "some content", 1)],
        );
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();
        let idx = TantivyIndex::open(tmp.path()).unwrap();
        let hits = idx
            .search(&Query::new("nonexistent-term-xyz", 10).unwrap())
            .unwrap();
        assert_eq!(hits.len(), 0);
    }

    #[test]
    fn multiple_sections_index_independently() {
        let tmp = tempfile::tempdir().unwrap();
        let doc = sample_doc(
            "doc/multi.txt",
            vec![
                section("Intro", vec!["intro"], "welcome to floating windows", 1),
                section("Setup", vec!["setup"], "configuration options here", 20),
            ],
        );
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();
        let idx = TantivyIndex::open(tmp.path()).unwrap();

        let floating = idx.search(&Query::new("floating", 10).unwrap()).unwrap();
        assert_eq!(floating.len(), 1);
        assert_eq!(floating[0].section_header.as_deref(), Some("Intro"));

        let config = idx
            .search(&Query::new("configuration", 10).unwrap())
            .unwrap();
        assert_eq!(config.len(), 1);
        assert_eq!(config[0].section_header.as_deref(), Some("Setup"));
    }

    #[test]
    fn max_hits_zero_uses_default_cap() {
        let tmp = tempfile::tempdir().unwrap();
        // 100 sections all containing the search term. If the cap is
        // respected the hit count clamps to DEFAULT_MAX_HITS (50).
        let sections = (0..100)
            .map(|i| section(&format!("s{i}"), vec![], "commonterm goes here", i + 1))
            .collect();
        let doc = sample_doc("doc/many.txt", sections);
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();
        let idx = TantivyIndex::open(tmp.path()).unwrap();
        let hits = idx.search(&Query::new("commonterm", 0).unwrap()).unwrap();
        assert_eq!(hits.len(), DEFAULT_MAX_HITS);
    }

    #[test]
    fn max_hits_positive_clamps_to_that_value() {
        let tmp = tempfile::tempdir().unwrap();
        let sections = (0..30)
            .map(|i| section(&format!("s{i}"), vec![], "term here", i + 1))
            .collect();
        let doc = sample_doc("doc/hits.txt", sections);
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();
        let idx = TantivyIndex::open(tmp.path()).unwrap();
        let hits = idx.search(&Query::new("term", 5).unwrap()).unwrap();
        assert_eq!(hits.len(), 5);
    }

    #[test]
    fn snippet_truncates_to_240_chars_with_ellipsis() {
        let long_body = "x".repeat(500) + " floating";
        let tmp = tempfile::tempdir().unwrap();
        let doc = sample_doc("doc/long.txt", vec![section("Long", vec![], &long_body, 1)]);
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();
        let idx = TantivyIndex::open(tmp.path()).unwrap();
        let hits = idx.search(&Query::new("floating", 10).unwrap()).unwrap();
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0].snippet.ends_with('…'),
            "snippet should end with ellipsis, got: {}",
            hits[0].snippet
        );
        // 240 body chars + 1 ellipsis char (3 bytes for the … codepoint).
        assert_eq!(hits[0].snippet.chars().count(), 241);
    }

    #[test]
    fn build_from_wipes_existing_index_files() {
        let tmp = tempfile::tempdir().unwrap();
        // Simulate a stale index file from a prior run.
        std::fs::write(tmp.path().join("stale.txt"), b"leftover").unwrap();

        let doc = sample_doc("doc/x.txt", vec![section("x", vec![], "term", 1)]);
        TantivyIndex::build_from(tmp.path(), vec![doc]).unwrap();

        assert!(
            !tmp.path().join("stale.txt").exists(),
            "build_from must clear pre-existing files"
        );
    }
}
