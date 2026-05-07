## Highlights

- **Uniform True Peak ceiling at -0.5 dBTP, with full per-run tuning.** AES TD1008's bitrate-dependent ceiling describes the limiter threshold *prior to lossy encoding* (§7B *Sources of Peak Overshoot — Codecs*). headroom processes already-encoded delivery files, where there is no further codec stage downstream to absorb additional overshoot, so the same -0.5 dBTP ceiling — the most aggressive value TD1008 sanctions for any limiter in the chain — is now applied uniformly to every file. Low-bitrate lossy files gain +0.5 dB of loudness for free; lossless and ≥256 kbps lossy behaviour is unchanged. Resolves [#34](https://github.com/M-Igashi/headroom/issues/34).
- **New `--tp-target <DB>` flag.** Override the uniform target with any custom value, e.g. `--tp-target -1.0` for the Spotify / Apple Music / YouTube delivery max, or `--tp-target -2.0` for a conservative master that leaves headroom for player-side SRC / Hilbert downmix.
- **New `--tp-split-bitrate` flag.** Opt back into the legacy bitrate-dependent split (-0.5 dBTP for ≥256 kbps, -1.0 dBTP for <256 kbps). Mirrors TD1008's pre-encode interpretation for users who prefer it.
- **Native-lossless threshold scales with the chosen target.** The True Peak below which an MP3/AAC file qualifies for in-place global_gain modification is now `target − 1.5 dB` rather than the hardcoded -2.0 / -2.5 dBTP used through v1.9.x.

## Documentation

- New [docs/true-peak-ceiling.md](docs/true-peak-ceiling.md) walks through the TD1008 reading with §-level citations, explains the pre-encode vs delivery distinction, and lists preset flag combinations for common targets (Spotify / R128 / TD1008 §4 / TD1008 §7B).
- README §True Peak Ceiling rewritten end-to-end with the new flag table and migration note.

## Migration

Users who relied on the v1.9.x bitrate-dependent split can restore it exactly with `--tp-split-bitrate`. Users who want to match streaming-platform delivery max can use `--tp-target -1.0`.
