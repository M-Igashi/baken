# Bake'n Deck 4.1.1 - `expressport` no longer copies another file's analysis when file names repeat

A patch release for `baken expressport` ([#180](https://github.com/M-Igashi/baken/issues/180), [#181](https://github.com/M-Igashi/baken/pull/181)). `headroom`, `rbsort` and `cdjsafe` are unchanged, and `expressport` stays a beta ([#116](https://github.com/M-Igashi/baken/issues/116)).

## Fix

rekordbox 7 keeps only the file name in its local analysis files, so `expressport` finds a track's analysis by file name and tells same-named files apart by the beat grid. Three gaps in that check could put another file's beat grid, waveforms and phrase data on the stick:

- A grid starting **later** than the XML's passed the first-beat check. On the reference library, a track kept as two copies and analysed separately at 126 BPM, one grid at 0.000 s and the other at 0.475 s, got the 0.475 s analysis for both copies: on the player every downbeat sat one beat off.
- A **single** same-named analysis file was taken without any check, including a stale one left by a track removed from the collection.
- When **nothing** matched, the first file found was used.

Now every candidate has to agree with the XML's first beat-grid entry (`TEMPO`): first beat within 2 ms either way and the same BPM, with the length breaking a tie. A track without a `TEMPO` only takes an analysis without a grid. Anything else counts as no rekordbox analysis: the track is skipped with a reason that says the analysis found does not match the XML's beat grid, or its analysis is generated under `--generate-analysis`. Replayed over the reference library (1,413 analysis files, 1,401 tracks with a candidate), exactly one pick changes, the track above, and no match is lost.

## Library users

- `baken-export`: `AnlzIndex::find` applies the check above and returns `None` where it used to fall back; `AnlzIndex::has_name` is new. Source-compatible with 4.1.0.
- `baken-core`: no changes.

## Upgrading

Run `baken expressport` again onto sticks written by 4.1.0 or earlier if your library has files with the same name in different folders. Only analysis files that change are rewritten, and audio already on the stick is not copied again. A track the plan now skips with "does not match the XML's beat grid" had its grid changed in rekordbox after the XML was exported: export the XML again.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
