# Bake'n Deck 3.0.1 — faster lossless gain via mp3rgain 2.10

## Highlights

- **Updated built-in mp3rgain library from 2.9.6 to 2.10.0.** The gain-apply pipeline now reads each file once instead of twice, roughly halving I/O on large batches, and APE tag-only updates rewrite just the file tail. Output stays bit-identical — this is a pure performance update, no behavior changes.

## Other Changes

- Deduplicated the shared interactive/scriptable pipeline in `cli.rs`
- Codebase simplification: deduplicated XML helpers, fixed a potential panic on non-ASCII paths, misc cleanups
- Added `homepage` to crate metadata (now live on crates.io)
- Excluded `web/` from the crates.io package

**Full Changelog**: https://github.com/M-Igashi/baken/compare/v3.0.0...v3.0.1
