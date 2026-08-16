//! Fixture-driven snapshot tests for the vimdoc parser. Each fixture is a
//! real-shape .txt file under tests/fixtures/vimdoc/; the parsed
//! `Document` is snapshotted via `insta::assert_debug_snapshot!`. When the
//! parser's output shape changes intentionally, run `cargo insta review`
//! to accept new snapshots.

use std::path::PathBuf;
use vimhelp_index::adapters::parser::vimdoc;

fn fixture(name: &str) -> PathBuf {
    let mut p = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    p.push("tests/fixtures/vimdoc");
    p.push(name);
    p
}

#[test]
fn tiny_fixture_parses_to_expected_shape() {
    let path = fixture("tiny.txt");
    let doc = vimdoc::parse_file(&path).expect("parse_file");

    // Assert the load-bearing shape without an insta dep on this test — one
    // strong assertion beats a snapshot for the first fixture, since a
    // snapshot on the first run is a rubber-stamp. Later fixtures can use
    // assert_debug_snapshot when the parser stabilises.
    assert_eq!(doc.sections.len(), 3, "preamble + 2 sections");

    // Top-level tag list is deduplicated + first-occurrence order.
    assert_eq!(
        doc.tags,
        vec![
            "tiny.txt".to_string(),
            "tiny-intro".to_string(),
            "tiny-ref".to_string(),
            "tiny-second-tag".to_string(),
        ]
    );

    // Preamble section has no header, holds the file-tag intro.
    assert_eq!(doc.sections[0].header, None);
    assert!(doc.sections[0].body.contains("*tiny.txt*"));

    // First body section: header text stripped of trailing tag marker.
    assert_eq!(doc.sections[1].header.as_deref(), Some("1. Introduction"));
    assert_eq!(doc.sections[1].tags, vec!["tiny-intro".to_string()]);

    // Second body section: two tags, one on header, one inline in body.
    assert_eq!(doc.sections[2].header.as_deref(), Some("2. Reference"));
    assert_eq!(
        doc.sections[2].tags,
        vec!["tiny-ref".to_string(), "tiny-second-tag".to_string()]
    );
}
