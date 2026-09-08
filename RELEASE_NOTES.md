# Bake'n Deck 3.3.0 - headroom now lowers loud tracks too

## Notice for existing users: the default behaviour of `baken headroom` has changed

Up to 3.2.x, `baken headroom` only ever turned files **up**. A track whose True Peak already sat above the -0.5 dBTP ceiling (a loudness-war master, or an MP3 with inter-sample overs) was skipped. From 3.3.0 such tracks are turned **down** to the ceiling, so every track on the stick ends up at the same True Peak, quiet and loud alike. This is the same idea as Rekordbox's Auto Gain, which also applies negative gain to loud tracks, but baked into the file so it survives the USB export to a CDJ. Unlike Auto Gain the target is still the True Peak ceiling, not an average-loudness figure, so no track is made quieter than it has to be to avoid clipping.

If you prefer the old raise-only behaviour, pass `--boost-only`. Nothing else about the flags, the report, or the backup layout has changed. As always, run with `--analyze-only` first if you want to see what would happen, and keep `--backup` on for the first run over an existing library. Files already processed by an earlier version are unaffected unless their True Peak is above the ceiling.

## Highlights

- **Loud files are lowered to the ceiling by default.** Lossless files (FLAC, AIFF, WAV) get the exact negative gain via ffmpeg. MP3 and AAC/M4A are lowered natively in 1.5 dB global_gain steps, rounded up so the result never sits above the ceiling (a file at +0.3 dBTP takes one step down and lands at -1.2 dBTP). Lowering never re-encodes: a lossy pass just to make a file quieter is not worth the quality cost.
- **New `--boost-only` flag** restores the pre-3.3 raise-only behaviour. The startup banner now prints the active gain mode next to the True Peak target.

## Other Changes

- `baken-core`: new `headroom::GainMode { Normalize, BoostOnly }`, passed to `headroom::analyze()` as a new third argument. `AudioAnalysis::has_headroom()` is renamed `needs_gain()`. Both are breaking changes for library users; the CLI is unaffected.
- The gain decision (method, effective gain, native step count) is now the pure function `decide_gain` with unit tests covering the rounding rules in both directions.
- Report headings for native MP3/AAC gain no longer state a "requires TP <= target - 1.5 dB" condition, since files above the ceiling are now handled too.
- README and `docs/true-peak-ceiling.md` describe the two-directional behaviour and the new flag.
