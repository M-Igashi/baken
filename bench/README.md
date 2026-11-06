# USB export benchmark

`bench.sh` times the same rekordbox playlist exported to the same USB stick by `baken expressport` (the engine of Bake'n Deck for Mac) and by rbxport, and records every file each tool changes outside the stick. `benchlib.py` holds the helpers: file snapshots by SHA-256, snapshot diffs, stick statistics, process sampling, and the summary. The write-up of the 2026-11-05 run is at https://baken.ravers.workers.dev/usb-export-measured.

## What it measures

- **full**: an emptied stick, the whole playlist, `export.pdb` plus OneLibrary (rbxport always writes both, so `baken` gets `--onelibrary`).
- **resync**: the same export again onto the stick the same tool just wrote.
- **convert**: an emptied stick with conversion for older players, `baken --cdjsafe` against rbxport's Maximum CDJ compatibility set to MP3 (both 320 kbps CBR). rbxport's conversion cache is emptied before each run, so every run is cold.
- **Audit**: before and after every run, the SHA-256 of every file in `~/Library/Pioneer` (rekordbox's library and analysis), `~/Library/Application Support/Pioneer` (its settings), and the music folder, plus a list of every file modified in the home folder.

Each scenario runs `REPS` times (default 3), the tool that goes first alternating between rounds, with `COOL` seconds (default 45) between runs. Finished runs are kept in `results/runs.csv` and skipped on a re-run; `TOOLS="rbxport"` limits a run to one tool.

## Running it

Use a dedicated macOS user account: rbxport opens the rekordbox library it finds, so it must not see a working one.

1. In that account, put the music in `/Users/Shared/rbbench/music`, import and analyse it in rekordbox, make a playlist named `bench` with all of it, export the collection to `/Users/Shared/rbbench/work/collection.xml`, set My Settings once, and quit rekordbox.
2. Put `baken` (the version to measure), `bench.sh`, and `benchlib.py` in `/Users/Shared/rbbench/bin`, rbxport in `/Applications`, and Homebrew's `ffmpeg` on the path for `--cdjsafe`.
3. Eject every drive that holds another rekordbox library, mount a FAT32 stick (the default name is `BAKEN`, `STICK=` changes it), and run:

```
bench.sh check
bench.sh first-launch     # audits rbxport's first launch; open and quit it when asked
caffeinate -dims bench.sh run
```

`results/summary.md` has the table; the per-run logs, snapshots, and diffs are next to it. The scripts use macOS's own bash 3.2 and Python 3.9.

## Results of 2026-11-05

`results-2026-11-05/` holds `runs.csv` and `environment.txt` from the run behind the write-up: MacBook Air (M3, 24 GB), macOS 27.0.1, SanDisk USB 3.2 Gen 1 stick (61.5 GB, FAT32 on MBR), rekordbox 7.2.18, baken 4.8.0, rbxport 1.9.0, and 200 tracks (9.0 GB) from a working DJ library: 91 FLAC, 82 AIFF, 16 MP3, 10 M4A, and 1 WAV. The track names are left out.

The three `bakendeck,convert` rows with status 1 are the first pass, when the test account had no `ffmpeg` on its path; the three rows after them are the rerun. Every other run exited 0, and every run of both tools wrote no file outside the stick in the audited folders.

| Scenario | baken 4.8.0 | rbxport 1.9.0 |
|---|---|---|
| full, median of 3 | 818.6 s, 197 tracks | 959.5 s, 200 tracks |
| resync, median of 3 | 3.6 s | 0.6 s |
| convert, median of 3 | 382.7 s, 188 converted | 1913.3 s, 97 converted |

baken left out three truncated M4A files (rbxport copied them in the full export and reported them as failed in the conversion). One more M4A has corrupt audio data: baken 4.8.0 converted it into an MP3 of noise without a warning, which baken 4.10.0 fixes (#299); rbxport reported it as failed.
