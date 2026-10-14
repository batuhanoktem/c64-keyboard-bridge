# C64 Keyboard Bridge 0.1.0 — release notes

The first GPL-3.0-only release brings the C64 Ultimate keyboard to macOS, Windows and Linux through a desktop companion or independently usable CLI.

- Rust CLI and desktop companion for macOS Apple Silicon/Intel, Windows x64 and Ubuntu 24.04+ amd64/arm64.
- Commands: `c64-keyboard-bridge` and `c64-keyboard-bridge-gui`; app ID: `com.flameliongames.c64keyboardbridge`.
- Genuine keyboard-client and networking-probe C64 PRGs, authenticated pairing, retained session diagnostics and conservative held-key recovery.
- Visual mapping editor, tray operation, platform-specific controls and optional OS credential storage.
- GNOME Wayland keyboard control through explicit desktop portal permission, without root or uinput device access.
- Immediate input enabling after connection, layout and permission checks; hosting, deployment and injection start disabled.
- A deliberate input test types `c64 keyboard bridge` and Enter with 20 balanced press/release pairs after a countdown.
- Drag-to-Applications Mac DMGs, per-user Windows installers, Ubuntu DEBs and portable archives with corresponding source and licensing notices.
- An approval-gated release workflow supports promotion of verified CI packages to signed and notarized Mac assets, followed by final download and checksum verification. Ordinary CI artifacts remain unsigned; public Mac release requires successful Apple acceptance and stapling. Local signed-only development packages carry a separate classification. Windows installers remain unsigned.

Default settings and pairing storage, and OS credentials, use the new application namespace. Earlier default data and saved credentials remain untouched and are not looked up or migrated; fresh configuration and pairing are needed when using the default directory. An explicit `BRIDGE_STATE_DIR` still selects the user's chosen state directory. Enter the device password if one is configured; blank passwords are supported. A newly paired client requires deliberate redeployment. macOS may require renewed application permissions or Keychain approval.

Physical C64 Ultimate typing and focused recovery checks passed on Apple Silicon macOS and Windows with standard US layouts. Native GNOME Wayland CLI-to-Text Editor phrase delivery also passed with explicitly approved keyboard control, without contacting the C64. These observations do not establish physical C64-to-GNOME or uinput acceptance, every key/layout, or the installed GUI's full native permission lifecycle. Linux XTEST fixtures and GUI screenshots are separate software evidence. See [compatibility](device-compatibility.md), [platform support](platforms.md) and [hardware testing](hardware-testing.md) for the recorded scope and remaining checks.

GNOME uses approved portal sessions; Sway/Hyprland uinput requires user-granted device access and its supported layout-query path. Installation grants no input permission and never runs the network application as root.

REST deployment resets and replaces the C64 program and requires fresh consent. Pairing traffic is plaintext on a trusted LAN. See [security](security.md), [configuration](configuration.md) and [the screenshot gallery](screenshots.md).
