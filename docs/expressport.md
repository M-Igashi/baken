# `baken expressport` reference

`baken expressport` writes the USB stick itself, straight from your exported `collection.xml`: the device library (`export.pdb`), the analysis files (`PIONEER/USBANLZ`), the audio under `Contents/`, and your CDJ/DJM My Settings. No rekordbox launch, no re-import, no waiting for analysis. rekordbox's own database is never read. The USB Export page in Bake'n Deck for Mac (1.2.0 and later) runs this same export in a window.

```bash
baken expressport ~/Music/rekordbox/collection.xml --device /Volumes/MYUSB --playlist "Sets/Friday" --playlist "Sets/Warmup"
```

Added in v3.5.0 as a beta, out of beta since v4.3.0. The output is byte-checked against real rekordbox exports (design record: [#115](https://github.com/M-Igashi/baken/issues/115)), and testers have played sticks written by `baken` on a CDJ-3000 (firmware 3.22) and on CDJ-2000NXS2 players (1.85 and 1.87): playlists, beat grid, waveforms, cues, keys, search, and My Settings, with copied and with generated analysis ([#116](https://github.com/M-Igashi/baken/issues/116), where further player reports are welcome). As with any stick, check it on a player before a gig.

The guide covers how it works, FAT32 and MBR, which players, and what rekordbox 7's library inconsistency warning means: [Export rekordbox playlists to a USB stick without opening rekordbox](https://baken.ravers.workers.dev/usb-export) ([日本語](https://baken.ravers.workers.dev/ja/usb-export)).

## Before the first export

- Analyse every track in rekordbox once, and export the collection (*File > Export Collection in xml format*).
- Open *Preferences > DJ System > My Settings* in EXPORT mode once, so rekordbox writes the settings files, or pass `--no-settings`.
- Use a stick of its own, formatted as FAT32 with an MBR partition table.
- Quit rekordbox, or eject the stick in it.
- Run with `--dry-run` first: it resolves everything and reports, and writes nothing.

Each point is explained below.

## Usage

```
baken expressport <XML> --device <DIR> [--playlist <PATH>]... [--anlz-dir <DIR>]... [--settings-dir <DIR> | --no-settings] [--device-name <NAME>] [--generate-analysis] [--cdjsafe] [--onelibrary] [--artwork] [--my-tags <FILE>] [--prune] [--dry-run]
```

| Flag | Description |
|------|-------------|
| `--device <DIR>` | Root of the stick (required) |
| `--playlist <PATH>` | Playlist to export; repeatable. Omitted: every TrackID-referenced playlist |
| `--anlz-dir <DIR>` | rekordbox analysis directory when auto-detection fails |
| `--settings-dir <DIR>` | Directory holding the `*SETTING.DAT` files and, if there is one, `djprofile.nxs` (default: rekordbox's own) |
| `--no-settings` | Write no My Settings and no `djprofile.nxs`; the player keeps its own settings |
| `--device-name <NAME>` | Name shown on the player (default: the stick's directory name) |
| `--generate-analysis` | Compute the analysis files from the audio for tracks with no rekordbox analysis (grid and cues still from the XML; no phrase data) |
| `--cdjsafe` | 320 kbps CBR MP3 for every track (needs ffmpeg) |
| `--onelibrary` | Also write OneLibrary (`exportLibrary.db`) for the CDJ-3000X, XDJ-AZ, OPUS-QUAD and OMNIS-DUO; checked on an XDJ-AZ so far |
| `--artwork` | Put the picture embedded in each audio file on the stick as its artwork |
| `--my-tags <FILE>` | My Tags from a JSON file beside the XML, see [My Tags](#my-tags---my-tags) |
| `--prune` | Delete audio and analysis on the stick this export no longer references |
| `--dry-run` | Resolve and report, write nothing |

## What goes onto the stick

### Track information, keys, and cues

Title, artist, BPM, key, colour, rating, playlists, beat grid and cues all come from the XML. The key is shown on the player the way the XML spells it, as on a stick rekordbox exports: Classic `Am` stays `Am`, Alphanumeric `8A` stays `8A` (up to 4.1.1 a Classic key was left blank). Hot cues keep the colour set in rekordbox: each of its 16 hot cue colours goes onto the stick with the colour code and colour bytes rekordbox itself writes for it ([#236](https://github.com/M-Igashi/baken/issues/236)); up to 4.4.0 every colour but green went out as code 0.

### Active loops

Active loops (a memory loop the player starts by itself when playback reaches it) have no field in the XML, so a memory loop whose name starts with `[active]` (in any case) becomes the track's active loop, as in `[active] Build`; the marker is left out of the comment the player shows ([#210](https://github.com/M-Igashi/baken/issues/210)). A player takes one active loop per track: when several memory loops are marked, the earliest is used and the plan says so. rekordbox's own XML does not say which loop is active, so a loop set active in rekordbox needs the marker in its comment as well.

### Waveforms and analysis

Waveforms are copied, not computed: every track should have been analysed in rekordbox once, and its `.DAT`/`.EXT`/`.2EX` files are copied from rekordbox's local analysis cache (`~/Library/Pioneer/rekordbox/share/PIONEER/USBANLZ`, `%APPDATA%\Pioneer\rekordbox\share\PIONEER\USBANLZ` on Windows, or `PIONEER/Master/share/PIONEER/USBANLZ` on a library drive). A library drive is looked for on every attached volume: on a Mac under `/Volumes`; on Linux under `/media`, `/mnt`, and `/run/media/$USER`; on Windows at the root of every local drive letter, removable or not (up to 4.8.0 Windows looked only in `%APPDATA%`). Network and optical drives are not searched, so a library there needs `--anlz-dir`. On the way the path is rewritten, the cue sections are filled from the XML and the phrase analysis is masked exactly as rekordbox does at export time. Tracks with no analysis are listed and left out. A FLAC's seek table in that analysis points at byte offsets in the file rekordbox analysed, so `expressport` checks it against the file and, when the file has changed since (a `baken headroom` run re-encodes FLAC), rebuilds it from the file the way rekordbox writes it; the plan lists those tracks ([#219](https://github.com/M-Igashi/baken/issues/219)).

### Tracks rekordbox never analysed (`--generate-analysis`)

`--generate-analysis` computes the analysis files from the audio instead, for tracks rekordbox has never seen (a Mixxx or Traktor library converted to `collection.xml`, or a machine without rekordbox). The beat grid and the cues come from the XML as always, so a track whose XML has no `TEMPO` gets no beat grid (the plan says how many; the player then detects the BPM while playing, and quantize and beat sync cannot use it); the waveforms are decoded and measured by `baken` ([#147](https://github.com/M-Igashi/baken/issues/147)). The scrolling waveform reproduces rekordbox's own to the pixel on the tracks it was checked against; the colour, three-band and preview waveforms are close approximations. Phrase analysis cannot be generated and is left out. Tracks that do have a rekordbox analysis are still copied, because copying is exact.

### My Settings

The My Settings files `MYSETTING.DAT`, `MYSETTING2.DAT` and `DJMMYSETTING.DAT` are copied from rekordbox's settings directory into `PIONEER/` on the stick (not its root; that is where rekordbox puts them too) and are **required** unless `--no-settings` is given, which writes none so the player keeps its own settings (what a DJ without rekordbox usually wants from a club's CDJs). rekordbox writes them once Preferences > DJ System > My Settings has been opened, a page that only exists in EXPORT mode. `DEVSETTING.DAT` is copied too when it is there; players write their own, and a real rekordbox 7 export does not always carry it. `djprofile.nxs`, which carries the name of your rekordbox DJ profile and which rekordbox puts on its sticks, is copied the same way; no player manual says what a player does with it ([#234](https://github.com/M-Igashi/baken/issues/234)). `--no-settings` leaves both out. A settings file a player saved onto the stick (its header names the player, where rekordbox's says `rekordbox`) is left as it is, so what you set on the CDJ survives the next export; the plan lists it.

### CDJ Safe on the way (`--cdjsafe`)

`--cdjsafe` transcodes every track to 320 kbps CBR MP3 on the way and ships it with the source track's analysis, for pre-NXS2 players.

### Artwork (`--artwork`)

Artwork comes with `--artwork`, from the picture embedded in each audio file: ID3 in MP3 and AIFF, the picture block of FLAC, the cover of M4A (not WAV), JPEG, PNG or BMP. The XML carries none, and rekordbox keeps its own thumbnails in its database. Every track with a picture gets the four files rekordbox writes in `PIONEER/Artwork`, 80 and 240 pixels square, the picture fitted on black, made like rekordbox's own (measured on 1091 of them). On the owner's library 24 of the 35 tracks of a playlist had a picture: 96 files, 856 KB, and no measurable time on a local disk. On a FAT32 stick through macOS each file costs 50 to 70 ms beyond its size (the per-file costs measured in [#196](https://github.com/M-Igashi/baken/issues/196) and [#221](https://github.com/M-Igashi/baken/issues/221)), so the first run with it takes about a quarter of a second more per track; later runs leave unchanged files alone, and `--prune` removes the ones no track uses ([#235](https://github.com/M-Igashi/baken/issues/235)). A track keeps its artwork id from one run to the next, so a new track or a re-sorted playlist does not rewrite the other tracks' files ([#290](https://github.com/M-Igashi/baken/issues/290)).

### My Tags (`--my-tags`)

rekordbox's XML carries no My Tags, so they come from a small file next to it ([#281](https://github.com/M-Igashi/baken/issues/281)):

```
baken expressport collection.xml --device /Volumes/MYUSB --my-tags mytags.json
```

```json
{
  "columns": [
    {"name": "Genre", "tags": ["House", "Disco"]},
    {"name": "Components", "tags": ["Vocal"]},
    {"name": "Situation", "tags": ["Peak Time"]},
    {"name": "Untitled Column", "tags": []}
  ],
  "tracks": {"12": ["House", "Vocal"], "40": ["Peak Time"]}
}
```

- `columns` are rekordbox's four My Tag columns in order, each with its tags in the order the player lists them. A file with fewer than four gets rekordbox's names for the rest (Genre, Components, Situation, Untitled Column).
- `tracks` maps the XML's `TrackID` to the names of that track's tags. Tracks the export does not take are ignored, so one file can cover a whole library. Since tracks name their tags, a tag name may appear in one column only.
- The file is checked before anything is written: a track naming a tag that no column lists, a tag listed twice, more than four columns, or a name longer than 100 characters stops the plan with the reason. The plan shows how many of the exported tracks carry tags.
- The tags go into `exportExt.pdb`, the file next to `export.pdb` where rekordbox keeps My Tags for the Device Library, and with `--onelibrary` into OneLibrary as well. Both follow rekordbox's layout: fed the My Tags rekordbox put on the owner's stick (30 tags on 132 of its 582 tracks), they come back on the same tracks, and the tag pages match rekordbox's byte for byte. Not checked on a player yet.
- Without `--my-tags` the stick gets no `exportExt.pdb`, and OneLibrary carries rekordbox's four columns without tags.

### Device Library and OneLibrary

The Device Library (`export.pdb`) is what the CDJ-3000, CDJ-2000NXS2, XDJ-XZ and older read. Players that need OneLibrary (`exportLibrary.db`: CDJ-3000X, XDJ-AZ, OPUS-QUAD, OMNIS-DUO) get it with `--onelibrary`. An XDJ-AZ (firmware 2.00) read a stick written by 4.7.0 with playlists in order, title, artist, BPM and key, search, waveforms, beat grid, hot and memory cues and artwork all in place ([#139](https://github.com/M-Igashi/baken/issues/139)); the CDJ-3000X, OPUS-QUAD and OMNIS-DUO read the same database but have not been tried yet. It lists the same tracks under the same ids as `export.pdb` and is encrypted the way rekordbox's is. Like rekordbox's own, it leaves cues and waveforms to the analysis files, and with `--artwork` it names each track's picture. My Tags come with `--my-tags`; hot cue banks are not in it, since the XML does not carry them.

## Running it again

### What is written again

Audio already on the stick stays when it still matches its source, so re-running after a playlist change is quick: a copied file when its size and 64 KiB at its start, middle and end match the source, a `--cdjsafe` MP3 when it is whole and was written at least an hour after the source last changed (the hour absorbs the clock change, which shifts every time on a FAT stick). Anything else is written again, so a re-export after `baken headroom`, which changes MP3 and AAC files in place at the same size, carries the new audio ([#231](https://github.com/M-Igashi/baken/issues/231)). `export.pdb` is rebuilt every run.

### What the stick keeps from its last export

The stick keeps two things from its last export: the play history a player recorded on it, and the settings rekordbox writes into every export from its preferences (colour names, the browse columns, category and sort lists, and the Device Library background colour). The XML carries neither, so `expressport` reads them from the `export.pdb` on the stick and writes them into the new one ([#286](https://github.com/M-Igashi/baken/issues/286), [#289](https://github.com/M-Igashi/baken/issues/289)); a stick without one gets rekordbox's defaults. The plan says what it kept. Plays of tracks the export does not put on the stick are left out and counted, so import the history in rekordbox before an export that drops tracks if you want them.

### Free space and interrupted runs

Before writing anything, `expressport` adds up what the run will write (audio not on the stick yet, the analysis files, `export.pdb` and the settings, in the stick's allocation units, plus 1 MiB) and stops when the stick has less free; the plan and `--dry-run` show both figures (macOS and Linux). If the stick still fills up, is pulled out or stops answering during the run, the export stops at that track and leaves `export.pdb` as it was, so the stick keeps its previous library. `export.pdb` and the settings are written next to the old files and renamed over them. `--prune` removes nothing in a run where a track failed, so a track that could not be written keeps the files an earlier export left for it ([#233](https://github.com/M-Igashi/baken/issues/233)).

A run in which a track failed ends with exit code 1, after the summary. When every track fails (the drive with the music went away after the plan, say), nothing is written and the stick keeps its previous library ([#321](https://github.com/M-Igashi/baken/issues/321)).

## The stick

### Format: FAT32 with an MBR partition table

Format the stick as **FAT32 with an MBR partition table**. A CDJ-2000NXS2 or older reads neither exFAT nor a GPT disk and does not show such a stick at all; a CDJ-3000 also reads exFAT. The players' manuals also list FAT16 and HFS+. `expressport` reads the stick's filesystem and partition table before writing and warns when they rule out a player (macOS and Linux; on macOS the partition table comes from `diskutil`, on Linux from `lsblk`), and on macOS also when an HFS+ stick is case-sensitive, which AlphaTheta says might not be recognised.

On Linux it also reads the partition type: `mkfs.vfat` on a partition that `fdisk` or `parted` made keeps type 83 (Linux), and two XDJ-700s showed FORMAT USB for such a stick until its type was set to `c`, W95 FAT32 LBA (`sudo fdisk /dev/sdX`, then `t`, `c`, `w`) ([#288](https://github.com/M-Igashi/baken/issues/288)). macOS does not mount FAT on such a partition at all.

### HFS+ on a Mac

HFS+ (Disk Utility: Mac OS Extended (Journaled), scheme Master Boot Record) writes faster, and the players' manuals list it next to FAT16 and FAT32 ([#221](https://github.com/M-Igashi/baken/issues/221)). macOS writes HFS+ itself, while FAT32 goes through FSKit on macOS 26. On a SanDisk USB 3.2 stick, a 62-track, 3.3 GB playlist took 260 s on HFS+ against 423 to 438 s on FAT32, and a 527-track, 7 GB one 469 to 567 s against 1324 to 1362 s, with a quarter to a half of the writes reaching the stick and the same files on both. The directory and the three analysis files each track needs cost about 450 ms on FAT32 and almost nothing on HFS+. A CDJ-3000 owner measured 75 tracks on one 250 GB SanDisk USB 3.2 drive: 23.6 s on HFS+ against 219.6 s on FAT32 ([#287](https://github.com/M-Igashi/baken/issues/287)). On HFS+ the library goes into the hidden `.PIONEER` folder, the only place a CDJ-3000 looks on such a stick (see below). HFS+ also keeps file metadata itself, so no `._` files are written.

Its limits: Windows cannot read or write it without extra software; the XDJ-R1, the first XDJ-RX and the CDJ-400 list FAT16 and FAT32 only (every other player manual checked lists HFS+); a case-sensitive one might not be recognised; and HFS+ stores accented and kana names decomposed (NFD) while `export.pdb` holds them composed (NFC), as on a stick rekordbox itself exports to HFS+. FAT32 stays the choice for a stick that also has to work on Windows or on those three players.

On an HFS+ stick rekordbox keeps its library in the hidden folder `.PIONEER` instead of `PIONEER` (the audio stays in `Contents`; checked with rekordbox 7.2.18), and a CDJ-3000 (firmware 3.22) reads an HFS+ stick's library only from there: rekordbox's own HFS+ stick with `.PIONEER` renamed to `PIONEER` showed "rekordbox database not found", and so did one written by `expressport` 4.10.0 or older, which always used `PIONEER`. `expressport` therefore writes `.PIONEER` on HFS+, and on an HFS+ stick an older version wrote it renames `PIONEER` to `.PIONEER` before writing, so the audio and analysis already there are kept. On other filesystems it writes into whichever of the two already holds the stick's library, and into `PIONEER` on a stick with none. A stick with a library in both stops the plan until one is moved off ([#287](https://github.com/M-Igashi/baken/issues/287)). Finder shows `.PIONEER` with Cmd-Shift-.

### Use a stick of its own

Use a stick dedicated to `expressport`; it is not meant to be layered over a stick rekordbox wrote. A stick rekordbox exported to loses its OneLibrary (`exportLibrary.db`, with `-wal` and `-shm`) and `exportExt.pdb` when `expressport` writes it, since they would go on describing rekordbox's library instead of the one just written: rekordbox reports that as a library inconsistency, and OneLibrary players would show the old library ([#208](https://github.com/M-Igashi/baken/issues/208)). The plan (and `--dry-run`) lists them before anything is written. With `--onelibrary` the database is replaced by the one just written instead, and with `--my-tags` so is `exportExt.pdb`; the rest go. Files players leave behind (`PIONEER/CDJ`, `RBFLTR.DAT`, ...) are never touched.

### Check it on a player, not in rekordbox

Check a finished stick on a player, not by opening it in rekordbox: rekordbox rewrites a device library it opens, and even changes a stick that is only mounted while it runs. The plan warns when rekordbox is running (macOS and Windows), so quit it or eject the stick in it before exporting ([#292](https://github.com/M-Igashi/baken/issues/292)). [rbsync](https://github.com/aquarazorda/rbsync) measured it deleting 768 of 5,825 playlist entries from a stick it opened and adding an `exportLibrary.db`. If that happened, run `expressport` again: it rewrites `export.pdb` and removes the `exportLibrary.db`, or replaces it with `--onelibrary`.

### File names on macOS 26

On macOS 26, which mounts ExFAT and FAT sticks through FSKit, the files on the stick keep the NFC names rekordbox puts in `export.pdb`, and Terminal's `rm` cannot remove files under such names ([#154](https://github.com/M-Igashi/baken/issues/154)); Finder and `--prune` can.

## How long it takes

An export runs at the stick's write speed, plus about 0.4 s per track for the directory and the three analysis files the format wants next to each audio file (measured on macOS 26 with a FAT32 stick: a new directory 150 ms, a file create 27 ms, a write and close 25 ms). A USB 3 stick that writes 25 MB/s needs about 40 s per GB, and many sticks slow down once warm: a 62-track, 3.3 GB playlist took 339 s on a cold stick and about 450 s later, and `cp -R` of the same files took 410 to 533 s. `--cdjsafe` adds the encoding, a few seconds per track ([#197](https://github.com/M-Igashi/baken/issues/197)). On Linux, check the mount options: udisks2 mounts FAT sticks with `flush`, which sends every closed file to the device at once, and cheap sticks are slow at that ([#173](https://github.com/M-Igashi/baken/issues/173)).

## Requirements

A rekordbox install is normally required, on the same machine or on an attached library drive: the waveforms and the My Settings files are copied from rekordbox's own. A `collection.xml` produced by another tool (Mixxx, Traktor, a converter) needs `--generate-analysis` for the waveforms (see [#145](https://github.com/M-Igashi/baken/issues/145) and [#147](https://github.com/M-Igashi/baken/issues/147)) and either `--settings-dir` pointing at the `*SETTING.DAT` files from any stick rekordbox once exported, or `--no-settings`; nothing in the XML can stand in for the settings.

Built into the binaries; `cargo install baken --no-default-features` leaves it out.
