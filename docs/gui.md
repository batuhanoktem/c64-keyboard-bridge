# Desktop companion

The companion and CLI share the keyboard protocol, native input backends, mappings, diagnostics and real C64 PRGs. Open **C64 Keyboard Bridge** from the installed application shortcut, or run `c64bridge-gui`. Version 0.1.0 does not start a network host, deploy a C64 program or enable input automatically. The desktop application identifier is `com.flameliongames.c64bridge`; the CLI and GUI retain their shared state directory.

## Install

| Platform | Package | Installation |
| --- | --- | --- |
| macOS Apple Silicon / Intel | Architecture-specific DMG | Drag **C64 Keyboard Bridge.app** to Applications. The CLI is also available inside `Contents/MacOS` and the portable archive. |
| Windows x64 | `-setup.exe` | Install for the current user. The installer creates Start Menu shortcuts and an uninstaller, without elevation, automatic launch or PATH changes. |
| Ubuntu 24.04+ amd64 / arm64 | `.deb` | Install with `sudo apt install ./c64-keyboard-bridge_0.1.0_ARCH.deb`. This installs both commands and a desktop launcher. |
| Supported native targets | Portable ZIP / tar.gz | Extract to a user-owned folder and run either executable. |

Installing a Linux package does not grant `/dev/uinput` access, load modules or run the network application as root. Arrange explicit device access separately when using a supported Wayland compositor. See [platform support](platforms.md). Windows runtime requirements and elevated-application restrictions continue to apply.

Uninstall removes program files and shortcuts and preserves private settings, pairing state and OS-vault credentials. Use **Forget saved password** to remove the credential deliberately. **Reset local bridge data** separately confirms removal of saved GUI settings, pairing state, GUI-owned profiles and saved credentials while hosting is stopped. It preserves imported `.env` files and external/exported profiles and requires fresh C64 pairing/deployment afterward.

## macOS setup

Install the app in `/Applications` before granting permission. In the companion's **Diagnostics** page, choose **Check / request permission**, then enable **C64 Keyboard Bridge** in the macOS permission settings opened by the request. You can also add `/Applications/C64 Keyboard Bridge.app` in **System Settings → Privacy & Security → Accessibility**; see [Apple's Accessibility permission guide](https://support.apple.com/guide/mac-help/allow-accessibility-apps-to-access-your-mac-mh43185/mac). Restart the companion after changing its permission and refresh Diagnostics. A grant for Terminal, Codex or the CLI does not grant the installed companion permission to post events. The bridge does not request Input Monitoring or capture host keyboard input.

Finder launches do not inherit the working directory or environment of your project terminal. To use an existing `.env`, enter its **absolute path** in **Settings / About → .env import path**, then save or apply. A missing import file does not supply its password or other values; connection fields in the companion remain explicit overrides. On the first launch, before GUI settings have been saved, you can also initialize the form by launching the app's executable with `--env-file /absolute/path/to/.env`. A saved import path remains absolute on later Finder launches; the companion does not copy, modify or bundle the file.

GUI settings and pairing state use `~/Library/Application Support/net.c64bridge.C64-Keyboard-Bridge` by default, shared with the CLI. The app's public bundle identifier is separate from this existing data-directory name. To share a custom CLI state directory, pass the same `--state-dir` to the companion or set `BRIDGE_STATE_DIR` in its launch environment; selecting an `.env` file does not choose the GUI instance's state directory. **Remember password** is optional. When it is off, enter the password again after quitting and reopening unless a readable imported `.env` or process environment supplies it.

## Connection and input

1. On **Connection**, enter the C64 Ultimate's current address. On **Settings / About**, enter its network password and choose a matching US/UK layout, then save or apply the settings. A selected `.env` file and process environment remain available; explicit companion settings override them. Saving settings does not rewrite `.env`.
2. Start hosting with input disabled. Existing CLI hosting in the same state directory is shown as externally owned. You can explicitly pause, resume or stop it, but quitting the companion does not stop that CLI process.
3. If the keyboard program needs deployment, choose the client or networking probe operation. Each load/run confirmation explains that both reset the C64 and replace its current BASIC program. A failed ambiguous request is never retried automatically.
4. Choose **Enable Input**, then switch to an empty scratch document during the five-second countdown. The companion refuses activation if it retains focus or connection, permission or layout checks fail.

Returning focus to the companion pauses GUI-owned input and releases its keys. **Pause** is also available in the tray. The deliberate input test has its own confirmation and countdown and types `c64 bridge` followed by Return into the focused application.

Closing the window hides it only when a tray is available. Tray commands open the window, pause/enable, stop hosting or quit. On Linux, an unavailable tray keeps the window visible. Losing an established tray restores the window and pauses owned input. **Quit** cancels pending countdowns and waits for graceful runtime cleanup. A cleanup failure stays visible so you can retry; the application does not abandon held keys to force an exit.

Changing connection or mapping settings restarts GUI-owned hosting with input disabled and never deploys automatically. Settings for a CLI-owned session remain pending. Changed network destinations or pairing tokens require a freshly patched C64 program; deploy deliberately when ready. Pairing-token rotation invalidates previously configured clients.

## Keyboard and mappings

**Keyboard / Mappings** renders the C64's physical keyboard. Select a key to see its inherited mapping or edit modifier conditions, native chords, text actions and Shift consumption. CTRL and Commodore roles are configurable separately. The editor uses the same validated TOML format and resolution rules as the CLI; previewing a mapping never generates input.

Choose a built-in modern US, modern UK or authentic C64 base. Import an override, validate it, save a copy or export the edited TOML. Reset-to-base removes overrides. Invalid drafts are preserved for correction and do not replace an active valid profile. Quitting with unsaved edits offers Save, Discard and Cancel.

SHIFT LOCK uses the observed Shift matrix state and cannot be mapped independently. RESTORE is unavailable because it is not a verified matrix event. Backend capability notices identify unsupported text actions and authentic symbols. Select the matching host layout manually; the companion cannot make an unsupported layout safe by changing a profile.

## Passwords and diagnostics

**Remember password** is opt-in and uses macOS Keychain, Windows Credential Manager or Linux Secret Service. The private settings file stores a credential reference rather than a password. An unavailable or locked store leaves the supplied password in memory and reports the failure; there is no plaintext fallback. **Forget saved password** reports deletion failure rather than claiming success.

**Diagnostics** shows current phase, failure counters, retained session failure and cleanup outcome, plus a bounded activity history. Exported diagnostics omit passwords, pairing/control tokens, addresses, key identities, matrices and peer-supplied text. Permission checks identify whether the companion or CLI is the posting process. On macOS, grant Accessibility to the process actually sending events; authorizing a terminal for CLI use does not authorize the app automatically.

## Signing and acceptance

Public CI builds macOS artifacts without a Developer ID signature and Windows installers without a Windows signing certificate. Local macOS release preparation can sign with **Developer ID Application: Flame Lion Ltd**. Its public certificate identifies the publisher; the private key remains in the local Keychain and is never included in source, application packages or GitHub CI. DMG metadata distinguishes `unsigned`, `signed-only` and successfully `notarized` artifacts. Signed-only does not assert that Apple notarization passed.

GUI tests use recording services and private graphical fixtures. Existing CLI hardware observations do not establish native GUI input acceptance, permission prompts, real desktop tray behavior or OS credential-vault interaction on every platform. Those checks remain deliberate acceptance work. No automatic login startup, updater or additional Wayland-compositor support is included.
