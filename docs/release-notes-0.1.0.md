# C64 Keyboard Bridge 0.1.0 — release notes

Prepared notes for the first release. This document does not create a tag or publish packages.

- Rust CLI and desktop companion for macOS Apple Silicon/Intel, Windows x64 and Ubuntu 24.04+ amd64/arm64.
- Commands: `c64-keyboard-bridge` and `c64-keyboard-bridge-gui`; app ID: `com.flameliongames.c64keyboardbridge`.
- Genuine keyboard-client and networking-probe C64 PRGs, authenticated pairing, retained session diagnostics and conservative held-key recovery.
- Visual mapping editor, tray operation, platform-specific controls and optional OS credential storage.
- GNOME Wayland keyboard control through explicit desktop portal permission, without root or uinput device access.
- Immediate input enabling after connection, layout and permission checks; hosting, deployment and injection start disabled.
- Drag-to-Applications Mac DMGs, per-user Windows installers, Ubuntu DEBs and portable archives with corresponding source and licensing notices.
- Protected release preparation signs and notarizes Mac packages after owner approval and verifies final downloads and checksums before explicit publication. Local signed-only development packages carry a separate classification. Windows installers remain unsigned.

Settings, pairing state and saved credential references are preserved across the executable/app-ID rename. macOS may require renewed application permissions or Keychain approval.

Device and platform acceptance is described in [compatibility](device-compatibility.md), [platform support](platforms.md) and [hardware testing](hardware-testing.md). Linux XTEST fixture success and GUI screenshots do not establish physical Wayland/uinput input acceptance. GNOME uses approved portal sessions; Sway/Hyprland uinput requires user-granted device access and its supported layout-query path. Installation grants no input permission and never runs the network application as root.

REST deployment resets and replaces the C64 program and requires fresh consent. Pairing traffic is plaintext on a trusted LAN. See [security](security.md), [configuration](configuration.md) and [the screenshot gallery](screenshots.md).
