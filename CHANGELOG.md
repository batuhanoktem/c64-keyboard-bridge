# Changelog

User-visible changes are collected here. Unreleased changes do not imply that a tag or downloadable release has been published.

## Unreleased — 0.1.0

### Added

- Rust CLI and desktop companion for macOS Apple Silicon/Intel, Windows x64 and Ubuntu 24.04+ amd64/arm64.
- Genuine C64 keyboard-client and networking-probe PRGs, authenticated pairing and retained connection diagnostics.
- Visual keyboard mapping editor, tray controls and optional OS credential storage.
- Native input backends, conservative held-key recovery and software-only simulators.
- Native installers, portable packages, matching corresponding source, licensing notices and verified checksums.
- Protected Mac signing and notarization promotion with explicit approval and draft release preparation.

### Changed

- Public commands are c64-keyboard-bridge and c64-keyboard-bridge-gui; the application ID is com.flameliongames.c64keyboardbridge.
- Enable input activates immediately after connection, permission and layout checks. The deliberate input test retains its countdown.

### Fixed

- Per-key C64 debounce and queued-release ordering prevent the previously observed missed or duplicated quick taps.
- Held-key pause/resume and traffic-loss recovery preserve release ownership and require held ordinary keys to be released before typing again.

Prepared [0.1.0 release notes](docs/release-notes-0.1.0.md) describe acceptance limits. Record a release date and version section here only when that version is published.
