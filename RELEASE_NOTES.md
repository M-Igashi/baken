# Bake'n Deck 3.3.2 - Apple Lossless (.m4a) is handled as lossless

## Notice for existing users: the default behaviour of `baken headroom` changed in 3.3.0

If you are upgrading from 3.2.x or earlier, read this before your first run. Up to 3.2.x, `baken headroom` only ever turned files **up**. A track whose True Peak already sat above the -0.5 dBTP ceiling (a loudness-war master, or an MP3 with inter-sample overs) was skipped. Since 3.3.0 such tracks are turned **down** to the ceiling, so every track on the stick ends up at the same True Peak, quiet and loud alike. This is the same idea as Rekordbox's Auto Gain, which also applies negative gain to loud tracks, but baked into the file so it survives the USB export to a CDJ. Unlike Auto Gain the target is still the True Peak ceiling, not an average-loudness figure, so no track is made quieter than it has to be to avoid clipping.

If you prefer the old raise-only behaviour, pass `--boost-only`. Nothing else about the flags, the report, or the backup layout has changed. As always, run with `--analyze-only` first if you want to see what would happen, and keep `--backup` on for the first run over an existing library. Files already processed by an earlier version are unaffected unless their True Peak is above the ceiling.

Library users of `baken-core`: 3.3.0 added a third `GainMode` argument to `headroom::analyze()` and renamed `AudioAnalysis::has_headroom()` to `needs_gain()`. Both are breaking changes; the CLI is unaffected.

## Highlights

- **ALAC in an `.m4a` container now gets the exact lossless gain.** Up to 3.3.1 every `.m4a` was classified as AAC by its extension, so an Apple Lossless file went down the native AAC gain path and failed with `mp3rgain failed to apply AAC gain: No AAC audio track found`. The analyzer now reads the codec from the loudnorm run's own input dump (no extra ffprobe call) and treats `alac` like FLAC: exact gain via ffmpeg, written back as ALAC at the source's bit depth (16-bit stays 16-bit, everything else is written as 24-bit). AAC files are unchanged. `apply_gain_ffmpeg` refuses an `.m4a`/`.mp4` whose payload is not ALAC, so nothing can be silently re-encoded to AAC by the lossless path.

## Other Changes

- Requires an ffmpeg with the `alac` encoder. Stock ffmpeg builds have it; minimal builds need `--enable-encoder=alac`.
- README lists ALAC/M4A among the scanned formats and in the processing-method table.
- Since 3.3.1, lossy files less than 1.0 dB below the ceiling are left alone instead of being offered a re-encode for the leftover of the 1.5 dB native step (`headroom::MIN_REENCODE_GAIN`).
