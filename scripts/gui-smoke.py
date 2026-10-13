#!/usr/bin/env python3
"""Launch the real desktop GUI in an isolated display/session with software-only services."""
import argparse
import json
import os
from pathlib import Path
import platform
import secrets
import shutil
import signal
import subprocess
import sys
import tempfile
import time

ROOT = Path(__file__).resolve().parent.parent


def wait_for(predicate, process, seconds=10):
    deadline = time.monotonic() + seconds
    while time.monotonic() < deadline:
        if predicate():
            return
        if process.poll() is not None:
            raise ValueError("GUI acceptance fixture exited before readiness")
        time.sleep(.05)
    raise ValueError("GUI acceptance fixture readiness timed out")


def terminate(process):
    if process.poll() is None:
        process.terminate()
        try:
            process.wait(timeout=5)
        except subprocess.TimeoutExpired:
            process.kill()
            process.wait(timeout=5)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', required=True, type=Path)
    parser.add_argument('--output', type=Path, default=ROOT / 'target/gui-smoke')
    parser.add_argument('--theme', choices=('light', 'dark'), default='light')
    parser.add_argument('--session-child', action='store_true', help=argparse.SUPPRESS)
    args = parser.parse_args()
    binary = args.binary.resolve()
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=True)
    if platform.system() == 'Linux' and not args.session_child:
        subprocess.run(['dbus-run-session', '--', sys.executable, __file__, '--binary', str(binary),
                        '--output', str(output), '--theme', args.theme, '--session-child'], check=True, timeout=90)
        return
    processes = []
    with tempfile.TemporaryDirectory(prefix='c64bridge-gui-smoke-') as temporary:
        private = Path(temporary)
        environment = os.environ.copy()
        # Ordinary startup reads .env and may inspect platform credentials. Smoke mode uses fake services;
        # a private working directory and user-storage roots add independent protection.
        environment.update({'XDG_CONFIG_HOME': str(private / 'config'), 'XDG_DATA_HOME': str(private / 'data'),
                            'XDG_CACHE_HOME': str(private / 'cache'), 'APPDATA': str(private / 'config'),
                            'LOCALAPPDATA': str(private / 'local'), 'LIBGL_ALWAYS_SOFTWARE': '1'})
        for name in list(environment):
            if name.startswith(('C64_', 'REPEAT_', 'KEYBOARD_', 'PAIRING_')):
                environment.pop(name)
        try:
            if platform.system() == 'Linux':
                for name in ('WAYLAND_DISPLAY', 'XDG_SESSION_TYPE'):
                    environment.pop(name, None)
                display = next(f':{number}' for number in range(180, 240)
                               if not Path(f'/tmp/.X11-unix/X{number}').exists() and not Path(f'/tmp/.X{number}-lock').exists())
                auth = private / 'Xauthority'
                auth.touch(mode=0o600)
                subprocess.run(['xauth', '-f', str(auth), 'add', display, '.', secrets.token_hex(16)], check=True)
                environment.update({'DISPLAY': display, 'XAUTHORITY': str(auth), 'XDG_SESSION_TYPE': 'x11'})
                xvfb = subprocess.Popen(['Xvfb', display, '-screen', '0', '1280x1200x24', '-nolisten', 'tcp', '-auth', str(auth)],
                                        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, env=environment)
                processes.append(xvfb)
                wait_for(lambda: Path('/tmp/.X11-unix/X' + display[1:]).exists(), xvfb)
                tray_report = private / 'tray.json'
                fixture = subprocess.Popen(['/usr/bin/python3', str(ROOT / 'scripts/gui-tray-fixture.py'), str(tray_report)],
                                           env=environment, cwd=private, stdout=subprocess.DEVNULL)
                processes.append(fixture)
                wait_for(tray_report.exists, fixture)
            report = output / 'gui-result.json'
            progress = report.with_suffix('.progress.json')
            screenshot = output / 'gui.png'
            images = [output / f'gui-{page}.png' for page in ('connection', 'diagnostics', 'settings')]
            report.unlink(missing_ok=True)
            progress.unlink(missing_ok=True)
            for image in [screenshot, *images]:
                image.unlink(missing_ok=True)
            state_dir = private / 'state'
            with (output / 'gui.log').open('w') as log:
                primary = subprocess.Popen([str(binary), '--smoke', '--state-dir', str(state_dir),
                                            '--smoke-report', str(report), '--screenshot', str(screenshot),
                                            '--smoke-theme', args.theme],
                                           env=environment, cwd=private, stdout=log, stderr=subprocess.STDOUT)
                processes.append(primary)
                if platform.system() == 'Linux':
                    wait_for(lambda: (state_dir / 'gui-runtime.json').exists(), primary)
                    subprocess.run([str(binary), '--smoke', '--state-dir', str(state_dir),
                                    '--smoke-report', str(private / 'duplicate-unused.json'),
                                    '--screenshot', str(private / 'duplicate-unused.png')],
                                   env=environment, cwd=private, stdout=log, stderr=subprocess.STDOUT,
                                   check=True, timeout=10)
                    wait_for(lambda: json.loads(tray_report.read_text()).get('items_registered', 0) >= 1, primary)
                    wait_for(lambda: screenshot.is_file() and all(image.is_file() for image in images), primary, seconds=15)
                    # File creation precedes save completion and GUI acknowledgment.
                    # Wait for the GUI's explicit hidden-stage acknowledgment.
                    wait_for(lambda: progress.is_file()
                             and json.loads(progress.read_text()).get('hidden_window_exercised') is True,
                             primary, seconds=20)
                    subprocess.run(['/usr/bin/python3', str(ROOT / 'scripts/gui-tray-control.py'),
                                    '--report', str(output / 'tray-menu-result.json')],
                                   env=environment, cwd=private, stdout=log, stderr=subprocess.STDOUT,
                                   check=True, timeout=15)
                    subprocess.run([str(binary), '--smoke', '--state-dir', str(state_dir),
                                    '--smoke-report', str(private / 'hidden-duplicate-unused.json'),
                                    '--screenshot', str(private / 'hidden-duplicate-unused.png')],
                                   env=environment, cwd=private, stdout=log, stderr=subprocess.STDOUT,
                                   check=True, timeout=10)
                    terminate(fixture)
                returncode = primary.wait(timeout=45)
            if returncode != 0:
                raise ValueError('Actual GUI smoke failed; inspect target/gui-smoke/gui.log')
            evidence = json.loads(report.read_text())
            if evidence.get('ok') is not True or evidence.get('frames', 0) < 1:
                raise ValueError('GUI did not confirm a rendered event-loop frame')
            if evidence.get('hosting') is not False or evidence.get('input_enabled') is not False:
                raise ValueError('GUI smoke did not confirm software-only defaults')
            if evidence.get('native_input') is not False or evidence.get('device_contacted') is not False:
                raise ValueError('GUI smoke did not confirm isolated recording services')
            if set(evidence.get('pages_rendered', [])) != {'connection', 'keyboard', 'diagnostics', 'settings'}:
                raise ValueError('GUI did not render all four companion pages')
            if not screenshot.is_file() or not screenshot.read_bytes().startswith(b'\x89PNG\r\n\x1a\n'):
                raise ValueError('Actual GUI did not produce the requested layout screenshot')
            if platform.system() == 'Linux':
                tray = json.loads(tray_report.read_text())
                if tray.get('items_registered', 0) < 1:
                    raise ValueError('GUI did not register an actual StatusNotifier tray item')
                if evidence.get('hidden_window_exercised') is not True:
                    raise ValueError('Actual GUI did not exercise a hidden window with its tray present')
                if evidence.get('tray_loss_observed') is not True or evidence.get('activation_requests', 0) < 2:
                    raise ValueError('Actual GUI did not observe watcher loss and both authenticated duplicate launches')
                if set(evidence.get('hidden_tray_commands', [])) != {'stop', 'enable'} or evidence.get('rejected_tray_enable') is not True:
                    raise ValueError('Actual hidden tray did not process Stop and safely reject Enable without a session')
                (output / 'tray-result.json').write_text(json.dumps(tray, indent=2) + '\n')
            print('PASS real GUI launch, four rendered pages, software-only defaults and clean exit')
        finally:
            for process in reversed(processes):
                terminate(process)


if __name__ == '__main__':
    try:
        main()
    except (OSError, ValueError, StopIteration, subprocess.SubprocessError) as error:
        sys.exit(str(error))
