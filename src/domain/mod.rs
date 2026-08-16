//! Domain types — the language of the problem.
//!
//! Three shapes drive everything downstream:
//!
//! - [`Document`] — the parsed representation of one vimdoc file, with the
//!   file's tags surfaced separately from its sectioned body so a caller can
//!   answer "which docs mention foo?" without re-scanning the source text.
//! - [`Query`] — the user's search input, bounded by a max-hit cap so a
//!   pathological query can't return the whole corpus.
//! - [`SearchHit`] — one result from a search, with enough context (path,
//!   tag or section, line, score, snippet) that a UI can render it inline
//!   or jump straight into `:help`.
//!
//! The types are storage-agnostic — no serde on the wire yet, no path to
//! disk. That belongs to `adapters`.

use std::path::PathBuf;

/// A parsed vimdoc file.
///
/// `path` is stored as supplied by the caller — usually absolute for the
/// runtime and relative for repo-local test fixtures. Callers that need
/// canonicalisation should do it before construction.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Document {
    pub path: PathBuf,
    /// All tags declared in the file, in source order, deduplicated. Kept
    /// alongside `sections[].tags` so a caller answering "which files
    /// contain tag X" doesn't have to walk sections.
    pub tags: Vec<String>,
    pub sections: Vec<Section>,
}

/// One section of a vimdoc file — the text between two `====` rules, or
/// from the top of the file to the first rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Section {
    /// The section's own header, when the parser could identify one.
    /// e.g. "1. Introduction" — a numbered heading beneath the `====`.
    pub header: Option<String>,
    /// Tags declared inside this section.
    pub tags: Vec<String>,
    /// Body text. Preserved verbatim so snippets can render faithfully.
    pub body: String,
    /// 1-indexed line number of the section's first byte within the file.
    /// Callers pass this to `:help` for cursor-precise jumps.
    pub line_start: usize,
}

/// Search input.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub text: String,
    /// Cap on results returned. Zero means "use the searcher's default cap"
    /// — it must NEVER be interpreted as "unbounded". Every path that
    /// eventually iterates a corpus has to have a provable upper bound;
    /// zero is a sentinel for "let the layer above decide", not "no cap".
    pub max_hits: usize,
}

impl Query {
    /// Convenience constructor that trims whitespace and rejects an empty
    /// query at the boundary. Downstream code can assume `text` is
    /// non-empty and stripped.
    pub fn new(text: impl Into<String>, max_hits: usize) -> Result<Self, QueryError> {
        let text = text.into().trim().to_string();
        if text.is_empty() {
            return Err(QueryError::Empty);
        }
        Ok(Self { text, max_hits })
    }
}

/// Errors constructing a [`Query`]. Thin because there's only one failure
/// mode today; the enum shape is here so callers can `match` exhaustively
/// as more failure modes emerge.
#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum QueryError {
    #[error("query text must not be empty")]
    Empty,
}

/// One result from a search over the index.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchHit {
    /// The document the hit came from.
    pub document: PathBuf,
    /// The nearest tag to the hit, if the searcher could resolve one.
    /// Callers use this to jump: `:h <tag>`.
    pub tag: Option<String>,
    /// The section header the hit fell under, if any.
    pub section_header: Option<String>,
    /// 1-indexed line number of the hit within the document.
    pub line: usize,
    /// Relevance score from the underlying engine. Higher = better.
    /// Uninterpreted here — comparable within one search, not across.
    pub score: f32,
    /// Pre-rendered excerpt for display. The searcher owns the snippet
    /// shape (length, highlight markers); domain doesn't parse it.
    pub snippet: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn query_new_trims_and_rejects_empty() {
        assert_eq!(
            Query::new("  floating window  ", 10).unwrap(),
            Query {
                text: "floating window".to_string(),
                max_hits: 10,
            }
        );
    }

    #[test]
    fn query_new_rejects_empty_string() {
        assert_eq!(Query::new("", 10).unwrap_err(), QueryError::Empty);
    }

    #[test]
    fn query_new_rejects_whitespace_only() {
        assert_eq!(Query::new("   \t  ", 10).unwrap_err(), QueryError::Empty);
    }

    #[test]
    fn query_preserves_zero_max_hits_as_sentinel() {
        // Zero is the "let the layer above decide" sentinel — the
        // constructor must not reject it, and must not silently rewrite it.
        let q = Query::new("x", 0).unwrap();
        assert_eq!(q.max_hits, 0);
    }

    #[test]
    fn document_and_hit_are_constructible() {
        // Compile-time proof the public shape can be built by callers
        // without needing a builder. If a field goes private this test
        // stops compiling — the coupling is intentional.
        let doc = Document {
            path: PathBuf::from("doc/example.txt"),
            tags: vec!["example".to_string()],
            sections: vec![Section {
                header: Some("1. Intro".to_string()),
                tags: vec!["example-intro".to_string()],
                body: "hello".to_string(),
                line_start: 1,
            }],
        };
        assert_eq!(doc.sections.len(), 1);

        let hit = SearchHit {
            document: PathBuf::from("doc/example.txt"),
            tag: Some("example-intro".to_string()),
            section_header: Some("1. Intro".to_string()),
            line: 3,
            score: 1.5,
            snippet: "hello".to_string(),
        };
        assert_eq!(hit.line, 3);
    }
}
