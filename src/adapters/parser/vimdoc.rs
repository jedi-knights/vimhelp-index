//! Vimdoc parser.
//!
//! Vimdoc's format is old but stable — the parser here handles the two
//! shapes that drive indexing:
//!
//! - **Section rules** — 40+ `=` on a line separate top-level sections. The
//!   line immediately after a rule is the section header. Text between one
//!   rule and the next (or EOF) is the section body.
//! - **Tags** — `*name*` markers where `name` has no whitespace and no
//!   `*`. These are the primary index unit; every tag surfaces both in
//!   its enclosing [`Section`] and in the [`Document`]'s top-level tag
//!   list (deduplicated, first-occurrence order preserved).
//!
//! Subsection rules (40+ `-` on a line) are deliberately NOT treated as
//! new sections — they stay inside their parent section's body. Vimdoc
//! subsections are typically small enough that fragmenting them hurts
//! full-text search relevance more than it helps.
//!
//! Cross-references (`|tag|`) are not parsed here; a later PR that owns
//! the search side will resolve them.

use crate::domain::{Document, Section};
use regex::Regex;
use std::path::{Path, PathBuf};
use std::sync::LazyLock;

/// Regex for extracting `*name*` tags. `name` is any run of non-whitespace,
/// non-`*` characters. Compiled once via `LazyLock` so the parser doesn't
/// re-compile on every call — vimdoc corpora can be thousands of files.
static TAG_RE: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"\*([^\s*]+)\*").unwrap());

/// Errors surfaced by the parser. Only IO fails today — parsing itself
/// is total (any input yields *some* Document, possibly with zero
/// sections). Enum shape kept so future failure modes stay a matter of
/// adding variants, not changing the return type.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    #[error("reading {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
}

/// Parse a vimdoc file from disk.
pub fn parse_file(path: impl AsRef<Path>) -> Result<Document, ParseError> {
    let path = path.as_ref();
    let source = std::fs::read_to_string(path).map_err(|source| ParseError::Io {
        path: path.to_path_buf(),
        source,
    })?;
    Ok(parse_source(&source, path))
}

/// Parse a vimdoc source string. Pure — no IO — so tests inject fixtures
/// directly. `path` is stamped into the returned `Document.path`.
pub fn parse_source(source: &str, path: impl AsRef<Path>) -> Document {
    let lines: Vec<&str> = source.lines().collect();

    // Walk once, collect all tags in first-occurrence order for
    // Document.tags. Same regex is used per-section below.
    let mut seen_tags = std::collections::HashSet::new();
    let mut ordered_tags: Vec<String> = Vec::new();
    for line in &lines {
        for cap in TAG_RE.captures_iter(line) {
            let tag = cap[1].to_string();
            if seen_tags.insert(tag.clone()) {
                ordered_tags.push(tag);
            }
        }
    }

    let sections = split_sections(&lines);

    Document {
        path: path.as_ref().to_path_buf(),
        tags: ordered_tags,
        sections,
    }
}

/// Split a document into sections at `====` rules.
fn split_sections(lines: &[&str]) -> Vec<Section> {
    let mut sections = Vec::new();

    // A "section" is: the line right after a rule (the header), followed by
    // every line up to the next rule or EOF. Content before the first rule
    // is emitted as a section with no header — many vimdoc files put a
    // block of metadata (author, license) up front and it shouldn't be lost.
    //
    // We track the RAW header line (with tag markers intact) so tag
    // extraction sees `*tag*` markers that appear on header lines. Only
    // the stored Section.header gets stripped for display.
    let mut current_start: usize = 0;
    let mut current_header_raw: Option<&str> = None;
    let mut current_body: Vec<&str> = Vec::new();

    let flush =
        |sections: &mut Vec<Section>, start: usize, header_raw: Option<&str>, body: Vec<&str>| {
            // Skip empty preambles / empty trailing chunks so we don't emit
            // no-content sections that bloat the report.
            let body_text = body.join("\n");
            let is_empty_content = header_raw.is_none() && body_text.trim().is_empty();
            if is_empty_content {
                return;
            }
            let tags = extract_tags(&body_text, header_raw);
            let header = header_raw.map(header_text);
            sections.push(Section {
                header,
                tags,
                body: body_text,
                line_start: start,
            });
        };

    let mut i = 0;
    while i < lines.len() {
        let line = lines[i];
        if is_section_rule(line) {
            // Rule ends the current section. Flush what we've collected.
            flush(
                &mut sections,
                current_start + 1, // 1-indexed
                current_header_raw.take(),
                std::mem::take(&mut current_body),
            );

            // The line after the rule is the new header (best-effort).
            i += 1;
            if i >= lines.len() {
                break; // rule at EOF, nothing follows
            }
            current_header_raw = Some(lines[i]);
            current_start = i;
            i += 1;
        } else {
            current_body.push(line);
            i += 1;
        }
    }

    // Flush the trailing section.
    flush(
        &mut sections,
        current_start + 1,
        current_header_raw.take(),
        current_body,
    );

    sections
}

/// A section rule is a line of 40+ `=` characters and nothing else.
fn is_section_rule(line: &str) -> bool {
    line.len() >= 40 && line.bytes().all(|b| b == b'=')
}

