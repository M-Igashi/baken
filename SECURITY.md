# Security Policy

## Supported Versions

| Version | Supported          |
| ------- | ------------------ |
| latest  | :white_check_mark: |

Only the latest release receives security fixes.

## Reporting a Vulnerability

If you discover a security vulnerability, please report it responsibly:

1. **Do NOT open a public issue**
2. Use [GitHub's private vulnerability reporting](https://github.com/M-Igashi/baken/security/advisories/new)

You should receive an initial response within 72 hours.

## Scope

This project is a CLI tool that processes local audio files. The primary security concerns are:

- Command injection via filenames passed to ffmpeg
- Dependency vulnerabilities in Rust crates
- Malicious input files causing unexpected behavior

## Supply chain

A tool that rewrites your music library and writes your gig stick has to be trustworthy down to its dependencies.

- **Dependencies are weighed before they come in.** SQLCipher and a vendored OpenSSL were dropped in 4.6.0, and on Apple platforms the encryption uses the operating system's own CommonCrypto since 4.6.1.
- **Every pull request goes through dependency review**, which fails on a known vulnerability of moderate severity or higher, and a licence check stops the build on any licence not on the accepted list.
- **Every GitHub Action is pinned to a full commit SHA**, and workflows are read-only unless a job needs more.
- **crates.io releases use Trusted Publishing**, so no crates.io token is stored anywhere.
- **Release archives carry SHA-256 checksums.**
