# Changelog

User-visible changes are collected here. Unreleased changes do not imply that a tag or downloadable release has been published.

## Unreleased — 0.1.0

### Added

- Rust CLI and desktop companion for macOS Apple Silicon/Intel, Windows x64 and Ubuntu 24.04+ amd64/arm64.
- Public commands `c64-keyboard-bridge` and `c64-keyboard-bridge-gui`, with application ID `com.flameliongames.c64keyboardbridge`.
- Genuine C64 keyboard-client and networking-probe PRGs, authenticated pairing and retained connection diagnostics.
- Visual keyboard mapping editor, tray controls and optional OS credential storage.
- Native input backends, per-key C64 debounce and ordered host repeat, with software-only simulators and recording tests.
- Held-key pause/resume and traffic-loss recovery that retain release ownership and require held ordinary keys to be released before typing again.
- GNOME Wayland keyboard control through explicit portal permission, without root or uinput access.
- Immediate input enabling after connection, permission and layout checks; hosting, deployment and injection start disabled.
- A deliberate input test that types `c64 keyboard bridge` and Enter with 20 balanced press/release pairs after a countdown.
- Private default settings and pairing storage in the application namespace, with no automatic lookup or migration of older default data or credentials. An explicit `BRIDGE_STATE_DIR` still selects the user's chosen state directory.
- Password-store operations on a dedicated worker, keeping connection and cleanup controls responsive while OS credential requests are pending.
- Native installers, portable packages, matching corresponding source, licensing notices and verified checksums.
- An approval-gated workflow for Mac signing and notarization, verified final downloads and draft release preparation. Ordinary CI packages and Windows installers remain unsigned.

Prepared [0.1.0 release notes](docs/release-notes-0.1.0.md) describe acceptance limits. Record a release date and version section here only when that version is published.
