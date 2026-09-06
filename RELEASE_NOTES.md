# Bake'n Deck 3.2.1 - ffmpeg health check fix

## Highlights

- **`check_ffmpeg` now fails when ffmpeg cannot actually run.** The startup check in `baken-core` only proved that the `ffmpeg -version` process spawned, so a binary that dyld kills immediately (broken code signature, missing dylib, library validation failure) still passed and the problem surfaced later as a confusing per-file "No loudnorm data found" error. The check now requires a successful exit and an `ffmpeg version` banner, and reports anything else through the new `Error::FfmpegFailed` variant, which carries the tail of ffmpeg's stderr so the real cause is visible (`ffmpeg was found but failed to run: dyld[...]: Library not loaded ...`). A missing binary still maps to `Error::FfmpegNotFound` (#99, #100).

## Other Changes

- None. The CLI's flags, prompts and output are unchanged.
