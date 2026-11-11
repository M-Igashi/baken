# `baken rbsort` reference

rekordbox has no "sort by key, then BPM". `baken rbsort` takes an exported rekordbox XML and rewrites every playlist in it so its tracks run **Alphanumeric key (1A → 12B) ascending**, then **BPM ascending**. Playlists keep their names and folder positions; only the track order inside each one changes. rekordbox reads the sorted file back as its `rekordbox xml` tree, so you end up with a key then BPM sorted mirror of your `Playlists` next to the originals.

It is the same idea as `baken headroom`, applied to playlist order: what rekordbox does only in software (Auto Gain, sorting by a column) does not follow your tracks to the CDJ. `rbsort` writes the order into the playlist itself, so when you export to USB in rekordbox's EXPORT mode, the CDJ plays the set in that exact order.

The guide has the steps with screenshots, a comparison against single-column sort, and the questions people ask: [How to sort a rekordbox playlist by key and BPM](https://baken.ravers.workers.dev/sort) ([日本語](https://baken.ravers.workers.dev/ja/sort)).

## Workflow

1. **Export**: *File > Export Collection in xml format*. Always save to the same path, e.g. `~/Music/rekordbox/collection.xml`. Either key display format works (*Preferences > View > Key display format*, Classic or Alphanumeric); the order is the same.
2. **Run rbsort** on that file. It is sorted in place:
   ```bash
   baken rbsort ~/Music/rekordbox/collection.xml

   # Only one playlist (top-level: just the name; nested: "Folder/Playlist")
   baken rbsort ~/Music/rekordbox/collection.xml --playlist "Sets/Friday"

   # Keep the export untouched and write elsewhere
   baken rbsort ~/Music/rekordbox/collection.xml -o ~/Music/rekordbox/sorted.xml
   ```
3. **One-time setup**: *Preferences > Advanced > Database > rekordbox xml > Imported Library* → select that same file.
4. **Restart rekordbox** (it only re-reads the XML on startup) and open the **`rekordbox xml` tree** in the left sidebar. It is a *separate* tree from your main library: switch to it from the sidebar icon column on the far left. It mirrors your `Playlists` folder structure, every playlist already in Key+BPM order: `1A` (lowest BPM) → `1B` → `2A` → … → `12B` (highest BPM).
5. **Use it**: drag any playlist from the `rekordbox xml` tree into your main `Playlists` (it lands as a new playlist; your original is unchanged), switch to *EXPORT* mode, plug in your USB / SD, then **right-click the playlist → Export Playlist**. CDJs read tracks in playlist order by default, so your key then BPM sort plays back on the deck in that exact order.

**Keeping it in sync**: whenever your playlists change, repeat steps 1, 2 and the restart. The file path never changes, so the Imported Library setting keeps pointing at the freshly sorted export and the `rekordbox xml` tree stays an always-sorted copy of your library.

> The sorted playlists live **only** in the `rekordbox xml` tree, not in your main `Playlists`. If you only see unsorted originals, you're looking at the local library, so switch sidebar trees.

## Usage

```
baken rbsort <XML> [--playlist <PATH>] [-o <PATH>]
```

| Argument / Flag | Description |
|------|-------------|
| `<XML>` | Exported rekordbox XML (required). Sorted in place unless `--output` is given; the file is replaced in one step (temp file + rename), so an interrupted run never leaves a truncated XML |
| `--playlist <PATH>` | Sort only this playlist. Top-level playlists: just the name (e.g. `"Happy House and Trance"`). Nested: `/`-separate folder/playlist names (e.g. `"Folder/SubFolder/MyPlaylist"`). Omitted: every TrackID-referenced playlist is sorted |
| `--output <PATH>` (`-o`) | Write the result here instead of overwriting the input XML |

## Sort rules

- **Primary**: Alphanumeric key ascending: `1A → 1B → 2A → 2B → … → 12A → 12B`
- **Secondary**: BPM ascending within each key group
- Tracks with no key sort **after** all known keys; within a key group, tracks with BPM 0 / unanalyzed sort last

See [rbsort-sort-comparison.md](rbsort-sort-comparison.md) for a 6-track walk-through showing how this compound sort differs from rekordbox / CDJ's single-column *Sort by Key* and *Sort by BPM*. The same walk-through, with the workflow around it, is at [baken.ravers.workers.dev/sort](https://baken.ravers.workers.dev/sort).

## Notes

- The `Tonality` field may be in either of rekordbox's key display formats: Alphanumeric (`8A`) or Classic (`Am`, `F#m`, `Db`). A Classic key sorts at its Alphanumeric position (`Am` with `8A`, `C` with `8B`). Anything else is sorted last.
- Only `KeyType="0"` (TrackID-referenced) playlists are sorted. In all-playlists mode, other playlists pass through unchanged; for a single target, `rbsort` errors out.
- Only the order of `<TRACK Key="…"/>` references changes. Playlist names, folder structure, `Count`/`Entries` attributes, whitespace and everything else in the XML are preserved byte-for-byte, so running `rbsort` twice on the same file is a no-op.
- `baken rbsort` does **not** require ffmpeg; only the `headroom` and `cdjsafe` subcommands do.
