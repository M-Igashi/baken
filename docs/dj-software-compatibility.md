# Using Bake'n Deck alongside other DJ software

A recurring support question, first asked on 2026-09-19 by a Mac App Store buyer who prepares in rekordbox, plays in Traktor, syncs the two with Lexicon, and occasionally plays CDJs: *if I run Headroom over my library, will the tracks still be fine in my other DJ app?*

The answer is yes, with one caveat that is not about Bake'n Deck at all. This file holds the ground truth, a reusable support reply, and the website FAQ entries, so the next person who asks gets the same answer.

The same buyer came back on 2026-09-21, after running Headroom over his collection and re-analysing it in Traktor: Traktor's Autogain is *still* a different value on every track, and the backup folder could not be found again. Both answers are below too, since both are the obvious next questions once the run has happened.

## What actually changes on disk

Headroom rewrites the audio file in place. It never renames a file, never moves it, never changes its duration and never shifts the audio by a sample.

| Format | How the gain is applied | File size | Sample count | Metadata |
|---|---|---|---|---|
| MP3 | `global_gain` rewritten in the frame headers, 1.5 dB steps (`mp3rgain`, `undo: false` so no APEv2 tag is appended) | Unchanged, byte for byte apart from the gain bits | Unchanged | Untouched, the container is never opened |
| AAC (`.m4a`) | Native gain in 1.5 dB steps | Unchanged | Unchanged | Untouched |
| FLAC, WAV, AIFF, ALAC | ffmpeg volume filter, exact gain, then tmp file plus rename | Changes | Unchanged | Vorbis comments survive ffmpeg; ID3v2 `GEOB`/`PRIV`, the WAV `id3 ` chunk and MP4 `----` atoms are lifted off the source and written back byte for byte (`baken-core` 3.4.0, `src/headroom/tags.rs`) |
| MP3/AAC within one 1.5 dB step of the ceiling | Left untouched | Unchanged | Unchanged | Untouched |

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

## The follow-up: re-analysing does not make the values match

Headroom aligns **true peak**, not loudness, and the name invites the other assumption, so this needs saying plainly. After a run every track sits at the same ceiling (-0.5 dBTP by default), which is what keeps one track from clipping on the deck and another from wasting 6 dB of headroom. Two files at the same peak can still be several dB apart in loudness: a compressed master is loud the whole way through, a dynamic one only reaches that peak on transients. Traktor's Autogain measures loudness, so it goes on setting a different gain per track. What is left after a re-analysis is the difference in peak to loudness ratio between the masters, not a leftover of the old levels.

Two smaller sources of spread, worth mentioning before someone measures and finds the peaks are not identical either:

