# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.0.0](https://github.com/jedi-knights/vimhelp-index/releases/tag/v0.0.0) - 2026-08-16

### Added

- *(cli)* --docs is repeatable — union globs into one corpus ([#11](https://github.com/jedi-knights/vimhelp-index/pull/11))
- cargo-dist + release-plz + composite Action ([#9](https://github.com/jedi-knights/vimhelp-index/pull/9))
- --incremental for re-indexing only changed files ([#8](https://github.com/jedi-knights/vimhelp-index/pull/8))
- *(adapter)* snippet highlighting via tantivy SnippetGenerator ([#7](https://github.com/jedi-knights/vimhelp-index/pull/7))
- *(cli)* wire build + search subcommands to parser and tantivy adapter ([#6](https://github.com/jedi-knights/vimhelp-index/pull/6))
- *(adapter)* add tantivy-backed full-text index and searcher ([#5](https://github.com/jedi-knights/vimhelp-index/pull/5))
- *(parser)* add vimdoc parser adapter ([#3](https://github.com/jedi-knights/vimhelp-index/pull/3))
- *(cli)* add build + search subcommands via clap derive ([#2](https://github.com/jedi-knights/vimhelp-index/pull/2))
- *(domain)* add Document, Query, SearchHit types ([#1](https://github.com/jedi-knights/vimhelp-index/pull/1))

### Other

- add lint + test workflow ([#4](https://github.com/jedi-knights/vimhelp-index/pull/4))
- *(scaffold)* initialize Rust workspace with hexagonal layout
