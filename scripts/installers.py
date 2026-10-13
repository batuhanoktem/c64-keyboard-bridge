#!/usr/bin/env python3
"""Native installer builders. Imported by package.py; never reads user configuration."""
import json
import os
from pathlib import Path
import plistlib
import runpy
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent
APP_NAME = "C64 Keyboard Bridge.app"
BUNDLE_ID = "com.flameliongames.c64keyboardbridge"
SIGNING_IDENTITY = "Developer ID Application: Flame Lion Ltd (TUGY329T35)"


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, **kwargs)


def macos_bundle(payload, version, minimum):
    app = payload / APP_NAME
    executables = app / "Contents/MacOS"
    resources = app / "Contents/Resources"
    executables.mkdir(parents=True)
    resources.mkdir(parents=True)
    for name in ("c64-keyboard-bridge", "c64-keyboard-bridge-gui"):
        shutil.copy2(payload / name, executables / name)
    for path in list(payload.iterdir()):
        if path == app or path.name in {"c64-keyboard-bridge", "c64-keyboard-bridge-gui"}:
            continue
        if path.is_dir():
            shutil.copytree(path, resources / path.name)
        else:
            shutil.copy2(path, resources / path.name)
    shutil.copy2(ROOT / "assets/c64bridge.icns", resources / "c64bridge.icns")
    info = {
        "CFBundleDevelopmentRegion": "en", "CFBundleExecutable": "c64-keyboard-bridge-gui",
        "CFBundleIdentifier": BUNDLE_ID, "CFBundleName": "C64 Keyboard Bridge",
        "CFBundleDisplayName": "C64 Keyboard Bridge", "CFBundlePackageType": "APPL",
        "CFBundleShortVersionString": version, "CFBundleVersion": version,
        "CFBundleIconFile": "c64bridge.icns", "LSMinimumSystemVersion": minimum,
        "NSPrincipalClass": "NSApplication", "NSHighResolutionCapable": True,
        "NSAccessibilityUsageDescription": "Send keys from your paired C64 keyboard when you explicitly enable input.",
        "NSLocalNetworkUsageDescription": "Connect to your configured C64 Ultimate on the local network and receive its keyboard data when you explicitly start hosting.",
        "NSHumanReadableCopyright": "GPL-3.0-only; C64 Keyboard Bridge contributors",
    }
    (app / "Contents/Info.plist").write_bytes(plistlib.dumps(info, sort_keys=True))
    return app


def validate_signing_identity(identity):
    if os.environ.get("CI") or os.environ.get("GITHUB_ACTIONS"):
        raise ValueError("Signing is local-only; no Apple private keys belong in public CI")
    if identity != SIGNING_IDENTITY:
        raise ValueError(f"Use the approved signing identity: {SIGNING_IDENTITY}")
    identities = run("security", "find-identity", "-v", "-p", "codesigning",
                     capture_output=True, text=True).stdout
    if f'"{identity}"' not in identities:
        raise ValueError("Approved Developer ID identity is not available in this Mac's Keychain")


def macos_sign_binaries(directory, identity):
    validate_signing_identity(identity)
    for binary in ("c64-keyboard-bridge", "c64-keyboard-bridge-gui"):
        run("codesign", "--force", "--sign", identity, "--options", "runtime", "--timestamp",
            directory / binary)
        run("codesign", "--verify", "--strict", directory / binary)


def macos_sign(app, identity):
    macos_sign_binaries(app / "Contents/MacOS", identity)
    run("codesign", "--force", "--sign", identity, "--options", "runtime", "--timestamp", app)
    run("codesign", "--verify", "--deep", "--strict", "--verbose=2", app)


def macos_dmg(app, output, identity=None, notary_profile=None):
    with tempfile.TemporaryDirectory(prefix="c64bridge-dmg-", dir=ROOT / "target/package-staging") as temporary:
        stage = Path(temporary)
        shutil.copytree(app, stage / app.name, symlinks=True)
        (stage / "Applications").symlink_to("/Applications")
        run("hdiutil", "create", "-volname", "C64 Keyboard Bridge", "-srcfolder", stage,
            "-format", "UDZO", "-ov", output)
    status = "unsigned"
    if identity:
        run("codesign", "--force", "--sign", identity, "--timestamp", output)
        run("codesign", "--verify", "--strict", "--verbose=2", output)
        status = "signed-only"
    if notary_profile:
        if not identity:
            raise ValueError("Notarization requires explicitly requested Developer ID signing")
        result = run("xcrun", "notarytool", "submit", output, "--keychain-profile", notary_profile,
                     "--wait", "--output-format", "json", capture_output=True, text=True)
        response = json.loads(result.stdout)
        if response.get("status") != "Accepted":
            raise ValueError("Apple did not accept this DMG for notarization; it is not labelled notarized")
        run("xcrun", "stapler", "staple", output)
        run("xcrun", "stapler", "validate", output)
        status = "notarized"
    return status


