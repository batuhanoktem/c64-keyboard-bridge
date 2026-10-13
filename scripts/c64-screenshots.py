#!/usr/bin/env python3
"""Capture genuine unconfigured PRG screens in VICE; no UCI or hardware claims."""
import argparse
import json
import os
from pathlib import Path
import shutil
import subprocess
import tempfile

ROOT = Path(__file__).resolve().parent.parent


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prgs', type=Path, default=ROOT / 'target/c64-prgs')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/c64-screenshots')
    args = parser.parse_args()
    emulator = os.environ.get('VICE', 'x64sc')
    if not shutil.which(emulator):
        parser.error('Install VICE x64sc and legally obtained ROMs, or set VICE. ROMs are never packaged.')
    prgs = args.prgs.resolve()
    subprocess.run([shutil.which('python3') or 'python', str(ROOT / 'scripts/verify-prgs.py'), str(prgs)], check=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    screens = []
    with tempfile.TemporaryDirectory(prefix='c64-keyboard-bridge-capture-') as directory:
        for name, label in [('KEYBRIDGE', 'keyboard-startup'), ('NETWORKTEST', 'probe-startup')]:
            image = output / (label + '.png')
            image.unlink(missing_ok=True)
            commands = Path(directory) / 'capture.mon'
            commands.write_text(f'load "{prgs / (name + ".PRG")}" 0\ng 080d\n')
            result = subprocess.run([emulator, '-console', '+sound', '-warp', '-limitcycles', '5000000',
                                     '-initbreak', 'ready', '-moncommands', str(commands), '-exitscreenshot', str(image)],
                                    capture_output=True, text=True, timeout=30)
            # VICE deliberately exits with code 1 on its bounded cycle limit.
            if result.returncode not in (0, 1) or not image.is_file() or not image.read_bytes().startswith(b'\x89PNG\r\n\x1a\n'):
                raise RuntimeError('VICE did not capture the expected PNG; check local VICE/ROM installation.')
            screens.append({'file': image.name, 'prg': name + '.PRG', 'origin': 'VICE emulator',
                            'state': 'genuine unconfigured startup; UCI is not emulated'})
    (output / 'capture.json').write_text(json.dumps({'screens': screens, 'hardware_contacted': False}, indent=2) + '\n')
    print('PASS two genuine C64 PRG startup screenshots in VICE; no hardware or UCI acceptance asserted')


if __name__ == '__main__':
    main()
