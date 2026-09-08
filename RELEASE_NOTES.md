# Bake'n Deck 3.3.2 - Apple Lossless (.m4a) is handled as lossless

## Highlights

- **ALAC in an `.m4a` container now gets the exact lossless gain.** Up to 3.3.1 every `.m4a` was classified as AAC by its extension, so an Apple Lossless file went down the native AAC gain path and failed with `mp3rgain failed to apply AAC gain: No AAC audio track found`. The analyzer now reads the codec from the loudnorm run's own input dump (no extra ffprobe call) and treats `alac` like FLAC: exact gain via ffmpeg, written back as ALAC at the source's bit depth (16-bit stays 16-bit, everything else is written as 24-bit). AAC files are unchanged. `apply_gain_ffmpeg` refuses an `.m4a`/`.mp4` whose payload is not ALAC, so nothing can be silently re-encoded to AAC by the lossless path.

## Other Changes

- Requires an ffmpeg with the `alac` encoder. Stock ffmpeg builds have it; minimal builds need `--enable-encoder=alac`.
- README lists ALAC/M4A among the scanned formats and in the processing-method table.
