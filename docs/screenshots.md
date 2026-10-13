# Screenshot gallery

Desktop images show the actual application renderer with isolated recording services. Hosting and input are off, passwords are empty and the visible loopback addresses belong to the fixture. These images demonstrate the interface, not physical keyboard acceptance. The [image manifest](screenshots/manifest.json) records origin, capture revision, dimensions and SHA-256 hashes.

## macOS

Connection controls and the Mac permission-settings link:

![Connection page on macOS with hosting stopped and input disabled](screenshots/macos-connection.png)

Physical keyboard and shared mapping preview:

![Keyboard mapping editor on macOS, isolated recording fixture](screenshots/macos-mappings.png)

Diagnostics for the GUI posting process:

![Diagnostics page on macOS with recording backend evidence](screenshots/macos-diagnostics.png)

Settings show Mac backends and Keychain instructions:

![Settings page on macOS showing Keychain and host repeat settings](screenshots/macos-settings.png)

Dark theme, selected only for this capture without changing the system theme:

![Dark Connection page on macOS, isolated recording fixture](screenshots/macos-dark.png)

## Ubuntu

Actual GUI captures on Ubuntu in a native Wayland session, using isolated recording services. Linux controls show GNOME portal guidance, the current backend choices and desktop-controlled repeat.

![Connection page on Ubuntu, with hosting and input off](screenshots/ubuntu-connection.png)

![Keyboard mapping editor on Ubuntu, isolated recording fixture](screenshots/ubuntu-mappings.png)

![Diagnostics page on Ubuntu with recording-backend evidence](screenshots/ubuntu-diagnostics.png)

![Settings page on Ubuntu with Linux backend and Secret Service instructions](screenshots/ubuntu-settings.png)

![Dark Connection page on Ubuntu, isolated recording fixture](screenshots/ubuntu-dark.png)

## C64 programs in VICE

These are the genuine, unconfigured release templates running in VICE 3.10. They display deployment instructions because no pairing token or network destination is embedded. VICE does not emulate Ultimate UCI, so these images do not show or claim successful networking. The original 384 × 272 pixel proportions are preserved.

![Genuine C64 keyboard-client startup in VICE, showing unconfigured deployment instructions](screenshots/c64-keyboard-startup.png)

![Genuine C64 networking-probe startup in VICE, showing unconfigured deployment instructions](screenshots/c64-probe-startup.png)

Real connected-client and successful-probe captures remain pending a separately approved hardware session. Capturing an existing video stream may be possible on suitable firmware; loading either PRG still requires fresh reset consent.

## Reproduce isolated captures

```sh
python3 scripts/gui-smoke.py --binary target/debug/c64-keyboard-bridge-gui --theme light --output target/captures/light
python3 scripts/gui-smoke.py --binary target/debug/c64-keyboard-bridge-gui --theme dark --output target/captures/dark
python3 scripts/c64-screenshots.py --output target/captures/c64
```

The GUI helper uses memory credentials and recording services. Linux runs in a private authenticated Xvfb display and D-Bus session. VICE requires separately installed, legally obtained C64 ROMs; no ROMs are distributed in this project.
