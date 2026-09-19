# Bake'n Deck 3.6.0 - `baken headroom` analysis is 15x faster

## Highlights

- **Analysis no longer waits on ffmpeg's `loudnorm` filter.** `baken headroom` now decodes each file in-process (symphonia) and measures integrated loudness and true peak with the BS.1770-4 analyzer from the built-in mp3rgain library, true peak meter per Annex 2 included. An 8.9 minute 320 kbps MP3 that took 14.3 s to analyse now takes 0.9 s; a 4.6 minute 24-bit WAV goes from 7.3 s to 0.3 s; a six-track test batch that took 124 s finishes in 8 s. Where the time went: `loudnorm` resamples everything to 192 kHz and runs a complete normalisation pass whose output baken never used, while the decode itself was about 3% of the cost ([#114](https://github.com/M-Igashi/baken/issues/114), [#129](https://github.com/M-Igashi/baken/issues/129)).
- **Same decisions, same numbers where it matters.** True peak agrees with the previous engine to 0.01 dB for every test file at or below 0 dBTP, and integrated loudness within 0.06 LU. Hard-clipped masters that sit well above the ceiling (+1.5 dBTP and beyond) can read 0.1 to 0.2 dB differently, because the two engines interpolate inter-sample overs differently; the proposed method and step count were identical on every test file, and files already processed by an earlier version are not proposed again.
- **Nothing that measured before stops measuring.** Anything symphonia cannot open (HE-AAC, an unusual container, a mislabelled file) is measured by the old `loudnorm` run automatically. ffmpeg is still required by `baken headroom` for applying gain to lossless files and for re-encodes.

## Other changes

- `baken expressport --help` describes the command as beta, matching the release notes.

## Library users

- `baken-core`: `headroom::measure` (and therefore `headroom::analyze`) decodes in-process. `Measurement`, `decide` and `analyze` keep their signatures and fields, so front-ends compile unchanged; a `Measurement` cached from 3.5 stays comparable (see the parity numbers above). The codec of an `.m4a` file now comes from the decoded stream rather than ffmpeg's input dump on this path.
- New dependency `symphonia 0.6` with the `mp3`, `aac`, `isomp4`, `flac`, `wav`, `aiff`, `alac` and `pcm` features; `mp3rgain` is now used with its `replaygain` feature (the BS.1770 analyzer lives behind it). The MP3 and AAC decoders were already compiled in through mp3rgain, so the release binary grows by about 1.3 MB for the lossless codecs.
- The ffmpeg binaries configured with `set_tools` are only used by `measure` for the fallback; a front-end that bundles them keeps working exactly as before, one without them measures every common format natively.

## Upgrading

No action needed. Analysis results and gain proposals for your library stay the same apart from the clipped-master delta described above; `--tp-target`, `--tp-split-bitrate` and `--boost-only` behave as before.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
