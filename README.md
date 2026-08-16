# vimhelp-index

Full-text index and search over vimdoc (`:h`). Rust CLI; ships companion to
the [`vimhelp.nvim`](https://github.com/jedi-knights/vimhelp.nvim) plugin.

> **Status:** early scaffold. Not yet usable — first indexer/search PRs
> pending. Watch this space, or the [architecture repo](https://github.com/jedi-knights/architecture)
> for portfolio-level progress.

## What it will do

`vimhelp` and Neovim's built-in `:help` search work well when you know the
exact tag. They fall over for:

- fuzzy queries across the corpus (`floating window` should surface
  `nvim_open_win`, `wincfg-title`, popup-menu docs, etc.)
- full-text search of body content, not just tags
- cross-referencing across runtimepath (which docs mention which)

`vimhelp-index` builds a [`tantivy`](https://github.com/quickwit-oss/tantivy)
full-text index over any `doc/*.txt` corpus and exposes a `search` CLI.
`vimhelp.nvim` shells out to it for `:VimHelpSearch` and `K`-handler hover.

## Planned CLI

```sh
vimhelp-index build --docs='/path/to/**/doc/*.txt' --out=./index
vimhelp-index search --index=./index 'floating window'
```

Details will land as follow-up PRs. Design pass in the
[architecture TODO](https://github.com/jedi-knights/architecture) tracks
the sequenced work.

## License

MIT. See [LICENSE](./LICENSE).
