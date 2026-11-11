# `baken cdjsafe` reference

Pre-NXS2 CDJs (CDJ-2000NXS, CDJ-2000, CDJ-900NXS, CDJ-850, …) have inconsistent or absent support for anything that isn't MP3: FLAC needs an NXS2 (2016+), and ALAC, AIFF, WAV, and AAC fail on specific firmware combinations, sometimes mid-set. `baken cdjsafe` is the emergency-backup path: it takes a gig playlist (or several, or every playlist in the XML) and produces a USB-ready set of files that **will play on any CDJ**, with your cues and beatgrid intact.

```bash
baken cdjsafe ~/Music/rekordbox/collection.xml \
  --playlist "Sets/Friday" \
  --out-dir ~/Music/cdjsafe-friday
```

Repeat `--playlist` for several playlists, or leave it out to take every playlist in the XML. Added in v3.0.0; the design discussion is in [#40](https://github.com/M-Igashi/baken/issues/40).

To put the MP3s straight onto a stick instead of importing them into rekordbox, use [`baken expressport --cdjsafe`](expressport.md#cdj-safe-on-the-way---cdjsafe).

## What it does

1. Reads the chosen playlists from your exported `collection.xml`: the ones given with `--playlist`, or every TrackID-referenced playlist when it is left out, except the ones an earlier run wrote (the `CDJ-safe (MP3)` folder and playlists named `…-CDJ-safe`, in case you imported them back into rekordbox). A track in several playlists is converted once ([#255](https://github.com/M-Igashi/baken/issues/255)).
2. Converts every track whose file exists to the CDJ-safe profile. Tracks whose files are missing on disk, or whose `Location` is not a decodable `file://` URL (a hand-edited row), are listed as skipped with the reason and left out (the emergency stick still gets everything that is there); only a run with nothing reachable at all is an error. The profile is **320 kbps CBR MP3 @ 44.1 kHz**, ID3v2.3 tags, artwork kept (JPEG, capped at 500×500):

   | Source | Action |
   |---|---|
   | FLAC, WAV, AIFF, ALAC | Re-encode |
   | AAC/M4A (any bitrate) | Re-encode |
   | MP3 not exactly 320 kbps CBR @ 44.1 kHz | Re-encode (lossy→lossy, reported) |
   | MP3 already 320 kbps CBR @ 44.1 kHz | **Byte-identical copy** (no generation loss, LAME header untouched) |

3. Emits an updated XML (default: `<input>-out.xml`) where each converted track is a **new entry with a fresh TrackID** that inherits the source's beatgrid (`TEMPO`) and hot/memory cues (`POSITION_MARK`) **verbatim**. Each playlist comes back as `<playlist>-CDJ-safe` in its own folders under a `CDJ-safe (MP3)` folder, so `Sets/Friday` becomes `CDJ-safe (MP3)/Sets/Friday-CDJ-safe`; a playlist none of whose tracks could be reached is left out. The `-CDJ-safe` suffix keeps an imported playlist from colliding with the original. New entries get a `[cdjsafe]` marker appended to their Comments so they're distinguishable after import.
4. Reports every lossy→lossy re-encode so you can refresh those tracks from lossless masters before the next gig.

If any track fails to convert, **no XML is written**: a partial USB defeats the point. The XML is written through a temp file and a rename, so an interrupted run never leaves a truncated file either. The same applies if the collection XML changed while the transcodes ran (a rekordbox export made mid-run): the MP3s already converted are kept and the command tells you to re-run, which only copies them.

## Importing back into rekordbox

1. *Preferences > Advanced > Database > rekordbox xml > Imported Library* → select the output XML, restart rekordbox.
2. Open the `rekordbox xml` sidebar tree → `CDJ-safe (MP3)`.
3. Right-click the imported tracks → **Import to Collection**. Cues and beatgrid come with them, with no re-analysis needed.
4. Export the playlists to USB in EXPORT mode as usual.

## Checking playlists first (`--check`)

```sh
baken cdjsafe ~/Music/rekordbox/collection.xml --playlist "Sets/Friday" --check
```

Probes every track and prints, per track, what a pre-NXS2 player (CDJ-2000NXS, CDJ-900NXS), a CDJ-2000NXS2 and a CDJ-3000 will do with it: ✓ within the formats the player's operating instructions list, ✗ outside them (with the reason, such as FLAC on a pre-NXS2 player, a 32-bit float WAV, or a 192 kHz file), and ? where the manual says nothing (mono or multichannel files, `WAVE_FORMAT_EXTENSIBLE` WAVs, AIFF-C, VBR MP3 without a Xing header). A summary line per player follows, such as "3 of 42 tracks will not play on a pre-NXS2 player". Nothing is written and `--out-dir` is not needed. With more than one playlist (leave `--playlist` out to check every playlist) the per-track table is left out: only the tracks that need a look are listed, each with its format, and the summary still counts every track. The exit code is 1 when a player refuses a track or a track is missing or unreadable, so a script can run it before an export.

`--player` checks against other players instead of those three, and can be given more than once: `--player xdj-az --player cdj-3000x`, or `--player all` for every table at once. Known are the pre-NXS2 class (also `CDJ-2000NXS` or `CDJ-900NXS`), CDJ-2000NXS2, CDJ-3000, CDJ-3000X, XDJ-1000MK2, XDJ-XZ, XDJ-RX3, XDJ-AZ, OPUS-QUAD and OMNIS-DUO ([#220](https://github.com/M-Igashi/baken/issues/220)). The exit code then follows the players asked for. The CDJ-3000X, XDJ-AZ and OPUS-QUAD list exactly what the CDJ-3000 lists; the OMNIS-DUO and the XDJ-1000MK2 stop at 48 kHz for lossless files; the XDJ-XZ (FLAC from firmware 1.10) and the XDJ-RX3 also stop at 48 kHz, play MP3 and AAC down to 32 kHz, and list no Apple Lossless. The CDJ-3000X, XDJ-AZ, OPUS-QUAD and OMNIS-DUO need OneLibrary, which `baken expressport` writes only with `--onelibrary`.

The tables come from Pioneer's own operating instructions only, never from another tool's table; each one is cited in `crates/baken-core/src/cdjsafe/matrix.rs`. The CDJ-2000NXS2, for instance, lists WAV, AIFF, Apple Lossless and FLAC at up to 96 kHz from USB, while the CDJ-3000 lists MP3 and AAC at 44.1 and 48 kHz only. Every manual also says that some files do not play even in a supported format, so ✓ means "within the list", not a guarantee. `--check` only reports: a conversion still converts every track.

MP3 and AAC tracks are also decoded to see where their audio stops. An encoder low-passes what its bitrate cannot pay for (LAME at about 17 kHz for 128 kbps, 19.5 kHz for 256 and 20 kHz for 320), and re-encoding at a higher bitrate keeps the lower cutoff, so a "320" made from a 128 kbps file has nothing above 17 kHz. A track of 256 kbps or more with nothing above 19 kHz, or of 192 kbps or more with nothing above 17 kHz, is listed with the measured frequency as likely transcoded from a lower bitrate ([#222](https://github.com/M-Igashi/baken/issues/222)). That is a measurement, not a verdict: some masters have little top end, so listen before deciding. It never changes the exit code, and nothing is skipped or converted because of it. Decoding costs about half a second of CPU per lossy track, spread over all cores: a 1374-track playlist with 533 MP3 and AAC files took 35 s instead of 8.

## Usage

```
baken cdjsafe <XML> [--playlist <PATH>]... --out-dir <DIR> [-o <PATH>]
baken cdjsafe <XML> [--playlist <PATH>]... --check [--player <MODEL>]...
```

| Argument / Flag | Description |
|------|-------------|
| `<XML>` | Exported rekordbox XML (required) |
| `--playlist <PATH>` | Playlist to convert or check; repeat for several. Top-level: just the name; nested: `Folder/Playlist`. Omitted: every playlist (see above) |
| `--out-dir <DIR>` | Directory for the MP3 files (required unless `--check`; created if missing) |
| `--output <PATH>` (`-o`) | Output XML path. Defaults to `<input-stem>-out.<ext>` next to the input |
| `--check` | Write nothing; report what each class of player does with every track (see above) |

## Notes

- The output profile is locked (320 kbps CBR / 44.1 kHz / ID3v2.3): it's the only combination that plays reliably across the whole CDJ fleet, and CBR sidesteps rekordbox's VBR cue-offset problem.
- ffmpeg writes a valid Xing/LAME header, so rekordbox compensates the LAME encoder delay and cues stay sample-aligned.
- Filenames are FAT32/exFAT-sanitized; collisions get a numeric suffix.
- Requires ffmpeg (with `libmp3lame`; `soxr` resampling is used when available).
