#!/usr/bin/env python3
"""Capture genuine unconfigured PRG screens in VICE; no UCI or hardware claims."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import re
import shutil
import struct
import subprocess
import sys
import tempfile
import zlib

ROOT = Path(__file__).resolve().parent.parent
SCREENS = (('KEYBRIDGE', 'keyboard-startup'), ('NETWORKTEST', 'probe-startup'))
CHIP_MODELS = {'ntsc': {'MOS8562', 'MOS6567', 'MOS6567R56A'},
               'pal': {'MOS8565', 'MOS6569', 'MOS6569R1'}}


def png_dimensions(data):
    """Read the exporter's native pixel extent without decoding or editing it."""
    if not 33 <= len(data) <= 8 * 1024 * 1024 or data[:16] != b'\x89PNG\r\n\x1a\n\0\0\0\rIHDR':
        raise ValueError('VICE did not export a PNG with a valid IHDR')
    if zlib.crc32(data[12:29]) != struct.unpack('>I', data[29:33])[0]:
        raise ValueError('VICE exported an invalid PNG header checksum')
    if data[-12:] != b'\0\0\0\0IEND\xaeB\x60\x82':
        raise ValueError('VICE exported an incomplete PNG or trailing bytes')
    width, height = struct.unpack('>II', data[16:24])
    if not 0 < width <= 2048 or not 0 < height <= 2048:
        raise ValueError('VICE exported an unexpected image extent')
    return width, height


def capture_command(emulator, directory, image, standard):
    return [emulator, '-default', '-config', str(directory / 'vice.cfg'), '+saveres',
            '-console', '+sound', '-' + standard, '-power60' if standard == 'ntsc' else '-power50',
            '-VICIIborders', '0', '-VICIIintpal', '-VICIIfilter', '0',
            '+remotemonitor', '+binarymonitor', '+ethernetcart', '+IDE64USB',
            '-userportdevice', '0', '-warp', '-limitcycles', '5000000', '-initbreak', 'ready',
            '-moncommands', str(directory / 'capture.mon'), '-exitscreenshot', str(image)]


def capture(emulator, prgs, output, standard, manifest):
    images = [output / (label + '.png') for _, label in SCREENS]
    if any(path.exists() for path in [*images, output / 'capture.json']):
        raise FileExistsError('Capture output exists; choose a fresh output directory to preserve evidence')
    output.mkdir(parents=True, exist_ok=True, mode=0o700)
    screens = []
    with tempfile.TemporaryDirectory(prefix='c64-keyboard-bridge-capture-') as temporary:
        directory = Path(temporary)
        # An explicit empty private config plus defaults avoids inherited peripherals.
        # +saveres and the private cwd prevent changes to the user's emulator settings.
        (directory / 'vice.cfg').write_text('[C64SC]\n')
        for name in ('config', 'data', 'cache'):
            (directory / name).mkdir(mode=0o700)
        environment = os.environ.copy()
        environment.update({'XDG_CONFIG_HOME': str(directory / 'config'),
                            'XDG_DATA_HOME': str(directory / 'data'),
                            'XDG_CACHE_HOME': str(directory / 'cache')})
        for (name, label), image in zip(SCREENS, images):
            commands = directory / 'capture.mon'
            load_path = (prgs / (name + '.PRG')).as_posix()
            if any(character in load_path for character in ('"', '\r', '\n')):
                raise ValueError('PRG directory cannot contain monitor command delimiters')
            commands.write_text(f'load "{load_path}" 0\ng 080d\n')
            result = subprocess.run(capture_command(emulator, directory, image, standard),
                                    cwd=directory, env=environment,
                                    capture_output=True, text=True, timeout=30)
            log = result.stdout + result.stderr
            # VICE deliberately exits with code 1 at this bounded cycle limit.
            if (result.returncode not in (0, 1) or not image.is_file()
                    or result.returncode == 1 and 'cycle limit reached.' not in log):
                raise RuntimeError('VICE did not capture the expected PNG; check local VICE/ROM installation')
            version = re.search(r'\*\*\* VICE Version ([0-9]+(?:\.[0-9]+)+) \*\*\*', log)
            models = re.findall(r'VIC-II: Initializing chip model "([A-Za-z0-9]+)"', log)
            if not version or not models or models[-1] not in CHIP_MODELS[standard]:
                raise RuntimeError('VICE did not confirm the selected video standard and build version')
            data = image.read_bytes()
            width, height = png_dimensions(data)
            image.chmod(0o600)
            screens.append({'file': image.name, 'width': width, 'height': height,
                            'sha256': hashlib.sha256(data).hexdigest(),
                            'prg': name + '.PRG', 'template_prg_sha256': manifest['files'][name + '.PRG'],
                            'origin': 'VICE emulator', 'emulator_version': version.group(1),
                            'video_standard': standard.upper(), 'vic_model': models[-1],
                            'palette': 'VICE internal calculated palette; no CRT filter',
                            'border_mode': 'normal',
                            'decoder': 'VICE built-in PNG exitscreenshot exporter; pixels unmodified',
                            'state': 'genuine unconfigured startup; UCI is not emulated'})
    report = {'schema': 1, 'video_standard': standard.upper(), 'screens': screens,
              'capture_tool_sha256': hashlib.sha256(Path(__file__).read_bytes()).hexdigest(),
              'c64_source_sha256': manifest['c64_source_sha256'],
              'cc65_revision': manifest['cc65_revision'],
              'configuration': 'isolated defaults and private config; settings not saved',
              'hardware_contacted': False, 'native_input': False, 'uci_emulated': False}
    report_path = output / 'capture.json'
    report_path.write_text(json.dumps(report, indent=2) + '\n')
    report_path.chmod(0o600)
    return report


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--prgs', type=Path, default=ROOT / 'target/c64-prgs')
    parser.add_argument('--output', type=Path, default=ROOT / 'target/c64-screenshots')
    parser.add_argument('--video-standard', choices=('pal', 'ntsc'), default='pal',
                        help='Explicit emulator video standard (default: pal); preserve native PNG dimensions')
    args = parser.parse_args()
    emulator = shutil.which(os.environ.get('VICE', 'x64sc'))
    if not emulator:
        parser.error('Install VICE x64sc and legally obtained ROMs, or set VICE. ROMs are never packaged.')
    prgs = args.prgs.resolve()
    subprocess.run([sys.executable, str(ROOT / 'scripts/verify-prgs.py'), str(prgs)], check=True)
    manifest = json.loads((prgs / 'prg-manifest.json').read_text())
    capture(emulator, prgs, args.output.resolve(), args.video_standard, manifest)
    print('PASS two genuine C64 PRG startup screenshots in VICE ' + args.video_standard.upper()
          + '; native pixel dimensions preserved; no hardware or UCI acceptance asserted')


if __name__ == '__main__':
    main()
