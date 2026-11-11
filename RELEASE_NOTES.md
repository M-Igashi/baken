# Bake'n Deck 4.12.0 - A whole-codebase review: no file lost, no failure reported as success

A minor release: the `baken-export` API gains an error variant and a report field (see Library users). `baken-core` has no API change.

Every fix in this release comes from a review of the whole codebase by Claude Code (Fable 5.2), which filed 25 issues ([#313](https://github.com/M-Igashi/baken/issues/313) to [#337](https://github.com/M-Igashi/baken/issues/337)). Each one was reproduced, or traced through the code where it could not be, before it was fixed.

## Highlights

- **Five ways to lose or damage a file are closed.**
  - `baken headroom` given one file twice, as in `baken headroom . *.mp3` or a relative and an absolute path, applied an MP3 or AAC gain twice. A file is now processed once, whatever path reaches it ([#338](https://github.com/M-Igashi/baken/pull/338), fixes [#313](https://github.com/M-Igashi/baken/issues/313)).
  - `baken cdjsafe` with `--out-dir` pointing at a folder that holds source files could delete a source, empty it, or overwrite it with another track's MP3. No output is ever written over a source now: an already compatible `ok.mp3` in that folder is copied to `ok (2).mp3` ([#339](https://github.com/M-Igashi/baken/pull/339), fixes [#314](https://github.com/M-Igashi/baken/issues/314) and [#325](https://github.com/M-Igashi/baken/issues/325)).
  - `baken rbsort` without `--playlist` wrote the tracks of the second of two playlists with the same name in one folder into the first ([#340](https://github.com/M-Igashi/baken/pull/340), fixes [#315](https://github.com/M-Igashi/baken/issues/315)).
  - `expressport --prune` deleted audio the new library uses when a folder or file name differed from the XML only in case, as after an artist tag changed from `Dj Foo` to `DJ Foo` ([#341](https://github.com/M-Igashi/baken/pull/341), fixes [#316](https://github.com/M-Igashi/baken/issues/316)).
  - `baken headroom --backup` replaced the original in the backup folder with the already processed file on a second run, and with inputs that share no parent folder (`.` and `../SetB`) it could write outside the backup folder. A backup is never replaced now: a second copy of a file is saved as `track (2).wav`, and every copy stays inside the backup folder ([#343](https://github.com/M-Igashi/baken/pull/343), fixes [#318](https://github.com/M-Igashi/baken/issues/318)).
- **A failure is reported as one.** `baken headroom` and `baken expressport` ended with exit code 0 even when every file failed. Both now print their summary and exit with 1 when a file or track failed. When every track of an export fails (the drive with the music went away after the plan, say), nothing is written and the stick keeps its previous library instead of getting an empty one. Something that cannot be done after the new library is written, such as a stale file `--prune` may not remove, is listed above the summary instead of ending the run with a bare error ([#349](https://github.com/M-Igashi/baken/pull/349) and [#356](https://github.com/M-Igashi/baken/pull/356), fixes [#320](https://github.com/M-Igashi/baken/issues/320), [#321](https://github.com/M-Igashi/baken/issues/321), and [#322](https://github.com/M-Igashi/baken/issues/322)).
- **`baken headroom` removes the ReplayGain tags of a file whose gain changed.** `REPLAYGAIN_*` and `R128_*` values describe the audio before the gain, so a player that honours them, such as Mixxx or foobar2000, corrected a second time. They are removed from Vorbis comments, ID3v2 `TXXX` frames, an MP3's APEv2 tag, and MP4 free-form items. Every other tag stays, and an MP3's ID3v2 tag keeps its version and its size. rekordbox and CDJs never read these tags ([#359](https://github.com/M-Igashi/baken/pull/359), fixes [#327](https://github.com/M-Igashi/baken/issues/327)).
- **The Linux binaries run on older distributions.** They are built on Ubuntu 22.04 and need glibc 2.35 at most (the x86_64 binary asks for 2.34) instead of 2.39, so they start on Ubuntu 22.04, Debian 12, and RHEL 9. The arm64 binary is built on an arm64 machine instead of being cross-compiled ([#355](https://github.com/M-Igashi/baken/pull/355), fixes [#334](https://github.com/M-Igashi/baken/issues/334)).

## Other changes

- `baken headroom`:
  - `--backup <folder>` given without a path took the folder as the backup location and rewrote the current directory. It now stops and says to put the path first. `--tp-target` accepts only values of 0 dBTP or below, and the guided mode (no arguments) stops at once when there is no terminal to ask in ([#348](https://github.com/M-Igashi/baken/pull/348), fixes [#319](https://github.com/M-Igashi/baken/issues/319)).
  - An MPEG Layer II file named `.mp3` is reported instead of being patched as an MP3, which damaged it without changing its level ([#347](https://github.com/M-Igashi/baken/pull/347), fixes [#329](https://github.com/M-Igashi/baken/issues/329)).
  - A lossless file keeps its permissions after the gain, and a temporary file left by an interrupted run is no longer taken for a track ([#352](https://github.com/M-Igashi/baken/pull/352), fixes [#328](https://github.com/M-Igashi/baken/issues/328)).
  - The README's quick start no longer says a run with a path asks before writing; it writes at once ([#342](https://github.com/M-Igashi/baken/pull/342), fixes [#317](https://github.com/M-Igashi/baken/issues/317)).
- `baken cdjsafe`:
  - An XML an earlier run wrote is brought up to date instead of getting a second `CDJ-safe (MP3)` folder and a second entry for every MP3 ([#358](https://github.com/M-Igashi/baken/pull/358), fixes [#332](https://github.com/M-Igashi/baken/issues/332)).
  - `--check` finds a VBR MP3 without a Xing header behind an ID3v2 tag larger than 512 KiB (a big cover) ([#351](https://github.com/M-Igashi/baken/pull/351), fixes [#331](https://github.com/M-Igashi/baken/issues/331)).
- `baken rbsort` and `baken cdjsafe`: `--playlist` finds a playlist whose own name holds a slash, such as `House/Techno` ([#350](https://github.com/M-Igashi/baken/pull/350), fixes [#326](https://github.com/M-Igashi/baken/issues/326)).
- `baken expressport`:
  - One odd entry anywhere in the XML no longer fails the whole export: a playlist that refers to tracks by location, a `Location` that is not a `file://` URL, an empty cue attribute, or a playlist entry written with a closing tag ([#346](https://github.com/M-Igashi/baken/pull/346), fixes [#323](https://github.com/M-Igashi/baken/issues/323)).
  - `--generate-analysis` no longer hangs on an impossible `TEMPO` (a BPM of 10^12, a start at minus infinity) ([#345](https://github.com/M-Igashi/baken/pull/345), fixes [#324](https://github.com/M-Igashi/baken/issues/324)).
  - The frame count of an MP3 with damage in the middle no longer stops at the damage ([#357](https://github.com/M-Igashi/baken/pull/357), fixes [#333](https://github.com/M-Igashi/baken/issues/333)).
- Seven checks against malformed input, among them an MP4 box with an absurd size, a corrupt `export.pdb` on the stick, a TrackID near the largest number, a My Tag name of emoji, and a NaN sample in generated waveforms ([#344](https://github.com/M-Igashi/baken/pull/344), fixes [#330](https://github.com/M-Igashi/baken/issues/330)).
- Releases: every build uses the committed `Cargo.lock`, every binary is run once before it is packaged, a tag that does not match the version stops the release before anything is published, and a release can be run again after a partial failure. Pull requests now run the whole test suite and clippy on Linux ([#355](https://github.com/M-Igashi/baken/pull/355), fixes [#335](https://github.com/M-Igashi/baken/issues/335) and [#336](https://github.com/M-Igashi/baken/issues/336)).
- Packaging: the README's links work on crates.io, each crate carries the licence text, and `rust-version` is set to 1.91 ([#353](https://github.com/M-Igashi/baken/pull/353) and [#354](https://github.com/M-Igashi/baken/pull/354), fixes [#337](https://github.com/M-Igashi/baken/issues/337)). `docs/headroom.md` says what the update check contacts and how to switch it off.
- The README is shorter and the reference for each subcommand moved to `docs/` ([#312](https://github.com/M-Igashi/baken/pull/312)); the Mac App Store link goes through the website ([#310](https://github.com/M-Igashi/baken/pull/310)).
- Dependencies: mp3rgain 3.16.2, rusqlite 0.42 (SQLite 3.54.1), and every compatible update ([#360](https://github.com/M-Igashi/baken/pull/360)).

## Contributors

- **Claude Code (Fable 5.2)** reviewed the whole codebase and filed the 25 issues this release fixes, [#313](https://github.com/M-Igashi/baken/issues/313) to [#337](https://github.com/M-Igashi/baken/issues/337).

## Upgrading from 4.11.0

- **Scripts:** `baken headroom` and `baken expressport` exit with 1 when a file or track failed. A script that treated exit code 0 as "nothing went wrong" now sees the failures it used to miss.
- **ReplayGain tags:** a file whose gain `baken headroom` changes loses its `REPLAYGAIN_*` and `R128_*` tags. To keep ReplayGain values, analyse the processed files again with your tagger.
- **Backups:** a second backup of a file goes beside the first as `name (2).ext` instead of replacing it.
- **`cdjsafe` with sources in the output folder:** an already compatible MP3 in `--out-dir` is now copied to a numbered name rather than onto itself.
- **`--tp-target`:** a positive value is refused. A ceiling above 0 dBTP turned every lossless file up into clipping.

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- Both crates declare `rust-version = "1.91"`.
- `baken-core`: no API change. Behaviour: `headroom::resolve_inputs` and `headroom::apply` take a file once whatever path reaches it; `headroom::scan_audio_files` skips `<name>.tmp.<ext>`; `headroom::apply` removes ReplayGain tags from the files it changes; `cdjsafe::convert` never writes over a source and brings an XML from an earlier run up to date.
- `baken-export`: new variant `Error::AllTracksFailed { failures }`, returned by `export()` before `export.pdb` is written when every track failed (an exhaustive `match` needs an arm). New field `Report::unfinished: Vec<String>`, listing what could not be done after `export.pdb` was written; such failures no longer make `export()` return an error (code that builds a `Report` with a struct literal needs the field). `collection::Library::parse` keeps an undecodable `Location` as written instead of failing, and reads a playlist `Key` as a TrackID only in a `KeyType="0"` playlist. `anlz::generate::beats` skips a `TEMPO` whose BPM is above 655.35 or whose start is not finite.
