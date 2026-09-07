# Bake'n Deck 3.2.2 - cdjsafe keeps going when files are missing

## Highlights

- **`cdjsafe` no longer aborts over a missing file.** A single stale playlist entry used to stop the whole emergency-stick run with `Source file not found`. `plan()` now partitions the playlist: every track whose file exists is converted, missing ones are listed as skipped (name, TrackID, location) and left out of both the MP3 folder and the new playlist. Only a playlist with no file present at all fails, via the new `Error::AllSourcesMissing`. The CLI prints each skipped track before converting and again in the final report. Library users get `Plan::skipped()` and `Report::skipped` (#101).
- **The generated playlist is now named `<playlist>-CDJ-safe`.** Importing the CDJ-safe playlist into rekordbox under the same name as the original made the two easy to confuse or overwrite. The folder in the XML is still `CDJ-safe (MP3)`; the playlist inside it carries the suffix. `Plan::output_playlist_name()` exposes the name and `Report::playlist_name` carries it (#101).

## Other Changes

- `Error::SourceNotFound` is kept for API compatibility but is no longer produced by `cdjsafe::plan()`.
- README: the cdjsafe section documents the skip behaviour and the `-CDJ-safe` playlist name.
