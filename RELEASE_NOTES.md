# Bake'n Deck 4.10.0 - DATE ADDED on the player, no noise from damaged audio

A minor release: `baken-export` structs gain fields, which changes its API (see Library users). `baken-core` has a fix and no API change.

## Highlights

- **CDJ Safe no longer turns damaged audio into noise.** ffmpeg exits normally after decoding a damaged or encrypted AAC stream, so `cdjsafe` and `expressport --cdjsafe` wrote a full-length MP3 of whatever the decoder produced and counted the track as converted. On a real library one such AAC came out as 334 seconds of noise from a 373-second source. A conversion now fails when ffmpeg reports more than ten decoding errors, and the track is named with the reason: `expressport --cdjsafe` leaves it out, standalone `cdjsafe` stops before writing the XML as it does for any failed conversion. A few bad frames in an old rip still pass ([#299](https://github.com/M-Igashi/baken/pull/299)).
- **DATE ADDED on the player is the date you added the track.** Players show and sort the DATE ADDED column from a string in the track row that rekordbox fills with the XML's `DateAdded`; `expressport` wrote the date of the export there, so every track showed the same day. It now writes `DateAdded`, and the track row's `masterContentId` is the XML's `TrackID`, as rekordbox writes it and as OneLibrary already had it. Checked on all 585 tracks of the reference export ([#294](https://github.com/M-Igashi/baken/pull/294), fixes [#284](https://github.com/M-Igashi/baken/issues/284)).
- **Play history a player recorded is no longer lost without a word.** Players record what was played in the stick's `export.pdb`, and rekordbox imports it from there. `expressport` replaces `export.pdb` whole, so the plan now reads the history on the stick and says what would be lost before anything is written. Carrying the history over is still open in [#286](https://github.com/M-Igashi/baken/issues/286) ([#297](https://github.com/M-Igashi/baken/pull/297)).

## Other changes

- `--prune` no longer deletes the stick's copy of a track that was skipped because its source was missing (a drive not mounted, a file moved). Such a skip now holds prune back, as a failure does, and the report counts it ([#296](https://github.com/M-Igashi/baken/pull/296), refs [#291](https://github.com/M-Igashi/baken/issues/291)).
- `--generate-analysis` never writes a zero column into the two small preview waveforms; it uses rekordbox's own minimum, which Nexus-era players need to treat the preview as finished ([#295](https://github.com/M-Igashi/baken/pull/295), fixes [#285](https://github.com/M-Igashi/baken/issues/285)).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: no API change.
- `baken-export`: new public fields, so a struct literal of these types needs them: `Skipped::source_missing`, `Plan::player_history` (`Option<pdb::PlayerHistory>`), `Report::sources_missing`, and in `pdb::rows::TrackRow` `master_content_id`, `date_created`, and `date_added`. New module `pdb::read` with `PlayerHistory { lists, entries }` and `PlayerHistory::read(&[u8])`.
