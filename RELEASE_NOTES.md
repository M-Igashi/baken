# Bake'n Deck 4.15.1 - mp3rgain 3.17.1

A patch release. `baken headroom` takes mp3rgain 3.17.1, which no longer crashes on M4A files with broken size fields. What baken writes is unchanged: the gains, the sticks, and the XML are the same as with 4.15.0.

## Changes

- **mp3rgain 3.17.1** (from 3.16.2). An M4A whose metadata item declares a 64-bit size near the maximum, or whose sample offset is near it, crashed mp3rgain where `baken headroom` uses it: the AAC gain, and reading and removing the ReplayGain tags of an AAC file. The bounds checks no longer wrap. By mp3rgain's notes the analysis and the applied gains are the same as in 3.16.2, byte for byte. The rest of mp3rgain [3.17.0](https://github.com/M-Igashi/mp3rgain/releases/tag/v3.17.0) and [3.17.1](https://github.com/M-Igashi/mp3rgain/releases/tag/v3.17.1) (`--audio-hash`, the new mp3rgui, a faster undo) is not used by baken ([#389](https://github.com/M-Igashi/baken/pull/389)).
- **My Settings saved on a player: checked on a CDJ-2000NXS2.** Since 4.5.0 `expressport` keeps a settings file on the stick whose header names a player instead of rekordbox, so that what you saved on the player survives the next export. Until now that header had been seen on one file only. MY SETTINGS > SAVE on a CDJ-2000NXS2 (firmware 1.87) rewrites `MYSETTING.DAT` and `DEVSETTING.DAT` with `CDJ-2000NXS2` and `1.87` in the header and leaves `MYSETTING2.DAT` and `DJMMYSETTING.DAT` as rekordbox wrote them. The next export kept the two and said so: "Kept as a player saved them on the stick: MYSETTING.DAT (CDJ-2000NXS2 1.87), DEVSETTING.DAT (CDJ-2000NXS2 1.87)". No change in behaviour; the saved files are now a test ([#388](https://github.com/M-Igashi/baken/pull/388), closes [#234](https://github.com/M-Igashi/baken/issues/234)).

## Upgrading from 4.14.0 or earlier

4.15.0 moved the cues and beat grid of MP3s made by `cdjsafe` onto the beat and fixed `expressport --prune` on Windows; the [4.15.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.15.0) have the details.

- **MP3s made by `baken cdjsafe` up to 4.14.0** play 51 ms later than the cues and beat grid they were given (25 ms when the source was a CBR MP3). MP3s that were copied, not converted, are fine. To fix the ones in rekordbox: run `baken cdjsafe` again, import the new XML in rekordbox, and run **Analyze Track** on the new files, because rekordbox keeps the waveform of the old file.
- **`expressport --cdjsafe`** needs nothing from you: the next run converts the MP3s an earlier version put on the stick once more by itself, so that run takes as long as a first export.
- **`expressport --prune` on Windows** removed the audio, analysis, and artwork files of the library it had just written, from 4.12.0 to 4.14.0. Export again with 4.15.0 or later to put them back.

## Upgrading from 4.11.0 or earlier

4.12.0 made `baken headroom` and `baken expressport` exit with 1 when a file or track failed, removes ReplayGain tags from files whose gain changed, and never replaces a backup. If you skipped it, read "Upgrading from 4.11.0" in the [4.12.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.12.0).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

No API change in `baken-core` or `baken-export` since 4.15.0. 4.15.0 itself changed both; see "Library users" in the [4.15.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.15.0).