- MP3 and AAC move in whole 1.5 dB `global_gain` steps, so they land within one step below the ceiling rather than exactly on it.
- A lossy file already inside that step is left untouched ([#138](https://github.com/M-Igashi/baken/issues/138)), so it keeps the peak it had.

The check that settles the question: analyse the processed folder again in the Mac app. The True Peak column reads the target on every row while the LUFS column stays spread over several dB, and that spread is exactly what Traktor is reacting to.

Baking equal loudness in instead would mean turning the loud masters down by the difference, because the dynamic ones cannot come up without clipping. That is a limiter-free tool giving away level on a player that has no auto gain at all, so it is not what the tool does. `--tp-target` moves the ceiling for every file; it does not change this.

Which leaves the same two options as before the run, now chosen on purpose rather than as a fix:

- **Autogain on**: loudness matched inside Traktor, computed from the new levels, working normally.
- **Autogain off**: what is in the file, which is what a CDJ plays off the USB stick.

## Where the Mac app puts its backups

Next to the music, never in Application Support or any app-private location. Each apply creates `<common parent of the processed tracks>/backup/<yyyyMMdd-HHmmss>/`, one per volume when a playlist spans volumes. In Folder mode that parent is the folder that was handed to the app; in Playlist mode it is the deepest folder every track of that run shares, which can sit further up the tree than the user expects. The originals keep their relative structure inside, so copying them back by hand in Finder works as well as Restore does.

`backup/.baken-backup` is the marker the core writes into every backup root, which makes it the fastest way to find them all:

```sh
find ~/Music /Volumes -maxdepth 8 -name .baken-backup 2>/dev/null
```

The backup bar in the app only tracks the runs of the current session, so after a relaunch it is gone and "Restore from Folder…" is the way back: it takes the timestamped folder inside `backup`, and refuses a folder whose parent is not a `backup` directory carrying the marker.

## Re-encoding, which no longer happens

Nothing is re-encoded for gain any more, in the command line tool or in the Mac app. A lossy file that would need a raise smaller than one 1.5 dB step is left where it is ([#138](https://github.com/M-Igashi/baken/issues/138)). A re-encode rewrites the whole file and can add a few milliseconds of encoder padding, which was the only path in either tool where a beatgrid could drift, and that path is now closed in both.

The Mac app used to offer those files behind an opt-in checkbox, unchecked by default. It is gone as of 1.0.2, released 2026-09-21, because there is nothing left for it to offer.

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
> 2. If you see a re-encode option, leave it off. The command line tool no longer re-encodes anything for gain: a file that would need a raise of less than one 1.5 dB step is simply left where it is. The Mac app had an opt-in checkbox for it, unchecked by default, and from 1.0.2 that is gone too. So there is nothing to switch off: a re-encode rewrites the whole file and can add a few milliseconds of encoder padding, which was the one case where a grid could drift, and neither tool does it any more.
>
> My suggestion: run it on one playlist first, open those tracks in Traktor and check a couple of cue points before doing the whole library. Every run is backed up into a timestamped folder anyway, and Restore puts the originals back in one click.

## Support reply template: after the run

For the follow-up questions, once Headroom has been run and re-analysed.

> **Why Traktor's Autogain still varies.** That is expected, and it is the one thing Headroom deliberately does not do. Headroom aligns true peak, not loudness: after a run every track sits at the same ceiling (-0.5 dBTP by default), so nothing clips and nothing wastes headroom, and since there is no limiter anywhere in the chain the dynamics are untouched.
>
> Traktor's Autogain measures perceived loudness, which is a different quantity. Two tracks can share the same peak and still be several dB apart in loudness: a heavily compressed master is loud all the way through, while a dynamic master only reaches that peak on transients. So after re-analysis Traktor correctly finds a different loudness per track and sets a different gain. What you are seeing is the difference in dynamic range between your masters, not a leftover from before the run.
>
> You can see the same thing inside the app: analyse the processed folder again and the True Peak column reads the target for everything, while the LUFS column is still spread over several dB. (One detail: MP3 and AAC gain moves in fixed 1.5 dB steps, since that is what keeps it lossless, so those land within one step of the ceiling rather than exactly on it.)
>
> That leaves you a choice, and both answers are valid. Leave Autogain on if you want every track to sound equally loud inside Traktor, since it is now working from the new levels and doing its job properly. Or switch it off (Preferences, Mixer) if you want to hear exactly what is in the files, which is also what a CDJ playing a USB stick does, because it has no auto gain of any kind. Filling that gap is why Headroom exists.
>
> Baking equal loudness into the files instead would mean turning the loud masters several dB down, because the dynamic ones cannot come up any further without clipping. That throws away level on the deck, so the tool aims at the ceiling and leaves loudness matching to the mixer.
>
> **Where the backups are.** Next to your music, never inside the app's own storage. Each run writes to `<the common parent folder of the tracks you processed>/backup/<yyyyMMdd-HHmmss>/`, so a Folder mode run on `~/Music/DJ` leaves `~/Music/DJ/backup/20260919-174512/`. In Playlist mode the base is the deepest folder that all tracks in that run share, which can sit higher up than you would expect, and a playlist spanning two drives produces one `backup` folder per drive. Inside, the original files keep their folder structure, so you can copy them back by hand if you ever want to.
>
> If you would rather not hunt for it, this lists every one of them: `find ~/Music /Volumes -maxdepth 8 -name .baken-backup 2>/dev/null`. `.baken-backup` is a small marker file the app writes in each `backup` folder, which is how Restore knows the folder is really ours, so the folder containing each hit is what you are after.
>
> And after a relaunch, when the backup bar is gone, use "Restore from Folder…" in Headroom: pick the timestamped folder inside `backup` and it restores that run.

## Website FAQ entries

Live on the Auto Gain page, in both languages, as the last five entries of its FAQ: three about other DJ software since 2026-09-19, and two more since 2026-09-21 (why Autogain still differs after a re-analysis, and where the Mac app writes its backups).

- <https://baken.ravers.workers.dev/auto-gain#faq>
- <https://baken.ravers.workers.dev/ja/auto-gain#faq>

Link one of those from a support reply instead of retyping the answer. The source is the `faq` array on the `/auto-gain` page in `M-Igashi/web-backup`, `headroom/src/index.js`; the visible HTML and the `FAQPage` JSON-LD are both generated from it, so a new `[question, answer]` tuple is the whole change. The front page carries its own hardcoded `FAQPage` block with no visible counterpart, which is why nothing was added there.

## Sources

- `baken-core` 3.7.0: `src/headroom/processor.rs` (`apply_gain_native`, `apply_gain_ffmpeg`), `src/headroom/analyzer.rs` (`MIN_REENCODE_GAIN`, now one full native step), `src/headroom/tags.rs`
- `mp3rgain` 3.8.1: `src/gain.rs`, `GainOptions::new` defaults to `undo: false`, so no APEv2 tag is appended and the file length is unchanged
- `M-Igashi/baken-mac`, `docs/gui-features.md`, for the backup and restore behaviour and the read-only refusal added in 1.0.2
- `M-Igashi/baken-mac`, `BakenDeck/Views/HeadroomView.swift` (`applyGainNow`, `restoreFromFolder`) and `docs/sandbox.md`, for the backup path, the per-volume split and the `.baken-backup` marker check
- `docs/true-peak-ceiling.md`, for why the ceiling is a true peak target rather than a loudness target
