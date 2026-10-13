# Changelog

User-visible changes are collected here. Unreleased changes do not imply that a tag or downloadable release has been published.

## Unreleased — 0.1.0

### Added

- Rust CLI and desktop companion for macOS Apple Silicon/Intel, Windows x64 and Ubuntu 24.04+ amd64/arm64.
- Genuine C64 keyboard-client and networking-probe PRGs, authenticated pairing and retained connection diagnostics.
- Visual keyboard mapping editor, tray controls and optional OS credential storage.
- Native input backends, conservative held-key recovery and software-only simulators.
- GNOME Wayland keyboard control through explicit portal permission, without root or uinput access.
- Native installers, portable packages, matching corresponding source, licensing notices and verified checksums.
- Protected Mac signing and notarization promotion with explicit approval and draft release preparation.

### Changed

- Public commands are c64-keyboard-bridge and c64-keyboard-bridge-gui; the application ID is com.flameliongames.c64keyboardbridge.
- Private settings, pairing and OS credentials use the new application namespace. Fresh configuration and pairing are required; older data remains untouched and is not read or migrated.
- The deliberate input test now types `c64 keyboard bridge` and Enter, with 20 balanced press/release pairs.
- Enable input activates immediately after connection, permission and layout checks. The deliberate input test retains its countdown.

### Fixed

- Per-key C64 debounce and queued-release ordering prevent the previously observed missed or duplicated quick taps.
- Held-key pause/resume and traffic-loss recovery preserve release ownership and require held ordinary keys to be released before typing again.
- Password-store operations run outside the networking runtime, preventing a Linux settings-save crash and keeping connection and cleanup controls responsive during delayed credential requests.
- Packaged notices and resources retain their contents without inheriting executable permissions from upstream sources.

Prepared [0.1.0 release notes](docs/release-notes-0.1.0.md) describe acceptance limits. Record a release date and version section here only when that version is published.
