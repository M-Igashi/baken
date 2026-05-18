# headroom 2.0.0 — `rbsort` ships

This is the first major-version bump since the project began. **headroom** is no longer just a loudness analyzer: it now has a CLI subcommand surface, and the inaugural subcommand `rbsort` ships in this release.

All v1.x loudness behaviour is **100% backward-compatible** — `headroom <paths>` and bare-invocation interactive mode are unchanged.

---

## Headliner: `rbsort` — Rekordbox playlist sorter for harmonic mixing

A pure XML-in / XML-out workflow. Takes a Rekordbox **collection.xml** export and produces a key/tempo-sorted copy ready to re-import. No audio is touched, no database is written, **ffmpeg is not required**.

### What it does

Sorts a target playlist — **or every TrackID-referenced playlist in the XML** — by:

1. **Camelot Key** ascending (`1A` → `1B` → `2A` → … → `12B`)
2. **BPM** ascending within each key group

Sorted copies are written into a brand-new `Sorted (Key+BPM)/` folder appended to the ROOT node, each playlist keeping its source name. Mirrors the analyzer's existing `backup/` directory pattern.

Tracks with no Camelot key sort after all known keys; within a key group, tracks with no/0 BPM sort last.

### Usage

```bash
# Sort *every* TrackID-referenced playlist in one pass (recommended)
headroom rbsort --xml /path/to/collection.xml

# Sort a single playlist with a custom name
headroom rbsort \
  --xml /path/to/collection.xml \
  --playlist "Folder/MyPlaylist" \
  --output sorted.xml \
  --name "MyPlaylist (Camelot+BPM)"
```

Defaults:

| Flag         | Behaviour when omitted                                                  |
|--------------|-------------------------------------------------------------------------|
| `--output`   | `<input-stem>-out.<ext>` next to the input (e.g. `coll.xml` → `coll-out.xml`) |
| `--playlist` | Sort **every** TrackID-referenced (`KeyType="0"`) playlist in the XML   |

### Rekordbox workflow

1. **Preferences > View > Key display format → Alphanumeric** so `Tonality` exports as Camelot (`1A`..`12B`). Non-Camelot tonalities silently sort last.
2. **File > Export Collection in xml format** → `collection.xml`.
3. Run `headroom rbsort --xml collection.xml`.
4. **Preferences > Advanced > Database > rekordbox xml → Imported Library** points at the output XML; restart Rekordbox.
5. Open the **rekordbox xml** tree in the left sidebar; drag the sorted playlist into your real Playlists tree, or right-click → **Export Playlist** for CDJ EXPORT mode.

### Implementation highlights

- New module `src/rbsort/` (`mod`, `camelot`, `xml`) on top of `quick-xml` 0.40.
- Two-pass design: scan `COLLECTION` + target playlists, then stream-rewrite with the sorted folder injected inside `<PLAYLISTS>` before the ROOT `</NODE>` closes. ROOT `Count` is bumped by 1.
- Robust against real Rekordbox exports — `<TRACK>` elements with nested `<TEMPO>` / `<POSITION_MARK>` children are handled correctly (the original `Event::Empty`-only path produced an empty collection map and a no-op sort).
- Subcommand dispatch short-circuits **before** the audio pipeline, the banner, ffmpeg check, and the update check — `rbsort` runs in milliseconds and has zero audio-stack dependencies.
- Verified end-to-end on a real 5,255-track / 24-playlist Rekordbox export — sorted in under a second with zero ordering violations.

---

## Why a major version?

Until now, `headroom` only did one thing: audio loudness analysis and gain adjustment. v2.0.0 introduces a **subcommand surface** to the CLI, and `rbsort` is the first to land there. Future non-audio DJ-prep workflows (set planning, transition analysis, etc.) will be added as subcommands too.

Existing flows are untouched:

- `headroom`                   → interactive loudness analyzer (unchanged)
- `headroom <paths> [flags]`   → scriptable loudness analyzer (unchanged)
- `headroom rbsort ...`        → **new** Rekordbox playlist sorter

---

## Internal

- XML scanner/rewriter switched to byte-level element-name comparison and single-pass attribute extraction — fewer allocations on large exports.
- `Option<T>` "None-last" ordering extracted into a reusable helper.
- `processor.rs` internal helpers tightened from `pub` to private; dead `AnalysisSummary::total()` removed.
- `default_output_path` simplified to use `Path::with_file_name`.
- 19 unit tests, including full XML round-trip with multi-playlist nested folders.

## Migration

**None.** The audio side is wire-compatible with v1.x. `rbsort` is purely additive.
