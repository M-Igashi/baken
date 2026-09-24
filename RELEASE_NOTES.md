# Bake'n Deck 4.0.2 - files you can delete again on macOS 26, and a first CDJ reading an `expressport` stick

A patch release built from the first hardware reports in [#116](https://github.com/M-Igashi/baken/issues/116) and the macOS 26 file-name issue [#154](https://github.com/M-Igashi/baken/issues/154). A CDJ-2000NXS2 has now read a stick written by `baken expressport`: it found My Settings, listed the playlist in order and loaded tracks with their waveforms. `expressport` stays a beta until the remaining #116 tests are done.

## Highlights

- **Files baken writes on macOS 26 can be deleted again** ([#154](https://github.com/M-Igashi/baken/issues/154), [#159](https://github.com/M-Igashi/baken/pull/159)). macOS 26 mounts ExFAT/FAT sticks through FSKit, which stores a name in the Unicode form it was created with, lists every name decomposed (NFD), and only deletes a file by its stored form. In 4.0.1 this meant that headroom backups and cdjsafe outputs could not be removed with `rm -rf`, that a track or `collection.xml` rewritten by headroom, rbsort or cdjsafe could no longer be removed by its listed name, and that `baken expressport` ended **every** export with `No such file or directory (os error 2)` as soon as an artist, album or file name was non-ASCII (the AppleDouble cleanup and `--prune` both tripped over it). Files baken creates next to your own now use the macOS form, and everything baken removes on a stick is removed by its stored name. The stick itself stays NFC, because that is how rekordbox writes `export.pdb` and how the player looks files up.
- **`DEVSETTING.DAT` is optional** (`baken expressport`, [#158](https://github.com/M-Igashi/baken/pull/158)). Some rekordbox 7 installs never create it, the player writes its own, and a CDJ-2000NXS2 read a stick without it. It is copied when present and skipped when not; `MYSETTING.DAT`, `MYSETTING2.DAT` and `DJMMYSETTING.DAT` stay required. The settings files are now validated before anything is written, so a bad one no longer stops the run after the audio and `export.pdb` are already on the stick, and `--dry-run` reports it. The messages point to Preferences > DJ System > My Settings in rekordbox 7's **EXPORT mode**, the only mode that shows that page.
- **Re-running an export after a playlist change is nearly free** (`baken expressport`, [#161](https://github.com/M-Igashi/baken/pull/161)). The local analysis cache is indexed in parallel, analysis files the stick already holds byte for byte are not written again, and the AppleDouble cleanup only walks the folders a run wrote into. On a 326-track, 18.4 GB test export, an unchanged re-run went from 5.8 s to 1.9 s (cold) and from 1.8 s to 0.6 s (warm); the first export from 34.2 s to 30.7 s.

## Other changes

- `baken expressport --no-settings` writes no My Settings, so the player keeps its own. It conflicts with `--settings-dir`.
- `baken expressport` picks the file type from the file extension. rekordbox's `Kind` is only used to tell ALAC from AAC in an `.m4a` and for unknown extensions. A FLAC that a third-party converter labelled `Kind="MP3 File"` hung the player on NOW LOADING; rekordbox's own exports always agree with the extension, so they are unaffected.
- `baken expressport --generate-analysis` warns about tracks whose XML has no beat grid (`TEMPO`), e.g. from mixxx2rekordbox: the player shows their BPM only after detecting it while playing, and quantize and beat sync cannot use them ([#162](https://github.com/M-Igashi/baken/pull/162)).
- Where there is no rekordbox analysis directory (Linux), the plan says so instead of printing an empty path.
- The AppleDouble cleanup also removes `._Contents` and `._PIONEER` at the stick root.
- README: `EXPORT mode` for My Settings, what a missing `TEMPO` means, and a note that Terminal's `rm` cannot remove the stick's NFC names on macOS 26 while Finder and `--prune` can.

## Library users

- `baken-core`: new module `fsname` (`native`, `nfc`, `replace`, `remove_file`, `remove_dir`) and a new dependency, `unicode-normalization`. Nothing is removed or renamed.
- `baken-export`: `settings::FILES` is split into `REQUIRED` and `OPTIONAL`, `settings::files()` is new and `copy_all` takes the file list; `Options` gains `no_settings`; `Plan::settings_dir` becomes an `Option` and `Plan` gains `settings_files` and `without_grid()`; `Report` gains `anlz_unchanged`.

## Upgrading

No action needed. Files that 4.0.x already renamed to NFC on an FSKit disk stay as they are; Finder removes them.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
