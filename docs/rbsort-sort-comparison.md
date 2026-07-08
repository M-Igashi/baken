# rbsort Sort Behavior — Single-column vs Multi-column

This document walks through what `rbsort` does using a minimal 6-track example, comparing single-column sort (Key only or BPM only — what Rekordbox and the CDJ-3000 / XDJ-1000MK2 expose in their UIs) to the multi-column sort `rbsort` produces.

## The gap rbsort fills

Rekordbox's desktop app, the CDJ-3000, and the XDJ-1000MK2 all let you sort a list by **Key** *or* **BPM** as a single column. None of them expose a **compound** sort — Key as the primary key, BPM ascending as the tiebreaker within each Key group. That gap is what `rbsort` fills.

The example below shows what each of the three sort modes actually produces from the same input.

## Input

Consider a playlist holding the following 6 tracks in registration order (i.e. the order they were dragged into the playlist, which is also what the playlist's *stored* order is):

| # | Track | Key | BPM |
|---|---|---|---|
| 1 | A  | 1A  | 130 |
| 2 | B  | 2A  | 124 |
| 3 | C  | 1A  | 124 |
| 4 | D  | 2B  | 130 |
| 5 | E  | 2A  | 128 |
| 6 | F  | 2B  | 124 |

## Sort by Key (single column)

Clicking the *Key* column header in Rekordbox, or picking *Sort by Key* in the CDJ's browse menu, groups tracks by Key. BPM inside each group falls back to whatever the playlist had:

| # | Track | Key | BPM |
|---|---|---|---|
| 1 | A  | 1A  | **130** |
| 2 | C  | 1A  | **124** |
| 3 | B  | 2A  | 124     |
| 4 | E  | 2A  | 128     |
| 5 | D  | 2B  | **130** |
| 6 | F  | 2B  | **124** |

In `1A` and `2B` the BPM runs 130 → 124 — the opposite direction of a harmonic warm-up.

## Sort by BPM (single column)

Sorting by *BPM* aligns BPM but scatters the Key grouping:

| # | Track | Key | BPM |
|---|---|---|---|
| 1 | C  | **1A** | 124 |
| 2 | B  | **2A** | 124 |
| 3 | F  | **2B** | 124 |
| 4 | E  | **2A** | 128 |
| 5 | A  | **1A** | 130 |
| 6 | D  | **2B** | 130 |

Key walks `1A → 2A → 2B → 2A → 1A → 2B` — a tour of the Camelot wheel that doesn't follow harmonic adjacency.

## rbsort (multi-column: Key → BPM)

`rbsort` produces a compound sort: Camelot Key ascending as the primary key, BPM ascending as the tiebreaker within each Key group:

| # | Track | Key | BPM |
|---|---|---|---|
| 1 | C  | 1A  | 124 |
| 2 | A  | 1A  | 130 |
| 3 | B  | 2A  | 124 |
| 4 | E  | 2A  | 128 |
| 5 | F  | 2B  | 124 |
| 6 | D  | 2B  | 130 |

Key groups walk `1A → 2A → 2B` (Camelot ascending) and BPM rises monotonically inside each group. This is the ordering a harmonic warm-up set typically wants.

## Summary

| Sort                       | Key order   | BPM within Key group           |
|----------------------------|-------------|--------------------------------|
| Sort by Key (single)       | aligned     | registration order (no guarantee) |
| Sort by BPM (single)       | scattered   | aligned (Key grouping gone)    |
| `rbsort` (multi-column)    | aligned     | ascending                       |

Toggling between *Sort by Key* and *Sort by BPM* in the UI does not combine them — that is the gap `rbsort` fills.

## Why this has to be baked into the playlist's stored order

Even when single-column sort is enough for browsing, CDJs play tracks in the playlist's **stored** order — they ignore any on-screen sort that was active in Rekordbox or on the deck's browser. Toggling *Sort by Key* on the desktop and then exporting to USB does not change the order the CDJ plays the tracks in.

`rbsort` rewrites the playlist's stored order inside the exported `collection.xml`, so the compound sort survives the USB export and reaches the CDJ. The same reason the loudness side of baken (`baken headroom`) writes gain into the audio files rather than relying on Rekordbox's Auto Gain tags: anything that lives only in the laptop app does not follow the music onto the deck.

## Related

- [README — Rekordbox Playlist Sorter (baken rbsort)](../README.md#rekordbox-playlist-sorter-baken-rbsort)
- [docs/true-peak-ceiling.md](true-peak-ceiling.md) — the same kind of design note for the loudness side
