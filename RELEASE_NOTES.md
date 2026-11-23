# Bake'n Deck 4.15.0 - cdjsafe MP3s on the beat, and rekordbox's own colour waveforms

A minor release for `baken cdjsafe` and `baken expressport`. MP3s made by `cdjsafe` now get cues and beat grids that land on the beat, `expressport --prune` on Windows no longer deletes the library it just wrote, and `--generate-analysis` computes the colour and three-band waveforms the way rekordbox does. Both library crates change their API (see Library users).

**If you imported MP3s made by `baken cdjsafe` into rekordbox, read "Upgrading from 4.14.0 or earlier" below.**

## Highlights

- **`cdjsafe` MP3s play on their cues and beat grid.** rekordbox decodes an MP3 without cutting the encoder delay its LAME header states, and plays the `Info` frame at the start of a CBR file as a frame of silence. So an MP3 made by `cdjsafe` plays 51 ms later than its lossless source (25 ms later when the source is itself a CBR MP3), while it was given the source's cues and beat grid unmoved: every cue and every beat sat 51 ms ahead of the sound. This was measured against the waveforms rekordbox 7 wrote for 254 MP3s of a real library. No sample of the audio is cut; the timing moves instead. `baken cdjsafe` writes each `TEMPO` and `POSITION_MARK` of a converted track that much later, and `expressport --cdjsafe` moves the beat grid, the cues, and the scrolling waveforms on the stick (eight columns of silence in front, 2 ms coarser than the shift). An MP3 that is copied because it already is 320 kbps CBR keeps its times. That the players use rekordbox's timeline is inferred (every MP3 rekordbox analyses would be off on them otherwise), not measured on one ([#382](https://github.com/M-Igashi/baken/pull/382), fixes [#374](https://github.com/M-Igashi/baken/issues/374)).
- **`expressport --prune` on Windows keeps the new library.** Since 4.12.0 it compared the paths to keep with the paths on the stick as text. On Windows one side came out as `E:\Contents/Artist/Album/x.mp3` and the other as `E:\Contents\Artist\Album\x.mp3`, so nothing matched and `--prune` removed every file the new `export.pdb` points to. Paths are now compared component by component, and the Windows CI job runs the prune tests. macOS and Linux were not affected ([#381](https://github.com/M-Igashi/baken/pull/381), fixes [#373](https://github.com/M-Igashi/baken/issues/373)).
- **`--generate-analysis` computes the colour and three-band waveforms the way rekordbox does.** Only the blue scrolling waveform was rekordbox's own before; the colour waveform, the three-band waveforms and their gains, and the first three bytes of the colour overview were approximations. They now follow the description of rekordbox's arithmetic that the rbxport project published, kept only where it reproduces rekordbox's own files: on 40 tracks in five formats, the colour and three-band scrolling waveforms are identical to rekordbox's on 99% of columns or more (the one AAC file's colour heights on 96%, all within 1), and the three gains are equal on 39 of the 40. The blue and colour overviews stay approximations. It takes longer: 35 tracks in 16.1 s instead of 7.6 s. A generated MP3 waveform now also sits under the XML's grid; up to 4.14.0 it was 25 to 51 ms early. Tracks with a rekordbox analysis are still copied as before. Not seen on a player yet ([#385](https://github.com/M-Igashi/baken/pull/385), fixes [#377](https://github.com/M-Igashi/baken/issues/377); [#382](https://github.com/M-Igashi/baken/pull/382)).

## Other changes

- **Memory cue colours from a marker in the name.** rekordbox's XML gives a hot cue's colour but not a memory cue's (checked with rekordbox 7.2.20). A memory cue or memory loop whose name starts with `[pink]`, `[red]`, `[orange]`, `[yellow]`, `[green]`, `[aqua]`, `[blue]`, or `[purple]`, in any case, now gets that colour on the stick, as in `[red] Drop`. The marker is left out of the comment the player shows and goes together with `[active]` in either order (`[active][blue] Build`). On a hot cue the marker is removed and changes nothing, since its colour comes from the XML. The plan says how many memory cues get a colour. The value written (the colour's row in the stick's colour table) follows published descriptions of the format and a player report elsewhere; no rekordbox export of a coloured memory cue was at hand to confirm it, and it has not been seen on a player ([#386](https://github.com/M-Igashi/baken/pull/386), closes [#379](https://github.com/M-Igashi/baken/issues/379)).
- **FLAC seek tables in generated analysis.** A FLAC with `--generate-analysis` gets the seek table (`PVB2`) rekordbox writes for it, unless its header gives no sample count (a file piped out of ffmpeg), for which rekordbox writes none either. A copied analysis without the table gets one when the file now allows it. A CDJ-2000NXS2 was reported to load FLACs without the table slowly ([#383](https://github.com/M-Igashi/baken/pull/383), fixes [#375](https://github.com/M-Igashi/baken/issues/375)).
- **Long runs keep the computer awake.** While `expressport`, `cdjsafe`, or `headroom` works, a Mac (`caffeinate -i`, tied to baken's process) or a Windows PC (`SetThreadExecutionState`) no longer idles into sleep and stops a stick mid-write. The display may still turn off, and a closed lid or Sleep from the menu still works. On Linux, run a long export under `systemd-inhibit --what=idle:sleep` ([#384](https://github.com/M-Igashi/baken/pull/384), fixes [#376](https://github.com/M-Igashi/baken/issues/376)).
- Dependencies: clap 4.6.8 ([#371](https://github.com/M-Igashi/baken/pull/371)), rusqlite 0.43.0 ([#372](https://github.com/M-Igashi/baken/pull/372)).

## Upgrading from 4.14.0 or earlier

- **MP3s made by `baken cdjsafe` up to 4.14.0** play 51 ms later than the cues and beat grid they were given (25 ms when the source was a CBR MP3). MP3s that were copied, not converted, are fine. To fix the ones in rekordbox: run `baken cdjsafe` again, import the new XML in rekordbox, and run **Analyze Track** on the new files, because rekordbox keeps the waveform of the old file.
- **`expressport --cdjsafe`** needs nothing from you: the next run converts the MP3s an earlier version put on the stick once more by itself, so that run takes as long as a first export.
- **`expressport --prune` on Windows** removed the audio, analysis, and artwork files of the library it had just written, from 4.12.0 to 4.14.0. Export again with 4.15.0 to put them back.

## Upgrading from 4.11.0 or earlier

4.12.0 made `baken headroom` and `baken expressport` exit with 1 when a file or track failed, removes ReplayGain tags from files whose gain changed, and never replaces a backup. If you skipped it, read "Upgrading from 4.11.0" in the [4.12.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.12.0).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`:
  - `cdjsafe::transcode` returns `Result<f64>` instead of `Result<()>`: the seconds the MP3 plays later than its source, by which its cues and grid have to move. The value is also stored in the MP3 as an ID3 `TXXX` frame `BAKEN_CDJSAFE` (in samples), and the new `cdjsafe::shift_of(path) -> Option<f64>` reads it back (`None` for an MP3 made by 4.14.0 or earlier).
  - `decode::decode_with` decodes an MP3 on rekordbox's timeline: the encoder and decoder delay is kept, and a CBR file's `Info` frame comes out as 1152 samples of silence. The signature is unchanged. `headroom`'s loudness and true peak of MP3 files do not move.
- `baken-export`:
  - `collection::Cue` gains `colour: Option<u8>` (1 to 8, from a name marker). Code that builds `Cue` with a struct literal needs the new field, or `..Default::default()`.
  - New: `collection::MEMORY_CUE_COLOURS`, `collection::Track::delayed(seconds)`, `Plan::coloured_memory_cues()`, `anlz::rewrite::delay(file, kind, seconds)`, and `anlz::generate::measure_delayed(path, delay)`.
