# Bake'n Deck 3.5.0 - `baken expressport` (beta): write the USB stick yourself

## Highlights

- **New subcommand `baken expressport` writes a rekordbox USB export straight from `collection.xml`.** The device library (`PIONEER/rekordbox/export.pdb`), the analysis files (`PIONEER/USBANLZ`), the audio under `Contents/` and your CDJ/DJM My Settings all land on the stick from one command, without launching rekordbox, re-importing, or waiting for analysis. rekordbox's own database is never read: everything a player needs is in the XML, and the waveforms are copied from the analysis cache rekordbox keeps locally, rewritten the way rekordbox rewrites them at export time (path, cue sections filled from `POSITION_MARK`, phrase analysis masked). Tracks that rekordbox never analysed are listed and left out ([#115](https://github.com/M-Igashi/baken/issues/115)).
- **This is a beta, and it has not met a player yet.** The writer is checked byte for byte against real rekordbox 7 exports: the keys, colours and columns pages of `export.pdb` are identical, and for 325 tracks of a real library the regenerated analysis files match the copies rekordbox put on the stick (DAT 304, EXT 294, 2EX 309; the rest were edited after that export). Whether a CDJ accepts the result can only be answered by a CDJ, and none was at hand for this release. If you own a CDJ-3000, CDJ-2000NXS2, XDJ-XZ or anything else that reads a rekordbox stick, [#116](https://github.com/M-Igashi/baken/issues/116) is the tester call. **Use a spare stick.**
- **`--cdjsafe` on the same command** transcodes every track to 320 kbps CBR MP3 on the way and ships it with the source track's analysis, so pre-NXS2 players get grid, cues and waveform without the XML round trip that `baken cdjsafe` needs.
- **My Settings are part of the export and required.** The four `*SETTING.DAT` files are copied verbatim from rekordbox's settings directory after their checksums are verified; a stick that resets the player to factory settings is not treated as an export. `--settings-dir` overrides the location.

## What it writes and what it does not

- Legacy device library only: `export.pdb` as read by CDJ-3000, CDJ-2000NXS2, XDJ-XZ and older players. The OneLibrary database (`exportLibrary.db`) needed by CDJ-3000X, XDJ-AZ, OPUS-QUAD and OMNIS-DUO is not written yet.
- Playlists come from the XML: `--playlist "Folder/Name"` repeatable, or every TrackID playlist when omitted. Folders above the selected playlists are created.
- Idempotent: audio is copied only when missing or of a different size, `export.pdb` is rebuilt every run, `--prune` removes what the export no longer references, `--dry-run` shows the plan. Files players leave on a stick (`PIONEER/CDJ`, `RBFLTR.DAT`, `export.pdb.bak`) are never touched.
- Artwork is not exported yet. Colour names are rekordbox's defaults (the XML carries only RGB).

## Library users

- New crate `baken-export` (MIT) with the `export.pdb` writer, the analysis-file pipeline, the `collection.xml` model and a two-phase `plan` / `export` API mirroring `cdjsafe`. `baken-core` is unchanged apart from re-exporting `cdjsafe::{decode_location, encode_location, sanitize_filename, probe, transcode, SourceInfo}` for it.
- The CLI feature `expressport` is on by default; `cargo install baken --no-default-features` builds the 3.4.0 command set.

## Upgrading

`headroom`, `rbsort` and `cdjsafe` are unchanged. Files processed by an earlier version are unaffected.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
