# Bake'n Deck 4.4.0 - `expressport` writes active loops and leaves one library on the stick

A minor release, mostly for `baken expressport` ([#208](https://github.com/M-Igashi/baken/issues/208), [#210](https://github.com/M-Igashi/baken/issues/210), [#212](https://github.com/M-Igashi/baken/issues/212)). It is a minor rather than a patch because the library crates changed their public API (see "Library users"). `headroom` and `rbsort` behave as in 4.3.1; `cdjsafe` picks a different playlist in one edge case, described below.

## Highlights

- **Active loops.** The rekordbox XML has no field for an active loop, a memory loop the player engages by itself when playback reaches it. A memory loop whose name starts with `[active]` (in any case) now becomes the track's active loop on the stick, marked the way rekordbox marks one ([#210](https://github.com/M-Igashi/baken/issues/210), [#214](https://github.com/M-Igashi/baken/pull/214)). The marker is left out of the cue's comment, so `[active] Build` shows as `Build`. There is one active loop per track, as the player manuals say: when several memory loops are marked the earliest wins and the plan warns, and it also warns when the marker is only on cues that are not memory loops. `--dry-run` shows all of this before anything is written. Not tested on a player yet.
- **A stick rekordbox exported to keeps one library.** rekordbox 7 writes two libraries side by side in `PIONEER/rekordbox/`: the Device Library (`export.pdb`, `exportExt.pdb`) and OneLibrary (`exportLibrary.db` with its `-wal` and `-shm`). Replacing only `export.pdb` left the other files describing rekordbox's old library, which rekordbox reports as "a library inconsistency on the device" and OneLibrary players read instead of ours. `expressport` now lists those files in the plan (and in `--dry-run`) and removes them right after the new `export.pdb` is written, never before, so a cancelled or failed run leaves both libraries as they were ([#208](https://github.com/M-Igashi/baken/issues/208), [#209](https://github.com/M-Igashi/baken/pull/209)). A OneLibrary player (CDJ-3000X, XDJ-AZ, OPUS-QUAD, OMNIS-DUO) then shows "OneLibrary not found" instead of rekordbox's old library.

## Fixes

- **Beat grids of hand-edited tracks with `--generate-analysis`.** rekordbox writes a `TEMPO` wherever an edited grid changes tempo, and some of those segments carry a `Bpm` that does not describe their spacing (a one-beat `Bpm="654.33"` on a 140 BPM track). `expressport` added 1 to 4 extra beats before the next `TEMPO` there. The beat count now also checks the `Battito` step to the next `TEMPO`, which on rekordbox's own exports always agrees with the count: on the reference export, 0 of 17,075 segments differ from rekordbox's grid, against 5 before ([#212](https://github.com/M-Igashi/baken/issues/212), [#213](https://github.com/M-Igashi/baken/pull/213)). An XML that puts `Battito="1"` on every `TEMPO` (mixxx2rekordbox does today) keeps the old count, and copied analysis keeps rekordbox's own grid as before.
- **A truncated audio file no longer stops `expressport`.** Reading the bit depth of a WAV, AIFF or FLAC file whose header was cut short crashed the plan, in the CLI and in the Mac app. Such a file now reads as 16-bit, like any file whose depth cannot be read ([#215](https://github.com/M-Igashi/baken/pull/215)).
- **`headroom` on a Windows network share.** MP3 and AAC gain occasionally failed a random file with "Access is denied" when Windows Defender opened it just before the final rename. mp3rgain 3.9.1 retries that rename, and baken now requires it.

## Other changes

- **`cdjsafe` and two playlists at one path.** When the XML holds two playlists with the same folder path and name, `cdjsafe` now converts the first one, as `rbsort` and `expressport` already did; up to 4.3.1 it took the last one ([#215](https://github.com/M-Igashi/baken/pull/215)).
- **One decoder.** The codebase was reviewed as a whole for duplicated and dead code ([#215](https://github.com/M-Igashi/baken/pull/215)). `headroom`'s loudness measurement and `expressport`'s generated analysis now share one decoder; measured values are identical to 6 decimals on MP3, FLAC, AIFF, WAV and ALAC files.

## Library users

- `baken-core`
  - Added `decode::decode_with(file, extension, sink) -> Result<Codec>`, the symphonia decode that `headroom`'s measurement and `baken-export`'s generated analysis share.
  - Removed `Error::SourceNotFound`. Nothing has constructed it since 3.2.2, where `cdjsafe` started skipping a missing source (now an entry in `cdjsafe::Plan::skipped()`) instead of failing; a `match` on it has to drop that arm.
  - `cdjsafe::plan` takes the first of two playlists at one path (it took the last).
  - `cdjsafe::probe`: when ffprobe reports `bits_per_raw_sample` as 0, `SourceInfo::bit_depth` now falls back to `bits_per_sample` instead of `None`.
  - The mp3rgain requirement is 3.9.1.
- `baken-export`
  - Removed `Error::Cancelled`, which was never constructed: `export` reports a cancel in `Report::cancelled`. A `match` on it has to drop that arm.
  - Removed the `anlz::generate::decode` module and `anlz::generate::decode_with`; use `baken_core::decode::decode_with`. `baken-export` no longer depends on symphonia directly.
  - Removed `anlz::cues::pcob_empty` and `anlz::rewrite::strip_pvb2`, and made `anlz::cues::pcob` and `anlz::cues::pco2` private; `anlz::cues::sections` and `anlz::cues::splice` build the cue sections.
  - Added `collection::Track::grid_bpm()`.
  - New in [#209](https://github.com/M-Igashi/baken/pull/209): `ONELIBRARY_FILES`, `Plan::onelibrary_files`, `Report::onelibrary_removed`, `Report::onelibrary_kept`.
  - New in [#214](https://github.com/M-Igashi/baken/pull/214): the public field `collection::Cue::marked` (code that builds a `Cue` without `..Default::default()` needs it), `collection::ACTIVE_LOOP_MARKER`, `collection::active_loop`, `collection::ActiveLoopWarning`, `Cue::is_memory_loop`, `Plan::active_loops`, `Plan::active_loop_warnings`.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
