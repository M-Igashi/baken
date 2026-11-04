# Bake'n Deck 4.9.0 - My Tags on the stick, library drives on Windows

A minor release: `baken-export` gains the My Tags option, which changes its API. `baken-core` is unchanged.

## Highlights

- **My Tags on the stick (`expressport --my-tags`).** rekordbox's XML carries no My Tags, so up to 4.8.0 a stick from `expressport` had none: no `exportExt.pdb`, and OneLibrary's four My Tag columns stayed empty. `--my-tags <FILE>` reads them from a small JSON file next to the XML (the columns with their tags, and each track's tags keyed by the XML's `TrackID`) and writes `exportExt.pdb` the way rekordbox does, and with `--onelibrary` OneLibrary's My Tag tables as well. The file is checked before anything is written, and the plan shows how many of the exported tracks carry tags. The format is in the README under "My Tags (`--my-tags`)". The tag pages come out byte for byte as rekordbox writes them, and rekordbox's own My Tags from a real stick come back on the same tracks; it has not been tried on a player yet. The CDJ-3000 uses My Tags in its Track Filter; reports are welcome in [#281](https://github.com/M-Igashi/baken/issues/281). Asked for and co-authored by @plep7 ([#282](https://github.com/M-Igashi/baken/pull/282)). Without the option nothing changes.
- **A rekordbox library on a drive of its own is found on Windows.** `expressport` looks for `PIONEER\Master\share\PIONEER\USBANLZ` at the root of every local drive letter, fixed or removable, instead of only in `%APPDATA%`, so such a library no longer needs `--anlz-dir` on every run. Network and optical drives are not searched, and an empty card reader no longer opens Windows' "no disk" box ([#279](https://github.com/M-Igashi/baken/pull/279), thanks to the report in [#276](https://github.com/M-Igashi/baken/pull/276)). Not tried on a real Windows machine yet. macOS and Linux search the same places as before.

## Other changes

- The `--onelibrary` line of the plan no longer says "not yet tested on those players"; an XDJ-AZ read such a stick in [#139](https://github.com/M-Igashi/baken/issues/139).
- The README describes Bake'n Deck for Mac 1.4.0 ([#280](https://github.com/M-Igashi/baken/pull/280)).
- Dependencies: rusqlite 0.41.0 with SQLite 3.54.1 for `--onelibrary` ([#278](https://github.com/M-Igashi/baken/pull/278)); the release workflow uses crates-io-auth-action 1.0.6 ([#277](https://github.com/M-Igashi/baken/pull/277)).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: no API change.
- `baken-export`:
  - `Options` gains `my_tags: Option<PathBuf>`, so a struct literal without `..Default::default()` needs it (`None` keeps the old behaviour).
  - `pdb::Export` gains `my_tags` and `my_tag_tracks`, with the same consequence for struct literals.
  - New: the `mytags` module (`MyTags::load`, `parse`, `apply`, `tagged`, `tag_count`, `columns`), `Plan::my_tags`, `Report::my_tags_written` and `Report::my_tags_error`, `pdb::write_ext`, `pdb::MyTag`, and `EXT_FILE`.
  - No new `Error` variant: a bad My Tags file is `Error::Other`, with a one-line message that names the file.
  - On Windows, `anlz::locate::default_roots` also lists the library path on every fixed or removable drive letter.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
