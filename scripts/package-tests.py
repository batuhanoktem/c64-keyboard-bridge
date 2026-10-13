#!/usr/bin/env python3
"""Portable packaging contract tests without compiling, signing or contacting hardware."""
import hashlib
import json
from pathlib import Path
import plistlib
import runpy
import re
import struct
import tempfile
import unittest

ROOT = Path(__file__).resolve().parent.parent
PACKAGE = runpy.run_path(str(ROOT / "scripts/package.py"))
INSTALLERS = runpy.run_path(str(ROOT / "scripts/installers.py"))


class PackagingTests(unittest.TestCase):
    def test_documentation_images_are_packaged_and_match_their_manifest(self):
        names = {path.relative_to(ROOT).as_posix() for path in PACKAGE['source_files']()}
        manifest = json.loads((ROOT / 'docs/screenshots/manifest.json').read_text())
        seen = set()
        for entry in manifest['screenshots']:
            self.assertRegex(entry['file'], r'^[a-z0-9-]+\.png$')
            self.assertNotIn(entry['file'], seen)
            seen.add(entry['file'])
            image = ROOT / 'docs/screenshots' / entry['file']
            self.assertIn(image.relative_to(ROOT).as_posix(), names)
            data = image.read_bytes()
            self.assertEqual(data[:8], b'\x89PNG\r\n\x1a\n')
            self.assertEqual(struct.unpack('>II', data[16:24]), (entry['width'], entry['height']))
            self.assertEqual(hashlib.sha256(data).hexdigest(), entry['sha256'])
            self.assertFalse(entry['hardware_contacted'])
        self.assertEqual(seen, {path.name for path in (ROOT / 'docs/screenshots').glob('*.png')})
        self.assertIn('docs/screenshots/manifest.json', names)
        for document in [ROOT / 'README.md', *list((ROOT / 'docs').glob('*.md'))]:
            for reference in re.findall(r'!\[[^\]]*\]\(([^)]+)\)', document.read_text()):
                if not reference.startswith(('https://', 'http://')):
                    image = (document.parent / reference).resolve()
                    self.assertIn(image.relative_to(ROOT).as_posix(), names)

    def test_source_allowlist_contains_gui_build_inputs_and_no_private_state(self):
        names = {path.relative_to(ROOT).as_posix() for path in PACKAGE["source_files"]()}
        for name in ["assets/c64-keyboard-bridge.png", "assets/c64-keyboard-bridge.svg", "assets/c64-keyboard-bridge.ico",
                     "assets/c64-keyboard-bridge.icns", "assets/generate-icons.py", "installers/nsis-notices/COPYING",
                     "installers/windows/c64-keyboard-bridge.nsi", "installers/linux/com.flameliongames.c64keyboardbridge.desktop"]:
            self.assertIn(name, names)
        self.assertTrue(any(name.startswith("crates/bridge-gui/") for name in names))
        for name in ("CHANGELOG.md", "CONTRIBUTING.md", "SECURITY.md", "docs/build-and-release.md",
                     ".github/ISSUE_TEMPLATE/bug.yml", ".github/PULL_REQUEST_TEMPLATE.md"):
            self.assertIn(name, names)
        self.assertFalse(any(name == ".env" or name.startswith("target/")
                             or Path(name).name in {"pairing-token", "runtime.json", "gui-runtime.json", "gui-settings.toml"}
                             for name in names))

    def test_retired_maintainer_guides_cannot_reenter_packages(self):
        function = PACKAGE["source_files"]
        original = function.__globals__["ROOT"]
        with tempfile.TemporaryDirectory() as directory:
            fixture = Path(directory)
            (fixture / "docs").mkdir()
            for name in ("secure-releases.md", "store-distribution.md", "build-and-release.md"):
                (fixture / "docs" / name).write_text("synthetic documentation fixture")
            try:
                function.__globals__["ROOT"] = fixture
                names = {path.relative_to(fixture).as_posix() for path in function()}
            finally:
                function.__globals__["ROOT"] = original
            self.assertIn("docs/build-and-release.md", names)
            self.assertNotIn("docs/secure-releases.md", names)
            self.assertNotIn("docs/store-distribution.md", names)

    def test_macos_bundle_contains_both_frontends_and_resources(self):
        with tempfile.TemporaryDirectory() as directory:
            payload = Path(directory)
            for name in ("c64-keyboard-bridge", "c64-keyboard-bridge-gui"):
                (payload / name).write_text(name)
            (payload / "c64").mkdir()
            (payload / "c64/KEYBRIDGE.PRG").write_bytes(b"unconfigured fixture")
            (payload / "LICENSE").write_text("license fixture")
            app = INSTALLERS["macos_bundle"](payload, "0.1.0", "11.0")
            info = plistlib.loads((app / "Contents/Info.plist").read_bytes())
            self.assertEqual(info["CFBundleIdentifier"], "com.flameliongames.c64keyboardbridge")
            self.assertEqual(info["CFBundleExecutable"], "c64-keyboard-bridge-gui")
            self.assertEqual(info["LSMinimumSystemVersion"], "11.0")
            self.assertIn("configured C64 Ultimate", info["NSLocalNetworkUsageDescription"])
            for name in ("c64-keyboard-bridge", "c64-keyboard-bridge-gui"):
                self.assertEqual((app / "Contents/MacOS" / name).read_text(), name)
            self.assertEqual((app / "Contents/Resources/c64/KEYBRIDGE.PRG").read_bytes(), b"unconfigured fixture")
            self.assertFalse((app / "Contents/Resources" / app.name).exists())

    def test_embedded_font_notices_are_preserved(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            crate = root / "vendor/epaint_default_fonts-0.36.2"
            (crate / "fonts").mkdir(parents=True)
            (crate / "Cargo.toml").write_text('[package]\nname="epaint_default_fonts"\nversion="0.36.2"\nlicense="MIT OR Apache-2.0"\n')
            (crate / "fonts/Ubuntu-LICENCE.txt").write_text("Ubuntu font license sentinel")
            (crate / "fonts/OFL.txt").write_text("Open font license sentinel")
            (crate / "fonts/Hack-LICENSE").write_text("Hack font license sentinel")
            PACKAGE["collect_crate_notices"](root / "vendor", root / "notices")
            for file in crate.rglob('*'):
                if file.is_file():
                    self.assertEqual(file.read_bytes(), (root / "notices" / crate.name / file.relative_to(crate)).read_bytes())

    def test_notice_supplements_are_exact_and_in_corresponding_source(self):
        names = {path.relative_to(ROOT).as_posix() for path in PACKAGE['source_files']()}
        origins = json.loads((ROOT / 'third-party/notices/origins.json').read_text())
        for origin in origins['packages']:
            for notice in origin['notices']:
                path = ROOT / 'third-party/notices' / f"{origin['package']}-{origin['version']}" / notice['file']
                self.assertIn(path.relative_to(ROOT).as_posix(), names)
                self.assertEqual(hashlib.sha256(path.read_bytes()).hexdigest(), notice['sha256'])

    def test_binary_notices_reject_a_different_published_crate_revision(self):
        origins = json.loads((ROOT / 'third-party/notices/origins.json').read_text())
        origin = next(item for item in origins['packages'] if item['package'] == 'eframe')
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            crate = root / 'vendor' / f"eframe-{origin['version']}"
            crate.mkdir(parents=True)
            (crate / 'Cargo.toml').write_text(f"[package]\nname='eframe'\nversion='{origin['version']}'\nlicense='MIT OR Apache-2.0'\n")
            vcs = crate / '.cargo_vcs_info.json'
            vcs.write_text(json.dumps({'git': {'sha1': origin['revision']}}))
            PACKAGE['collect_crate_notices'](root / 'vendor', root / 'notices')
            for notice in origin['notices']:
                original = ROOT / 'third-party/notices' / crate.name / notice['file']
                self.assertEqual(original.read_bytes(), (root / 'notices' / crate.name / 'upstream-supplement' / notice['file']).read_bytes())
            vcs.write_text(json.dumps({'git': {'sha1': '0' * 40}}))
            with self.assertRaisesRegex(ValueError, 'published.*revision'):
                PACKAGE['collect_crate_notices'](root / 'vendor', root / 'changed-notices')


if __name__ == "__main__":
    unittest.main()
