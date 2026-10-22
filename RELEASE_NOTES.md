# Bake'n Deck 4.7.0 - AAC in `headroom`: no more FDK-AAC clicks, HE-AAC skipped, raw `.aac` fixed

A minor release about AAC in `baken headroom`, built on mp3rgain 3.12.0. It is a minor rather than a patch because `baken-core` gained an error variant and a field (see "Library users").

## Highlights

- **AAC encoded with FDK-AAC no longer clicks after `baken headroom`.** FDK-AAC (fdkaac, ffmpeg's `libfdk_aac`, foobar2000 through fdkaac, Android) ends some frames with a 14-byte fill element, which mp3rgain misread from 2.8.0 to 3.11.1. Those frames kept their original level while every other frame moved, so each one played louder or quieter than its neighbours for 23 ms: a 4.5-minute fdkaac encode at 256 kbps had 13 such clicks. mp3rgain 3.12.0 reads them correctly ([M-Igashi/mp3rgain#402](https://github.com/M-Igashi/mp3rgain/issues/402)). Apple's encoder, ffmpeg's own `aac` and faac were never affected.
- **HE-AAC is skipped.** `global_gain` only reaches the AAC core of HE-AAC; the decoder rebuilds the high band from SBR data stored at absolute levels, so a native gain change left the top end at its old level, as many dB too loud or too quiet as the change. Nothing short of a re-encode can move it, and `headroom` never re-encodes, so HE-AAC files (v1 and v2, in `.m4a` or raw `.aac`) are now recognised before they are measured or backed up and listed as skipped after the analysis, not counted as failed. Their measurement was off too: the built-in decoder reads some HE-AAC without its SBR band, which put the true peak 0.7 to 0.8 dB low ([#268](https://github.com/M-Igashi/baken/issues/268)).

## Already processed AAC files that click

Running `headroom` again does not repair an FDK-AAC file an earlier version processed: the next run moves every frame by the same step count, so the frames that were skipped keep their offset. Every version from 2.1.0 to 4.6.1 is affected. Restore such a file from a `--backup` copy or from its source, then run 4.7.0 on it.

## Fixes

- **Raw ADTS `.aac` files are adjusted.** They went through the MP4 path, so every `.aac` was measured, backed up and then failed with `No moov box found in MP4 file`. They now go through mp3rgain's ADTS path ([#269](https://github.com/M-Igashi/baken/pull/269)).
- **An AAC file with frames mp3rgain cannot parse is left alone**, with a message, before anything is backed up. Such frames would keep their level while the rest moved, which plays as a click. No encoder tested produces them with mp3rgain 3.12.0 ([#269](https://github.com/M-Igashi/baken/pull/269)).

## Other changes

- The README opens with what baken is and is not: built for rekordbox, not instead of it ([#267](https://github.com/M-Igashi/baken/pull/267)). It also says that native AAC gain means AAC-LC and why HE-AAC is skipped.
- Dependencies: mp3rgain 3.12.0, serde_json 1.0.154, getrandom 0.4.3, rustls 0.23.46 ([#266](https://github.com/M-Igashi/baken/pull/266)). CI: actions/upload-artifact 7.0.2, actions/download-artifact 8.0.2, dtolnay/rust-toolchain ([#263](https://github.com/M-Igashi/baken/pull/263), [#264](https://github.com/M-Igashi/baken/pull/264), [#265](https://github.com/M-Igashi/baken/pull/265)).

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: new `Error::HeAac`. `headroom::measure` returns it for an HE-AAC file without decoding it, and `headroom::analyze` puts such files in the new `AnalyzeOutcome::he_aac` instead of `failures`. `headroom::apply` refuses HE-AAC with the same message before the backup, so an analysis cached by an earlier version cannot reach mp3rgain's refusal after a copy was made. Code that matches `Error` exhaustively or builds `AnalyzeOutcome` with a struct literal needs the new variant and field.
- `baken-export`: no API change.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
