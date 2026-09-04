# baken-core

Library behind [Bake'n Deck](https://github.com/M-Igashi/baken) (`baken`), the Rekordbox → CDJ prep toolkit. It exposes the three workflows without any terminal UI so that front-ends (the `baken` CLI, the Mac app) can share one implementation:

- `headroom`: LUFS / True Peak analysis and gain application (lossless via mp3rgain, or ffmpeg re-encode)
- `rbsort`: in-place Camelot Key + BPM sort of playlists in an exported Rekordbox XML
- `cdjsafe`: playlist transcode to 320 kbps CBR MP3 with cues and beatgrid carried over into a new XML

`headroom` and `cdjsafe` shell out to ffmpeg and ffprobe. They are looked up on `PATH` by default; call `baken_core::set_tools` to point at bundled binaries. Long-running operations take a `Progress` callback and a `CancelToken`.
