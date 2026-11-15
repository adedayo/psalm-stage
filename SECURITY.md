# Security Policy

## Supported Versions

Security fixes are released for the latest minor release on `main`:

| Version | Supported |
| :--- | :--- |
| `0.1.x` | Yes |
| `< 0.1.0` | No |

## Reporting a Vulnerability

Please **do not** open a public GitHub issue for security vulnerabilities (such as command execution risks in external toolchain invocations, path traversal in song/setlist bundle loading, or Tauri IPC capability bypasses).

Instead:
1. Use GitHub's **Private Vulnerability Reporting** feature on `https://github.com/adedayo/psalm-stage/security/advisories/new` (recommended).
2. Include steps to reproduce, affected OS/version, and potential impact.

We aim to acknowledge reports within 72 hours and coordinate a fix and release advisory promptly.
