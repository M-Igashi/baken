# Bake'n Deck (`baken`)

**Built for the Rekordbox → CDJ workflow.**

Rekordbox's Auto Gain doesn't survive USB export, and Rekordbox has never offered compound Key+BPM sort (Serato had it for years — but never exported to CDJs). Bake'n Deck bakes both into the files themselves, so what you prep is what plays on the deck.

> [!NOTE]
> **headroom is now Bake'n Deck 3.0.** As announced in [#60](https://github.com/M-Igashi/baken/issues/60), the binary, crate, Homebrew formula, and repository were renamed from `headroom` to `baken` at v3.0.0, and the site moved to `baken.ravers.workers.dev`. This was a **hard cut** — the old `headroom` install channels (brew/winget/cargo/AUR) no longer receive updates; reinstall via the `baken` packages below. The loudness analyzer now lives under the `baken headroom` subcommand.

## What is this?

**Bake'n Deck** is a CLI toolkit with three subcommands:

- **`baken headroom`** simulates the behavior of Rekordbox's Auto Gain feature, but with a key difference: it identifies files with available headroom (True Peak below the target ceiling) and applies gain adjustment **without using a limiter**. Designed for DJs and producers who want to maximize loudness while preserving dynamics, ensuring tracks hit the optimal True Peak ceiling without clipping.
- **`baken rbsort`** sorts a Rekordbox playlist by **Camelot Key (1A→12B) then BPM ascending**, and appends the result as a new playlist to your `collection.xml`. Useful for harmonic mixing prep when the Rekordbox UI does not expose multi-column sort. See [Rekordbox Playlist Sorter](#rekordbox-playlist-sorter-baken-rbsort).
- **`baken cdjsafe`** *(new in v3.0)* transcodes every track in a Rekordbox playlist to **CDJ-safe MP3s (320 kbps CBR @ 44.1 kHz)** and emits an updated XML in which the new tracks inherit the originals' **cue points and beatgrid** — for gigs on pre-NXS2 CDJs that won't play FLAC/ALAC/AIFF/AAC. See [CDJ-safe Transcoder](#cdj-safe-transcoder-baken-cdjsafe).

## Key Features

- **Single binary**: mp3rgain is built-in as a library — only ffmpeg required as external dependency
- **Uniform True Peak ceiling**: -0.5 dBTP for every file by default — the most aggressive, AES TD1008–blessed delivery target — fully overridable via `--tp-target`
- **Multiple processing methods**: ffmpeg for lossless formats, built-in mp3rgain for lossless MP3/AAC gain, ffmpeg re-encode for precise gain
- **Non-destructive workflow**: Original files are backed up before processing
- **Metadata preservation**: Audio tags (ID3v2, Vorbis comment, BWF) are preserved during processing, and files are overwritten in place so Rekordbox cue points and other external metadata remain linked
- **No limiter**: Pure gain adjustment only — dynamics are preserved
- **Interactive CLI**: Guided step-by-step process with two-stage confirmation
- **Scriptable CLI**: Non-interactive mode for pipelines and CI (paths, globs, and flags)
- **Rekordbox playlist sorter**: `baken rbsort` produces a new playlist sorted by Camelot Key then BPM
- **CDJ-safe transcoder** *(v3.0+)*: `baken cdjsafe` normalizes a gig playlist to 320 kbps CBR MP3 with cues and beatgrid carried over via XML

## Installation

baken requires ffmpeg. Package managers install it automatically.

| Platform | Command |
|----------|---------|
| **macOS (Homebrew)** | `brew install M-Igashi/tap/baken` |
| **Windows (winget)** | `winget install M-Igashi.baken` |
| **Arch Linux (AUR)** | `yay -S baken-bin` |
| **Cargo** | `cargo install baken` (ffmpeg must be installed separately) |

Pre-built binaries are available on the [Releases](https://github.com/M-Igashi/baken/releases) page (ffmpeg must be installed separately).

### Build from Source

```bash
git clone https://github.com/M-Igashi/baken.git
cd baken
cargo build --release
```

## Command Overview

```bash
baken headroom [PATHS] [FLAGS]   # loudness analyzer & gain adjustment
baken rbsort --xml <collection.xml> [FLAGS]   # Key+BPM playlist sorter
baken cdjsafe --xml <collection.xml> --playlist <name> --out-dir <dir>   # CDJ-safe MP3 transcoder
```

Run `baken --help` or `baken <subcommand> --help` for the full reference.

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

```
$ cd ~/Music/DJ-Tracks
$ baken headroom

╭─────────────────────────────────────╮
│            baken v3.0.0             │
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

Rekordbox does not expose a "sort by Key AND BPM" option in its UI. `baken rbsort` reads your `collection.xml`, sorts each target playlist by **Camelot Key (1A → 12B) ascending** then **BPM ascending**, and emits the sorted copies into a new `Sorted (Key+BPM)/` folder appended to the same XML. Originals are left untouched. The layout mirrors the analyzer's `backup/` directory: one container folder, each item inside keeps its source name.

This is the same idea as `baken headroom` applied to playlist order: Rekordbox's software-only features (Auto Gain, multi-column sort) don't follow your tracks to the CDJ. `rbsort` bakes Key+BPM order into the playlist itself — so when you export to USB in Rekordbox's EXPORT mode, the CDJ plays the set in that exact order with no on-deck reordering.

### Workflow

1. **Set key display to Alphanumeric (1A..12B notation)** in Rekordbox: *Preferences > View > Key display format > Alphanumeric*.
2. **Export**: *File > Export Collection in xml format* → e.g. `~/Music/rekordbox/collection.xml`.
3. **Run rbsort**:
   ```bash
   # Sort every TrackID-referenced playlist in the XML
   baken rbsort --xml ~/Music/rekordbox/collection.xml

   # Or target one playlist (top-level: just the name)
   baken rbsort \
     --xml ~/Music/rekordbox/collection.xml \
     --playlist "Happy House and Trance"

   # Or target one nested under a folder
   baken rbsort \
     --xml ~/Music/rekordbox/collection.xml \
     --playlist "Sets/Friday"
   ```
   Output defaults to `collection-out.xml` next to the input. Pass `--output <PATH>` to override.
4. **Point Rekordbox at the output XML**: *Preferences > Advanced > Database > rekordbox xml > Imported Library* → select `collection-out.xml`, then **restart Rekordbox** (Rekordbox only re-reads the XML on startup).
5. **Open the `rekordbox xml` tree** in the left sidebar. It is a *separate* tree from your main library — switch to it from the **sidebar icon column** on the far left (the icon labeled `rekordbox xml`). Inside you'll find a new folder `Sorted (Key+BPM)/` containing each sorted playlist under its original name.
6. **Verify the sort** by clicking any playlist inside `Sorted (Key+BPM)/` — tracks should run `1A` (lowest BPM) → `1B` → `2A` → … → `12B` (highest BPM).
7. **Drag** the playlists you want from `Sorted (Key+BPM)/` into your main `Playlists` collection. Your original playlists (still in `Playlists`) are unchanged.
8. **Export to USB for CDJ**: switch Rekordbox to *EXPORT* mode (top-left dropdown), plug in your USB / SD, then **right-click the playlist → Export Playlist**. CDJs read tracks in playlist order by default — your Key+BPM sort plays back on the deck in that exact order.

> The sorted results live **only** inside the `rekordbox xml > Sorted (Key+BPM)/` folder, not in your main `Playlists`. If you only see the originals, you're looking at the local library — switch sidebar trees.

### Flags

| Flag | Description |
|------|-------------|
| `--xml <PATH>` | Path to `collection.xml` (required) |
| `--playlist <PATH>` | Source playlist under the Rekordbox `Playlists` root. **Optional** — if omitted, every TrackID-referenced playlist in the XML is sorted. Top-level playlists: just the name (e.g. `"Happy House and Trance"`). Nested: `/`-separate folder/playlist names (e.g. `"Folder/SubFolder/MyPlaylist"`) |
| `--output <PATH>` (`-o`) | Output XML path. Optional — defaults to `<input-stem>-out.<ext>` next to the input |
| `--name <NAME>` | Override the sorted playlist's name. Only valid with `--playlist`. When sorting all playlists, each sorted copy reuses its source name |

### Sort Rules

- **Primary**: Camelot Key ascending — `1A → 1B → 2A → 2B → … → 12A → 12B`
- **Secondary**: BPM ascending within each key group
- Tracks with no Camelot key sort **after** all known keys; within a key group, tracks with BPM 0 / unanalyzed sort last

See [docs/rbsort-sort-comparison.md](docs/rbsort-sort-comparison.md) for a 6-track walk-through showing how this compound sort differs from Rekordbox / CDJ's single-column *Sort by Key* and *Sort by BPM*.

### Notes

- Requires the `Tonality` field to be exported as 1A..12B (Rekordbox's "Alphanumeric" key display format). Non-matching values (e.g. `Am`, `C#`) are silently sorted last.
- Only `KeyType="0"` (TrackID-referenced) playlists are supported. In all-playlists mode, non-`KeyType=0` playlists are silently skipped; for a single target, `rbsort` errors out.
- `baken rbsort` does **not** require ffmpeg — only the `headroom` and `cdjsafe` subcommands do.
- A single `Sorted (Key+BPM)/` folder is appended inside the `<PLAYLISTS>` ROOT NODE, regardless of how many playlists were processed. The ROOT `Count` is bumped by 1.

## CDJ-safe Transcoder (`baken cdjsafe`)

*Added in v3.0.0. Design discussion: [#40](https://github.com/M-Igashi/baken/issues/40).*

Pre-NXS2 CDJs (CDJ-2000NXS, CDJ-2000, CDJ-900NXS, CDJ-850, …) have inconsistent or absent support for anything that isn't MP3: FLAC needs an NXS2 (2016+), and ALAC/AIFF/WAV/AAC fail on specific firmware combinations — sometimes mid-set. `baken cdjsafe` is the emergency-backup path: it takes a gig playlist and produces a USB-ready set of files that **will play on any CDJ**, with your cues and beatgrid intact.

```bash
baken cdjsafe \
  --xml ~/Music/rekordbox/collection.xml \
  --playlist "Sets/Friday" \
  --out-dir ~/Music/cdjsafe-friday
```

### What it does

1. Reads the target playlist from your exported `collection.xml`.
2. Converts **every** track to the CDJ-safe profile — **320 kbps CBR MP3 @ 44.1 kHz**, ID3v2.3 tags, artwork kept (JPEG, capped at 500×500):

   | Source | Action |
   |---|---|
   | FLAC, WAV, AIFF, ALAC | Re-encode |
   | AAC/M4A (any bitrate) | Re-encode |
   | MP3 not exactly 320 kbps CBR @ 44.1 kHz | Re-encode (lossy→lossy, reported) |
   | MP3 already 320 kbps CBR @ 44.1 kHz | **Byte-identical copy** (no generation loss, LAME header untouched) |

3. Emits an updated XML (default: `<input>-out.xml`) where each converted track is a **new entry with a fresh TrackID** that inherits the source's beatgrid (`TEMPO`) and hot/memory cues (`POSITION_MARK`) **verbatim**, grouped in a `CDJ-safe (MP3)/<playlist>` folder. New entries get a `[cdjsafe]` marker appended to their Comments so they're distinguishable after import.
4. Reports every lossy→lossy re-encode so you can refresh those tracks from lossless masters before the next gig.

If any track fails to convert, **no XML is written** — a partial USB defeats the point.

### Importing back into Rekordbox

1. *Preferences > Advanced > Database > rekordbox xml > Imported Library* → select the output XML, restart Rekordbox.
2. Open the `rekordbox xml` sidebar tree → `CDJ-safe (MP3)/<playlist>`.
3. Right-click the imported tracks → **Import to Collection**. Cues and beatgrid come with them — no re-analysis needed.
4. Export the playlist to USB in EXPORT mode as usual.

### Flags

| Flag | Description |
|------|-------------|
| `--xml <PATH>` | Path to `collection.xml` (required) |
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
