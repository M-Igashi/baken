# Bake'n Deck 4.15.2 - mp3rgain 3.17.2, without the sha2 crate

A patch release for those who build on the library crates. The `baken` program behaves exactly as 4.15.1 does: the gains, the sticks, and the XML are the same.

## Changes

- **mp3rgain 3.17.2, and no `sha2` in `baken-core`'s dependency tree.** mp3rgain 3.17.0 added an audio hash and an unconditional dependency on `sha2`, so with 4.15.1 everything that links `baken-core` got `sha2`, `digest`, `block-buffer`, `crypto-common`, `generic-array`, and `cpufeatures`, although baken never calls the hash. Bake'n Deck for Mac checks on every release that its dependency tree holds no crypto crate, because its export compliance answer rests on all cryptography running in the operating system's CommonCrypto. mp3rgain [3.17.2](https://github.com/M-Igashi/mp3rgain/releases/tag/v3.17.2) puts the hash behind a feature ([M-Igashi/mp3rgain#505](https://github.com/M-Igashi/mp3rgain/issues/505)) that `baken-core` does not enable, and the six crates are gone again. On Apple targets the tree is free of crypto crates, as it was up to 4.15.0; other targets keep the RustCrypto crates `baken-export` uses for OneLibrary. The code baken calls in mp3rgain is the same as in 3.17.1 ([#393](https://github.com/M-Igashi/baken/pull/393), closes [#392](https://github.com/M-Igashi/baken/issues/392)).
- **`scripts/build-ffmpeg.sh`**, the published build script of the ffmpeg that Bake'n Deck for Mac bundles: `asetpts` and `adelay` join the filter list, since `cdjsafe` has needed them from 4.15.0 on (a stock ffmpeg has both), and two checks at the end no longer misread a pipe closed early. The script is in no crate and no release archive ([#391](https://github.com/M-Igashi/baken/pull/391)).

## From 4.15.1

- mp3rgain 3.17.1: an M4A whose metadata item or sample offset declares a size near the 64-bit maximum no longer crashes `baken headroom` ([#389](https://github.com/M-Igashi/baken/pull/389)).
- My Settings saved on a CDJ-2000NXS2 (firmware 1.87) are kept by `expressport`, checked on the player ([#388](https://github.com/M-Igashi/baken/pull/388), [#234](https://github.com/M-Igashi/baken/issues/234)). The [4.15.1 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.15.1) have the details.

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
