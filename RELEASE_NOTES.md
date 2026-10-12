# Bake'n Deck 4.6.0 - `cdjsafe` takes a whole XML, binaries back to their old size

A minor release. `cdjsafe` converts or checks every playlist when `--playlist` is omitted, and the binaries lose the OpenSSL that `--onelibrary` brought in with 4.5.0, which roughly halves the Linux downloads. It is a minor rather than a patch because `baken-core` changed its public API (see "Library users").

## Highlights

- **`cdjsafe` on a whole XML.** `--playlist` is now optional and repeatable. Without it, `baken cdjsafe` converts or checks every playlist in the XML, like `expressport` and `rbsort`; playlists an earlier run wrote (the `CDJ-safe (MP3)` folder and names ending in `-CDJ-safe`) are left out unless you name them. A track in several playlists is converted and checked once. The output XML mirrors each playlist's folders under `CDJ-safe (MP3)`, so `Sets/Friday` becomes `CDJ-safe (MP3)/Sets/Friday-CDJ-safe`, and a playlist none of whose tracks could be reached is not written. `--check` on more than one playlist prints no per-track table, only the tracks that need a look (each with its format) and the summary; a single playlist keeps the table ([#255](https://github.com/M-Igashi/baken/issues/255), [#260](https://github.com/M-Igashi/baken/pull/260)). Thanks to @sairutra for the request.
- **Smaller binaries.** 4.5.0 doubled the Linux binaries (x86_64 7.1 to 15.1 MB), almost all of it OpenSSL, vendored for SQLCipher, which `--onelibrary` uses to encrypt `exportLibrary.db`. The database is now written with plain SQLite and encrypted by baken itself, page by page with SQLCipher 4's scheme, and the release binaries are fully stripped: Linux x86_64 8.6 MB, Linux aarch64 7.7 MB, macOS arm64 7.4 MB (4.5.0: 15.1, 14.6 and 8.7 MB). The file format is unchanged: SQLCipher 4 itself opens a database baken wrote, and rekordbox's own `exportLibrary.db` decrypts and passes the integrity checks with the new code ([#256](https://github.com/M-Igashi/baken/issues/256), [#259](https://github.com/M-Igashi/baken/pull/259)). Thanks to @sairutra for the report.

## Behaviour change

- A single nested playlist now lands one folder deeper in `cdjsafe`'s output XML: `CDJ-safe (MP3)/Sets/Friday-CDJ-safe` instead of `CDJ-safe (MP3)/Friday-CDJ-safe`. Top-level playlists are unchanged ([#260](https://github.com/M-Igashi/baken/pull/260)).

## Fixes

- **`cdjsafe` and a playlist that lists the same track twice.** It failed with `Playlist references TrackID 1 not present in <COLLECTION>`. The plan now holds each track once, and every playlist keeps its own order, duplicates included ([#260](https://github.com/M-Igashi/baken/pull/260)).

## Other changes

- The README tells `headroom` users to export processed tracks to a stick again even when they are already on it: rekordbox copies a track's file only the first time it exports that track, so the stick keeps the old audio. `baken expressport` copies a changed file again on its own ([#257](https://github.com/M-Igashi/baken/issues/257)). Thanks to @ristoryti for the question.
- `THIRD_PARTY_LICENSES.md` no longer carries SQLCipher and OpenSSL ([#259](https://github.com/M-Igashi/baken/pull/259)).
- Dependencies: libc 0.2.190, miniz_oxide 0.9.1 ([#253](https://github.com/M-Igashi/baken/pull/253), [#254](https://github.com/M-Igashi/baken/pull/254)). CI: dtolnay/rust-toolchain ([#252](https://github.com/M-Igashi/baken/pull/252)).

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`
  - `cdjsafe`: new `plan_playlists(xml, &[String])` (an empty slice means every playlist), `Plan::playlists()` and `Report::playlists`. `Plan::playlist_name()` and `Plan::output_playlist_name()` are removed; use `Plan::playlists()` and `Report::playlists` instead. `cdjsafe::plan(xml, &str)` and `Report::playlist_name` (the first playlist when there are several) stay ([#260](https://github.com/M-Igashi/baken/pull/260)).
  - `Error` has a new variant `NoSourcesFound { playlists, count }` (several playlists, none of whose tracks is on disk; a single playlist still gives `AllSourcesMissing`), so exhaustive matches break. `NoSortablePlaylists` now reads "No TrackID-referenced playlists with tracks found" ([#260](https://github.com/M-Igashi/baken/pull/260)).
- `baken-export`
  - No API change. `rusqlite` is built with `bundled` instead of `bundled-sqlcipher` (Apple) or `bundled-sqlcipher-vendored-openssl` (elsewhere), so OneLibrary no longer links CommonCrypto, Security.framework or OpenSSL; SQLite (public domain) stays. New dependencies: `aes`, `cbc`, `hmac`, `sha2`, `pbkdf2` and `getrandom`, all MIT OR Apache-2.0 ([#259](https://github.com/M-Igashi/baken/pull/259)).

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