def windows_installer(payload, output, version):
    compiler = runpy.run_path(str(ROOT / "scripts/bootstrap-nsis.py"))["bootstrap"]()
    # The official compiler's stub and zlib compression are included in the EXE.
    notices = payload / "third-party/nsis"
    notices.mkdir(parents=True, exist_ok=True)
    shutil.copy2(ROOT / "installers/nsis-notices/COPYING", notices / "COPYING")
    with tempfile.TemporaryDirectory(prefix="c64bridge-nsis-", dir=ROOT / "target/package-staging") as temporary:
        manifest = Path(temporary) / "uninstall-files.nsh"
        lines = []
        # Delete only the installed inventory; user-created .env and other files are preserved.
        def escaped(path):
            return path.relative_to(payload).as_posix().replace('/', '\\').replace('$', '$$').replace('"', '$\\"')
        for path in sorted(payload.rglob('*')):
            if path.is_file():
                lines.append(f'  Delete "$INSTDIR\\{escaped(path)}"')
        for path in sorted((path for path in payload.rglob('*') if path.is_dir()),
                           key=lambda path: len(path.parts), reverse=True):
            lines.append(f'  RMDir "$INSTDIR\\{escaped(path)}"')
        manifest.write_text('\n'.join(lines) + '\n')
        run(compiler, "/WX", "/V3", f"/DPAYLOAD={payload}", f"/DOUTPUT={output}",
            f"/DVERSION={version}", f"/DICON={ROOT / 'assets/c64bridge.ico'}",
            f"/DUNINSTALL_FILES={manifest}", ROOT / "installers/windows/c64bridge.nsi")


def linux_deb(payload, output, version, architecture):
    with tempfile.TemporaryDirectory(prefix="c64bridge-deb-", dir=ROOT / "target/package-staging") as temporary:
        stage = Path(temporary)
        library = stage / "usr/lib/c64-keyboard-bridge"
        shutil.copytree(payload, library)
        executable = stage / "usr/bin"
        executable.mkdir(parents=True)
        for name in ("c64-keyboard-bridge", "c64-keyboard-bridge-gui"):
            (executable / name).symlink_to(f"../lib/c64-keyboard-bridge/{name}")
        desktop = stage / "usr/share/applications"
        desktop.mkdir(parents=True)
        shutil.copy2(ROOT / "installers/linux/com.flameliongames.c64keyboardbridge.desktop",
                     desktop / "com.flameliongames.c64keyboardbridge.desktop")
        icons = stage / "usr/share/icons/hicolor/256x256/apps"
        icons.mkdir(parents=True)
        shutil.copy2(ROOT / "assets/c64bridge.png", icons / "com.flameliongames.c64keyboardbridge.png")
        scalable = stage / "usr/share/icons/hicolor/scalable/apps"
        scalable.mkdir(parents=True)
        shutil.copy2(ROOT / "assets/c64bridge.svg", scalable / "com.flameliongames.c64keyboardbridge.svg")
        control = stage / "DEBIAN"
        control.mkdir()
        size = sum(path.stat().st_size for path in stage.rglob('*') if path.is_file()) // 1024
        (control / "control").write_text(
            f"Package: c64-keyboard-bridge\nVersion: {version}\nArchitecture: {architecture}\n"
            "Maintainer: C64 Keyboard Bridge contributors <noreply@github.com>\n"
            "Section: utils\nPriority: optional\n"
            f"Installed-Size: {size}\n"
            "Depends: libc6 (>= 2.39), libgcc-s1, libegl1, libgl1, libx11-6, libxcb1, "
            "libxkbcommon0, libxkbcommon-x11-0, libxi6, libxrandr2, libxcursor1, "
            "libwayland-client0, libwayland-cursor0, libwayland-egl1\n"
            "Recommends: gnome-keyring | kwallet6\n"
            "Homepage: https://github.com/batuhanoktem/c64-keyboard-bridge\n"
            "Description: C64 Ultimate keyboard bridge with a desktop companion and CLI\n"
            " Maps the C64 keyboard to native input through authenticated UCI TCP.\n"
            " Hosting and input start disabled. Installation does not grant uinput access.\n")
        run("dpkg-deb", "--root-owner-group", "-Zgzip", "--build", stage, output)
