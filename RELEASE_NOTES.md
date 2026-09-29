# Bake'n Deck 4.3.0 - `expressport` leaves beta, and gets out of the stick's way

`baken expressport` has been a beta since 3.5.0. Testers have now played sticks written by `baken` on a CDJ-3000 (firmware 3.22) and on CDJ-2000NXS2 players (1.85 and 1.87): playlists, beat grid, waveforms, cues, keys, search and My Settings, with copied and with generated analysis, up to a 1121-track library ([#116](https://github.com/M-Igashi/baken/issues/116)). The beta banner is gone ([#202](https://github.com/M-Igashi/baken/pull/202)). Players that need OneLibrary (CDJ-3000X, XDJ-AZ, OPUS-QUAD, OMNIS-DUO) are still not supported ([#139](https://github.com/M-Igashi/baken/issues/139)). `headroom`, `rbsort` and `cdjsafe` are unchanged.

## Faster exports

An export runs at the stick's write speed plus about 0.4 s per track for the directory and the three analysis files the format wants next to each audio file; the measurements are in [#196](https://github.com/M-Igashi/baken/issues/196), and the README now says what to expect ([#198](https://github.com/M-Igashi/baken/issues/198), [#199](https://github.com/M-Igashi/baken/pull/199)). Two things were in the way and are gone:

- **Audio is copied as bytes only** ([#196](https://github.com/M-Igashi/baken/issues/196), [#200](https://github.com/M-Igashi/baken/pull/200)). `std::fs::copy` on macOS also copies extended attributes, ACLs, mode and times, so a downloaded track's quarantine attribute or a Finder tag became a `._` file on the stick. A reader thread now keeps a few MB ahead of the writes, so a library on a NAS or an HDD no longer adds its read time to the stick's write time.
- **`--cdjsafe` encodes ahead of the stick** ([#197](https://github.com/M-Igashi/baken/issues/197), [#203](https://github.com/M-Igashi/baken/pull/203)). Up to 4.2.1 every ffmpeg run happened on the thread that writes the stick, one track after the other, and the finished MP3 was read back from the stick to count its frames. The two preparation workers now probe and encode into a temp file on the local disk while the stick is busy with the previous track, and the writer copies the result like any other file. Six mixed-format tracks: 24 s to 15 s on an SSD, and the stick sees one plain copy per track instead of ffmpeg's 32 KB writes and a seek-back.

## Other changes

- The README's expressport section says how long an export takes, with the figures measured on a FAT32 stick, and points Linux users at the `flush` mount option udisks2 adds by default, which sends every closed file to the device at once ([#199](https://github.com/M-Igashi/baken/pull/199)).
- The bug report form no longer calls `expressport` a beta ([#202](https://github.com/M-Igashi/baken/pull/202)).

## Library users

- `baken-export`: no API changes. `export` copies audio through its own routine instead of `std::fs::copy`, and with `cdjsafe` it writes temp files named `baken-expressport-<pid>-<n>.mp3` in the system temp directory, removed as soon as each is on the stick.
- `baken-core`: no changes.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
