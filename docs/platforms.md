# Platform support

Build and type checking establish that the backend code compiles. They do not establish that events reach a real application's focused window. Run `doctor`, then the deliberate `test-input` command, and complete the physical tests on each platform you use.

Both the CLI and [desktop companion](gui.md) use these same backends. Native installers contain both frontends and retain the documented permission and layout checks. Existing CLI acceptance results do not automatically cover a newly installed GUI process's permission or credentials. The companion requires a working OpenGL/EGL graphics driver; Linux builds use X11 or Wayland for the window independently of the input backend's stricter admission policy.

## macOS

The minimum host OS is macOS 10.15 on Intel or macOS 11.0 on Apple Silicon. The CoreGraphics event-posting permission APIs require macOS 10.15. These requirements apply to both source installations and packaged executables. Install the pinned Rust 1.99.0 toolchain and Apple's command-line developer tools; install the C64 tools with `brew install cc65`, or use the pinned [build scripts](../scripts/README.md). The backend posts CoreGraphics events with explicit modifier flags. It uses no input hook, event tap, or keyboard capture.

Accessibility permission is required. `doctor` and `permissions` only inspect it. `permissions --request` explicitly calls the macOS prompting API; authorize the executable or terminal macOS identifies under System Settings → Privacy & Security → Accessibility, then run `doctor` again. Input Monitoring is not used by this implementation.

Unicode text and host-generated repeat are implemented. Supported input sources are U.S./ABC for US and British for UK; other sources, IMEs, and unverified variants are rejected. US and British layouts use different physical symbol mappings; macOS British `@` uses Shift+2, while Windows/Linux UK use Shift+Quote. Select the matching host input source manually. Permission success does not prove delivery to a particular application.

On 2026-10-11, the user observed the documented TextEdit and VS Code typing, symbol, cursor and shortcut checks pass on ARM macOS with US layout, including repeat stopping on release and held-key pause/resume. This acceptance does not establish UK layout, other applications or other host platforms. See [hardware observations](hardware-testing.md).

## Windows

The packaged x64 executable requires the Microsoft Visual C++ v14 x64 runtime and Windows' Universal C Runtime. Install the current supported [x64 Visual C++ Redistributable](https://learn.microsoft.com/en-us/cpp/windows/latest-supported-vc-redist/) if it is missing. The archive does not bundle these system runtime DLLs. Windows 10/11 provides the Universal C Runtime; keep the system updated.

Use Rust 1.99.0 with an appropriate native linker toolchain. Install cc65 on `PATH`, or provide genuine prebuilt PRGs through `C64_PRG_DIR`. GNU Make is needed only for the standalone C64 make targets; Cargo invokes `cl65` directly. The release executable is `target/release/c64-keyboard-bridge.exe`. The [host-only scripted checks](../scripts/README.md) use Git Bash and verified PRGs from the pinned Unix build.

