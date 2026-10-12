#!/usr/bin/env python3
"""Extract/install native packages in private test directories, without starting either frontend."""
import argparse
import hashlib
import json
from pathlib import Path
import platform
import plistlib
import runpy
import shutil
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent
PRGS = runpy.run_path(str(ROOT / 'scripts/verify-prgs.py'))


def run(*args, **kwargs):
    return subprocess.run([str(arg) for arg in args], check=True, timeout=120, **kwargs)


def verify_payload(path, windows=False):
    provenance = json.loads((path / "build-provenance.json").read_text())
    for name in ("c64bridge", "c64bridge-gui"):
        name += ".exe" if windows else ""
        executable = path / name
        if hashlib.sha256(executable.read_bytes()).hexdigest() != provenance["executables"][name]:
            raise ValueError(f"Installer executable does not match portable build: {name}")
    if PRGS['verify'](path / 'c64') != provenance['prgs']:
        raise ValueError('Installer PRGs do not match portable package provenance')
    for name in ("LICENSE", "THIRD_PARTY_NOTICES.md", "third-party/rust-crates/inventory.json",
                 "third-party/rust-runtime/COPYRIGHT-library.html"):
        if not (path / name).is_file():
            raise ValueError(f"Installer omitted a required resource: {name}")
    return provenance


def linux_check(directory):
    for package in sorted(directory.glob('*.deb')):
        with tempfile.TemporaryDirectory(prefix="c64bridge-install-") as temporary:
            root = Path(temporary) / "root"
            root.mkdir()
            sentinel = root / "home/sandbox/.config/c64bridge/preserved-settings"
            sentinel.parent.mkdir(parents=True)
            sentinel.write_text("PRIVATE_USER_SENTINEL")
            control = Path(temporary) / "control"
            run("dpkg-deb", "--control", package, control)
            unexpected = {path.name for path in control.iterdir()} - {"control"}
            if unexpected:
                raise ValueError("Installer contains unexpected maintainer scripts or permissions changes")
            args = ["dpkg", "--root", root, "--log", root / "dpkg.log",
                    "--force-not-root", "--force-bad-path", "--force-depends"]
            run(*args, "--install", package)
            verify_payload(root / "usr/lib/c64-keyboard-bridge")
            for name in ("c64bridge", "c64bridge-gui"):
                if not (root / "usr/bin" / name).is_symlink():
                    raise ValueError("Installed command launcher is missing")
            if not (root / "usr/share/applications/net.c64bridge.desktop.desktop").is_file():
                raise ValueError("Installed desktop launcher is missing")
            run(*args, "--purge", "c64-keyboard-bridge")
            if (root / "usr/lib/c64-keyboard-bridge").exists():
                raise ValueError("Uninstall left program files behind")
            if sentinel.read_text() != "PRIVATE_USER_SENTINEL":
                raise ValueError("Uninstall altered user settings")
            print(f"PASS isolated DEB install/uninstall: {package.name}")


def windows_check(directory):
    for package in sorted(directory.glob('*-setup.exe')):
        with tempfile.TemporaryDirectory(prefix="c64bridge-install-") as temporary:
            install = Path(temporary) / "program"
            sentinel = Path(temporary) / "private-user-settings"
            sentinel.write_text("PRIVATE_USER_SENTINEL")
            try:
                # NSIS requires /D to be the final parameter and consumes the remainder as the path.
                subprocess.run(f'"{package}" /S /D={install}', check=True, timeout=120)
                verify_payload(install, windows=True)
                (install / '.env').write_text('PRIVATE_USER_SENTINEL')
                if not (install / "third-party/nsis/COPYING").is_file():
                    raise ValueError("Installer omitted NSIS/zlib notices")
            finally:
                uninstaller = install / "uninstall.exe"
                if uninstaller.is_file():
                    subprocess.run(f'"{uninstaller}" /S', check=True, timeout=120)
            deadline = time.monotonic() + 10
            while (install / 'c64bridge-gui.exe').exists() and time.monotonic() < deadline:
                time.sleep(.1)
            if not install.exists() or {path.name for path in install.iterdir()} != {'.env'}:
                raise ValueError("Uninstall left program files behind")
            if sentinel.read_text() != "PRIVATE_USER_SENTINEL" or (install / '.env').read_text() != 'PRIVATE_USER_SENTINEL':
                raise ValueError("Uninstall altered user settings")
            print(f"PASS per-user NSIS install/uninstall: {package.name}")


def macos_check(directory):
    for package in sorted(directory.glob('*.dmg')):
        with tempfile.TemporaryDirectory(prefix="c64bridge-mount-") as temporary:
            mount = Path(temporary)
            attached = False
            try:
                run("hdiutil", "attach", "-readonly", "-nobrowse", "-mountpoint", mount, package)
                attached = True
                app = mount / "C64 Keyboard Bridge.app"
                info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
                if info["CFBundleIdentifier"] != "net.c64bridge.desktop":
                    raise ValueError("Wrong macOS application identity")
                resources = app / "Contents/Resources"
                provenance = json.loads((resources / "build-provenance.json").read_text())
                if PRGS['verify'](resources / 'c64') != provenance['prgs']:
                    raise ValueError('DMG PRGs do not match portable package provenance')
                for name in ("c64bridge", "c64bridge-gui"):
                    executable = app / "Contents/MacOS" / name
                    if not executable.is_file():
                        raise ValueError("macOS app omitted a frontend")
                    if provenance["signing"] == "unsigned" and hashlib.sha256(executable.read_bytes()).hexdigest() != provenance["executables"][name]:
                        raise ValueError("DMG executable does not match portable build")
                    if provenance['signing'] != 'unsigned':
                        metadata = json.loads(package.with_suffix('.metadata.json').read_text())
                        if hashlib.sha256(executable.read_bytes()).hexdigest() != metadata['app_executables'][name]:
                            raise ValueError('Signed DMG executable does not match its signed-artifact metadata')
                if not (mount / "Applications").is_symlink() or (mount / "Applications").readlink() != Path("/Applications"):
                    raise ValueError("DMG omitted the drag-to-Applications target")
                if provenance["signing"] != "unsigned":
                    run("codesign", "--verify", "--deep", "--strict", app)
                print(f"PASS read-only DMG extraction: {package.name}")
            finally:
                if attached:
                    run("hdiutil", "detach", mount)


if __name__ == '__main__':
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('directory', type=Path)
    args = parser.parse_args()
    try:
        {"Darwin": macos_check, "Windows": windows_check, "Linux": linux_check}[platform.system()](args.directory.resolve())
    except (OSError, ValueError, KeyError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
