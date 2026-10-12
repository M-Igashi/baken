# Bake'n Deck 4.6.1 - OneLibrary encrypted with CommonCrypto on macOS

A patch release. On macOS, `--onelibrary` now encrypts `exportLibrary.db` with CommonCrypto, which is part of macOS, instead of the RustCrypto crates that 4.6.0 brought in. Linux and Windows are unchanged, and so is the file format on every platform.

## Changes

- **OneLibrary encryption on macOS goes through CommonCrypto.** Since 4.6.0 baken encrypts `exportLibrary.db` itself with SQLCipher 4's scheme (PBKDF2-HMAC-SHA512, AES-256-CBC and HMAC-SHA512 per page). On Apple targets those three primitives now come from CommonCrypto, as they did through SQLCipher up to 4.5.0, so the Bake'n Deck Mac app again uses only the encryption built into macOS. Linux and Windows keep RustCrypto. rekordbox's own `exportLibrary.db` still decrypts with every page's HMAC intact, and new known-answer tests hold both implementations to Python's `hashlib` and the OpenSSL command line. The macOS binary stays at 7.4 MB.
- CI runs the new encryption tests on Linux and Windows alongside the OneLibrary ones.

## Upgrading from 4.5.0

4.6.0 changed where `cdjsafe` puts a single nested playlist in its output XML and made `--playlist` optional; the [4.6.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.6.0) have the details.

## Upgrading from 4.4.0 or earlier

4.5.0 renamed files on the stick the way rekordbox names them and replaces audio that changed since the last export. If you skipped it, read "Sticks written by an earlier version" in the [4.5.0 release notes](https://github.com/M-Igashi/baken/releases/tag/v4.5.0) before you export to a stick an earlier version wrote.

## Library users

- `baken-export`: no API change. `aes`, `cbc`, `hmac`, `sha2` and `pbkdf2` are now dependencies only outside Apple targets; on Apple targets `baken-export` calls CommonCrypto from libSystem, so nothing new needs linking. `getrandom` stays on every target.

## Notice for users upgrading from 3.2.x or earlier: the default behaviour of `baken headroom` changed in 3.3.0

Up to 3.2.x, `baken headroom` only ever turned files **up**. Since 3.3.0 tracks above the -0.5 dBTP ceiling are turned **down** to it, so every track on the stick ends up at the same True Peak. Pass `--boost-only` for the old raise-only behaviour, and run with `--analyze-only` first if you want to see what would happen.
