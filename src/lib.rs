//! `vimhelp-index` — full-text index and search over vimdoc.
//!
//! Hexagonal layout: `domain` holds the language of the problem (documents,
//! queries, search hits); `ports` are the interfaces the adapters implement;
//! `adapters` bind concrete tech (filesystem walkers, the tantivy engine, the
//! JSON reporter) to those interfaces; `cli` is the clap-driven binary entry
//! that wires them together. Keeping IO out of `domain` keeps the tests fast
//! and the design portable across future index backends.

pub mod adapters;
pub mod cli;
pub mod domain;
pub mod ports;
