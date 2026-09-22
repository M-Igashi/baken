# Bake'n Deck 3.8.0 - `baken expressport` writes a stick for libraries rekordbox has never analysed

A feature release for `expressport`, prompted by the first report from a machine without rekordbox: a Mixxx library converted to `collection.xml` on Ubuntu ([#145](https://github.com/M-Igashi/baken/issues/145)). `expressport` stays a beta until a player has read a stick it wrote; what that takes is in [#116](https://github.com/M-Igashi/baken/issues/116).

## Highlights

- **`--generate-analysis` computes the analysis files from the audio** for tracks rekordbox never analysed, instead of leaving them out ([#147](https://github.com/M-Igashi/baken/issues/147), [#148](https://github.com/M-Igashi/baken/pull/148)). The beat grid and the cues come from the XML as before; the waveforms are decoded in-process (symphonia, the same decoder `baken headroom` uses) and written as rekordbox 7 writes them: `.DAT`, `.EXT` and `.2EX` with every section but the phrase analysis, which only rekordbox can produce. The rules were measured against 1,070 rekordbox analysis files paired with their audio: the scrolling waveform's height and whiteness reproduce rekordbox on 99.5% of columns, the three-band and colour waveforms are close approximations (correlation 0.90 to 0.99), and the beat grid expands from `TEMPO` with rekordbox's own rounding. Off by default, and tracks that do have a rekordbox analysis are still copied, because copying is exact.
- **With the flag, rekordbox is no longer required for the waveforms.** A Traktor or Mixxx library converted to `collection.xml` can go straight to a stick. The four My Settings files are still required, because nothing in the XML can stand in for them; `--settings-dir` takes the `PIONEER` folder of any stick rekordbox once exported.
- **`expressport` says what it needs when rekordbox is not there** ([#146](https://github.com/M-Igashi/baken/pull/146)). On Linux it no longer suggests a macOS path under `$HOME/Library`, it searches the platform mount points (`/media`, `/mnt`, `/run/media/$USER`) for an attached library drive, and both the settings error and the analysis error state what is actually required and which flag supplies it.

## Other changes

- `--dry-run` reports how many tracks will have their analysis generated, and the summary counts them separately from the copied ones.
- `docs/dj-software-compatibility.md` answers two questions that came in after the 3.7.0 run: why re-analysing in Traktor still leaves a different autogain on every track (Headroom aligns true peak, not loudness), and where the Mac app writes its backups. It also stops describing the Mac app's re-encode checkbox, which Bake'n Deck 1.0.2 removed.
- Issue housekeeping: the `expressport` design record ([#115](https://github.com/M-Igashi/baken/issues/115)) is closed as complete, and [#116](https://github.com/M-Igashi/baken/issues/116) is the one place that says what remains before release and what to test.

## Library users

- `baken-export`: new module `anlz::generate` (`decode`, `grid`, `waveform`, `assemble`; `build_files` returns the three `AnlzFile`s for a track). `Options` gains `generate_analysis: bool` (the struct is `Default`, so `..Default::default()` literals keep compiling). `PlanTrack::anlz` is now `Option<Entry>` (`None` means generate), `Plan::generated()` counts those tracks and `Report` gains `anlz_generated`. `anlz::cues::sections` builds the cue sections without an existing file. `Error::NoAnlzRoot` is not returned when `generate_analysis` is set.
- `baken-core`: no API change. `symphonia` moved to a workspace dependency shared with `baken-export`.

## Upgrading

No action needed. `expressport` behaves exactly as in 3.7.0 unless you pass `--generate-analysis`. The beta banner stays until [#116](https://github.com/M-Igashi/baken/issues/116) has a hardware result; if you own a CDJ, that issue says what to test and in which order.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
