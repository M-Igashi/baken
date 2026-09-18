# Bake'n Deck 3.4.0 - your DJ software's own tags survive the gain pass

## Highlights

- **Binary DJ metadata is no longer lost when `baken headroom` rewrites a file.** Gain on lossless formats, and on MP3/AAC under the opt-in `--reencode`, goes through an ffmpeg re-mux, and ffmpeg only re-emits the metadata it can map onto its own key/value model. Everything else was silently dropped: ID3v2 `GEOB` and `PRIV` frames on AIFF and WAV, the whole `id3 ` chunk on WAV, and the free-form `----` atoms that Serato and rekordbox write into `.m4a`. baken now lifts those payloads off the source before the conversion and writes them back over the output byte for byte. Text tags, cover art and audio timing are unchanged, as they already were. MP3 and AAC native gain were never affected: mp3rgain edits `global_gain` in place and never touches the container ([#117](https://github.com/M-Igashi/baken/issues/117)).
- **`baken cdjsafe` refuses to overwrite a collection XML that changed while it was working.** `plan` reads the XML into memory and `convert` rewrote it from that snapshot at the end of the run, which can be minutes later for a long playlist. A rekordbox export made in between was silently replaced by the old collection plus the new playlist. `convert` now re-reads the file after the transcodes succeed and stops with `XmlChanged` if the bytes differ, writing nothing. The converted MP3s stay where they are, so planning again and re-running only copies them ([#110](https://github.com/M-Igashi/baken/issues/110)).

## Library users of `baken-core`

- New: `headroom::measure(path) -> Measurement` and `headroom::decide(&Measurement, TpTargetMode, GainMode) -> Decision`, the two halves of what `analyze` does per file. A `Measurement` (loudness, True Peak, bitrate, codec) is serde-serialisable and depends on no setting, so a front-end can cache it per file and re-decide when the ceiling, the `GainMode` or the decision rules change, without running loudnorm again. `AudioAnalysis::new` joins the two back together. `analyze` itself is unchanged ([#111](https://github.com/M-Igashi/baken/issues/111)).
- New error variant `Error::XmlChanged { path }` for the cdjsafe guard above. Consumers with a catch-all match arm need no change.
- No other API changes. Everything in 3.3.2 still compiles.

## Other Changes

- Mac tune-up checklist added under `docs/mac-tuneup.md`, with a Japanese version at `docs/mac-tuneup.ja.md`: the macOS settings that slow rekordbox down, and the manual steps to fix them.
- README documents the Mac app edition, and spells out what does and does not survive a rewrite.
- The LGPL ffmpeg build script used for the Mac app is published at `scripts/build-ffmpeg.sh`, as the written source offer for those binaries.
- Dependency bumps: `clap` 4.6.7, `windows-sys` 0.16.6, `mp3rgain` 3.8.0.

## Upgrading

Nothing about the flags, the report, or the backup layout has changed. Files processed by an earlier version are unaffected.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. A track whose True Peak already sat above the -0.5 dBTP ceiling (a loudness-war master, or an MP3 with inter-sample overs) was skipped. Since 3.3.0 such tracks are turned **down** to the ceiling, so every track on the stick ends up at the same True Peak, quiet and loud alike. This is the same idea as rekordbox's Auto Gain, which also applies negative gain to loud tracks, but baked into the file so it survives the USB export to a CDJ. Unlike Auto Gain the target is still the True Peak ceiling, not an average-loudness figure, so no track is made quieter than it has to be to avoid clipping.

If you prefer the old raise-only behaviour, pass `--boost-only`. As always, run with `--analyze-only` first if you want to see what would happen, and keep `--backup` on for the first run over an existing library.

Library users coming from 3.2.x: 3.3.0 added a third `GainMode` argument to `headroom::analyze()` and renamed `AudioAnalysis::has_headroom()` to `needs_gain()`. Both are breaking changes; the CLI is unaffected.
