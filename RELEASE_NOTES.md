# headroom 2.1.1 — rbsort performance & maintenance

A small maintenance release. `rbsort` XML parsing is faster, and the dependency and CI toolchains are refreshed. No new flags, no behavior changes: output is byte-identical to 2.1.0.

---

## Performance

- **Faster rbsort XML parsing** (#56). Both the scan and rewrite passes now read events directly from the in-memory XML slice (quick-xml's zero-copy slice reader) instead of copying each event through an intermediate buffer, and the rewrite pass writes borrowed events instead of deep-copying them. The collection map is also pre-sized from the `COLLECTION Entries` attribute. ~10% faster on a 50k-track (32 MB) collection export, with byte-identical output.

## Other changes

- Dependencies: quick-xml 0.40 → 0.41, mp3rgain 2.8.0 → 2.9.4, plus routine `rust-minor-patch` group updates (serde, serde_json, chrono, clap, …).
- CI: actions/checkout v6 → v7, dtolnay/rust-toolchain refreshed, softprops/action-gh-release 3.0.0 → 3.0.1.

## Migration

**None.** Fully backward-compatible with v2.1.0.

---
