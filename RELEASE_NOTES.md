# Bake'n Deck 4.2.1 - `expressport` handles macOS `._` files on the stick

A patch release for `baken expressport` ([#192](https://github.com/M-Igashi/baken/issues/192), [#194](https://github.com/M-Igashi/baken/pull/194)). `headroom`, `rbsort` and `cdjsafe` are unchanged, and `expressport` stays a beta ([#116](https://github.com/M-Igashi/baken/issues/116)) until a player shows the keys from a stick written by 4.2.0 or later.

## Fixes

macOS keeps a file's extended attributes on a FAT stick in a hidden `._` file next to it (AppleDouble). `expressport` removes those at the end of a run, because they are clutter on a player's stick. Two things went wrong with them:

- **`--prune` failed on a stick that already held `._` files** with `No such file or directory (os error 2)`. Prune removed a stale `X` and then its `._X` as a second stale file, but macOS had already dropped `._X` together with `X`. Now a `._X` is decided by its `X`: it is left alone while `X` stays, and removed once `X` is gone if the volume has not dropped it already.
- **A sandboxed app could not finish an export.** Inside the App Sandbox, every file an app writes carries a quarantine attribute, and macOS refuses to delete its `._` file while the file exists. The export then stopped with `Operation not permitted` after every track and `export.pdb` had been written, and prune stopped partway. Such a `._` file is now left on the stick and counted instead of failing the run. The command line is not sandboxed and removes them as before. This is what the upcoming USB export in Bake'n Deck for Mac needs.

## Other changes

- New issues on GitHub go through forms (bug report, feature request, question) that warn against posting personal data, and point to the private support form for anything personal ([#193](https://github.com/M-Igashi/baken/pull/193)).

## Library users

- `baken-export`: `Report` has a new field `apple_double_kept`, the number of `._` files the system did not let the export remove. `Report` is only produced by `export`.
- `baken-core`: no changes.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
