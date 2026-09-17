# Making rekordbox start faster on a Mac

[日本語版](mac-tuneup.ja.md)

rekordbox gets blamed for a lot of things macOS is doing. A thirty-second stall on startup, a track that takes a beat too long to load, an analysis run that crawls: on a Mac, five system-level settings account for most of it, and none of them are inside rekordbox.

This page is the whole checklist, with the manual steps. You do not need any tool to work through it. [Bake'n Deck for Mac](https://baken.ravers.workers.dev) has a page that checks all five and fixes the ones a sandboxed app is allowed to fix, but the settings are the point, not the app.

Everything below is a macOS setting or a file attribute. None of it touches your audio, your tags, your cues or the rekordbox database.

## 1. Keep your music out of `~/Downloads`

**Why.** Every file that arrives through a browser carries the `com.apple.quarantine` attribute, and Gatekeeper re-checks quarantined files when they are opened. A library that lives in `~/Downloads` is a library Gatekeeper inspects over and over. This one cause alone produces the classic thirty-to-sixty-second stall when rekordbox starts or loads a track.

**Check.**

```sh
find ~/Downloads -type f \( -name '*.mp3' -o -name '*.flac' -o -name '*.wav' -o -name '*.aif*' -o -name '*.m4a' \) | wc -l
```

**Fix.** Move the files to a permanent folder, for example `~/Music/DJ`. In rekordbox, point the library at the new location: select the tracks and right-click → *Relocate*, or *File* → *Display All Missing Files*. Then run step 5 on the new folder, because the quarantine attribute survives the move.

## 2. Exclude the library from Spotlight

**Why.** Spotlight indexes a freshly populated music folder at the same time rekordbox is analysing it, and the two compete for the same disk and the same CPU. Excluding the music folder and `~/Library/Pioneer` keeps them out of each other's way.

**Fix, internal drive.** *System Settings* → *Spotlight* → *Search Privacy* → **+**, and add your music folder and `~/Library/Pioneer`. Press ⌘⇧G in the file dialog to type a path that Finder hides.

**Fix, external drive.** Spotlight's privacy list is awkward for removable volumes, so use the marker file the indexer looks for:

```sh
touch /Volumes/YOUR_DRIVE/.metadata_never_index
```

It takes effect the next time the drive is mounted. To check what Spotlight currently thinks:

```sh
mdutil -s /Volumes/YOUR_DRIVE
```

## 3. Give rekordbox Full Disk Access

**Why.** Without it, every file read goes through a permission check first. One check is nothing; a few thousand of them, on startup and on every playlist load, is seconds.

**Fix.** *System Settings* → *Privacy & Security* → *Full Disk Access*, turn on rekordbox (add it with **+** from `/Applications` if it is not in the list), then quit and reopen rekordbox.

## 4. Keep the library out of iCloud

**Why.** Desktop & Documents sync and iCloud Drive both evict local files to the cloud and download them again on demand. A DJ library in a synced folder means rekordbox waits on the network for reads that should have been local, and a set prepared on a plane finds half the files missing.

**Fix.** *System Settings* → *Apple Account* → *iCloud* → *Drive*: turn off *Desktop & Documents Folders*, or move the music out of `~/Desktop`, `~/Documents` and `~/Library/Mobile Documents`. `~/Library/Pioneer/rekordbox` in particular must be fully local; it never belongs in a synced folder. Relocate any moved tracks in rekordbox afterwards.

## 5. Clear quarantine flags and AppleDouble files

**Why.** Quarantine flags survive a move, so the files you rescued from `~/Downloads` in step 1 still trigger Gatekeeper on every open. AppleDouble files (the `._` siblings macOS writes on FAT32 and exFAT volumes) are metadata leftovers that slow directory scans and confuse some CDJs.

**Check.**

```sh
find ~/Music/DJ -xattrname com.apple.quarantine | wc -l   # quarantined files
find /Volumes/YOUR_USB -name '._*' | wc -l                # AppleDouble leftovers
```

**Fix.**

```sh
xattr -d -r com.apple.quarantine ~/Music/DJ   # removes only the quarantine attribute
dot_clean -m /Volumes/YOUR_USB                # deletes the ._ files
```

`xattr -cr` is the version you will find in most forum posts. It works, but `-c` clears *every* extended attribute, including Finder tags and comments, so prefer `-d -r com.apple.quarantine` unless you actually want everything gone.

Two things worth knowing: on exFAT and FAT32 volumes the kernel hides `._` sidecars from a normal scan, so a count of zero there does not always mean the drive is clean, and a recursive `xattr` run over a whole drive prints complaints about `.Spotlight-V100` and `.Trashes` that you can ignore.

## After the clean-up

Quit and reopen rekordbox once, then let it sit for a minute so any analysis queue drains. The startup difference shows up immediately; the difference when loading a track shows up the first time you load one that used to be quarantined.

If rekordbox is still slow after all five, the remaining usual suspects are not macOS settings: a collection with tens of thousands of tracks on a slow USB 2 drive, a nearly full boot volume, or an antivirus product scanning audio files on access.

## What Bake'n Deck for Mac adds

Nothing that is not on this page. It runs all five checks against your actual library (it reads the track locations out of your exported collection XML), writes the Spotlight marker on external drives, deletes the `._` files after a confirmation, opens the exact System Settings pane for the steps that need one, and explains next to each row why it makes rekordbox faster. The one step it cannot do for you is the quarantine attribute: the App Store sandbox refuses `removexattr` on `com.apple.quarantine`, so the app hands you the command for your own folders instead, which is the same command as above.

The `baken` command line does not touch any of this. It is macOS hygiene, not a feature of the tool, which is why it lives here rather than behind a purchase.
