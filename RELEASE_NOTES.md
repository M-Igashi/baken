# Bake'n Deck 4.2.0 - Classic key names, a player check for `cdjsafe`, faster generated analysis

A minor release. `rbsort` and `expressport` read the Classic key notation, `expressport` writes key names as rekordbox does, warns when the stick's format rules out a player and prepares tracks ahead of the stick writes, and `cdjsafe` gains `--check`. `headroom` is unchanged.

`expressport` stays a beta for this release ([#116](https://github.com/M-Igashi/baken/issues/116)). A CDJ-3000 and a CDJ-2000NXS2 passed with 4.1.1 except for the key, which was blank because the XML used Classic names. That is fixed here; once a player shows the keys from a 4.2.0 stick, a patch release takes the beta label off.

## Highlights

- **Classic key notation** ([#189](https://github.com/M-Igashi/baken/pull/189), [#190](https://github.com/M-Igashi/baken/pull/190)). rekordbox writes `Tonality` in whichever key display format is set (Preferences > View > Key display format): Alphanumeric (`8A`) or Classic (`Am`, `F#m`, `Db`). Up to 4.1.1 only Alphanumeric was understood, so `rbsort` sorted a Classic export by BPM alone and `expressport` left the key blank on the player. Both now read either format, sharp or flat spelling. `expressport` also writes the key names the way the XML spells them, as rekordbox does on its own sticks, so a Classic export shows `Cm` on the player and an Alphanumeric one `5A`. You no longer have to switch rekordbox to Alphanumeric before exporting the XML.
- **`baken cdjsafe --check`** ([#183](https://github.com/M-Igashi/baken/issues/183), [#188](https://github.com/M-Igashi/baken/pull/188)). A read-only report of which tracks in a playlist a player will refuse, for three classes of player: pre-NXS2 (CDJ-2000NXS, CDJ-900NXS), CDJ-2000NXS2 and CDJ-3000. The rules come only from Pioneer's operating instructions, cited in the code. Where no manual says (mono or multichannel files, `WAVE_FORMAT_EXTENSIBLE` WAV, AIFF-C, a VBR MP3 without a Xing header, a stick path over 255 characters), the verdict is "unknown". Nothing is written, and the exit code is 1 when a player refuses a track or a track is missing or unreadable.
- **`expressport` warns about the stick's format** ([#184](https://github.com/M-Igashi/baken/issues/184), [#187](https://github.com/M-Igashi/baken/pull/187)). Before anything is copied: exFAT (a CDJ-3000 reads it, a CDJ-2000NXS2 and older do not), NTFS (no player), APFS or anything else (not in any player's list), and a GPT partition table (not supported by the CDJ-3000 and the CDJ-2000, and reported not to mount on a CDJ-2000NXS2). FAT16, FAT32 and HFS+ give no warning. Checked on macOS and Linux, not yet on Windows.

## Other changes

- `expressport` prepares tracks ahead of the stick writes ([#160](https://github.com/M-Igashi/baken/issues/160), [#170](https://github.com/M-Igashi/baken/pull/170)). Reading and rewriting rekordbox's analysis, or with `--generate-analysis` decoding the audio, now runs on two worker threads while the stick is written. The stick sees the same writes in the same order as before. Generated analysis now takes about as long as copied analysis: on a FAT32 USB 3 stick, 62 tracks took 452 s instead of 466 to 471 s, and with the audio on a network hard disk it was faster too. A track whose preparation fails is no longer copied to the stick first, and a decoder crashing on a broken file fails that track instead of the whole export.
- The 1A to 12B key notation is called Alphanumeric, rekordbox's own name for it, in the README, the help text and the docs ([#186](https://github.com/M-Igashi/baken/pull/186)).

## Library users

- `baken-core`:
  - `rbsort::parse_key` is new: Alphanumeric or Classic key name to the 0..=23 wheel index.
  - `cdjsafe::check` is new (`check(plan, progress, cancel) -> CheckReport`, with `TrackCheck`, `Facts`, `Format`, `Player` and `Verdict`), and so is `cdjsafe::stick_path`, which moved here from `baken-export`'s layout code so that both use one rule.
  - `cdjsafe::SourceInfo` gains `container`, `codec_tag`, `profile`, `bit_depth` and `channels`. Code that builds a `SourceInfo` with a struct literal must set them.
- `baken-export` (not source-compatible with 4.1.x):
  - `pdb::Export` has a new field `keys: Vec<(u32, String)>`, and `pdb::fixed::KEYS` is removed.
  - `Plan` gains `filesystem` and `partition_table` and the method `format_warnings()`; the module `volume` is new. A caller that cannot run `diskutil` or `lsblk` (a sandboxed app) can set `partition_table` itself.

## Upgrading

- If you switched rekordbox's key display to Alphanumeric only for baken, you can switch it back.
- A stick written by 4.1.1 or earlier from a Classic XML has no keys. Run the same `baken expressport` command again with 4.2.0: `export.pdb` is rebuilt, only analysis files that change are rewritten, and audio already on the stick is not copied again.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
