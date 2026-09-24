# Bake'n Deck 4.0.0 - `baken headroom` never re-encodes a file, and the code now says so

A major release because `baken-core` loses public items ([#141](https://github.com/M-Igashi/baken/issues/141), [#151](https://github.com/M-Igashi/baken/pull/151)). For command line users the behaviour is the one 3.7.0 and 3.8.0 already had: every file gets the same decision it got in 3.8.0, and a script written for 3.x keeps running. `expressport` is unchanged and still a beta ([#116](https://github.com/M-Igashi/baken/issues/116)).

## Highlights

- **The lossy re-encode path is gone** ([#141](https://github.com/M-Igashi/baken/issues/141), [#151](https://github.com/M-Igashi/baken/pull/151)). Since 3.7.0 ([#138](https://github.com/M-Igashi/baken/issues/138)) the gain decision has had a floor of one full native step for MP3 and AAC, and a raise of a step or more is applied natively by definition, so no file could be classified for a re-encode any more. What remained was a path that could not run: the "re-encode required for precise gain" prompt, its lines in the summary, its group in the report and the apply code behind them. All of it is removed. The product statement is now the simple one: MP3 and AAC move in native 1.5 dB steps through mp3rgain, lossless formats move exactly through ffmpeg, and nothing is ever re-encoded to change its gain.
- **`--reencode` and `--no-reencode` are still accepted and ignored**, and no longer listed in `--help`, so a 3.x script that passes either keeps working. Drop them when convenient. In the CSV report the `Method` column simply never says `re-encode`; no value changes meaning.
- **`expressport` is the 3.8.0 code, still in beta.** The beta banner and the "beta" in the README come off in a patch release once [#116](https://github.com/M-Igashi/baken/issues/116) reports a pass on a CDJ-3000 and a CDJ-2000NXS2. The two were decoupled on purpose: this major does not wait on hardware time, and the beta removal does not wait on a major.

## Other changes

- The release binaries build with symphonia 0.6.1 and mp3rgain 3.8.1 ([#150](https://github.com/M-Igashi/baken/pull/150)), the versions the Mac app has been decoding with since 2026-09-21, so both builds of the same core use the same decoder. Lockfile only, the crate requirements did not change.

## Library users

- `baken-core` 4.0.0 is a breaking release. Removed: `GainMethod::Mp3Reencode`, `GainMethod::AacReencode`, `AudioAnalysis::requires_reencode()`, `headroom::MIN_REENCODE_GAIN`, and `AnalysisSummary::mp3_reencode_count`, `aac_reencode_count` and `total_reencode()`. `select_processable(analyses, lossless)` loses its third parameter. The #138 rationale that lived on `MIN_REENCODE_GAIN` is now a comment in `decide_gain`. `measure`, `decide`, `analyze`, `apply` and `Measurement` are unchanged.
- `baken-export`: no API change, the version moves with the workspace.
- The Mac app (`baken-ffi`) pins `baken-core = "3.8.0"` and is unaffected until it moves to 4.x; what to change when it does is listed in [#151](https://github.com/M-Igashi/baken/pull/151).

## Upgrading

No action needed. `baken headroom` proposes and applies the same gain to every file as 3.8.0 did, and `--reencode` / `--no-reencode` still parse. `cdjsafe` transcodes as before: that is a different feature, and this release does not touch it.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
