//! `search` subcommand — open an index and print hits ranked by score.
//!
//! `--format=console` (default): human-readable per-hit blocks.
//! `--format=json`: `{ "query": ..., "hits": [...] }` with a wire-shape
//! `HitJson` decoupled from `domain::SearchHit` so downstream JSON
//! consumers don't break when the domain evolves.

use crate::adapters::tantivy::TantivyIndex;
use crate::domain::{Query, SearchHit};
use std::path::Path;
use std::str::FromStr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OutputFormat {
    Console,
    Json,
}

impl FromStr for OutputFormat {
    type Err = anyhow::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "console" => Ok(Self::Console),
            "json" => Ok(Self::Json),
            other => anyhow::bail!("invalid --format {other:?} (expected console|json)"),
        }
    }
}

/// Execute the `search` subcommand.
pub fn run(
    index_dir: &Path,
    query_text: &str,
    limit: usize,
    format: OutputFormat,
) -> anyhow::Result<()> {
    let query = Query::new(query_text, limit)?;
    let idx = TantivyIndex::open(index_dir)?;
    let hits = idx.search(&query)?;

    let out = match format {
        OutputFormat::Console => render_console(query_text, &hits),
        OutputFormat::Json => render_json(query_text, &hits)?,
    };
    println!("{out}");
    Ok(())
}

/// Human-readable console output. Anchored by tests below so downstream
/// tooling (a shell wrapper, a future picker) can rely on the shape.
fn render_console(query: &str, hits: &[SearchHit]) -> String {
    if hits.is_empty() {
        return format!("no hits for {query:?}");
    }

    let mut out = String::new();
    for (i, h) in hits.iter().enumerate() {
        let rank = i + 1;
        let tag = h.tag.as_deref().unwrap_or("<no tag>");
        out.push_str(&format!(
            "{rank}. {tag}  (score {score:.2})\n",
            score = h.score
        ));
        let header = h.section_header.as_deref().unwrap_or("<no header>");
        out.push_str(&format!(
            "   {}:{}  — {}\n",
            h.document.display(),
            h.line,
            header
        ));
        if !h.snippet.is_empty() {
            out.push_str(&format!("   {}\n", h.snippet));
        }
        out.push('\n');
    }
    // Trim trailing blank so println! doesn't emit two newlines at the end.
    out.trim_end().to_string()
}

/// JSON wire type — decoupled from the domain so we can evolve
/// `SearchHit` without breaking JSON consumers.
#[derive(serde::Serialize)]
struct HitJson<'a> {
    document: &'a std::path::Path,
    tag: Option<&'a str>,
    section_header: Option<&'a str>,
    line: usize,
    score: f32,
    snippet: &'a str,
}

#[derive(serde::Serialize)]
struct SearchOutputJson<'a> {
    query: &'a str,
    hits: Vec<HitJson<'a>>,
}

fn render_json(query: &str, hits: &[SearchHit]) -> anyhow::Result<String> {
    let output = SearchOutputJson {
        query,
        hits: hits
            .iter()
            .map(|h| HitJson {
                document: h.document.as_path(),
                tag: h.tag.as_deref(),
                section_header: h.section_header.as_deref(),
                line: h.line,
                score: h.score,
                snippet: &h.snippet,
            })
            .collect(),
    };
    serde_json::to_string_pretty(&output).map_err(Into::into)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn hit(tag: &str, header: &str, line: usize, score: f32, snippet: &str) -> SearchHit {
        SearchHit {
            document: PathBuf::from("doc/example.txt"),
            tag: Some(tag.to_string()),
            section_header: Some(header.to_string()),
            line,
            score,
            snippet: snippet.to_string(),
        }
    }

    #[test]
    fn output_format_from_str_accepts_console_and_json() {
        assert_eq!(
            OutputFormat::from_str("console").unwrap(),
            OutputFormat::Console
        );
        assert_eq!(OutputFormat::from_str("json").unwrap(), OutputFormat::Json);
    }

    #[test]
    fn output_format_from_str_rejects_unknown() {
        let err = OutputFormat::from_str("yaml").unwrap_err();
        assert!(err.to_string().contains("invalid --format"));
    }

    #[test]
    fn render_console_names_no_hits() {
        let out = render_console("floating", &[]);
        assert_eq!(out, r#"no hits for "floating""#);
    }

    #[test]
    fn render_console_ranks_hits_1_indexed_with_scores_and_paths() {
        let hits = vec![
            hit(
                "telescope-find-files",
                "Find files",
                42,
                2.35,
                "fuzzy find files",
            ),
            hit(
                "telescope-live-grep",
                "Live grep",
                55,
                1.10,
                "grep across files",
            ),
        ];
        let out = render_console("find", &hits);
        assert!(out.contains("1. telescope-find-files"));
        assert!(out.contains("score 2.35"));
        assert!(out.contains("doc/example.txt:42"));
        assert!(out.contains("— Find files"));
        assert!(out.contains("fuzzy find files"));
        assert!(out.contains("2. telescope-live-grep"));
        assert!(out.contains("score 1.10"));
    }

    #[test]
    fn render_json_is_parseable_with_expected_fields() {
        let hits = vec![hit("t1", "H1", 3, 0.5, "body text")];
        let out = render_json("q", &hits).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(parsed["query"], "q");
        assert_eq!(parsed["hits"][0]["tag"], "t1");
        assert_eq!(parsed["hits"][0]["section_header"], "H1");
        assert_eq!(parsed["hits"][0]["line"], 3);
        assert_eq!(parsed["hits"][0]["document"], "doc/example.txt");
        assert_eq!(parsed["hits"][0]["snippet"], "body text");
    }

    #[test]
    fn render_json_serialises_nulls_for_missing_tag_and_header() {
        let hits = vec![SearchHit {
            document: PathBuf::from("x"),
            tag: None,
            section_header: None,
            line: 1,
            score: 0.1,
            snippet: String::new(),
        }];
        let out = render_json("q", &hits).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert!(parsed["hits"][0]["tag"].is_null());
        assert!(parsed["hits"][0]["section_header"].is_null());
    }
}
