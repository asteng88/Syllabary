# Security Policy

## Supported Versions

Security fixes are applied to the latest release of **Syllabary**:

| Version | Supported |
| ------- | --------- |
| Latest (`0.2.x`) | :white_check_mark: |
| Older versions | :x: |

## Privacy & Offline Guarantee

Syllabary is designed to run **100% offline**:
- Documents are never uploaded to external servers.
- Spell checking runs locally via the Windows OS spell checker or system Hunspell dictionaries on Linux.
- The only network requests occur when checking for updates from GitHub Releases (`api.github.com/repos/asteng88/Syllabary/releases/latest`) or running the Syllabary Installer, both of which enforce **HTTPS-only** downloads and **SHA-256 checksum verification** when digests are provided.

## Reporting a Vulnerability

If you discover a security vulnerability (such as a parser crash, unsafe file handling, or updater integrity issue), please **do not** open a public issue. Instead, report it privately via [GitHub Security Advisories](https://github.com/asteng88/Syllabary/security/advisories/new) or contact **asteng88** via [station88retro.com](https://station88retro.com).
