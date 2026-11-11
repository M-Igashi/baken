# `baken headroom` reference

`baken headroom` measures loudness (LUFS) and true peak for every audio file, then writes one gain change into each file so all of them sit at one true peak ceiling, -0.5 dBTP by default. Gain only: no limiter, no compression, and no MP3 or AAC is ever re-encoded.

For the background, see the guides: [rekordbox Auto Gain: why it does not reach your CDJ](https://baken.ravers.workers.dev/auto-gain) ([日本語](https://baken.ravers.workers.dev/ja/auto-gain)) and [Loudness normalization for DJs: a LUFS target or a true peak ceiling?](https://baken.ravers.workers.dev/loudness-normalization) ([日本語](https://baken.ravers.workers.dev/ja/loudness-normalization)).

## How it works

1. Scans the target directory for audio files (FLAC, AIFF, WAV, MP3, AAC/M4A, ALAC/M4A)
2. Measures LUFS (Integrated Loudness) and True Peak per ITU-R BS.1770-4, decoding in-process (ffmpeg is used for files the built-in decoder cannot open). A file that does not decode cleanly is reported as damaged and left alone (see [Damaged Files](#damaged-files))
3. Computes the gain that puts each file's True Peak at the ceiling (-0.5 dBTP by default). Quiet files get a positive gain, loud files a negative one; `--boost-only` restricts this to positive gains.
4. Categorizes files by processing method:
   - **Green**: Lossless files (ffmpeg)
   - **Yellow**: MP3/AAC files with enough headroom for native lossless gain
   - MP3/AAC files already within one 1.5 dB step of the ceiling are left alone: nothing is re-encoded to move a file by less than a step
5. Displays categorized report
6. Confirmation: "Apply lossless gain adjustment?" (lossless + native MP3/AAC)
7. Creates backups and processes files

## Usage

### Interactive

Run `baken headroom` without further arguments to use the guided workflow in the current directory:

```bash
cd ~/Music/DJ-Tracks
baken headroom
```

The tool will guide you through:
1. Scanning and analyzing all audio files
2. Reviewing the categorized report
3. Confirming lossless processing
4. Creating backups (recommended)

### Scriptable

Pass paths, globs, or flags to run non-interactively (useful for pipelines and scripts). Nothing is asked: unless `--analyze-only` is given, the gain is written at once, and a backup is made only with `--backup`.

```bash
# Analyze a directory without modifying anything
baken headroom --analyze-only ~/Music/DJ-Tracks

# Apply only lossless gain, with backup, save report to a specific path
baken headroom --lossless --backup ./bak --report results.csv ./album/

# Operate on specific files
baken headroom --lossless track1.mp3 track2.flac

# Glob patterns
baken headroom --lossless --no-report "./music/**/*.mp3"

# Tighter ceiling for streaming-platform delivery (Spotify / Apple / YouTube max)
baken headroom --lossless --tp-target -1.0 ./album/

# Restore the legacy bitrate-dependent split (pre-v1.10 behaviour)
baken headroom --lossless --tp-split-bitrate ./album/

# Only raise quiet tracks, never lower loud ones (pre-v3.3 behaviour)
baken headroom --lossless --boost-only ./album/
```

**Non-interactive defaults** (when any flag or path is provided):
- `--lossless` is **on** unless `--no-lossless`
- `--reencode` and `--no-reencode` are accepted for compatibility and do nothing: no file is re-encoded for gain
- `--backup` is **off** unless provided; bare `--backup` uses `<target>/backup`
- CSV report is written unless `--no-report`; `--report PATH` sets a custom location
- `--analyze-only` runs analysis + report only, skips processing
- The exit code is 1 when a file could not be analyzed or written, after the summary is printed
- `--boost-only` skips files above the ceiling instead of lowering them

Run `baken headroom --help` for the full flag reference.

## Example session

<details>
<summary>Full interactive session (28 files analyzed → 8 processed)</summary>

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

● 3 MP3 files (native lossless, 1.5 dB steps)
  Filename        LUFS    True Peak    Target        Gain
  track04.mp3    -14.0    -5.5 dBTP   -0.5 dBTP   +4.5 dB
  track05.mp3    -13.5    -6.0 dBTP   -0.5 dBTP   +4.5 dB
  track11.mp3     -6.8     0.3 dBTP   -0.5 dBTP   -1.5 dB

● 2 AAC/M4A files (native lossless, 1.5 dB steps)
  Filename        LUFS    True Peak    Target        Gain
  track08.m4a    -13.0    -4.0 dBTP   -0.5 dBTP   +3.0 dB
  track09.m4a    -12.5    -4.5 dBTP   -0.5 dBTP   +3.0 dB

▸ TP target: -0.5 dBTP (uniform delivery ceiling, AES TD1008 §7B)
▸ Gain mode: normalize (raise quiet files, lower loud ones)

✓ Report saved: ./baken_report_20250109_123456.csv

? Apply lossless gain adjustment to 3 lossless + 3 MP3 (lossless gain) + 2 AAC/M4A (lossless gain) files? [y/N] y

? Create backup before processing? [Y/n] y
✓ Backup directory: ./backup

✓ Done! 8 files processed.
  • 3 lossless files (ffmpeg)
  • 3 MP3 files (native, lossless)
  • 2 AAC/M4A files (native, lossless)
```

</details>

## How each format is handled

baken selects the optimal method for each file based on format and headroom:

| Format | Method | Precision | Quality Loss |
|--------|--------|-----------|--------------|
| FLAC, AIFF, WAV | ffmpeg | Arbitrary | None |
| MP3, AAC-LC (`.m4a`, `.mp4`, raw `.aac`) | mp3rgain (built-in) | 1.5dB steps | **None** (global_gain modification) |
| ALAC/M4A | ffmpeg (re-encoded as ALAC) | Arbitrary | **None** (lossless codec) |

A lossy file closer to the ceiling than one 1.5 dB step is left alone rather than re-encoded, so no MP3 or AAC ever loses a generation to a gain change.

HE-AAC (v1 and v2, in `.m4a` or raw `.aac`) is skipped and listed after the analysis. `global_gain` only reaches its AAC core: the decoder rebuilds the high band from SBR data stored at absolute levels, so a native gain change would leave the top end where it was, and nothing short of a re-encode can move it ([#268](https://github.com/M-Igashi/baken/issues/268)).

Lossless files are written back in their **original sample format** (a 16-bit AIFF stays 16-bit, a 32-bit float WAV stays 32-bit float), so file size does not grow and float masters are not truncated. FLAC is the one partial exception: ffmpeg's FLAC encoder only accepts 16- and 24-bit output, so an 8-bit FLAC becomes 16-bit and a 20-bit FLAC becomes 24-bit.

A WAV keeps its header form too. ffmpeg writes `WAVE_FORMAT_EXTENSIBLE` for integer PCM deeper than 16 bits and for anything faster than 48 kHz, so a plain WAV, the way DAWs and download stores write it, gets its own `fmt ` chunk back after the rewrite and reads the same in `baken cdjsafe --check` before and after a run ([#218](https://github.com/M-Igashi/baken/issues/218)).

## Raising and lowering

The gain for each file is `ceiling − measured True Peak`. Files below the ceiling get a positive gain, files above it (loudness-war masters, inter-sample overs from lossy encoding) get a negative one, so every track ends up at the same True Peak with no limiter involved. Pass `--boost-only` to keep the pre-v3.3 behaviour of raising quiet files only and leaving loud files untouched.

## MP3 and AAC: whole 1.5 dB steps

Both MP3 and AAC store a `global_gain` value as an integer, and each ±1 changes the level by `2^(1/4)`, which is **1.5 dB**. That is a property of the formats, not of the tool. baken uses the built-in [mp3rgain](https://github.com/M-Igashi/mp3rgain) library to rewrite this field directly, with no decoding and no re-encoding.

What happens to an MP3 or AAC file therefore depends on where its True Peak sits:

| True Peak | What happens |
|---|---|
| Above the ceiling | Lowered in whole steps, rounded so the result never exceeds the ceiling. A file at +0.3 dBTP takes one step (-1.5 dB) and lands at -1.2 dBTP |
| At least one step (1.5 dB) below the ceiling | Raised in whole steps, rounded down so it never overshoots. A file at -3.5 dBTP takes two steps (+3.0 dB) and lands at -0.5 dBTP |
| Less than one step below the ceiling, or within 0.05 dB of it | Left alone |
| Above the ceiling with `--boost-only` | Left alone |

The raise threshold follows the ceiling. It is always `target − 1.5 dB`:

| Target | Raised when True Peak ≤ |
|---|---|
| -0.5 dBTP (default) | -2.0 dBTP |
| -1.0 dBTP (`--tp-target -1.0`) | -2.5 dBTP |
| -2.0 dBTP (`--tp-target -2.0`) | -3.5 dBTP |

**Why files within one step are left alone.** Such a file could only be moved by re-encoding it, and that costs a generation of quality to gain less than 1.5 dB. It is also where every lossy file ends up after a native step, so re-encoding there would mean re-encoding a large part of the library again on every later run ([#138](https://github.com/M-Igashi/baken/issues/138)). Both directions therefore stop at whole steps, and a run over a library that has already been processed finds nothing left to do.

An AAC file with frames mp3rgain cannot parse is left alone too: those frames would keep their old level while the rest moved, which plays as a click on each one.

## True peak ceiling

### The default: one ceiling for every file

Every file targets **-0.5 dBTP** by default. This is the maximum-aggression value that [AES TD1008](https://www.aes.org/technical/documentDownloads.cfm?docID=731) §7B describes for high-rate codec inputs ("may work satisfactorily with as little as -0.5 dBTP for the limiting threshold").

| File class | Ceiling | Native lossless raise requires |
|---|---|---|
| Lossless (FLAC, AIFF, WAV) | **-0.5 dBTP** | n/a |
| MP3 (any bitrate) | **-0.5 dBTP** | TP ≤ -2.0 dBTP (any TP above the ceiling is lowered natively) |
| AAC/M4A (any bitrate) | **-0.5 dBTP** | TP ≤ -2.0 dBTP (any TP above the ceiling is lowered natively) |

### Why one ceiling

TD1008's two numbers (-1 dBTP in §4, and -0.5 dBTP for high-rate codecs in §7B) both describe the limiter that sits in front of an encoder. baken works on files that are already encoded, with no codec stage after it to add overshoot, so the bitrate-dependent slack does not apply and one ceiling for every file is the right reading. -0.5 dBTP is the most aggressive value TD1008 sanctions anywhere in the chain. [true-peak-ceiling.md](true-peak-ceiling.md) has the full walk-through with citations.

### Tuning the ceiling

| Goal | Flag | Resulting ceiling |
|---|---|---|
| Default (max-aggressive delivery) | *(none)* | -0.5 dBTP for all files |
| Match Spotify / Apple Music / YouTube delivery max | `--tp-target -1.0` | -1.0 dBTP for all files |
| Conservative master with extra player headroom | `--tp-target -2.0` | -2.0 dBTP for all files |
| Mirror TD1008's pre-encode interpretation | `--tp-split-bitrate` | -0.5 dBTP ≥256 kbps, -1.0 dBTP <256 kbps |

`--tp-target` and `--tp-split-bitrate` are mutually exclusive. `--tp-split-bitrate` reproduces the pre-1.10 default exactly.

The native-lossless raise threshold scales with the chosen ceiling: it is always `target − 1.5 dB` (e.g. `-0.5` → TP ≤ -2.0; `-1.0` → TP ≤ -2.5; `-2.0` → TP ≤ -3.5). Files above the ceiling are always lowered natively.

## Damaged files

A file with audio frames the decoder rejects is reported as damaged, with the number of frames and where the first one is, and gets no gain. So does a file that measures above 0 LUFS or +20 dBTP, a level no real file reaches. The numbers of such a file describe what the decoder made of the damage, not the music: an AAC whose last two and a half minutes are corrupt measured +22.3 LUFS and +37.6 dBTP through ffmpeg, and up to 4.4.0 it was offered -39 dB, which would have left the track almost silent ([#223](https://github.com/M-Igashi/baken/issues/223)). The damaged part may also play as a burst of noise on a player, so replace the file from its source; rekordbox analyses such a file without complaint. The terminal lists every damaged file with its full path, and the CSV report names the reason in its `Damage` column.

## Output

### CSV Report

| Filename | Format | Bitrate (kbps) | LUFS | True Peak (dBTP) | Target (dBTP) | Headroom (dB) | Method | Effective Gain (dB) | Damage |
|----------|--------|----------------|------|------------------|---------------|---------------|--------|---------------------|--------|
| track01.flac | Lossless | - | -13.3 | -3.2 | -0.5 | +2.7 | ffmpeg | +2.7 | |
| track04.mp3 | MP3 | 320 | -14.0 | -5.5 | -0.5 | +5.0 | mp3rgain | +4.5 | |
| track06.mp3 | MP3 | 320 | -12.0 | -1.5 | -0.5 | +1.0 | none | 0.0 | |
| track08.m4a | AAC | 256 | -13.0 | -4.0 | -0.5 | +3.5 | native | +3.0 | |
| track10.m4a | AAC | 256 | -12.5 | -1.2 | -0.5 | +0.7 | none | 0.0 | |
| track11.m4a | - | 307 | -7.7 | 3.3 | -0.5 | -3.8 | none | +0.0 | 1460 audio frames failed to decode, the first at 3:41.7 |

### Backup Structure

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

## Notes

- **Files are overwritten in place** after the backup, so rekordbox keeps its cues and beatgrids linked to them
- **A backup is never replaced.** When the backup folder already holds a file of that name from an earlier run, the new copy is written beside it as `track (2).flac`, so the first copy stays the original
- **Analyse processed FLAC files again in rekordbox before a USB export from rekordbox.** headroom re-encodes FLAC, which moves every frame, and rekordbox copies the seek table of its earlier analysis onto the stick unchanged, so the player's table no longer points at the frames ([#219](https://github.com/M-Igashi/baken/issues/219)). `baken expressport` rebuilds the table from the file itself. Only FLAC analyses carry such a table, and MP3 and AAC are adjusted in place
- **Export processed tracks to a stick again, even if they are already on it.** rekordbox copies a track's file only the first time it exports that track; later exports update the track information on the stick but leave the old file there ([AlphaTheta community](https://forums.pioneerdj.com/hc/en-us/community/posts/24823970381465-Metadata-update-export-sync-to-USB), [#257](https://github.com/M-Igashi/baken/issues/257)). Delete the processed tracks from the device in rekordbox (or format the stick) and export again. `baken expressport` copies a changed file again on its own ([#231](https://github.com/M-Igashi/baken/issues/231))
- **Tags survive the rewrite**: MP3/AAC native gain never rewrites the container, and where ffmpeg does (the lossless formats) the source's raw tags are put back over the output byte for byte. That covers the payloads DJ software writes and ffmpeg has nowhere to put: ID3v2 `GEOB`/`PRIV` frames on MP3, AIFF and WAV, and free-form `----` atoms on ALAC and AAC in `.m4a` ([#117](https://github.com/M-Igashi/baken/issues/117)). A WAV's Broadcast Wave `bext` chunk (description, originator, dates, time reference) goes back the same way, which ffmpeg would write with the description, originator and dates blank; only BWF v2 loudness values are left out, since the gain changes them ([#218](https://github.com/M-Igashi/baken/issues/218))
- Only files whose True Peak is **more than 0.05 dB away from the ceiling** are shown and processed
- macOS resource fork files (`._*`) are automatically ignored
