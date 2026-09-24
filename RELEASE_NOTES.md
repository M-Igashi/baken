# Bake'n Deck 4.0.1 - `cdjsafe` and `rbsort` agree with the Mac app on odd XML, and never leave a truncated collection behind

A patch release for the three findings of the 2026-09-23 Mac app review ([#152](https://github.com/M-Igashi/baken/issues/152), fixed in [#155](https://github.com/M-Igashi/baken/pull/155)). The app's pre-flight mirrors core's XML parsing but skipped the rows core rejected, so the two reached different conclusions on the same file. Nothing else changed since 4.0.0: `headroom` and `expressport` are untouched, and `expressport` stays a beta ([#116](https://github.com/M-Igashi/baken/issues/116)).

## Highlights

- **A `Location` that cannot be decoded is skipped like a missing file** (`baken cdjsafe`). A row whose `Location` is not a `file://` URL or has a bad percent-escape, typically a hand-edited one, used to fail the whole plan with `Unsupported Location URL`, while a file that decoded but was not on disk was skipped and counted. Both mean the source cannot be reached, and both are now skipped and listed with the reason, in playlist order, so one bad row no longer stops a 300-track conversion. Only a playlist with nothing reachable at all is an error, as before.
- **An empty playlist is found, not "not found".** rekordbox writes a playlist with no tracks as a self-closing `<NODE .../>` (verified in a rekordbox 7 export), and `cdjsafe` never matched that form, so a name the app had just listed came back as `Playlist not found`. It is now found with no tracks and reported as `Playlist 'X' has no tracks`. `rbsort` already handled the case: selecting such a playlist is a no-op, and a test now locks that in.
- **The collection XML is written atomically.** `rbsort` (which sorts in place by default) and `cdjsafe` wrote the rewritten collection straight over the target path, so a crash or a full disk mid-write left a truncated `collection.xml`. Both now write a temp file in the same folder and rename it over the target, the way `baken headroom` has always written audio. The cost was only a re-export, since the rekordbox database is never touched, but the export is the file the whole workflow runs on.

## Other changes

- `cdjsafe` playlist lookup failures are the typed errors `rbsort` already used (`PlaylistNotFound`, `UnsupportedPlaylistType`), so a front-end can match them instead of parsing a message. The messages are unchanged.

## Library users

- `baken-core`: `cdjsafe::SkippedTrack` gains `reason: cdjsafe::SkipReason`, which is `NotFound` or `BadLocation(message)`; in the latter case `location` holds the raw attribute value. New public items only, nothing is removed or renamed: a caller that reads the fields keeps compiling, one that builds the struct itself has one field to add. `baken-export`: no change.
- The Mac app's FileWatcher already re-arms after the rename that replaces the XML, so nothing changes there when it moves to 4.x.

## Upgrading

No action needed. Results are identical for any collection the previous version accepted. The difference is what happens on a hand-edited row or an empty playlist, and what a crash mid-write leaves behind.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
