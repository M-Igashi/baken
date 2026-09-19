# Using Bake'n Deck alongside other DJ software

A recurring support question, first asked on 2026-09-19 by a Mac App Store buyer who prepares in rekordbox, plays in Traktor, syncs the two with Lexicon, and occasionally plays CDJs: *if I run Headroom over my library, will the tracks still be fine in my other DJ app?*

The answer is yes, with one caveat that is not about Bake'n Deck at all. This file holds the ground truth, a reusable support reply, and the website FAQ entries, so the next person who asks gets the same answer.

## What actually changes on disk

Headroom rewrites the audio file in place. It never renames a file, never moves it, never changes its duration and never shifts the audio by a sample.

| Format | How the gain is applied | File size | Sample count | Metadata |
|---|---|---|---|---|
| MP3 | `global_gain` rewritten in the frame headers, 1.5 dB steps (`mp3rgain`, `undo: false` so no APEv2 tag is appended) | Unchanged, byte for byte apart from the gain bits | Unchanged | Untouched, the container is never opened |
| AAC (`.m4a`) | Native gain in 1.5 dB steps | Unchanged | Unchanged | Untouched |
| FLAC, WAV, AIFF, ALAC | ffmpeg volume filter, exact gain, then tmp file plus rename | Changes | Unchanged | Vorbis comments survive ffmpeg; ID3v2 `GEOB`/`PRIV`, the WAV `id3 ` chunk and MP4 `----` atoms are lifted off the source and written back byte for byte (`baken-core` 3.4.0, `src/headroom/tags.rs`) |
| MP3/AAC needing a re-encode to reach the ceiling | Full re-encode, opt-in, unchecked by default | Changes | Can change by a few ms of encoder padding | Re-emitted |

Everything a cue point or a beatgrid is anchored to (the file path, the sample position, the duration) is identical before and after. That is why rekordbox, Traktor, Serato and Engine DJ all keep their analysis: there is nothing for them to notice.

## The caveat: the host's own gain value

Every DJ app analyses loudness itself and stores a gain offset per track. After Headroom changes the level in the file, that stored offset describes a level that no longer exists, so the app partly cancels out the correction.

| App | Where the stale value lives | Fix |
|---|---|---|
| Traktor | `collection.nml` (`LOUDNESS PERCEIVED_DB` / `ANALYZED_DB`) | Re-run analysis, or switch Autogain off in Preferences under Mixer |
| Serato | the `Serato Autotags` GEOB frame inside the file, which Bake'n Deck faithfully preserves | Re-analyse the tracks, or turn Auto Gain off |
| rekordbox | its own database | Re-analyse, or turn Auto Gain off |
| CDJ from a USB export | nowhere, the player reads the raw file | Nothing to do. This is the gap Headroom exists to fill |

Two honest framings of the same fix, and users pick by taste:

- **Turn the host's auto-gain off.** Then what you hear is the uniform ceiling baked into the files, which is the point of running Headroom, and it matches what the CDJs will do.
- **Re-analyse.** The app recomputes its gain from the new level and behaves normally. If the app might touch beatgrids during analysis, lock the tracks first (Traktor's padlock, Serato's lock).

## The one option to leave off

The analysis summary has an opt-in checkbox for the handful of MP3/AAC files that could only reach the ceiling by being re-encoded (a raise of 1.0 to 1.5 dB; smaller raises are left alone since `baken-core` 3.3.1). Those rows are unchecked by default. A re-encode rewrites the whole file and can add a few milliseconds of encoder padding, which is the only path in the tool where a beatgrid could drift. For anyone running two DJ apps over one set of files, recommend leaving it off.

## Library sync tools

Lexicon, and every other sync tool, matches tracks by file path. Paths do not change, so a Headroom pass is invisible to them. Order of work does not matter for cues; if the user re-analyses gain in one app afterwards, sync after that so the new value propagates.

## Support reply template

Adjust the greeting, keep the structure.

> Short answer: yes, this is safe for a rekordbox plus Traktor setup. Headroom rewrites the audio file itself, in place. It never renames or moves a file, never changes its duration, and never shifts the audio by a single sample:
>
> - MP3 and AAC: the gain is written into the frame gain field, in the format's native 1.5 dB steps. Nothing is re-encoded and the container is not touched at all.
> - FLAC, WAV, AIFF and Apple Lossless: the gain is applied sample for sample, so the output has exactly the same number of samples as the input. Tags are carried across, including the binary frames DJ software hides in there (ID3 GEOB and PRIV, the free-form atoms in .m4a).
>
> Since nothing a cue point or a beatgrid is anchored to moves, Traktor keeps its analysis and your cues, and so does rekordbox. Lexicon matches on file paths, which don't change either, so your sync keeps working as before.
>
> Two things worth knowing:
>
> 1. Traktor's own Autogain. Traktor stores a gain value per track from its own analysis. For tracks it analysed before the conversion, that value still describes the old level, so Traktor will partly cancel out what Headroom did. Two ways around it: re-run analysis in Traktor afterwards so it picks up the new level (lock the tracks first if you want to be certain Traktor leaves your beatgrids alone), or switch Autogain off in Preferences under Mixer, so what you hear is the level baked into the file. On CDJs from a USB export there is nothing to do: they play what is in the file, which is exactly the gap Headroom fills, since rekordbox Auto Gain never makes it onto the stick.
> 2. Leave the re-encode option off. After an analysis, the summary has an opt-in checkbox for the handful of MP3/AAC files that could only reach the ceiling by being re-encoded. Those rows are unchecked by default and I would keep it that way in your setup: a re-encode rewrites the whole file and can add a few milliseconds of encoder padding, which is the one case where a grid could drift. Everything else is done without re-encoding.
>
> My suggestion: run it on one playlist first, open those tracks in Traktor and check a couple of cue points before doing the whole library. Every run is backed up into a timestamped folder anyway, and Restore puts the originals back in one click.

## Website FAQ entries

Live since 2026-09-19 on the Auto Gain page, in both languages, as the last three entries of its FAQ:

- <https://baken.ravers.workers.dev/auto-gain#faq>
- <https://baken.ravers.workers.dev/ja/auto-gain#faq>

Link one of those from a support reply instead of retyping the answer. The source is the `faq` array on the `/auto-gain` page in `M-Igashi/web-backup`, `headroom/src/index.js`; the visible HTML and the `FAQPage` JSON-LD are both generated from it, so a new `[question, answer]` tuple is the whole change. The front page carries its own hardcoded `FAQPage` block with no visible counterpart, which is why nothing was added there.

## Sources

- `baken-core` 3.6.0: `src/headroom/processor.rs` (`apply_gain_native`, `apply_gain_ffmpeg`, `apply_gain_reencode`), `src/headroom/tags.rs`
- `mp3rgain` 3.8.1: `src/gain.rs`, `GainOptions::new` defaults to `undo: false`, so no APEv2 tag is appended and the file length is unchanged
- `M-Igashi/baken-mac`, `docs/gui-features.md`, for the re-encode opt-in and the backup and restore behaviour
