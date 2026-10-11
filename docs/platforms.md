# Platform support

Build and type checking establish that the backend code compiles. They do not establish that events reach a real application's focused window. Run `doctor`, then the deliberate `test-input` command, and complete the physical tests on each platform you use.

## macOS

The minimum host OS is macOS 10.15 on Intel or macOS 11.0 on Apple Silicon. The CoreGraphics event-posting permission APIs require macOS 10.15. These requirements apply to both source installations and packaged executables. Install the pinned Rust 1.99.0 toolchain and Apple's command-line developer tools; install the C64 tools with `brew install cc65`, or use the pinned [build scripts](../scripts/README.md). The backend posts CoreGraphics events with explicit modifier flags. It uses no input hook, event tap, or keyboard capture.

Accessibility permission is required. `doctor` and `permissions` only inspect it. `permissions --request` explicitly calls the macOS prompting API; authorize the executable or terminal macOS identifies under System Settings → Privacy & Security → Accessibility, then run `doctor` again. Input Monitoring is not used by this implementation.

Unicode text and host-generated repeat are implemented. Supported input sources are U.S./ABC for US and British for UK; other sources, IMEs, and unverified variants are rejected. US and British layouts use different physical symbol mappings; macOS British `@` uses Shift+2, while Windows/Linux UK use Shift+Quote. Select the matching host input source manually. Permission success does not prove delivery to a particular application.

On 2026-10-11, the user observed the documented TextEdit and VS Code typing, symbol, cursor and shortcut checks pass on ARM macOS with US layout, including repeat stopping on release and held-key pause/resume. This acceptance does not establish UK layout, other applications or other host platforms. See [hardware observations](hardware-testing.md).

## Windows

Use Rust 1.99.0 with an appropriate native linker toolchain. Install cc65 on `PATH`, or provide genuine prebuilt PRGs through `C64_PRG_DIR`. GNU Make is needed only for the standalone C64 make targets; Cargo invokes `cl65` directly. The release executable is `target/release/c64bridge.exe`. The [host-only scripted checks](../scripts/README.md) use Git Bash and verified PRGs from the pinned Unix build.

The backend uses scan-code `SendInput`, extended-key flags, Unicode UTF-16 packets, and host-generated repeat. The application and the target window must be allowed to exchange input under Windows integrity restrictions; an ordinary process cannot inject into a more privileged target. `doctor` cannot reliably prove this restriction for the currently focused window. Do the focused-application test at normal privilege first. The bridge has no automatic elevation or permission-prompt path on Windows. [Microsoft SendInput documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).

The selected foreground application's layout must be standard US or United Kingdom. Variants and IMEs are rejected even if they share a locale identifier. Changing focus can also change that layout; a mismatch disables input and attempts release.

## Linux

Install Rust, a C/linker toolchain, and cc65 from your distribution or [upstream](https://cc65.github.io/). The X11 backend uses the Rust `x11rb` client and the XTEST extension. It accepts only a local AF_UNIX display such as `DISPLAY=:0` or `DISPLAY=unix:0`; remote/TCP displays and SSH-forwarded displays such as `localhost:10.0` are refused. It requires an authenticated connection, XKB support, and a matching standard US/UK map in active group zero. Physical-row and punctuation fingerprints reject unknown layouts and dead-key variants. `auto` selects X11 outside a detected Wayland session.

The local X11 connection is nonblocking. Abstract/named socket connection attempts and the setup handshake share a 500 ms deadline; subsequent request/reply operations use a fresh 500 ms deadline and finite polling. A stalled server disables input instead of blocking the worker indefinitely. Failed releases retain ownership for another cleanup attempt rather than claiming the native key was released.

Production-stream mock tests were exercised on macOS, and the Linux mock test module was cross-compiled. Those checks do not establish a connection to a real Linux X server or native application delivery; both remain unverified.

Under Wayland, `auto` selects `uinput`. XTEST through XWayland cannot establish delivery to native Wayland applications, so this implementation refuses that path in a detected Wayland session. The uinput backend requires a usable `/dev/uinput` or `/dev/input/uinput` and explicit administrator-configured access for this user. Do not run the network server as root or grant arbitrary users unrestricted uinput access. The bridge does not change device permissions or load a kernel module automatically. [Kernel uinput documentation](https://docs.kernel.org/input/uinput.html).

Wayland uinput support is currently limited to Sway with `swaymsg` and Hyprland with `hyprctl` in the matching session. The backend queries the actual bridge virtual keyboard and requires exactly one device using standard `English (US)` or `English (UK)`, matching the selected layout. Unknown variants, mismatches, missing/ambiguous devices, and other compositors disable admission. GNOME, KDE, and other Wayland environments are currently unsupported under this policy.

`doctor` can check that uinput opens, without creating a virtual keyboard. If the device is absent, its layout remains unverified. The virtual keyboard is created only when injection is deliberately enabled, then checked before any key. Queries are bounded to 500 ms and 64 KiB, and a matching result is cached for at most 100 ms. Compositor acceptance and device discovery timing still require a focused-application test. Linux currently supports physical keys only: Unicode text actions are blocked, and repeat follows desktop settings rather than `REPEAT_*`.

Native backends check layout before input and during enabled operation. A mismatch disables input and attempts cleanup; key releases bypass the layout guard so a switch cannot prevent cleanup solely because the new layout differs.

## Using prebuilt C64 templates

Both templates must be genuine outputs from this source and retain their single `KBCFGv1` block and `$0801` load address. Cargo validates these properties before embedding them.

On a POSIX shell:

```sh
make -C c64 all
C64_PRG_DIR="$PWD/c64/build" cargo build --release --locked
```

In PowerShell, after creating the templates:

```powershell
$env:C64_PRG_DIR = (Resolve-Path c64/build).Path
cargo build --release --locked
```

`CL65` selects a different compiler executable for Cargo's build script. The standalone Makefile also accepts `CC65`, `CA65`, `CL65`, `HOST_CC`, and `BUILD`. Keep cross-compilation checks separate from actual execution on the target operating system.
