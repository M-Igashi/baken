# Bake'n Deck 3.3.1 - no more re-encode proposals for sub-1 dB raises

## Notice for existing users: the default behaviour of `baken headroom` changed in 3.3.0

If you are upgrading from 3.2.x or earlier, read this before your first run. Up to 3.2.x, `baken headroom` only ever turned files **up**. A track whose True Peak already sat above the -0.5 dBTP ceiling (a loudness-war master, or an MP3 with inter-sample overs) was skipped. Since 3.3.0 such tracks are turned **down** to the ceiling, so every track on the stick ends up at the same True Peak, quiet and loud alike. This is the same idea as Rekordbox's Auto Gain, which also applies negative gain to loud tracks, but baked into the file so it survives the USB export to a CDJ. Unlike Auto Gain the target is still the True Peak ceiling, not an average-loudness figure, so no track is made quieter than it has to be to avoid clipping.

If you prefer the old raise-only behaviour, pass `--boost-only`. Nothing else about the flags, the report, or the backup layout has changed. As always, run with `--analyze-only` first if you want to see what would happen, and keep `--backup` on for the first run over an existing library. Files already processed by an earlier version are unaffected unless their True Peak is above the ceiling.

Library users of `baken-core`: 3.3.0 added a third `GainMode` argument to `headroom::analyze()` and renamed `AudioAnalysis::has_headroom()` to `needs_gain()`. Both are breaking changes; the CLI is unaffected.

## Highlights

- **Lossy files less than 1.0 dB below the ceiling are left alone.** MP3 and AAC can only be raised natively in 1.5 dB steps, so after one step every file sits somewhere under 1.5 dB below the ceiling, and 3.3.0 then proposed a lossy re-encode for that leftover on the next analysis (a +0.16 dB re-encode, for example). `decide_gain` now returns `None` for lossy raises under the new `headroom::MIN_REENCODE_GAIN` (1.0 dB); re-encoding is still offered, opt-in as before, for raises between 1.0 and 1.5 dB. Lossless files are unaffected and are still raised exactly, however small the gain. This is the same reasoning as 3.3.0's "lowering never re-encodes": a generation of loss for less than 1 dB is not a trade anyone wants.

## Other Changes

- README and `docs/true-peak-ceiling.md` describe the 1.0 dB floor; the CSV example row for a small AAC raise now shows `none`.
- `MIN_REENCODE_GAIN` is public so GUI front ends can explain the rule.
- The rekordbox mark is written in lowercase throughout the docs and CLI copy (#102).