/// Strip trailing `*tag*` markers and whitespace from a header line so the
/// stored header is just the human-readable text.
fn header_text(line: &str) -> String {
    let stripped = TAG_RE.replace_all(line, "");
    stripped.trim().to_string()
}

/// Extract all `*tag*` matches from a chunk of body text plus its header.
/// Deduplicated in first-occurrence order.
fn extract_tags(body: &str, header: Option<&str>) -> Vec<String> {
    let mut seen = std::collections::HashSet::new();
    let mut ordered = Vec::new();
    let mut collect = |text: &str| {
        for cap in TAG_RE.captures_iter(text) {
            let tag = cap[1].to_string();
            if seen.insert(tag.clone()) {
                ordered.push(tag);
            }
        }
    };
    if let Some(h) = header {
        collect(h);
    }
    collect(body);
    ordered
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_yields_document_with_no_sections_or_tags() {
        let doc = parse_source("", "doc/empty.txt");
        assert_eq!(doc.path, PathBuf::from("doc/empty.txt"));
        assert!(doc.tags.is_empty());
        assert!(doc.sections.is_empty());
    }

    #[test]
    fn plain_text_with_no_rules_yields_one_headerless_section() {
        let src = "just some prose\nno rules at all\n";
        let doc = parse_source(src, "doc/plain.txt");
        assert_eq!(doc.sections.len(), 1);
        let sec = &doc.sections[0];
        assert_eq!(sec.header, None);
        assert_eq!(sec.body, "just some prose\nno rules at all");
        assert_eq!(sec.line_start, 1);
    }

    #[test]
    fn extracts_tags_from_body_in_first_occurrence_order() {
        let src = "text with *first* and *second* and *first* again\n";
        let doc = parse_source(src, "doc/tags.txt");
        assert_eq!(doc.tags, vec!["first".to_string(), "second".to_string()]);
    }

    #[test]
    fn asterisks_around_whitespace_are_not_tags() {
        // *foo bar* would be a tag if we naively matched *...*. Whitespace
        // in the tag body must exclude it — vimdoc tags never have spaces.
        let src = "*foo bar* is not a tag; *foo-bar* is\n";
        let doc = parse_source(src, "doc/mixed.txt");
        assert_eq!(doc.tags, vec!["foo-bar".to_string()]);
    }

    #[test]
    fn section_rule_boundary_at_40_equals() {
        assert!(is_section_rule(&"=".repeat(40)));
        assert!(is_section_rule(&"=".repeat(78)));
        assert!(!is_section_rule(&"=".repeat(39)));
        assert!(!is_section_rule("=== not a rule ==="));
        assert!(!is_section_rule(""));
    }

    #[test]
    fn subsection_dashes_do_not_split_sections() {
        let rule = "=".repeat(78);
        let sub_rule = "-".repeat(78);
        let src = format!(
            "preamble\n{rule}\nHEADER *tag*\nbody line 1\n{sub_rule}\nSubsection *sub-tag*\nmore body\n"
        );
        let doc = parse_source(&src, "doc/nested.txt");
        // preamble + one section (subsection dashes are part of the same body).
        assert_eq!(doc.sections.len(), 2);
        let sec = &doc.sections[1];
        assert_eq!(sec.header.as_deref(), Some("HEADER"));
        // The subsection rule and its content are inside the parent body.
        assert!(sec.body.contains(&sub_rule));
        assert!(sec.body.contains("Subsection"));
        // Both tags surface on Document (union) and on the parent section
        // (nothing further split them).
        assert_eq!(doc.tags, vec!["tag".to_string(), "sub-tag".to_string()]);
        assert_eq!(sec.tags, vec!["tag".to_string(), "sub-tag".to_string()]);
    }

    #[test]
    fn header_strips_trailing_tag_markers() {
        assert_eq!(
            header_text("CONTENTS                                     *operator-contents*"),
            "CONTENTS"
        );
    }

    #[test]
    fn empty_preamble_before_first_rule_is_omitted() {
        let rule = "=".repeat(78);
        let src = format!("{rule}\nHEADER\nbody\n");
        let doc = parse_source(&src, "doc/prefixed.txt");
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].header.as_deref(), Some("HEADER"));
    }

    #[test]
    fn section_line_start_is_1_indexed() {
        let rule = "=".repeat(78);
        let src = format!("preamble\n{rule}\nSECOND\nbody\n");
        let doc = parse_source(&src, "doc/positioned.txt");
        assert_eq!(doc.sections.len(), 2);
        // Preamble section starts at line 1.
        assert_eq!(doc.sections[0].line_start, 1);
        // Second section header is on line 3 (preamble=1, rule=2, header=3).
        assert_eq!(doc.sections[1].line_start, 3);
    }

    #[test]
    fn rule_at_eof_does_not_panic() {
        let rule = "=".repeat(78);
        let src = format!("body\n{rule}");
        let doc = parse_source(&src, "doc/trailing-rule.txt");
        // The trailing rule opens a section with no header and no body,
        // which is elided.
        assert_eq!(doc.sections.len(), 1);
        assert_eq!(doc.sections[0].header, None);
    }
}
