# baken-core

Library behind [Bake'n Deck](https://github.com/M-Igashi/baken) (`baken`), the rekordbox → CDJ prep toolkit. It exposes the three workflows without any terminal UI so that front-ends (the `baken` CLI, the Mac app) can share one implementation:

- `headroom`: LUFS / True Peak analysis and gain application (MP3/AAC natively via mp3rgain, lossless formats exactly via ffmpeg; nothing is re-encoded)
- `rbsort`: in-place Camelot Key + BPM sort of playlists in an exported rekordbox XML
- `cdjsafe`: playlist transcode to 320 kbps CBR MP3 with cues and beatgrid carried over into a new XML

`headroom` also splits the analysis in two for callers that cache: `headroom::measure` decodes the file once (symphonia, with mp3rgain's BS.1770-4 analyzer for loudness and True Peak; ffmpeg `loudnorm` as the fallback for anything symphonia cannot open) and returns a `Measurement` (loudness, True Peak, bitrate, codec) that does not depend on any setting, and `headroom::decide` turns a measurement into the gain proposal for a given ceiling and `GainMode` without touching the file. `headroom::analyze` is the two in sequence.

`headroom` (lossless gain application and the measurement fallback) and `cdjsafe` shell out to ffmpeg and ffprobe. They are looked up on `PATH` by default; call `baken_core::set_tools` to point at bundled binaries. Long-running operations take a `Progress` callback and a `CancelToken`.
