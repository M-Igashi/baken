# Bake'n Deck 3.6.1 - `baken headroom` refuses files it cannot write, and says what it actually did

A fix release, all of it from one tester report on 3.6.0: read-only tracks cannot be rewritten, and nothing tells you. The first half turned out to be true only on Windows, and the second half was true everywhere.

## Highlights

- **Read-only files are refused, on every platform and in every format.** The lossless path finished with a rename, which on Unix only needs write permission on the *directory*, so macOS replaced read-only FLAC, WAV, AIFF and ALAC anyway and the replacement came out writable: the protection you set was destroyed in passing. MP3 and AAC already failed, and Windows already refused every format. `baken headroom` now checks that it can open a file for writing before it touches it, and refuses with a message naming the reason ([#131](https://github.com/M-Igashi/baken/issues/131)).
- **You hear about them before the run, not after it.** The files are listed and skipped up front instead of failing one at a time at the end, and they are no longer copied into the backup folder for a rewrite that was never going to happen ([#134](https://github.com/M-Igashi/baken/issues/134)).
- **The summary reports the result instead of the plan.** It counted the files it was about to process, so a run where every file failed still finished on a green tick claiming success, with the warnings scrolled off the top. It now counts what landed, says how many did not, and the per-format breakdown describes the same set ([#132](https://github.com/M-Igashi/baken/issues/132)).
- **Failures say why.** `mp3rgain failed to apply MP3 gain` is now `file is read-only or locked: Permission denied (os error 13)`. The cause was in the error chain all along and the CLI was dropping it ([#133](https://github.com/M-Igashi/baken/issues/133)).

## Other changes

- Analysis no longer runs the `loudnorm` fallback for errors a second engine cannot resolve. A file that could not be opened reported ffmpeg's `No loudnorm data found in ffmpeg output` instead of `Permission denied`, and a silent file was decoded twice to reach the answer the first pass already had ([#135](https://github.com/M-Igashi/baken/issues/135)).
- Tag restoration closes its read handle before the rename that replaces the file. Unix does not care, but Windows has to delete the destination to replace it. This is a portability fix and possibly, but not confirmably, the cause of [#137](https://github.com/M-Igashi/baken/issues/137), a Windows report that every MP3 re-encode fails with `Failed to restore metadata`. That report is still open: the error underneath that message is exactly the one the CLI used to drop, so it needs a re-run on this release to identify.
- New document: `docs/dj-software-compatibility.md`, on what Headroom does and does not change when you also play the same files in Traktor, Serato or Engine DJ.

## Library users

- `baken-core` gains `headroom::is_writable(&Path) -> bool` and `headroom::unwritable(&[AudioAnalysis]) -> Vec<PathBuf>`. Both check the file the way the apply will, by opening it for writing and closing it again, which is not the same as `access(W_OK)` on volumes mounted `noowners`. Call `unwritable` on the selected set to put the warning in front of the user before anything runs.
- Behaviour change: `headroom::apply` now fails a file it cannot open for writing, before the backup copy rather than after it. A front end that relied on the old macOS behaviour, where a read-only lossless file was replaced regardless, will now see that file in `ApplyOutcome::failures`. That is the fix, not a regression.
- `Measurement`, `decide`, `analyze`, `ApplyOutcome` and `AnalyzeOutcome` keep their signatures and fields. A `Measurement` cached from 3.6.0 stays comparable; nothing about the measurement changed except which errors are returned directly.

## Upgrading

No action needed. If your library contains read-only files, this is the release that starts telling you so instead of quietly doing the wrong thing with half of them.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
