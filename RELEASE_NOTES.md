# Bake'n Deck 4.14.0 - Browse categories and sort keys from a file, and a faster re-export with artwork

A minor release for `baken expressport`: one new option, and a re-export with `--artwork` that no longer redoes every picture. The `baken-export` API gains a field (see Library users); `baken-core` has no API change.

## Highlights

- **`--browse <FILE>` sets the categories a player lists and the keys it sorts by.** Which categories show in a player's browse menu (ARTIST, ALBUM, KEY, ...) and which sort keys it offers are set per stick in rekordbox's device preferences, and they live in `export.pdb`, not in the XML. Since 4.10.0 a stick keeps the lists a rekordbox export put on it, but without rekordbox the only way to get them was someone else's `export.pdb`. Now a small JSON file does it: `{"categories": ["PLAYLIST", "ARTIST", "KEY", "BPM", "FOLDER", "SEARCH"], "sort": ["DEFAULT", "BPM", "KEY", "DATE ADDED"]}`. A listed item is shown, in the file's order, and every other item is hidden; a key left out keeps the stick's list. The lists also go into OneLibrary with `--onelibrary`, and later exports without the option keep them. An unknown name, a name listed twice, or an empty list stops the plan before anything is written. rekordbox's default lists written as a file give rekordbox's default tables byte for byte; the option has not been seen on a player yet. The names and the details are in [docs/expressport.md](https://github.com/M-Igashi/baken/blob/main/docs/expressport.md#categories-and-sort-keys---browse). Thanks to @sairutra for asking ([#368](https://github.com/M-Igashi/baken/pull/368), closes [#311](https://github.com/M-Igashi/baken/issues/311)).
- **A re-export with `--artwork` leaves the pictures of unchanged tracks alone.** Every embedded picture was decoded and scaled again on each run, and only the writes were saved. A track's four thumbnail files now stay as they are, without the picture being read, when the stick's `export.pdb` names an image for the track, the four files are on the stick, and the audio there still matches its source. On a FAT32 stick with 3,206 tracks, 1,724 of them with artwork, the write step of a re-export that copies no audio went from 186 s to 41 s. A picture replaced inside an audio file changes the file, so the track is copied and its artwork made anew. Thanks to @freerider197 for the report and the measurement on a real stick ([#369](https://github.com/M-Igashi/baken/pull/369), closes [#367](https://github.com/M-Igashi/baken/issues/367)).

## Other changes

- Thumbnails another tool put on the stick under a track's artwork id (rekordbox's own, for example) are no longer overwritten while that track's audio is unchanged. They are made anew when the audio is copied again ([#369](https://github.com/M-Igashi/baken/pull/369)).
- The plan prints the lists a `--browse` file sets, and no longer says "kept from the stick's export.pdb" for a list the file replaced ([#368](https://github.com/M-Igashi/baken/pull/368)).
- CI: the release workflow uses softprops/action-gh-release 3.0.4 ([#366](https://github.com/M-Igashi/baken/pull/366)).

## Upgrading from 4.11.0 or earlier

4.12.0 made `baken headroom` and `baken expressport` exit with 1 when a file or track failed, removes ReplayGain tags from files whose gain changed, and never replaces a backup. If you skipped it, read "Upgrading from 4.11.0" in the [4.12.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.12.0).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: no change.
- `baken-export`:
  - `Options` gains `browse: Option<PathBuf>` and `Plan` gains `browse: Option<browse::Browse>`. Code that builds `Options` with a struct literal needs the new field, or `..Default::default()`.
  - New module `browse`: `Browse::load` and `Browse::parse` check a file, `Browse::apply` puts its lists into a `pdb::Preferences`, and `browse::categories()` and `browse::sorts()` return every name a file may use. `plan()` applies the file to `Plan::preferences` after the stick's own lists are read.
  - New constants `pdb::fixed::CATEGORY_LIST` and `pdb::fixed::SORT_LIST`, the names `Preferences::changed` uses for the two lists.
  - `Report::artwork_unchanged` also counts the thumbnail files left alone beside unchanged audio (four per track). No type changed for this.
