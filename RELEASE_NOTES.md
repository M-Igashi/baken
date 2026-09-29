# Bake'n Deck 4.3.1 - `expressport` puts hot cues D to H where rekordbox does

A patch release for `baken expressport` ([#205](https://github.com/M-Igashi/baken/issues/205), [#206](https://github.com/M-Igashi/baken/pull/206)). `headroom`, `rbsort` and `cdjsafe` are unchanged.

## Fixes

- **Hot cues D to H were missing on the player.** rekordbox splits hot cues over two of the analysis files it writes for each track: A to C go into the `.DAT` file, D to H into the `.EXT` file. `expressport` wrote all of them into the `.DAT`, and a CDJ-2000NXS2 showed none past C (reported with a Mixxx library in [dimashenme/mixxx2rekordbox#3](https://github.com/dimashenme/mixxx2rekordbox/pull/3)). The cue lists are now split the way rekordbox splits them, with copied and with generated analysis. The one track of our reference export that has a hot cue past C now gets the same cue lists as in rekordbox's own export.
- **Hot cues past H are left out.** rekordbox has eight hot cues, but an XML written by another tool can hold more (Mixxx allows more than eight). They were written as hot cue 9 and up.

A stick written by an earlier version is fixed by running the same export again with 4.3.1: only the analysis files that change are rewritten, and the audio stays where it is.

## Library users

- `baken-export`: no API changes. `anlz::cues::sections` now returns the hot cues D to H in the `.EXT` `PCOB` instead of the `.DAT` one, and drops hot cues numbered above 7.
- `baken-core`: no changes.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
