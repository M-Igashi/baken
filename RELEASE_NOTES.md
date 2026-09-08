# Bake'n Deck 3.3.1 - no more re-encode proposals for sub-1 dB raises

## Highlights

- **Lossy files less than 1.0 dB below the ceiling are left alone.** MP3 and AAC can only be raised natively in 1.5 dB steps, so after one step every file sits somewhere under 1.5 dB below the ceiling, and 3.3.0 then proposed a lossy re-encode for that leftover on the next analysis (a +0.16 dB re-encode, for example). `decide_gain` now returns `None` for lossy raises under the new `headroom::MIN_REENCODE_GAIN` (1.0 dB); re-encoding is still offered, opt-in as before, for raises between 1.0 and 1.5 dB. Lossless files are unaffected and are still raised exactly, however small the gain. This is the same reasoning as 3.3.0's "lowering never re-encodes": a generation of loss for less than 1 dB is not a trade anyone wants.

## Other Changes

- README and `docs/true-peak-ceiling.md` describe the 1.0 dB floor; the CSV example row for a small AAC raise now shows `none`.
- `MIN_REENCODE_GAIN` is public so GUI front ends can explain the rule.
