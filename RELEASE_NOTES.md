# Bake'n Deck 3.0.0 — headroom is now `baken`

headroom is now **Bake'n Deck** (issue #60). The binary, crate, Homebrew formula, and repository are renamed to `baken`, the CLI moves to a subcommand layout, and a third subcommand — `cdjsafe` — completes the Rekordbox → CDJ prep toolkit: loudness gain, Key+BPM playlist sort, and CDJ-safe MP3 transcode, all baked into the files so they survive USB export.

---

## Highlights

- **Rebrand: headroom → Bake'n Deck (`baken`)** (#60). New install channels: `brew install M-Igashi/tap/baken`, `winget install M-Igashi.baken`, `cargo install baken`, `yay -S baken-bin`. Website moves to baken.ravers.workers.dev. This is a **hard cut** — the old `headroom` channels stop receiving updates (pre-announced in #61).
- **Subcommand CLI.** The loudness analyzer now lives under `baken headroom`; the playlist sorter stays at `baken rbsort`. Bare `baken` prints help. All analyzer flags and both interactive/scriptable modes are unchanged, just one level deeper.
- **New: `baken cdjsafe`** (#40). Emergency-backup USB prep for pre-NXS2 CDJs: converts every track in a Rekordbox playlist to 320 kbps CBR MP3 @ 44.1 kHz (ID3v2.3, artwork kept) and emits an updated XML in which each new track inherits the source's beatgrid and hot/memory cues verbatim — import, "Import to Collection", export to USB, no re-analysis. Sources already at the safe profile are copied byte-identically; lossy→lossy re-encodes are listed in the run report.

## Other changes

- CSV report default filename is now `baken_report_<timestamp>.csv`.
- Update-check opt-out env var renamed: `HEADROOM_NO_UPDATE_CHECK` → `BAKEN_NO_UPDATE_CHECK`.
- Backup directories are marked with `.baken-backup`; directories with the legacy `.headroom-backup` marker are still skipped, so existing v2 backups are never re-processed.
- Cargo metadata refreshed: description and keywords now reflect the DJ/Rekordbox/CDJ scope.
- Docs and release workflow rebranded; the Homebrew formula published by CI is now `Formula/baken.rb`.

## Migration

Reinstall via the new `baken` packages — the old `headroom` install channels no longer receive updates:

| Old | New |
|---|---|
| `brew install M-Igashi/tap/headroom` | `brew install M-Igashi/tap/baken` |
| `winget install M-Igashi.headroom` | `winget install M-Igashi.baken` |
| `cargo install headroom` | `cargo install baken` |
| `yay -S headroom-bin` | `yay -S baken-bin` |
| `headroom [paths] [flags]` | `baken headroom [paths] [flags]` |
| `headroom rbsort ...` | `baken rbsort ...` |
| `HEADROOM_NO_UPDATE_CHECK` | `BAKEN_NO_UPDATE_CHECK` |

Scripts calling the old bare `headroom` command must switch to `baken headroom`; flags are unchanged.

---
