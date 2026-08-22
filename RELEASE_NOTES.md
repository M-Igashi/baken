# Bake'n Deck 3.0.4 - artwork preservation

## Highlights

- **Embedded cover art keeps its original format.** `baken headroom` passed no video codec to ffmpeg when applying gain, so the muxer default took over and re-encoded the attached picture. FLAC, MP3, and AIFF outputs got PNG, turning a JPEG cover into a much larger PNG (a 500x500 photo-like cover took a FLAC from 205 KB to 404 KB in a single pass). M4A was worse: the ipod muxer defaults to h264 and rejects it, so the AAC re-encode path failed outright on any file with artwork. Both ffmpeg gain paths now stream-copy the picture, so the original bytes survive untouched. Fixes #77.

**Full Changelog**: https://github.com/M-Igashi/baken/compare/v3.0.3...v3.0.4
