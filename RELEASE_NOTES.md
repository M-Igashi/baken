# Bake'n Deck 3.7.0 - running `baken headroom` twice stops asking to re-encode what the first run already did

A fix release for one defect, reported by the same tester as 3.6.1 and worth a minor version because it changes which files the tool touches.

## Highlights

- **A processed library stays processed.** A lossy file moves in whole 1.5 dB steps, and a lowering is rounded up so the result can never sit above the ceiling, which leaves every file somewhere inside the step below it. The threshold for offering a re-encode was 1.0 dB, so on the next run about a third of an already-processed library came back listed as "re-encode required for precise gain", and would lose another generation of quality to move by around a decibel. The reporter saw 43 re-encodes on a first pass over 24,000 tracks and 7,700 on the second ([#138](https://github.com/M-Igashi/baken/issues/138)).
- **The threshold is now one full native step**, the only value that cannot feed on its own output, because the leftover after a step is under one step by construction. A lossy file closer than that to the ceiling is left where it is, which is the better trade anyway: a lossy generation is not worth less than 1.5 dB. Analysing a library that has already been through `baken headroom` now reports that there is nothing to do, and that is a property test over every headroom, codec and gain mode rather than a comment.

## Other changes

- `--reencode` and `--no-reencode` still parse, for scripts, but no file is classified as a re-encode for gain any more, so they select nothing. Their help text says so.
- README, `docs/true-peak-ceiling.md` and `docs/dj-software-compatibility.md` describe two tiers for lossy files instead of three.

## Library users

- `headroom::MIN_REENCODE_GAIN` is now `GAIN_STEP` rather than 1.0. As a result `headroom::decide` never returns `GainMethod::Mp3Reencode` or `GainMethod::AacReencode`, and the `reencode` argument to `headroom::select_processable` no longer changes its result.
- Nothing is removed. Both variants, `AudioAnalysis::requires_reencode` and the apply path behind them keep their signatures, so this is not a breaking change; retiring them is [#141](https://github.com/M-Igashi/baken/issues/141), for the next major.
- **If you cache analyses, invalidate them on this upgrade.** The measurement is unchanged, but the decision for any lossy file whose True Peak sits within one step of the ceiling is not, so a cached `Decision` or `AudioAnalysis` from 3.6.x can still propose a re-encode. A cached `Measurement` is fine: re-run `decide` on it.

## Upgrading

No action needed, and nothing you have already processed is damaged. If you had been re-running `baken headroom` over the same library and accepting the re-encodes, those files took one lossy generation per run; this release is where that stops.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
