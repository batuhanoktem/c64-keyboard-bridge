#!/usr/bin/env python3
"""Portable capture provenance checks; no emulator, ROMs, input or hardware used."""
import hashlib
import json
import os
from pathlib import Path
import runpy
import struct
import subprocess
import tempfile
import unittest
from unittest.mock import patch
import zlib

ROOT = Path(__file__).resolve().parent.parent
CAPTURE = runpy.run_path(str(ROOT / 'scripts/c64-screenshots.py'))
VERIFY = runpy.run_path(str(ROOT / 'scripts/verify-prgs.py'))


def png(width, height):
    def chunk(kind, payload):
        body = kind + payload
        return struct.pack('>I', len(payload)) + body + struct.pack('>I', zlib.crc32(body))
    header = struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0)
    pixels = zlib.compress((b'\0' + bytes(width * 3)) * height)
    return b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', header) + chunk(b'IDAT', pixels) + chunk(b'IEND', b'')


class CaptureTests(unittest.TestCase):
    manifest = {'files': {'KEYBRIDGE.PRG': 'a' * 64, 'NETWORKTEST.PRG': 'b' * 64},
                'c64_source_sha256': 'c' * 64, 'cc65_revision': 'd' * 40}

    def capture(self, root, standard='ntsc', backend=None):
        if backend is None:
            backend = self.fake_vice
        with patch.object(CAPTURE['capture'].__globals__['subprocess'], 'run', side_effect=backend):
            return CAPTURE['capture']('fixture-vice', root / 'unconfigured-prgs', root / 'screens',
                                      standard, self.manifest)

    def fake_vice(self, command, **kwargs):
        private = kwargs['cwd']
        config = Path(command[command.index('-config') + 1])
        self.assertEqual(config.parent, private)
        self.assertEqual(config.read_text(), '[C64SC]\n')
        self.assertIn('-default', command)
        self.assertIn('+saveres', command)
        self.assertIn('+remotemonitor', command)
        self.assertIn('+binarymonitor', command)
        self.assertIn('+ethernetcart', command)
        self.assertIn('+IDE64USB', command)
        for setting in ('XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_CACHE_HOME'):
            directory = Path(kwargs['env'][setting])
            self.assertEqual(directory.parent, private)
            self.assertTrue(directory.is_dir())
        image = Path(command[command.index('-exitscreenshot') + 1])
        ntsc = '-ntsc' in command
        self.assertIn('-power60' if ntsc else '-power50', command)
        image.write_bytes(png(384, 247 if ntsc else 272))
        model = 'MOS8562' if ntsc else 'MOS8565'
        log = f'*** VICE Version 3.10 ***\nVIC-II: Initializing chip model "{model}"\n'
        return subprocess.CompletedProcess(command, 1, log, 'cycle limit reached.')

    def test_native_dimensions_are_preserved_and_private_paths_are_excluded(self):
        for standard, height in (('ntsc', 247), ('pal', 272)):
            with self.subTest(standard=standard), tempfile.TemporaryDirectory() as directory:
                root = Path(directory) / 'sentinel-secret-address-token-password'
                root.mkdir()
                report = self.capture(root, standard)
                self.assertEqual(report['video_standard'], standard.upper())
                self.assertIs(report['hardware_contacted'], False)
                self.assertIs(report['native_input'], False)
                self.assertIs(report['uci_emulated'], False)
                serialized = (root / 'screens/capture.json').read_text()
                self.assertEqual(json.loads(serialized), report)
                self.assertNotIn('sentinel-secret', serialized)
                self.assertNotIn(str(root), serialized)
                for entry in report['screens']:
                    data = (root / 'screens' / entry['file']).read_bytes()
                    self.assertEqual((entry['width'], entry['height']), (384, height))
                    self.assertEqual(data, png(384, height))
                    self.assertEqual(entry['sha256'], hashlib.sha256(data).hexdigest())
                    self.assertEqual(entry['template_prg_sha256'], self.manifest['files'][entry['prg']])
                    if os.name != 'nt':
                        self.assertEqual((root / 'screens' / entry['file']).stat().st_mode & 0o777, 0o600)

    def test_rejects_a_different_emulated_video_standard(self):
        def different_standard(*args, **kwargs):
            result = self.fake_vice(*args, **kwargs)
            result.stdout = result.stdout.replace('MOS8562', 'MOS8565')
            return result
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(RuntimeError, 'video standard'):
                self.capture(root, backend=different_standard)
            self.assertFalse((root / 'screens/capture.json').exists())

    def test_failed_emulator_exit_cannot_produce_a_verified_report(self):
        def failed(*args, **kwargs):
            result = self.fake_vice(*args, **kwargs)
            result.stderr = 'ROM or startup failure'
            return result
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            with self.assertRaisesRegex(RuntimeError, 'did not capture'):
                self.capture(root, backend=failed)
            self.assertFalse((root / 'screens/capture.json').exists())

    def test_existing_evidence_is_never_overwritten(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            screens = root / 'screens'
            screens.mkdir()
            image = screens / 'keyboard-startup.png'
            image.write_bytes(b'existing evidence')
            with self.assertRaises(FileExistsError):
                self.capture(root)
            self.assertEqual(image.read_bytes(), b'existing evidence')

    def test_rejects_corrupt_or_unbounded_png_headers(self):
        valid = png(384, 247)
        for invalid in (b'not PNG', valid[:24], valid[:-12], valid + b'trailing bytes',
                        valid[:20] + b'\0\0\0\1' + valid[24:], png(4096, 1)):
            with self.subTest(header=invalid[:24]), self.assertRaises(ValueError):
                CAPTURE['png_dimensions'](invalid)

    def test_configured_prgs_are_rejected_by_the_shared_verifier(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            template = b'\x01\x08' + bytes(32) + VERIFY['UNCONFIGURED'] + bytes(32)
            for name in VERIFY['NAMES']:
                (root / name).write_bytes(template)
            VERIFY['inspect'](root)
            configured = bytearray(template)
            configured[configured.index(VERIFY['MARKER']) + 10] = 192
            (root / 'KEYBRIDGE.PRG').write_bytes(configured)
            with self.assertRaisesRegex(ValueError, 'configured'):
                VERIFY['inspect'](root)

    def test_monitor_command_delimiters_cannot_change_the_program(self):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory) / 'invalid"\nquit'
            with patch.object(subprocess, 'run') as run, \
                    patch.object(tempfile, 'TemporaryDirectory') as private:
                with self.assertRaisesRegex(ValueError, 'delimiters'):
                    CAPTURE['capture']('fixture-vice', root / 'unconfigured-prgs', root / 'screens',
                                       'pal', self.manifest)
                run.assert_not_called()
                private.assert_not_called()
            self.assertEqual(list(Path(directory).iterdir()), [])


if __name__ == '__main__':
    unittest.main()
