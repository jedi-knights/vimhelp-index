# vimhelp-index

[![CI](https://github.com/jedi-knights/vimhelp-index/actions/workflows/ci.yml/badge.svg)](https://github.com/jedi-knights/vimhelp-index/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/jedi-knights/vimhelp-index?include_prereleases&sort=semver)](https://github.com/jedi-knights/vimhelp-index/releases)
[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](./LICENSE)

You know the docs are in there somewhere. You remember it was about
floating windows, or was it popups? `:helpgrep` returns nine screens of
noise. `:help nvim_open_win` works only if you already know the tag.

`vimhelp-index` is the missing full-text search over `:help`. Rust CLI;
[`tantivy`](https://github.com/quickwit-oss/tantivy)-backed; ships companion
to the [`vimhelp.nvim`](https://github.com/jedi-knights/vimhelp.nvim) plugin
that wires it into `:VimHelpSearch` and the `K`-handler.

**Requirements:** none at install time. `vimhelp-index` is a single
statically-linked binary — no Rust toolchain, no Node, no Neovim required
just to build/search an index.

**Status:** pre-v0.1.0 tag; the first release will be published by
`release-plz` on the next `main` push after this repo's distribution
setup lands.

## Install

### Homebrew

```sh
brew install jedi-knights/tap/vimhelp-index
```

### Shell installer (Linux + macOS)

```sh
curl --proto '=https' --tlsv1.2 -LsSf https://github.com/jedi-knights/vimhelp-index/releases/latest/download/vimhelp-index-installer.sh | sh
```

### PowerShell installer (Windows)

```powershell
irm https://github.com/jedi-knights/vimhelp-index/releases/latest/download/vimhelp-index-installer.ps1 | iex
```

### Pre-built binaries

Download the archive for your OS/arch from the
[releases page](https://github.com/jedi-knights/vimhelp-index/releases),
extract, and put the binary on `PATH`.

### From source

Requires Rust 1.95+ (pinned via `rust-toolchain.toml`).

```sh
cargo install --git https://github.com/jedi-knights/vimhelp-index vimhelp-index
```

### GitHub Actions

Install-only composite action — no opinionated defaults. Callers write their own `build` / `search` steps against a binary on `PATH`.

```yaml
- uses: jedi-knights/vimhelp-index@v0
- run: vimhelp-index build --docs 'doc/*.txt' --out ./index
- run: vimhelp-index search --index ./index 'floating window'
```

## Usage

### `build` — index a glob of vimdoc files

```sh
vimhelp-index build --docs='/path/to/**/doc/*.txt' --out=./index
vimhelp-index build --docs='/path/to/**/doc/*.txt' --out=./index --incremental
```

`--incremental` only re-indexes files that changed since the last build
(detected via mtime + size, stored in `<out>/vimhelp-manifest.json`).
Falls back to a full build when the manifest is absent or on an
incompatible version.

Sample output:

```
Incremental update at ./index:
  new:       1
  changed:   0
  removed:   0
  unchanged: 187
```

### `search` — query a built index

```sh
vimhelp-index search --index=./index 'floating window'
vimhelp-index search --index=./index --limit=10 'floating window'
vimhelp-index search --index=./index --format=json 'floating window'
```

Console output (default) — BM25-ranked hits with the snippet centered
on the matched term:

```
1. nvim_open_win  (score 3.42)
   $VIMRUNTIME/doc/api.txt:1287  — 3.2. Window functions
   Opens a new floating window. Windows attach to a buffer …

2. wincfg-title  (score 1.85)
   ...
```

JSON output shape (for pickers / other tooling):

```json
{
  "query": "floating window",
  "hits": [
    {
      "document": "$VIMRUNTIME/doc/api.txt",
      "tag": "nvim_open_win",
      "section_header": "3.2. Window functions",
      "line": 1287,
      "score": 3.42,
      "snippet": "Opens a new floating window. Windows attach to a buffer …"
    }
  ]
}
```

## How it works

- **Parser** — walks vimdoc, splits at `====` section rules, extracts
  `*tags*`. Subsection `----` rules deliberately stay inside their parent
  section (fragmenting hurts full-text relevance).
- **Index** — one tantivy segment per section, not per file. Hits attribute
  to the narrowest addressable unit (header + line inside a doc) so a UI
  can jump the user precisely into `:help`.
- **Search** — `tantivy::QueryParser` fans out across tags / header / body.
  BM25 gives shorter fields higher scores by construction, so header/tag
  matches rank above body matches naturally.
- **Snippets** — `tantivy::SnippetGenerator` centers the excerpt on the
  matched term. Tag-only matches (no body match) fall back to a bounded
  first-N-char excerpt so hits always have renderable context.

## Development

```sh
cargo test --all-targets
cargo clippy --all-targets -- -D warnings
cargo fmt --check
```

Tests live in `src/**/mod.rs` (unit) and `tests/*.rs` (integration).
CLI end-to-end tests must be at integration position (`tests/cli.rs`)
because `assert_cmd::Command::cargo_bin` only rebuilds the binary for
integration targets — a `src/`-position unit test runs against a stale
`target/debug/` binary.

## License

MIT. See [LICENSE](./LICENSE).
