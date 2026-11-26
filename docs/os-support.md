# OS support: Bake'n Deck for Mac and Bake'n Rec

Which macOS and iOS versions the apps run on, which ones get fixes, and when the minimum moves. Written on 2026-11-26. The `baken` command-line tool is not covered here: it builds wherever Rust and ffmpeg do, and its platforms are listed in the [README](../README.md).

## Today

| | Installs on | Fixes for | Architectures |
|---|---|---|---|
| Bake'n Deck for Mac (1.5, and 1.6 once it is out) | macOS 14 or later | macOS 27, 26, and 15 | Apple silicon and Intel (universal) |
| Bake'n Rec for iPhone (1.0, in App Review) | iOS 17 or later | iOS 27 and 26 | |

"Fixes for" is the promise: a problem that shows on one of those versions gets fixed. On an older version the app still installs and is expected to work, but a problem that only shows there may be answered with "update macOS" when it cannot be reproduced.

Development and release checks run on the current macOS on Apple silicon. Intel Macs and older macOS versions are not checked before a release; reports from them are welcome through the [support form](https://baken.ravers.workers.dev/support).

## The rule

- **macOS: the current version and the two before it.** The minimum moves up by one each autumn, in the first feature release after the new macOS is out.
- **iOS: the current version and the one before it,** raised only when a feature needs a newer API. Bake'n Rec takes audio over USB, so in practice it is used on iPhones with USB-C, all of which shipped with iOS 17 or later.
- **A version is dropped in a feature release, never in a fix.** The App Store keeps offering the last compatible version to a Mac or iPhone that cannot run the new one, so nobody who bought the app is locked out.

## Plan

| When | Change |
|---|---|
| 1.6.x | Minimum stays macOS 14. 1.6.0 builds the bundled ffmpeg for macOS 14 for real: up to 1.5.0 its ffmpeg was stamped for a newer macOS than the app, so on macOS 14 and 15 the audio engine may not have started. |
| The feature release after 1.6 | Minimum macOS 15. |
| Autumn 2027, after macOS 28 is out | Minimum macOS 26. |
| Autumn 2028, after macOS 29 is out | Minimum macOS 27, and the app becomes Apple silicon only. |

## Intel Macs

macOS 26 is the last macOS that runs on Intel, so Intel is not dropped as a decision of its own: it goes when the minimum reaches macOS 27. Until then the app stays a universal binary. Since 2026-11 Intel is no longer verified before a release, because no Intel Mac is at hand.

## What moves the plan forward

- A problem reported on an old version that cannot be reproduced or fixed without a machine running it.
- A feature that needs an API of a newer version, where carrying both paths costs more than the old version is worth. The BPM, key, energy, and vocal analysis already needs macOS 27 and is simply absent below it.

What moves it back: a meaningful share of copies still on a version that is due to go. The share is read from the App Store's download reports before each change.
