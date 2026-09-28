# Bake'n Deck 4.1.0 - `expressport` sticks laid out more like rekordbox's, and generated analysis in a few MB

A minor release for `baken expressport`, built from the first hardware reports in [#116](https://github.com/M-Igashi/baken/issues/116) and from checking our writer against [rbsync](https://github.com/aquarazorda/rbsync), whose `export.pdb` has been read by a CDJ-2000NXS2 with 3,436 tracks. `expressport` stays a beta until the remaining #116 tests are done. `headroom`, `rbsort` and `cdjsafe` are unchanged.

## Highlights

- **`export.pdb` strings are aligned like rekordbox's, and the track count is written** ([#168](https://github.com/M-Igashi/baken/pull/168)). rekordbox starts every UTF-16 string (any name that is not plain ASCII) on a 4-byte boundary of its row; baken packed them, some at odd offsets. A CDJ-2000NXS2 froze while drawing the ARTIST preview of a stick with non-ASCII names, and this is the prime suspect; please retest on an NXS2. The track count in the history row was 0, which a CDJ-3000 showed as `Songs 0`. Thanks to @Alex2Code for the report that found both.
- **Tracks that share an analysis folder no longer overwrite each other** ([#176](https://github.com/M-Igashi/baken/issues/176), [#177](https://github.com/M-Igashi/baken/pull/177)). The folder under `PIONEER/USBANLZ` is a hash of the track's path modulo 200003, so two tracks can land in the same one: about one pair in 600 tracks, three in 1,100. rekordbox then writes the second track's files as `ANLZ0001.*`; baken wrote `ANLZ0000.*` for both, so one of the two tracks got the other's waveform, beat grid and cues. They are now numbered in export order, exactly as rekordbox did in the reference export.
- **`--generate-analysis` needs a few MB instead of gigabytes** ([#171](https://github.com/M-Igashi/baken/issues/171), [#178](https://github.com/M-Igashi/baken/pull/178)). Every track used to be decoded whole into memory, and the process kept most of it: 1.8 GB for a 352-track export. The waveforms are now measured as the audio is decoded: 50 MB for the same export, with every file on the stick byte-identical to 4.0.2's.

## Other changes

- `baken expressport --generate-analysis` fills in sample rate, bitrate and length from the audio when the XML has them as 0, following the values rekordbox itself writes: the nominal rate for MP3, the PCM rate for lossless (`1411`, `2116`), whole seconds for the length ([#169](https://github.com/M-Igashi/baken/pull/169)). A CDJ-3000 showed such 320 kbps MP3s as VBR. A value from the XML always wins.
- `baken expressport` checks that the stick can be written before the first track, and says why when it cannot: not mounted, read-only, full, or a mount left behind on Linux after the stick was pulled ([#165](https://github.com/M-Igashi/baken/issues/165), [#166](https://github.com/M-Igashi/baken/pull/166)). The plan also warns when `--device` is not the root of a mounted volume, which would put the export on your own disk. Thanks to @sairutra for the report.
- The report says where the My Settings files went: `PIONEER/` on the stick, not its root ([#172](https://github.com/M-Igashi/baken/issues/172), [#174](https://github.com/M-Igashi/baken/pull/174)). Thanks to @sairutra.
- README: a CDJ-2000NXS2 or older reads only FAT32 on an MBR partition table, and does not show an exFAT or GPT stick at all ([#175](https://github.com/M-Igashi/baken/pull/175)); check a finished stick on a player, not by opening it in rekordbox, which rewrites a device library it opens.
- CI: `dtolnay/rust-toolchain` bumped ([#164](https://github.com/M-Igashi/baken/pull/164)).

## Library users

- `baken-core`: no changes; the version follows the workspace.
- `baken-export` is **not source-compatible** with 4.0.x (`baken` is its only known user):
  - `anlz::generate::{decode, Pcm}` are removed. `measure(path)` returns a `Measured` (sample rate, channels, frame count, columns), `Meter` measures buffers as they are decoded, and `waveform::analyze` and `build_files` take `&Measured`.
  - `DeviceTrack` gains `anlz_index` and `anlz_path(ext)`; `anlz::hash::AnlzSlots` hands out the numbers.
  - `anlz::rewrite::mp3_audio_frames` is now `mp3_audio`, returning `Mp3Audio` (frames, bytes, sample rate, `kbps()`).
  - `Plan` gains `volume_root`, `Error` gains `DeviceWrite`, and `pdb::rows::history_property` takes the track count.

## Upgrading

- Run `baken expressport` again onto sticks written by 4.0.x. `export.pdb` is rebuilt on every run and changed analysis files are rewritten; audio already on the stick is not copied again.
- For #116 testers: an `ANLZ0001.*` file next to an `ANLZ0000.*` is now normal. It is baken numbering two tracks that share a folder, not the player rejecting a file.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
