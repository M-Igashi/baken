# Bake'n Deck 3.0.3 — bit depth preservation

## Highlights

- **Lossless files keep their original bit depth.** `apply_gain_ffmpeg` hardcoded its output format, so every processed lossless file came back at 24-bit regardless of the source — a 16-bit WAV grew ~50% (FLAC up to ~95%) for no added resolution, and 32-bit float masters were silently truncated. The source is now probed with ffprobe and written back in its own sample format, with a fallback to the previous 24-bit output if the probe fails. Fixes #74.

## Other Changes

- Dependency updates: mp3rgain 3.2.0, clap 4.6.6, thiserror 2.0.20
- CI: bumped dtolnay/rust-toolchain

**Full Changelog**: https://github.com/M-Igashi/baken/compare/v3.0.2...v3.0.3
