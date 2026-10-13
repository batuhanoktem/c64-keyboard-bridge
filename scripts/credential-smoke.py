#!/usr/bin/env python3
"""Exercise real Linux keyring integration on a private bus without service activation."""
import argparse
import json
import os
from pathlib import Path
import subprocess
import tempfile
import time


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--binary', type=Path, required=True)
    args = parser.parse_args()
    with tempfile.TemporaryDirectory(prefix='c64-keyboard-bridge-credential-') as directory:
        private = Path(directory)
        private.chmod(0o700)
        config = private / 'bus.conf'
        config.write_text('''<busconfig><type>session</type>
<listen>unix:tmpdir=''' + str(private) + '''</listen><auth>EXTERNAL</auth>
<policy context="default"><allow own="*"/><allow send_destination="*"/>
<allow receive_sender="*"/></policy></busconfig>''')
        # No include, servicedir or standard_session_servicedirs: this daemon
        # cannot activate an installed password store or join the user's bus.
        environment = os.environ.copy()
        for name in list(environment):
            if name.startswith(('C64_', 'BRIDGE_', 'KEYBOARD_', 'REPEAT_')) or name in (
                    'INPUT_BACKEND', 'MAPPINGS_FILE', 'HOST_ADVERTISE_IP', 'AUTO_CONNECT',
                    'AUTO_DEPLOY', 'REIS_DEBUG', 'DISPLAY', 'WAYLAND_DISPLAY',
                    'DBUS_SESSION_BUS_ADDRESS', 'DBUS_STARTER_ADDRESS', 'DBUS_STARTER_BUS_TYPE'):
                environment.pop(name, None)
        for name, leaf in (('HOME', 'home'), ('XDG_CONFIG_HOME', 'config'),
                           ('XDG_DATA_HOME', 'data'), ('XDG_STATE_HOME', 'state'),
                           ('XDG_CACHE_HOME', 'cache'), ('XDG_RUNTIME_DIR', 'runtime')):
            path = private / leaf
            path.mkdir(mode=0o700)
            environment[name] = str(path)
        environment['DBUS_SYSTEM_BUS_ADDRESS'] = 'unix:path=' + str(private / 'no-system-bus')
        address_file = private / 'address'
        with address_file.open('wb') as address:
            daemon = subprocess.Popen(['dbus-daemon', '--nofork', '--config-file=' + str(config),
                                       '--print-address=1'], env=environment, cwd=private,
                                      stdout=address, stderr=subprocess.DEVNULL)
        try:
            deadline = time.monotonic() + 5
            while not address_file.read_text().strip():
                if daemon.poll() is not None or time.monotonic() >= deadline:
                    raise RuntimeError('Private credential bus did not start')
                time.sleep(.02)
            environment['DBUS_SESSION_BUS_ADDRESS'] = address_file.read_text().strip()
            environment['C64_KEYBOARD_BRIDGE_PRIVATE_CREDENTIAL_PROBE'] = '1'
            result = subprocess.run([str(args.binary.resolve()), str(private / 'gui-state')],
                                    env=environment, cwd=private, capture_output=True, timeout=25)
            # Never print captured panic/error output or supplied secret sentinels.
            if result.returncode or result.stderr:
                raise RuntimeError('Isolated credential integration failed; captured output withheld')
            evidence = json.loads(result.stdout)
            assert evidence['real_gui_credential_adapter'] and evidence['hosting_and_cleanup_responsive'] and evidence['unavailable_store_reported']
            assert not evidence['live_vault_accessed'] and not evidence['device_contacted'] and not evidence['native_input']
            print(json.dumps(evidence, sort_keys=True))
        finally:
            daemon.terminate()
            try:
                daemon.wait(timeout=5)
            except subprocess.TimeoutExpired:
                daemon.kill()
                daemon.wait(timeout=5)


if __name__ == '__main__':
    main()
