# Bake'n Deck (`baken`)

[![crates.io](https://img.shields.io/crates/v/baken)](https://crates.io/crates/baken)
[![Downloads](https://img.shields.io/github/downloads/M-Igashi/baken/total)](https://github.com/M-Igashi/baken/releases)
[![License: MIT](https://img.shields.io/github/license/M-Igashi/baken)](LICENSE)
![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)

**What you prep in Rekordbox is what plays on the deck.**

Rekordbox does three things in software that never survive the trip to a CDJ. Bake'n Deck bakes each one into the files themselves:

| Subcommand | The gap it fills | What it does |
|---|---|---|
| [`baken headroom`](#loudness-normalizer-baken-headroom) | Auto Gain is ignored on USB export | Measures LUFS / True Peak and bakes safe gain into the audio file — **no limiter**, dynamics preserved, cues stay linked |
| [`baken rbsort`](#rekordbox-playlist-sorter-baken-rbsort) | No compound Key+BPM sort in Rekordbox | Sorts every playlist by **Camelot Key (1A→12B) then BPM** inside your exported XML — CDJs play it in that exact order |
| [`baken cdjsafe`](#cdj-safe-transcoder-baken-cdjsafe) | Pre-NXS2 CDJs only play MP3 reliably | Transcodes a whole playlist to **320 kbps CBR MP3** with **cues and beatgrid carried over** — the emergency-backup USB |

🌐 **[baken.ravers.workers.dev](https://baken.ravers.workers.dev)** — full docs, workflow guides, and FAQ.

## Installation

baken requires ffmpeg. Package managers install it automatically.

| Platform | Command |
|----------|---------|
| **macOS (Homebrew)** | `brew install M-Igashi/tap/baken` |
| **Windows (winget)** | `winget install M-Igashi.baken` |
| **Arch Linux (AUR)** | `yay -S baken-bin` |
| **Cargo** | `cargo install baken` (ffmpeg must be installed separately) |

Pre-built binaries are available on the [Releases](https://github.com/M-Igashi/baken/releases) page (ffmpeg must be installed separately). To build from source: `git clone https://github.com/M-Igashi/baken.git && cd baken && cargo build --release`.

The processing logic is a separate library crate, [`baken-core`](https://crates.io/crates/baken-core), so other front-ends can embed it without the terminal UI.

## Quick Start

```bash
baken headroom ~/Music/DJ-Tracks                # analyze & bake loudness gain (interactive)
baken rbsort collection.xml                     # sort every playlist by Key+BPM, in place
baken cdjsafe collection.xml --playlist "Sets/Friday" --out-dir ~/Music/cdjsafe
```

Run `baken --help` or `baken <subcommand> --help` for the full reference.

> [!NOTE]
> **Renamed from `headroom` at v3.0.0** ([#60](https://github.com/M-Igashi/baken/issues/60)). The old `headroom` install channels (brew/winget/cargo/AUR) no longer receive updates — reinstall via the `baken` packages above. The loudness analyzer now lives under the `baken headroom` subcommand.

## Highlights

- **Single binary** — [mp3rgain](https://github.com/M-Igashi/mp3rgain) built in; only ffmpeg required (and `rbsort` doesn't even need that)
- **Truly lossless MP3/AAC gain** — global_gain header modification in 1.5 dB steps, no re-encode
- **Uniform True Peak ceiling** — -0.5 dBTP by default (AES TD1008 §7B), tunable via `--tp-target`
- **Non-destructive** — automatic backups; `rbsort`/`cdjsafe` only ever touch an exported XML, never your Rekordbox library
- **Metadata preserved** — files overwritten in place, so Rekordbox cues, hot cues, and beatgrids stay linked
- **Interactive or scriptable** — guided two-stage confirmation, or flags/globs for pipelines and CI

## Loudness Normalizer (`baken headroom`)

### How It Works

1. Scans the target directory for audio files (FLAC, AIFF, WAV, MP3, AAC/M4A)
2. Measures LUFS (Integrated Loudness) and True Peak using ffmpeg
3. Categorizes files by processing method:
   - **Green**: Lossless files (ffmpeg)
   - **Yellow**: MP3/AAC files with enough headroom for native lossless gain
   - **Magenta**: MP3/AAC files requiring re-encode
4. Displays categorized report
5. Two-stage confirmation:
   - First: "Apply lossless gain adjustment?" (lossless + native MP3/AAC)
   - Second: "Also process files with re-encoding?" (MP3/AAC requiring re-encode)
6. Creates backups and processes files

#### Example

<details>
<summary>Full interactive session (28 files analyzed → 10 processed)</summary>

```
$ cd ~/Music/DJ-Tracks
$ baken headroom

╭─────────────────────────────────────╮
│            baken v3.0.1             │
│   Bake'n Deck — CDJ Prep Toolkit    │
╰─────────────────────────────────────╯

▸ Target directory: /Users/xxx/Music/DJ-Tracks

✓ Found 28 audio files
✓ Analyzed 28 files

● 3 lossless files (ffmpeg, precise gain)
  Filename        LUFS    True Peak    Target        Gain
  track01.flac   -13.3    -3.2 dBTP   -0.5 dBTP   +2.7 dB
  track02.aif    -14.1    -4.5 dBTP   -0.5 dBTP   +4.0 dB
  track03.wav    -12.5    -2.8 dBTP   -0.5 dBTP   +2.3 dB

● 2 MP3 files (native lossless, 1.5 dB steps, requires TP ≤ -2.0 dBTP)
  Filename        LUFS    True Peak    Target        Gain
  track04.mp3    -14.0    -5.5 dBTP   -0.5 dBTP   +4.5 dB
  track05.mp3    -13.5    -6.0 dBTP   -0.5 dBTP   +4.5 dB

● 2 AAC/M4A files (native lossless, 1.5 dB steps, requires TP ≤ -2.0 dBTP)
  Filename        LUFS    True Peak    Target        Gain
  track08.m4a    -13.0    -4.0 dBTP   -0.5 dBTP   +3.0 dB
  track09.m4a    -12.5    -4.5 dBTP   -0.5 dBTP   +3.0 dB

● 2 MP3 files (re-encode required for precise gain)
  Filename        LUFS    True Peak    Target        Gain
  track06.mp3    -12.0    -1.5 dBTP   -0.5 dBTP   +1.0 dB
  track07.mp3    -11.5    -1.2 dBTP   -0.5 dBTP   +0.7 dB

● 1 AAC/M4A files (re-encode required)
  Filename        LUFS    True Peak    Target        Gain
  track10.m4a    -12.5    -1.8 dBTP   -0.5 dBTP   +1.3 dB

▸ TP target: -0.5 dBTP (uniform delivery ceiling, AES TD1008 §7B)

✓ Report saved: ./baken_report_20250109_123456.csv

? Apply lossless gain adjustment to 3 lossless + 2 MP3 (lossless gain) + 2 AAC/M4A (lossless gain) files? [y/N] y

ℹ 2 MP3 + 1 AAC/M4A files have headroom but require re-encoding for precise gain.
  • Re-encoding causes minor quality loss (inaudible at 256kbps+)
  • Original bitrate will be preserved
? Also process these files with re-encoding? [y/N] y

? Create backup before processing? [Y/n] y
✓ Backup directory: ./backup

✓ Done! 10 files processed.
  • 3 lossless files (ffmpeg)
  • 2 MP3 files (native, lossless)
  • 2 AAC/M4A files (native, lossless)
  • 2 MP3 files (re-encoded)
  • 1 AAC/M4A files (re-encoded)
```

</details>

### Usage

#### Interactive Mode

Run `baken headroom` without further arguments to use the guided workflow in the current directory:

```bash
cd ~/Music/DJ-Tracks
baken headroom
```

The tool will guide you through:
1. Scanning and analyzing all audio files
2. Reviewing the categorized report
3. Confirming lossless processing
4. Optionally enabling MP3/AAC re-encoding
5. Creating backups (recommended)

#### Scriptable Mode

Pass paths, globs, or flags to run non-interactively (useful for pipelines and scripts):

```bash
# Analyze a directory without modifying anything
baken headroom --analyze-only ~/Music/DJ-Tracks

# Apply only lossless gain, with backup, save report to a specific path
baken headroom --lossless --backup ./bak --report results.csv ./album/

# Enable re-encoding as well
baken headroom --lossless --reencode --backup ./bak ./album/

# Operate on specific files
baken headroom --lossless track1.mp3 track2.flac

# Glob patterns
baken headroom --lossless --no-report "./music/**/*.mp3"

# Tighter ceiling for streaming-platform delivery (Spotify / Apple / YouTube max)
baken headroom --lossless --tp-target -1.0 ./album/

# Restore the legacy bitrate-dependent split (pre-v1.10 behaviour)
baken headroom --lossless --tp-split-bitrate ./album/
```

**Non-interactive defaults** (when any flag or path is provided):
- `--lossless` is **on** unless `--no-lossless`
- `--reencode` is **off** unless `--reencode` is explicitly passed
- `--backup` is **off** unless provided; bare `--backup` uses `<target>/backup`
- CSV report is written unless `--no-report`; `--report PATH` sets a custom location
- `--analyze-only` runs analysis + report only, skips processing

Run `baken headroom --help` for the full flag reference.

### Processing Methods

baken selects the optimal method for each file based on format and headroom:

| Format | Method | Precision | Quality Loss |
|--------|--------|-----------|--------------|
| FLAC, AIFF, WAV | ffmpeg | Arbitrary | None |
| MP3, AAC/M4A | mp3rgain (built-in) | 1.5dB steps | **None** (global_gain modification) |
| MP3, AAC/M4A | ffmpeg re-encode | Arbitrary | Inaudible at ≥256kbps |

Lossless files are written back in their **original sample format** — a 16-bit AIFF stays 16-bit, a 32-bit float WAV stays 32-bit float — so file size does not grow and float masters are not truncated. FLAC is the one partial exception: ffmpeg's FLAC encoder only accepts 16- and 24-bit output, so an 8-bit FLAC becomes 16-bit and a 20-bit FLAC becomes 24-bit.

#### Three-Tier Approach for Lossy Formats (MP3/AAC)

Each MP3 and AAC/M4A file is categorized into one of three tiers:

1. **Native Lossless** — ≥1.5 dB headroom to the configured ceiling
   - Truly lossless global_gain header modification in 1.5dB steps
   - Uses built-in [mp3rgain](https://github.com/M-Igashi/mp3rgain) library
   - Applied automatically (no user confirmation needed)

2. **Re-encode** — headroom exists but <1.5 dB to ceiling
   - Uses ffmpeg for arbitrary precision gain
   - MP3: `libmp3lame` with `-q:a 0` / AAC: `libfdk_aac` (falls back to built-in `aac`)
   - Preserves original bitrate; requires explicit user confirmation

3. **Skip** — no headroom available

### True Peak Ceiling

#### Default — uniform delivery target

Every file targets **-0.5 dBTP** by default. This is the maximum-aggression value that [AES TD1008](https://www.aes.org/technical/documentDownloads.cfm?docID=731) §7B describes for high-rate codec inputs ("may work satisfactorily with as little as -0.5 dBTP for the limiting threshold").

| File class | Ceiling | Native lossless requires |
|---|---|---|
| Lossless (FLAC, AIFF, WAV) | **-0.5 dBTP** | — |
| MP3 (any bitrate) | **-0.5 dBTP** | TP ≤ -2.0 dBTP |
| AAC/M4A (any bitrate) | **-0.5 dBTP** | TP ≤ -2.0 dBTP |

#### Why a single ceiling — pre-encode vs delivery

TD1008 has two related but distinct numbers:

1. **Generic delivery recommendation (§4)** — "Maximum True Peak level not exceed -1 dBTP at the codec input of lossy-encoded streams." This is the *pre-encode* limiter threshold.
2. **High-rate codec relaxation (§7B)** — "High-rate (e.g., 256 kbps) coders may work satisfactorily with as little as -0.5 dBTP" — also a *codec-input* threshold; "the limiting threshold may need to be reduced below the recommended -1.0 dBTP" for lower bit rates.

Both bullets describe the *limiter that sits in front of the encoder*. baken operates in the opposite position: on **already-encoded delivery files**. There is no further codec stage downstream to absorb additional overshoot, so the bitrate-dependent slack TD1008 grants the pre-encode limiter does not transfer to the end product. A single, codec-agnostic delivery ceiling is the correct interpretation. -0.5 dBTP is chosen because it is the most aggressive value TD1008 sanctions for any limiter in the chain; lossless and high-rate lossy files were already at -0.5, and low-rate files now stop giving up an unnecessary 0.5 dB of loudness.

See [docs/true-peak-ceiling.md](docs/true-peak-ceiling.md) for a longer walk-through with citations.

#### Tuning the ceiling

| Goal | Flag | Resulting ceiling |
|---|---|---|
| Default (max-aggressive delivery) | *(none)* | -0.5 dBTP for all files |
| Match Spotify / Apple Music / YouTube delivery max | `--tp-target -1.0` | -1.0 dBTP for all files |
| Conservative master with extra player headroom | `--tp-target -2.0` | -2.0 dBTP for all files |
| Mirror TD1008's pre-encode interpretation | `--tp-split-bitrate` | -0.5 dBTP ≥256 kbps, -1.0 dBTP <256 kbps |

`--tp-target` and `--tp-split-bitrate` are mutually exclusive. `--tp-split-bitrate` reproduces the pre-1.10 default exactly.

The native-lossless threshold scales with the chosen ceiling: it is always `target − 1.5 dB` (e.g. `-0.5` → TP ≤ -2.0; `-1.0` → TP ≤ -2.5; `-2.0` → TP ≤ -3.5).

### Output

#### CSV Report

| Filename | Format | Bitrate (kbps) | LUFS | True Peak (dBTP) | Target (dBTP) | Headroom (dB) | Method | Effective Gain (dB) |
|----------|--------|----------------|------|------------------|---------------|---------------|--------|---------------------|
| track01.flac | Lossless | - | -13.3 | -3.2 | -0.5 | +2.7 | ffmpeg | +2.7 |
| track04.mp3 | MP3 | 320 | -14.0 | -5.5 | -0.5 | +5.0 | mp3rgain | +4.5 |
| track06.mp3 | MP3 | 320 | -12.0 | -1.5 | -0.5 | +1.0 | re-encode | +1.0 |
| track08.m4a | AAC | 256 | -13.0 | -4.0 | -0.5 | +3.5 | native | +3.0 |
| track10.m4a | AAC | 256 | -12.5 | -1.8 | -0.5 | +0.7 | re-encode | +0.7 |

#### Backup Structure

```
./
├── track01.flac             ← Modified
├── track04.mp3              ← Modified
├── track08.m4a              ← Modified
├── subfolder/
│   └── track06.mp3          ← Modified
└── backup/                  ← Created by baken
    ├── track01.flac         ← Original
    ├── track04.mp3          ← Original
    ├── track08.m4a          ← Original
    └── subfolder/
        └── track06.mp3      ← Original
```

### Notes & Technical Details

- **Files are overwritten in place** after backup — Rekordbox metadata remains linked
- Only files with **positive effective gain** are shown and processed
- MP3/AAC native lossless requires at least **1.5dB headroom** to be processed
- MP3/AAC re-encoding is **opt-in** and requires explicit confirmation
- macOS resource fork files (`._*`) are automatically ignored

#### Why 1.5dB Steps?

Both MP3 and AAC store a "global_gain" value as an integer. Each ±1 increment changes the gain by `2^(1/4)` = **±1.5 dB**. This is a format-level constraint, not a tool limitation.

baken uses the built-in [mp3rgain](https://github.com/M-Igashi/mp3rgain) library to directly modify this field — no decoding or re-encoding involved.

#### Native Lossless Threshold

Since native lossless gain only works in 1.5 dB steps, at least 1.5 dB of headroom to the configured target ceiling is required. The threshold scales automatically:

| Target | Requires TP ≤ |
|---|---|
| -0.5 dBTP (default) | -2.0 dBTP |
| -1.0 dBTP (`--tp-target -1.0`) | -2.5 dBTP |
| -2.0 dBTP (`--tp-target -2.0`) | -3.5 dBTP |

Example: 320 kbps file at -3.5 dBTP, default target → 2 steps (+3.0 dB) → -0.5 dBTP (optimal).

#### Re-encode Quality

At ≥256kbps, re-encoding introduces quantization noise below -90dB — far below audible threshold. Only gain is applied (no EQ, compression, or dynamics processing), and original bitrate is preserved.

## Rekordbox Playlist Sorter (`baken rbsort`)

Rekordbox does not expose a "sort by Key AND BPM" option in its UI. `baken rbsort` takes an exported Rekordbox XML and rewrites every playlist in it so its tracks run **Camelot Key (1A → 12B) ascending** then **BPM ascending**. Playlists keep their names and folder positions; only the track order inside each one changes. Rekordbox reads the sorted file back as its `rekordbox xml` tree, so you end up with a Key+BPM-sorted mirror of your `Playlists` sitting next to the originals.

This is the same idea as `baken headroom` applied to playlist order: Rekordbox's software-only features (Auto Gain, multi-column sort) don't follow your tracks to the CDJ. `rbsort` bakes Key+BPM order into the playlist itself — so when you export to USB in Rekordbox's EXPORT mode, the CDJ plays the set in that exact order with no on-deck reordering.

### Workflow

1. **Set key display to Alphanumeric (1A..12B notation)** in Rekordbox: *Preferences > View > Key display format > Alphanumeric*.
2. **Export**: *File > Export Collection in xml format*. Always save to the same path, e.g. `~/Music/rekordbox/collection.xml`.
3. **Run rbsort** on that file. It is sorted in place:
   ```bash
   baken rbsort ~/Music/rekordbox/collection.xml

   # Only one playlist (top-level: just the name; nested: "Folder/Playlist")
   baken rbsort ~/Music/rekordbox/collection.xml --playlist "Sets/Friday"

   # Keep the export untouched and write elsewhere
   baken rbsort ~/Music/rekordbox/collection.xml -o ~/Music/rekordbox/sorted.xml
   ```
4. **One-time setup**: *Preferences > Advanced > Database > rekordbox xml > Imported Library* → select that same file.
5. **Restart Rekordbox** (it only re-reads the XML on startup) and open the **`rekordbox xml` tree** in the left sidebar. It is a *separate* tree from your main library — switch to it from the sidebar icon column on the far left. It mirrors your `Playlists` folder structure, every playlist already in Key+BPM order: `1A` (lowest BPM) → `1B` → `2A` → … → `12B` (highest BPM).
6. **Use it**: drag any playlist from the `rekordbox xml` tree into your main `Playlists` (it lands as a new playlist; your original is unchanged), switch to *EXPORT* mode, plug in your USB / SD, then **right-click the playlist → Export Playlist**. CDJs read tracks in playlist order by default — your Key+BPM sort plays back on the deck in that exact order.

**Keeping it in sync**: whenever your playlists change, repeat steps 2, 3 and the restart. The file path never changes, so the Imported Library setting keeps pointing at the freshly sorted export and the `rekordbox xml` tree stays an always-sorted copy of your library.

> The sorted playlists live **only** in the `rekordbox xml` tree, not in your main `Playlists`. If you only see unsorted originals, you're looking at the local library — switch sidebar trees.

### Usage

```
baken rbsort <XML> [--playlist <PATH>] [-o <PATH>]
```

| Argument / Flag | Description |
|------|-------------|
| `<XML>` | Exported Rekordbox XML (required). Sorted in place unless `--output` is given |
| `--playlist <PATH>` | Sort only this playlist. Top-level playlists: just the name (e.g. `"Happy House and Trance"`). Nested: `/`-separate folder/playlist names (e.g. `"Folder/SubFolder/MyPlaylist"`). Omitted: every TrackID-referenced playlist is sorted |
| `--output <PATH>` (`-o`) | Write the result here instead of overwriting the input XML |

### Sort Rules

- **Primary**: Camelot Key ascending — `1A → 1B → 2A → 2B → … → 12A → 12B`
- **Secondary**: BPM ascending within each key group
- Tracks with no Camelot key sort **after** all known keys; within a key group, tracks with BPM 0 / unanalyzed sort last

See [docs/rbsort-sort-comparison.md](docs/rbsort-sort-comparison.md) for a 6-track walk-through showing how this compound sort differs from Rekordbox / CDJ's single-column *Sort by Key* and *Sort by BPM*.

### Notes

- Requires the `Tonality` field to be exported as 1A..12B (Rekordbox's "Alphanumeric" key display format). Non-matching values (e.g. `Am`, `C#`) are silently sorted last.
- Only `KeyType="0"` (TrackID-referenced) playlists are sorted. In all-playlists mode, other playlists pass through unchanged; for a single target, `rbsort` errors out.
- Only the order of `<TRACK Key="…"/>` references changes. Playlist names, folder structure, `Count`/`Entries` attributes, whitespace and everything else in the XML are preserved byte-for-byte, so running `rbsort` twice on the same file is a no-op.
- `baken rbsort` does **not** require ffmpeg — only the `headroom` and `cdjsafe` subcommands do.

## CDJ-safe Transcoder (`baken cdjsafe`)

*Added in v3.0.0. Design discussion: [#40](https://github.com/M-Igashi/baken/issues/40).*

Pre-NXS2 CDJs (CDJ-2000NXS, CDJ-2000, CDJ-900NXS, CDJ-850, …) have inconsistent or absent support for anything that isn't MP3: FLAC needs an NXS2 (2016+), and ALAC/AIFF/WAV/AAC fail on specific firmware combinations — sometimes mid-set. `baken cdjsafe` is the emergency-backup path: it takes a gig playlist and produces a USB-ready set of files that **will play on any CDJ**, with your cues and beatgrid intact.

```bash
baken cdjsafe ~/Music/rekordbox/collection.xml \
  --playlist "Sets/Friday" \
  --out-dir ~/Music/cdjsafe-friday
```

### What it does

1. Reads the target playlist from your exported `collection.xml`.
2. Converts every track whose file exists to the CDJ-safe profile. Tracks whose files are missing on disk are listed as skipped and left out (the emergency stick still gets everything that is there); only a playlist with no file present at all is an error. Profile: — **320 kbps CBR MP3 @ 44.1 kHz**, ID3v2.3 tags, artwork kept (JPEG, capped at 500×500):

   | Source | Action |
   |---|---|
   | FLAC, WAV, AIFF, ALAC | Re-encode |
   | AAC/M4A (any bitrate) | Re-encode |
   | MP3 not exactly 320 kbps CBR @ 44.1 kHz | Re-encode (lossy→lossy, reported) |
   | MP3 already 320 kbps CBR @ 44.1 kHz | **Byte-identical copy** (no generation loss, LAME header untouched) |

3. Emits an updated XML (default: `<input>-out.xml`) where each converted track is a **new entry with a fresh TrackID** that inherits the source's beatgrid (`TEMPO`) and hot/memory cues (`POSITION_MARK`) **verbatim**, grouped in a `CDJ-safe (MP3)/<playlist>-CDJ-safe` folder. The `-CDJ-safe` suffix keeps the imported playlist from colliding with the original. New entries get a `[cdjsafe]` marker appended to their Comments so they're distinguishable after import.
4. Reports every lossy→lossy re-encode so you can refresh those tracks from lossless masters before the next gig.

If any track fails to convert, **no XML is written** — a partial USB defeats the point.

### Importing back into Rekordbox

1. *Preferences > Advanced > Database > rekordbox xml > Imported Library* → select the output XML, restart Rekordbox.
2. Open the `rekordbox xml` sidebar tree → `CDJ-safe (MP3)/<playlist>-CDJ-safe`.
3. Right-click the imported tracks → **Import to Collection**. Cues and beatgrid come with them — no re-analysis needed.
4. Export the playlist to USB in EXPORT mode as usual.

### Usage

```
baken cdjsafe <XML> --playlist <PATH> --out-dir <DIR> [-o <PATH>]
```

| Argument / Flag | Description |
|------|-------------|
| `<XML>` | Exported Rekordbox XML (required) |
| `--playlist <PATH>` | Playlist to convert (required). Top-level: just the name; nested: `Folder/Playlist` |
| `--out-dir <DIR>` | Directory for the MP3 files (required; created if missing) |
| `--output <PATH>` (`-o`) | Output XML path. Defaults to `<input-stem>-out.<ext>` next to the input |

### Notes

- The output profile is locked (320 kbps CBR / 44.1 kHz / ID3v2.3) — it's the only combination that plays reliably across the whole CDJ fleet, and CBR sidesteps Rekordbox's VBR cue-offset problem.
- ffmpeg writes a valid Xing/LAME header, so Rekordbox compensates the LAME encoder delay and cues stay sample-aligned.
- Filenames are FAT32/exFAT-sanitized; collisions get a numeric suffix.
- Requires ffmpeg (with `libmp3lame`; `soxr` resampling is used when available).

## License

MIT
