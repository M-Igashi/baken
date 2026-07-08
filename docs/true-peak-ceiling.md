# True Peak Ceiling — Design Notes

This document explains why baken uses a **uniform -0.5 dBTP** delivery target by default, and what the `--tp-target` and `--tp-split-bitrate` flags do. The change landed in v1.10.0 in response to [issue #34](https://github.com/M-Igashi/baken/issues/34).

## Background

[AES TD1008](https://www.aes.org/technical/documentDownloads.cfm?docID=731) (*Recommendations for Loudness of Internet Audio Streaming and On-Demand Distribution*, v1.21-9, 2021-09-24) is the document baken cites when computing its True Peak ceiling. TD1008 contains two related but distinct numbers:

1. **§4 Recommendations** — *"For all content, it is recommended that the Maximum True Peak level not exceed -1 dBTP **at the codec input** of lossy-encoded streams."*
2. **§7B Sources of Peak Overshoot — Codecs** — *"High-rate (e.g., 256 kbps) coders may work satisfactorily with as little as **-0.5 dBTP for the limiting threshold**. Typically, peak overshoot increases as the bit rate decreases, so **the limiting threshold may need to be reduced below the recommended -1.0 dBTP**."*

Both passages describe a **limiter that sits in front of the encoder**. The bitrate dependency is there because lossy encoding can push peaks up by a codec-specific amount; the cushion in front of the codec is meant to absorb that overshoot so the *decoded* peak still fits inside 0 dBTP.

## Why the old rule was wrong

Up to v1.9.x, headroom (as baken was then named) used a bitrate-split ceiling on the *output* file:

| Class | Old delivery ceiling |
|---|---|
| Lossless | -0.5 dBTP |
| Lossy ≥256 kbps | -0.5 dBTP |
| Lossy <256 kbps | -1.0 dBTP |

That mapping is correct for choosing a *limiter threshold before encoding*. It is **not** the right rule for an already-encoded delivery file. Once a file has been encoded, the codec stage is gone — there is no further encoder downstream to absorb the slack TD1008 reserves for the pre-encode limiter. The bit rate of the existing container only tells you what the file *was* encoded at; it has no bearing on how much extra headroom the *delivery* file needs.

The argument was made on Hydrogenaudio (*Best way to mass normalize*, reply #25 by Case, Global Moderator):

> "I think you have misunderstood AESTD1008. The extra headroom for lower bitrate lossy is only needed prior to encoding. Idea being that the lossier the end result the more the frequency response and peaks can change. It makes no sense to have different limits for the end product anymore."

Reading TD1008 §4 and §7B together confirms this: every passage that introduces bitrate dependence is talking about the *codec input*, not the delivered stream.

## Why the new default is -0.5 dBTP, not -1.0 dBTP

Once the delivery / pre-encode confusion is removed, two single-target candidates are reasonable:

- **-1.0 dBTP** — TD1008's general recommendation in §4, also the practical maximum True Peak target most major streaming services normalize to (Spotify, Apple Music, YouTube, Tidal). This is the conservative choice.
- **-0.5 dBTP** — TD1008's §7B floor for high-rate codec inputs. Already used today for baken's lossless and ≥256 kbps lossy classes; lossless files are *delivery files* with no codec stage, and the existing tool has shipped with -0.5 dBTP for those for a long time without reports of player-side trouble.

baken is built around maximizing loudness without a limiter. Its users are DJs and producers who want every dB of headroom they're entitled to. **-0.5 dBTP is the most aggressive value TD1008 mentions anywhere**, and it is the value the tool already used for the loudness-sensitive bulk of its workload (lossless + high-rate lossy). Switching low-rate lossy to the same value buys those files +0.5 dB of loudness without going beyond anything TD1008 sanctions; high-rate / lossless behaviour is unchanged.

For users who prefer the safer streaming-services number, `--tp-target -1.0` is one flag away.

## What about player-side overshoot?

TD1008 §7 enumerates several sources of overshoot that occur after the file is delivered:

- **BS.1770 true-peak measurement error** — typically <0.6 dB.
- **Sample rate conversion in the player** — can introduce overshoot if the SRC isn't true-peak limited; modern players use linear-phase SRC and float DSP, which is well-behaved.
- **Mono downmix / Hilbert transform** — can raise peaks for true stereo content played through a single speaker; mostly relevant for smart speakers.
- **Fixed-point decoders** — older devices; rare in modern playback chains.

A -0.5 dBTP delivery target leaves ~0.5 dB of margin against 0 dBTP, which covers BS.1770 measurement error. It does **not** leave room for downstream SRC or Hilbert transforms. If your delivery chain is known to feed players that perform either, choose `--tp-target -1.0` (or lower) explicitly.

## Flag reference

```
--tp-target <DB>      Uniform delivery ceiling. Default: -0.5
--tp-split-bitrate    Restore pre-v1.10 split: -0.5 dBTP for ≥256 kbps,
                      -1.0 dBTP for <256 kbps. Mirrors TD1008 §7B
                      pre-encode limiter recommendations.
```

`--tp-target` and `--tp-split-bitrate` are mutually exclusive.

The native-lossless threshold (the True Peak below which an MP3/AAC file qualifies for in-place global_gain modification rather than re-encoding) is always `target − 1.5 dB`, since global_gain only works in 1.5 dB steps.

## Preset crib sheet

| Intent | Flag | Notes |
|---|---|---|
| Default — max-aggressive delivery | *(none)* | TD1008 §7B floor, -0.5 dBTP for all files |
| Streaming-services delivery max | `--tp-target -1.0` | Spotify / Apple Music / YouTube / Tidal target ceiling |
| EBU R128 broadcast-style delivery | `--tp-target -1.0` | R128 max true peak is -1 dBTP |
| Conservative master with player-side margin | `--tp-target -2.0` | Leaves room for downstream SRC and Hilbert downmix |
| Mirror TD1008 §7B pre-encode limiter | `--tp-split-bitrate` | -0.5 dBTP ≥256 k, -1.0 dBTP <256 k |
| Mirror TD1008 §4 generic pre-encode limiter | `--tp-target -1.0` | One uniform value, the §4 number |

## References

- AES TD1008.1.21-9 — *Recommendations for Loudness of Internet Audio Streaming and On-Demand Distribution* — <https://www.aes.org/technical/documentDownloads.cfm?docID=731>
- ITU-R BS.1770 — *Algorithms to measure audio Program Loudness and true-peak audio level* — <https://www.itu.int/rec/R-REC-BS.1770/en>
- EBU R128 — *Loudness Normalisation and Permitted Maximum Level of Audio Signals* — <https://tech.ebu.ch/docs/r/r128.pdf>
- Hydrogenaudio thread *Best way to mass normalize*
- baken issue #34 — <https://github.com/M-Igashi/baken/issues/34>
