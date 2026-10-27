# Bake'n Deck 4.8.0 - mp3rgain 3.14.0, OneLibrary read on an XDJ-AZ

A minor release built on mp3rgain 3.14.0. Neither library crate changes its API.

## Highlights

- **Silent MP3 frames stay silent when `baken headroom` raises a file.** Encoders write a `global_gain` of 0 for silent frames, and some leave audio data behind it that a raised gain would make audible. Up to 4.7.0 a raise moved those frames up with the rest; mp3rgain 3.14.0 leaves a 0 where it is, as mp3gain does ([M-Igashi/mp3rgain#417](https://github.com/M-Igashi/mp3rgain/issues/417)). Every other frame moves by the same step count as before, and lowering, the measurement, and AAC are unchanged.
- **OneLibrary has been read on a player.** An XDJ-AZ (firmware 2.00) read a stick written by 4.7.0 with `--onelibrary --artwork` like a rekordbox stick: playlists in order, title, artist, BPM and key, search, waveforms, beat grid, hot and memory cues, and artwork. `--help` and the README now say "checked on an XDJ-AZ so far" instead of "untested" ([#139](https://github.com/M-Igashi/baken/issues/139), [#274](https://github.com/M-Igashi/baken/pull/274)). The CDJ-3000X, OPUS-QUAD, and OMNIS-DUO read the same database but have not been tried yet; reports are welcome in #139.

## Other changes

- The README links the Auto Gain, loudness normalization, and USB export guides on the website ([#273](https://github.com/M-Igashi/baken/pull/273)).
- The Mac tune-up guide (English and Japanese) has two workarounds for rekordbox windows that float above every other app, such as the dialogs of a USB export: hide rekordbox during the export (it keeps its speed), or give it a desktop of its own ([#258](https://github.com/M-Igashi/baken/issues/258), [#272](https://github.com/M-Igashi/baken/pull/272)).
- The copyright line in `LICENSE` and the crates' author field now read Jesse Pinkman (JPHFA), the name on the GitHub account. The licence is still MIT.
- Dependencies: mp3rgain 3.14.0, unicode-normalization 0.1.26, libc 0.2.191 ([#271](https://github.com/M-Igashi/baken/pull/271)).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: no API change. It depends on mp3rgain 3.14.0, so `headroom::apply` leaves MP3 frames with a `global_gain` of 0 unchanged on a raise.
- `baken-export`: no API change.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
