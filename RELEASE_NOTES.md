# Bake'n Deck 4.11.0 - HFS+ sticks a CDJ-3000 reads, and what the stick already holds kept

A minor release: the `baken-export` API changes (see Library users). `baken-core` has no API change.

## Highlights

- **HFS+ sticks: the library goes into `.PIONEER`, where a CDJ-3000 looks.** On a stick formatted as Mac OS Extended, rekordbox keeps its library in the hidden folder `.PIONEER`, and a CDJ-3000 (firmware 3.22) reads it only from there. A stick written by `expressport` 4.10.0 or older, which always used `PIONEER`, showed "rekordbox database not found", and so did rekordbox's own stick once its folder was renamed to `PIONEER`. `expressport` now writes `.PIONEER` on HFS+. **If you exported to an HFS+ stick with an earlier version, run `expressport` on it again:** it renames `PIONEER` to `.PIONEER` first and keeps the audio and analysis already there. FAT32 and exFAT sticks are not affected. To see `.PIONEER` in Finder, press Cmd-Shift-period. Thanks to @nora7297 for the tests on a CDJ-3000 ([#306](https://github.com/M-Igashi/baken/pull/306), [#307](https://github.com/M-Igashi/baken/pull/307), fixes [#287](https://github.com/M-Igashi/baken/issues/287)).
- **What the stick already holds stays.** `expressport` replaces `export.pdb` whole, and until now that dropped what only the stick held. It now reads the stick's `export.pdb` first and carries three things into the new one. The play history a player recorded follows its tracks, so rekordbox can still import it; plays of tracks that leave the stick are left out, and the plan says so. The colour names, browse columns, category and sort lists, and background colour set in rekordbox's preferences, which the XML does not carry, are kept (in OneLibrary too). And each track keeps its artwork id, so one new track no longer makes the next run rewrite every later picture ([#300](https://github.com/M-Igashi/baken/pull/300), fixes [#286](https://github.com/M-Igashi/baken/issues/286), [#289](https://github.com/M-Igashi/baken/issues/289), and [#290](https://github.com/M-Igashi/baken/issues/290)).
- **A track whose source is offline stays on the stick.** When a source file is not there (a drive not connected, a NAS asleep), the copy an earlier export left on the stick now stands in for it if it has exactly the XML's `Size`, or with `--cdjsafe` if it is a complete 320 kbps MP3. The track keeps its place in its playlists, `--prune` runs as usual, and the plan lists such tracks. Before, the track dropped off the stick ([#302](https://github.com/M-Igashi/baken/pull/302), fixes [#291](https://github.com/M-Igashi/baken/issues/291)).

## Other changes

- `export.pdb` pages carry rekordbox's own record of the last write in each row group, which matters once rekordbox or a player edits rows on the stick. A track whose text (a very long comment, say) does not fit a database page is skipped by name instead of stopping the run with a panic. Every file written through a temporary file and a rename (`export.pdb`, `exportExt.pdb`, OneLibrary, the settings files, and the XML that `rbsort` and `cdjsafe` write) is flushed to the device before the rename ([#298](https://github.com/M-Igashi/baken/pull/298), fixes [#293](https://github.com/M-Igashi/baken/issues/293)).
- On Linux the plan warns about a FAT stick on an MBR partition whose type is not a FAT type, such as type 83 after `mkfs.vfat` on a partition that `fdisk` made. Two XDJ-700s showed FORMAT USB for such a stick; the warning names the fix ([#303](https://github.com/M-Igashi/baken/pull/303), fixes [#288](https://github.com/M-Igashi/baken/issues/288)).
- The CLI warns when rekordbox is running on macOS or Windows, since rekordbox rewrites a device library on any stick it can see ([#303](https://github.com/M-Igashi/baken/pull/303), fixes [#292](https://github.com/M-Igashi/baken/issues/292)).
- The USB export benchmark kit behind the published measurements and its 2026-11-05 results are in `bench/` ([#304](https://github.com/M-Igashi/baken/pull/304)).
- Dependencies: mp3rgain 3.15.0, rusqlite 0.41.1, and aes 0.9.4 ([#305](https://github.com/M-Igashi/baken/pull/305)).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: no API change. `fsname::write_atomic` flushes the temporary file before the rename, and `libc` is a direct dependency on Apple targets (#298).
- `baken-export`, library folder (#306, #307): new `Root` (`layout::Root`: `Pioneer` or `Hidden`), `Plan::root`, `Plan::moves_library`, `pdb::Export::root`, and `AnlzSlots::under`. `anlz_dir`, `artwork::path`, `artwork::files`, and `build::build` take the root; `settings::player_saved` and `settings::copy_all` take the library folder instead of the device. New error variants `Error::TwoLibraries` and `Error::HiddenFolderInTheWay`.
- `baken-export`, what the stick holds (#300): `Plan` gains `preferences`, `preferences_unreadable`, and `artwork_ids`; `Report` gains `history_plays` and `history_dropped`; `pdb::Export` gains `preferences`, `history_lists`, and `history_entries`. New `pdb::Preferences`. `pdb::PlayerHistory` gains `dropped` and `carry_into` and has private fields, so only `PlayerHistory::read` makes one. `pdb::rows::history_property` takes the display bytes, and `pdb::rows::history_entry` is new. `Report::artwork` no longer equals the highest artwork id.
- `baken-export`, offline sources (#302): `PlanTrack::source_offline`, `Plan::offline()`, and `Layout::peek`.
- `baken-export`, partition type (#303): `Plan::partition_type`, `volume::partition_type`, `volume::partition_type_warning`, `volume::FAT_PARTITION_TYPES`, and the variant `FormatWarning::PartitionType` (an exhaustive `match` needs an arm).
- `baken-export`, writer (#298): `pdb::write` and `pdb::write_ext` return `Result<Vec<u8>, pdb::RowTooLarge>`, and `pdb::page::MAX_ROW` is new.
