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
