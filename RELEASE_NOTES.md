# Bake'n Deck 3.1.0 - rbsort in-place sorting and a simpler CLI

## Highlights

- **`rbsort` now sorts playlists in place, with no wrapper folder.** Previously every sorted playlist was written into a new `Sorted (Key+BPM)/` folder, so the output XML held both an unsorted original and a sorted copy of each playlist. That copy was redundant: the file you feed `rbsort` is a throwaway export, and Rekordbox loads the result as the separate `rekordbox xml` tree, so your real library is never at risk. Each playlist is now re-sorted inside its own node instead. Names, folder structure, `Count` and `Entries` attributes, and even whitespace are preserved byte-for-byte, so the `rekordbox xml` tree becomes a Key+BPM-sorted mirror of your `Playlists`, and running `rbsort` twice on the same file is a no-op.
- **The XML path is now a positional argument for both `rbsort` and `cdjsafe`.** `--xml` carried no information when the XML is the one thing every invocation must name, so `baken rbsort --xml collection.xml` is now `baken rbsort collection.xml`, and `baken cdjsafe --xml collection.xml --playlist … --out-dir …` is now `baken cdjsafe collection.xml --playlist … --out-dir …`. `rbsort` also defaults to sorting the input file in place, with `-o` available when you want the export left untouched. Its `--name` flag is gone, since in-place sorting never creates a playlist to name.
- **A repeatable sync loop.** Export your collection to a fixed path, run `rbsort` on it, and point *Preferences > Advanced > Database > rekordbox xml > Imported Library* at that same file once. From then on, re-exporting to the same path, re-running `rbsort`, and restarting Rekordbox keeps the `rekordbox xml` tree in sync with your library, always sorted.

## Breaking Changes

- `baken rbsort --xml <PATH>` → `baken rbsort <PATH>`
- `baken cdjsafe --xml <PATH>` → `baken cdjsafe <PATH>`
- `baken rbsort` writes to its input file by default instead of `<stem>-out.xml`. Pass `-o <PATH>` for the old behavior. `cdjsafe` still defaults to `<stem>-out.xml`.
- `baken rbsort --name <NAME>` has been removed.
- The `Sorted (Key+BPM)/` folder is no longer produced. Sorted playlists appear in the `rekordbox xml` tree under their original names and folders.

## Other Changes

- **Fixed the build against quick-xml 0.42** (#80), which moved element and attribute names from bytes to `&str`. The dependency bump had landed without a compile check, since no CI job builds on pushes to `main`.
- Refreshed the lockfile off a yanked `chacha20` 0.10.1 (a transitive dependency) onto 0.10.2.
- Updated the built-in mp3rgain library from 3.4.0 to 3.6.0. Gain application is unchanged; verified that MP3, AAC/M4A, and FLAC gain still land on target with the MP3 file staying byte-identical in size.

**Full Changelog**: https://github.com/M-Igashi/baken/compare/v3.0.4...v3.1.0