The backend uses scan-code `SendInput`, extended-key flags, Unicode UTF-16 packets, and host-generated repeat. The application and the target window must be allowed to exchange input under Windows integrity restrictions; an ordinary process cannot inject into a more privileged target. `doctor` cannot reliably prove this restriction for the currently focused window. Do the focused-application test at normal privilege first. The bridge has no automatic elevation or permission-prompt path on Windows. [Microsoft SendInput documentation](https://learn.microsoft.com/en-us/windows/win32/api/winuser/nf-winuser-sendinput).

The selected foreground application's layout must be standard US or United Kingdom. Variants and IMEs are rejected even if they share a locale identifier. Changing focus can also change that layout; a mismatch disables input and attempts release.

On 2026-10-12, Windows 11 Enterprise with standard US layout passed the deliberate software phrase test in Notepad and the initial C64 physical transition sequence with input disabled. The first full physical typing test reported intermittent missed or duplicated quick taps. After scanner debounce and ordered-repeat fixes, the user confirmed exact quick typing, rolling `ab` pairs and punctuation in the Notepad retest, followed by VS Code typing, cursors, F1/Escape, shortcuts and repeat stopping. Held-key pause/resume and a six-second C64 CPU traffic stall also passed the user-observed cleanup/recovery checks. One overlapping symbol chord was rejected as designed and succeeded after retry. This coverage does not establish every key/layout, elevated applications or Wi-Fi outage recovery. See the [recorded observations](hardware-testing.md#windows-and-wslg-observations).

## Linux

Install Rust, a C/linker toolchain, and cc65 from your distribution or [upstream](https://cc65.github.io/). The X11 backend uses the Rust `x11rb` client and the XTEST extension. It accepts only a local AF_UNIX display such as `DISPLAY=:0` or `DISPLAY=unix:0`; remote/TCP displays and SSH-forwarded displays such as `localhost:10.0` are refused. It requires an authenticated connection, XKB support, and a matching standard US/UK map in active group zero. Physical-row and punctuation fingerprints reject unknown layouts and dead-key variants. `auto` selects X11 outside a detected Wayland session.

Linux release binaries are dynamically linked GNU/Linux builds produced on Ubuntu 24.04 CI runners. They do not promise compatibility with every older distribution's glibc. If an older Ubuntu installation reports a missing `GLIBC_*` version, build the matching source on that distribution with the pinned toolchain instead of replacing its system libc. The executable architecture must also match the distribution's architecture. CI uses Python 3.12 for the accompanying scripts.

The local X11 connection is nonblocking. Abstract/named socket connection attempts and the setup handshake share a 500 ms deadline; subsequent request/reply operations use a fresh 500 ms deadline and finite polling. A stalled server disables input instead of blocking the worker indefinitely. Failed releases retain ownership for another cleanup attempt rather than claiming the native key was released.

Production-stream mock tests passed on native macOS and Linux CI runners. On 2026-10-12, the [isolated Xvfb smoke test](../scripts/README.md#isolated-linux-x11-check) passed with the packaged ARM64 Linux executable in an Ubuntu 24.04 container: standard US/UK typing, balanced events, no held keys, layout-mismatch refusal and a bounded absent-server connection failure. The same harness passed on native Linux x64 and ARM64 CI runners. This establishes real Linux XTEST delivery in a private virtual X server. Native Linux physical desktop, Wayland uinput and physical C64-to-Linux-backend acceptance remain unverified. Windows-to-WSLg delivery was observed separately below; see the [recorded test scope](hardware-testing.md#isolated-linux-x11-observation).

Under Wayland, `auto` selects `uinput`. XTEST through XWayland cannot establish delivery to native Wayland applications, so this implementation refuses that path in a detected Wayland session. The uinput backend requires a usable `/dev/uinput` or `/dev/input/uinput` and explicit administrator-configured access for this user. Do not run the network server as root or grant arbitrary users unrestricted uinput access. The bridge does not change device permissions or load a kernel module automatically. [Kernel uinput documentation](https://docs.kernel.org/input/uinput.html).

Wayland uinput support is currently limited to Sway with `swaymsg` and Hyprland with `hyprctl` in the matching session. The backend queries the actual bridge virtual keyboard and requires exactly one device using standard `English (US)` or `English (UK)`, matching the selected layout. Unknown variants, mismatches, missing/ambiguous devices, and other compositors disable admission. GNOME, KDE, and other Wayland environments are currently unsupported under this policy.

`doctor` can check that uinput opens, without creating a virtual keyboard. If the device is absent, its layout remains unverified. The virtual keyboard is created only when injection is deliberately enabled, then checked before any key. Queries are bounded to 500 ms and 64 KiB, and a matching result is cached for at most 100 ms. Compositor acceptance and device discovery timing still require a focused-application test. Linux currently supports physical keys only: Unicode text actions are blocked, and repeat follows desktop settings rather than `REPEAT_*`.

Native backends check layout before input and during enabled operation. A mismatch disables input and attempts cleanup; key releases bypass the layout guard so a switch cannot prevent cleanup solely because the new layout differs.

### Ubuntu in WSLg

WSLg presents Linux X11 and Wayland application windows on the Windows desktop. Its system distribution runs Weston and XWayland and exposes their sockets with `DISPLAY` and `WAYLAND_DISPLAY` set in the user distribution. See [Microsoft's GUI guide](https://learn.microsoft.com/en-us/windows/wsl/tutorials/gui-apps) and [official WSLg architecture](https://github.com/microsoft/wslg#wslg-architecture-overview).

Use these paths as separate acceptance results:

| Test path | What it can establish |
| --- | --- |
| Windows executable → focused Windows scratch editor | Windows `SendInput`, layout, modifiers, repeat and cleanup |
| Windows executable → focused WSLg scratch editor | Windows input delivery through WSLg's remoting path, if observed in that application |
| Ubuntu executable → recording simulator | Linux execution, protocol, runtime controls and cleanup |
| Ubuntu executable → private Xvfb collector | Real Linux XTEST/XKB delivery in an isolated virtual X server |

On 2026-10-12, the user observed the Windows-to-WSLg path pass in a blank GTK text area under the normal Ubuntu user: typing, symbols, Backspace, shifted cursors, repeat stopping and Select All. Input was paused afterward. It exercises the Windows backend and is recorded separately from Linux backend acceptance; it does not establish delivery to every WSLg application.

The default WSLg desktop is currently unsupported by the Linux native backend. A present `WAYLAND_DISPLAY`, or `XDG_SESSION_TYPE=wayland`, makes `auto` select uinput. Weston cannot satisfy the current Sway/Hyprland device-layout admission policy, even if a uinput device is writable. Selecting `--backend x11` also refuses a detected Wayland session. Do not remove Wayland environment variables to bypass this guard on the real WSLg desktop. The Xvfb harness removes them only from children directed to its own authenticated private display.

Start with the native Windows bridge for physical C64 tests. A C64 connecting directly to a server inside WSL2 additionally needs a reachable Linux listener. Default NAT does not expose it to the LAN automatically; mirrored mode and applicable firewall rules can permit LAN connections. Any forwarding arrangement must preserve or explicitly validate the peer address used by the bridge's allowlist. See [Microsoft's WSL networking guide](https://learn.microsoft.com/en-us/windows/wsl/networking). Loopback recording and Xvfb tests require no LAN forwarding or firewall changes.

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
