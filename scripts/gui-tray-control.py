#!/usr/bin/env python3
"""Exercise only the harness-owned StatusNotifier fixture on its private D-Bus session."""
import argparse
import json
from pathlib import Path
import sys


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--report', required=True, type=Path)
    parser.add_argument('--command', required=True, choices=('stop', 'enable'))
    args = parser.parse_args()
    import dbus
    # A system tray does not export this fixture-only interface. The helper
    # cannot select another tray item or issue arbitrary menu operations.
    watcher = dbus.SessionBus().get_object('org.kde.StatusNotifierWatcher',
                                          '/StatusNotifierWatcher', introspect=False)
    fixture = dbus.Interface(watcher, 'com.flameliongames.c64keyboardbridge.SmokeFixture1')
    result = json.loads(str(fixture.ExerciseHiddenMenu(args.command, timeout=12)))
    expected = ['Stop bridge'] if args.command == 'stop' else ['Stop bridge', 'Enable input']
    if (result.get('menu_commands') != expected
            or result.get('dispatched_command') != args.command
            or result.get('item_id') != 'com.flameliongames.c64keyboardbridge'
            or result.get('actual_dbusmenu') is not True):
        raise ValueError('Tray fixture did not confirm real menu dispatch')
    args.report.parent.mkdir(parents=True, exist_ok=True)
    temporary = args.report.with_suffix('.tmp')
    temporary.write_text(json.dumps(result, indent=2) + '\n')
    temporary.replace(args.report)
    print('PASS hidden tray menu dispatched its requested command through D-Bus')


if __name__ == '__main__':
    try:
        main()
    except Exception:
        sys.exit('Private tray-menu fixture failed; no system menu was invoked')
