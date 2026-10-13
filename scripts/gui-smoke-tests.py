#!/usr/bin/env python3
"""Portable tests for native-menu fixture synchronization without opening a display or D-Bus."""
import json
from pathlib import Path
import runpy
import tempfile
import unittest

SMOKE = runpy.run_path(str(Path(__file__).with_name('gui-smoke.py')))


class TrayHandshakeTests(unittest.TestCase):
    def setUp(self):
        self.temporary = tempfile.TemporaryDirectory()
        self.addCleanup(self.temporary.cleanup)
        self.progress = Path(self.temporary.name) / 'progress.json'

    def evidence(self, sequence, command, outcome, **fields):
        self.progress.write_text(json.dumps({
            'hidden_window_exercised': True,
            'tray_command_sequence': sequence,
            'tray_command_ack': {'sequence': sequence, 'command': command, 'outcome': outcome},
            'tray_stop_completed': command == 'stop' and outcome == 'completed',
            'rejected_tray_enable': command == 'enable' and outcome == 'rejected',
            **fields,
        }))

    def test_dispatch_and_stale_or_unrelated_results_do_not_acknowledge_service_completion(self):
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'stop', 0))
        self.progress.write_text(json.dumps({'hidden_window_exercised': True, 'tray_command_sequence': 0}))
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'stop', 0))
        self.evidence(4, 'stop', 'completed')
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'stop', 4))
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'enable', 3))
        self.evidence(5, 'enable', 'rejected', hidden_window_exercised=False)
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'enable', 4))

    def test_stop_completion_precedes_fresh_enable_rejection(self):
        self.evidence(1, 'stop', 'completed')
        self.assertTrue(SMOKE['tray_completion'](self.progress, 'stop', 0))
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'enable', 1))
        self.evidence(2, 'enable', 'rejected', tray_stop_completed=True)
        self.assertTrue(SMOKE['tray_completion'](self.progress, 'enable', 1))

    def test_cancellation_or_failed_stop_cannot_pass_the_rejection_check(self):
        for command, outcome in (('enable', 'cancelled'), ('enable', 'completed'), ('stop', 'rejected')):
            self.evidence(2, command, outcome)
            with self.assertRaisesRegex(ValueError, 'unexpected service outcome'):
                SMOKE['tray_completion'](self.progress, command, 1)

    def test_progress_counter_and_typed_ack_must_agree(self):
        self.evidence(2, 'enable', 'rejected', tray_command_ack={'sequence': 1, 'command': 'enable', 'outcome': 'rejected'})
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'enable', 1))
        self.evidence(True, 'enable', 'rejected')
        self.assertFalse(SMOKE['tray_completion'](self.progress, 'enable', 0))


if __name__ == '__main__':
    unittest.main()
