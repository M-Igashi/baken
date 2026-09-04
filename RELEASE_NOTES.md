# Bake'n Deck 3.2.0 - baken-core library split

## Highlights

- **The processing engine is now a separate library crate, [`baken-core`](https://crates.io/crates/baken-core).** Loudness analysis and gain application, the Key+BPM playlist sorter, and the CDJ-safe transcoder live in `crates/baken-core`; the `baken` binary in `crates/baken` is a thin terminal front-end over it. The library carries no terminal UI dependencies and never prints, so other front-ends can embed it. It exposes a `Progress` callback and a `CancelToken` for long-running operations, a `set_tools` call to point at bundled `ffmpeg` and `ffprobe` binaries instead of `PATH`, and a typed `Error` enum at the public boundary. `cdjsafe` is split into `plan` (read and validate the playlist without touching disk) and `convert`. This is groundwork for a native Mac app (#96); the CLI's flags, prompts and output are unchanged (#84, #97).

## Other Changes

- **Fixed `--backup` and `--report` without a value.** `baken headroom <paths> --backup` failed with "a value is required" even though the directory is documented as optional, because clap's built-in path parser rejects the empty sentinel used to mean "flag given, use the default". Both flags now accept the bare form again and fall back to `<target>/backup` and `<target>/baken_report_<timestamp>.csv` (#98).
- Per-file failures during analysis and processing are printed after the progress bar finishes instead of interleaved with it.
- The release workflow publishes `baken-core` to crates.io before `baken`.
