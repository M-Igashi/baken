# Bake'n Deck (`baken`)

[![crates.io](https://img.shields.io/crates/v/baken)](https://crates.io/crates/baken)
[![Downloads](https://img.shields.io/github/downloads/M-Igashi/baken/total)](https://github.com/M-Igashi/baken/releases)
[![License: MIT](https://img.shields.io/github/license/M-Igashi/baken)](https://github.com/M-Igashi/baken/blob/main/LICENSE)
![Platforms](https://img.shields.io/badge/platform-macOS%20%7C%20Windows%20%7C%20Linux-lightgrey)

<a href="https://baken.ravers.workers.dev/get?from=baken-readme"><img src="https://toolbox.marketingtools.apple.com/api/v2/badges/download-on-the-mac-app-store/black/en-us" alt="Download on the Mac App Store" height="48"></a>

**What you prep in rekordbox is what plays on the deck.**

rekordbox does three things in software that never survive the trip to a CDJ: Auto Gain, a key then BPM order, and tolerance for file formats. Bake'n Deck bakes each one into the files themselves and writes the USB stick, and your library stays in rekordbox.

| Tool | The gap | What it does |
|---|---|---|
| [**Headroom**](#headroom-baken-headroom)<br>`baken headroom` | Auto Gain is never written into the file | Gain only: no limiter, no compression, dynamics untouched. Every track lands on one true peak ceiling, -0.5 dBTP, and cues stay linked |
| [**Sort**](#sort-baken-rbsort)<br>`baken rbsort` | A key then BPM order cannot be stored | Alphanumeric key 1A→12B, then BPM, written into the playlist itself. The CDJ plays the set in that order |
| [**CDJ Safe**](#cdj-safe-baken-cdjsafe)<br>`baken cdjsafe` | Older CDJs only trust MP3 | A whole playlist as 320 kbps CBR MP3, cues and beatgrids carried over. The emergency stick that plays in any booth |
| [**USB Export**](#usb-export-baken-expressport)<br>`baken expressport` | Writing the stick means opening rekordbox | The stick for CDJ and XDJ players, written straight from the collection XML with rekordbox's own waveforms, beat grids, and phrases |

Two editions, one engine:

- **The `baken` command line**: free, MIT-licensed, on macOS, Windows, and Linux, and it always will be.
- **[Bake'n Deck for Mac](#baken-deck-for-mac)**: the same engine in a native app on the Mac App Store.

🌐 **[baken.ravers.workers.dev](https://baken.ravers.workers.dev)** has the guides, the comparison with other tools, and the FAQ.

## Install

| Platform | Command |
|----------|---------|
| **macOS (Homebrew)** | `brew install M-Igashi/tap/baken` |
| **Windows (winget)** | `winget install M-Igashi.baken` |
| **Arch Linux (AUR)** | `yay -S baken-bin` |
| **Cargo** | `cargo install baken` |

The command line needs ffmpeg (`baken rbsort` does not). Homebrew, winget, and the AUR install it for you; with Cargo or a binary from the [Releases](https://github.com/M-Igashi/baken/releases) page, install ffmpeg yourself. The Mac app bundles its own.

> [!NOTE]
> **Renamed from `headroom` at v3.0.0** ([#60](https://github.com/M-Igashi/baken/issues/60)). The old `headroom` packages no longer receive updates, so reinstall as `baken`. The loudness tool is now `baken headroom`.

## Quick start

Export your collection once from rekordbox (*File > Export Collection in xml format*), then:

```bash
baken headroom ~/Music/DJ-Tracks --backup        # measure, then bake loudness gain, originals kept in backup/
baken rbsort collection.xml                      # sort every playlist by key, then BPM
baken cdjsafe collection.xml --playlist "Sets/Friday" --out-dir ~/Music/cdjsafe
baken expressport collection.xml --device /Volumes/MYUSB --playlist "Sets/Friday"
```

`baken <subcommand> --help` lists every flag.

Given a path, `baken headroom` rewrites the files at once, without asking. Add `--analyze-only` to measure and change nothing, or run `baken headroom` with no arguments inside the folder for the guided mode, which asks before it writes.

## The four tools

### Headroom (`baken headroom`)

rekordbox's Auto Gain lives in rekordbox's library, so a player reading your USB stick never sees it. Headroom measures loudness (LUFS) and true peak for every track, then writes one gain change into each file so the whole library sits at one true peak ceiling, -0.5 dBTP by default. Quiet tracks come up, loud ones come down, and nothing else is touched.

- **Gain only.** No limiter, no compression, no EQ.
- **MP3 and AAC are never re-encoded.** The gain goes into the format's own gain field, in 1.5 dB steps, with the audio bit for bit as it was.
- **Cues and beatgrids stay linked.** Files are rewritten in place with their tags, after an optional backup.

```bash
baken headroom --analyze-only ~/Music/DJ-Tracks      # report only, change nothing
baken headroom ~/Music/DJ-Tracks --backup            # apply, keeping the originals in a backup folder
```

Reference: [docs/headroom.md](https://github.com/M-Igashi/baken/blob/main/docs/headroom.md). Guides: [why Auto Gain does not reach your CDJ](https://baken.ravers.workers.dev/auto-gain), [a LUFS target or a true peak ceiling?](https://baken.ravers.workers.dev/loudness-normalization)

### Sort (`baken rbsort`)

rekordbox sorts by key or by BPM, one column at a time, and a CDJ plays a playlist in the order it was stored. Sort rewrites the track order inside every playlist of your exported `collection.xml`: Alphanumeric key 1A → 12B, ascending BPM inside each key. Names and folders stay as they are. Point rekordbox's Imported Library at the file once, and the `rekordbox xml` tree becomes a sorted mirror of your playlists.

```bash
baken rbsort ~/Music/rekordbox/collection.xml
baken rbsort ~/Music/rekordbox/collection.xml --playlist "Sets/Friday"
```

Reference: [docs/rbsort.md](https://github.com/M-Igashi/baken/blob/main/docs/rbsort.md). Guide: [how to sort a rekordbox playlist by key and BPM](https://baken.ravers.workers.dev/sort)

### CDJ Safe (`baken cdjsafe`)

Players older than the NXS2 are only dependable with MP3, and the failure shows up in the booth. CDJ Safe converts a playlist to 320 kbps CBR MP3 at 44.1 kHz and writes a matching playlist into a new XML, where each track inherits the source's beatgrid and cues verbatim. `--check` writes nothing and reports what each player will do with every track.

```bash
baken cdjsafe collection.xml --playlist "Sets/Friday" --check
baken cdjsafe collection.xml --playlist "Sets/Friday" --out-dir ~/Music/cdjsafe-friday
```

Reference: [docs/cdjsafe.md](https://github.com/M-Igashi/baken/blob/main/docs/cdjsafe.md)

### USB Export (`baken expressport`)

A player reads a stick through a handful of files: the Device Library (`export.pdb`), the analysis files for each track, the audio, and your My Settings. `baken expressport` writes them straight from `collection.xml`, with no rekordbox launch. The waveforms, beat grid, and phrases are rekordbox's own analysis files, copied.

- A re-run writes only what changed.
- `--cdjsafe` puts every track on the stick as 320 kbps CBR MP3.
- `--onelibrary` also writes OneLibrary for the CDJ-3000X, XDJ-AZ, OPUS-QUAD, and OMNIS-DUO. An XDJ-AZ has read it; the other three have not been tried yet.
- Give it a stick of its own, formatted as FAT32 with an MBR partition table, and check the stick on a player before a gig, as with any stick.

```bash
baken expressport collection.xml --device /Volumes/MYUSB --playlist "Sets/Friday" --dry-run
baken expressport collection.xml --device /Volumes/MYUSB --playlist "Sets/Friday"
```

Reference: [docs/expressport.md](https://github.com/M-Igashi/baken/blob/main/docs/expressport.md). Guides: [export to a USB stick without opening rekordbox](https://baken.ravers.workers.dev/usb-export), [what a USB export touches, measured](https://baken.ravers.workers.dev/usb-export-measured)

## Built for rekordbox, not instead of it

There is no shortage of tools that set out to replace rekordbox. Bake'n Deck is not one of them. rekordbox is where you analyse tracks, set beat grids and cues, build playlists, and keep your library. Bake'n Deck takes on only the few steps rekordbox leaves out of the trip to the CDJ.

- **Your library stays in rekordbox.** Every tool works on the collection XML rekordbox exports, and results go back through rekordbox's own Imported Library. Nothing to migrate, nothing to sync back.
- **rekordbox's database is never touched.** Its `master.db` is never read, decrypted, or written.
- **rekordbox's own analysis is copied, not re-guessed.** Analysis is generated only for tracks rekordbox has never seen, and only when you ask for it.
- **Measured, not guessed.** The stick format is checked byte for byte against real rekordbox 7 exports, and player limits come from Pioneer DJ's operating instructions only. Whatever has not been measured or tried on a player is labelled as such.
- **Tried on real players.** DJs have played sticks written by `baken` on a CDJ-3000, CDJ-2000NXS2 players, a CDJ-2000NXS, and an XDJ-AZ ([#116](https://github.com/M-Igashi/baken/issues/116), [#242](https://github.com/M-Igashi/baken/issues/242), [#139](https://github.com/M-Igashi/baken/issues/139)).
- **Supply chain, handled with care.** Dependencies are weighed before they come in, checked on every pull request, and releases carry checksums. See [SECURITY.md](https://github.com/M-Igashi/baken/blob/main/SECURITY.md#supply-chain).
- **The DJ decides.** A suspect file is flagged, not quietly fixed, and no feature, AI or otherwise, takes over a decision that belongs to the DJ.

## Bake'n Deck for Mac

[**Bake'n Deck for Mac**](https://baken.ravers.workers.dev/get?from=baken-readme) runs the same `baken-core` engine, with the same numbers, in a window.

- **Playlist** takes one rekordbox playlist through Headroom, Sort, and onto the stick in skippable steps, and shows the sorted order before anything is written.
- **Headroom** for a whole folder, with a review table, audio preview, and per-run backups with one-click restore.
- **USB Export** with a pre-flight and a report. It marks the tracks the player you choose would refuse, and converts to 320 kbps MP3 on the way with CDJ Safe.
- **On macOS 27**, a [BPM and key check against rekordbox, and energy and vocal tags on the stick](https://baken.ravers.workers.dev/music-understanding).
- **Mac Tune-up** for the macOS settings that slow rekordbox down.

One-time purchase, no subscription, no account, no network access. English and Japanese, macOS 14 or later, Apple silicon and Intel.

## Documentation

| | |
|---|---|
| [docs/headroom.md](https://github.com/M-Igashi/baken/blob/main/docs/headroom.md) | `baken headroom` reference: formats, 1.5 dB steps, the ceiling, the report, backups |
| [docs/rbsort.md](https://github.com/M-Igashi/baken/blob/main/docs/rbsort.md) | `baken rbsort` reference: the rekordbox workflow, flags, sort rules |
| [docs/cdjsafe.md](https://github.com/M-Igashi/baken/blob/main/docs/cdjsafe.md) | `baken cdjsafe` reference: the conversion, importing back, `--check` and the player tables |
| [docs/expressport.md](https://github.com/M-Igashi/baken/blob/main/docs/expressport.md) | `baken expressport` reference: what is written, flags, My Tags, stick formats, speed |
| [docs/true-peak-ceiling.md](https://github.com/M-Igashi/baken/blob/main/docs/true-peak-ceiling.md) | Why the ceiling is -0.5 dBTP for every file |
| [docs/rbsort-sort-comparison.md](https://github.com/M-Igashi/baken/blob/main/docs/rbsort-sort-comparison.md) | A six-track example: one-column sort against key then BPM |
| [docs/dj-software-compatibility.md](https://github.com/M-Igashi/baken/blob/main/docs/dj-software-compatibility.md) | Headroom next to Traktor, Serato, Engine DJ, and Lexicon |
| [docs/mac-tuneup.md](https://github.com/M-Igashi/baken/blob/main/docs/mac-tuneup.md) | Five macOS settings that slow rekordbox down, with the manual steps ([日本語](https://github.com/M-Igashi/baken/blob/main/docs/mac-tuneup.ja.md)). No tool required |
| [bench/README.md](https://github.com/M-Igashi/baken/blob/main/bench/README.md) | The USB export benchmark against rbxport: script and raw results |

The processing logic is a separate library crate, [`baken-core`](https://crates.io/crates/baken-core), so other front-ends can embed it without the terminal UI. To build from source: `git clone https://github.com/M-Igashi/baken.git && cd baken && cargo build --release`.

## License

MIT

The binary also contains third-party code, among it SQLite (public domain) for `--onelibrary`. Every release archive carries `THIRD_PARTY_LICENSES.md` with their notices and those of all Rust crates in the binary, generated from `about.toml` and `about.hbs`.
