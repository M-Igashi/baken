# Bake'n Deck 4.13.0 - Failed and skipped tracks a library caller can match and translate

A minor release for library users of `baken-export`: two public types change (see Library users). The `baken` command prints exactly what 4.12.0 printed, and `baken-core` has no API change. If you only use the command line, nothing changes for you.

## Highlights

- **A failed track carries its source path.** `Report::failures` held the track's name and the message only, so a front-end that shows the plan as a list could not tell two tracks with the same title apart and marked both with the first one's message. Each failure is now a `Failure { name, source, message }`, where `source` is the path the progress callback gets for that track ([#364](https://github.com/M-Igashi/baken/pull/364), fixes [#363](https://github.com/M-Igashi/baken/issues/363)).
- **The reason a track was skipped is a type, not English text.** `Skipped::reason` is the new `SkipReason` enum: not in the collection, source file missing (with the path), no rekordbox analysis, an analysis that does not match the XML's beat grid, or text too large for a database row. A caller that translates the reasons matches on the variant instead of reading the sentence by its first words, and a reason added later is a compile error instead of an untranslated row. Printing a `SkipReason` gives the same sentence as before ([#364](https://github.com/M-Igashi/baken/pull/364), fixes [#363](https://github.com/M-Igashi/baken/issues/363)).

## Other changes

- Docs: the README and the pages under `docs/` describe what 4.12.0 changed: `baken headroom` removes ReplayGain values, the Linux binaries need glibc 2.34 or newer, `baken cdjsafe` brings an XML from an earlier run up to date and never writes over a source, and `--playlist` finds a name holding a slash ([#362](https://github.com/M-Igashi/baken/pull/362)).

## Upgrading from 4.11.0 or earlier

4.12.0 made `baken headroom` and `baken expressport` exit with 1 when a file or track failed, removes ReplayGain tags from files whose gain changed, and never replaces a backup. If you skipped it, read "Upgrading from 4.11.0" in the [4.12.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.12.0).

## Upgrading from 4.6.1 or earlier

4.7.0 fixed clicks in AAC files encoded with FDK-AAC and skips HE-AAC. An FDK-AAC file that an earlier version processed keeps its clicks after another run; restore it from a `--backup` copy or its source first. The [4.7.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.7.0) have the details.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-core`: no change.
- `baken-export`:
  - `Report::failures` and `Error::AllTracksFailed { failures }` are `Vec<Failure>` instead of `Vec<(String, String)>`. `Failure` has the public fields `name: String`, `source: PathBuf` (the track's `PlanTrack::source`), and `message: String`. Replace `for (name, message) in &report.failures` with `for f in &report.failures` and read `f.name` and `f.message`.
  - `Skipped::reason` is a `SkipReason` instead of a `String`. The variants are `NotInCollection`, `SourceMissing(PathBuf)`, `NoAnalysis`, `AnalysisMismatch`, and `RowTooLarge { bytes, max }`. `SkipReason` implements `Display` with the sentences 4.12.0 put into the string, so `skipped.reason.to_string()` gives the old value. The enum is not `#[non_exhaustive]`: a later release that adds a reason will say so here. `Skipped::source_missing` is unchanged.
