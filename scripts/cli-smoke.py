#!/usr/bin/env python3
"""Exercise the real CLI using a temporary state directory and loopback recording backend."""
import argparse
import json
import os
from pathlib import Path
import socket
import subprocess
import sys
import tempfile
import time


def smoke(binary: Path) -> None:
    binary = binary.resolve()
    with tempfile.TemporaryDirectory(prefix="c64bridge-smoke-") as temporary:
        directory = Path(temporary)
        environment = os.environ.copy()
        config_names = {
            "C64_HOST", "C64_PORT", "C64_NETWORK_PASSWORD", "C64_ALLOWED_IPS",
            "BRIDGE_BIND_ADDRESS", "HOST_ADVERTISE_IP", "BRIDGE_PORT", "AUTO_CONNECT",
            "AUTO_DEPLOY", "KEYBOARD_LAYOUT", "KEYBOARD_PROFILE", "MAPPINGS_FILE",
            "INPUT_BACKEND", "LOG_LEVEL", "BRIDGE_STATE_DIR", "REPEAT_DELAY_MS", "REPEAT_INTERVAL_MS",
        }
        for name in config_names:
            environment.pop(name, None)
        with socket.socket() as reservation:
            reservation.bind(("127.0.0.1", 0))
            port = reservation.getsockname()[1]
        environment.update({
            "C64_HOST": "127.0.0.1", "C64_PORT": "80", "C64_NETWORK_PASSWORD": "",
            "C64_ALLOWED_IPS": "127.0.0.1", "BRIDGE_BIND_ADDRESS": "127.0.0.1",
            "HOST_ADVERTISE_IP": "127.0.0.1", "BRIDGE_PORT": str(port), "AUTO_CONNECT": "false",
            "AUTO_DEPLOY": "false", "KEYBOARD_LAYOUT": "US", "KEYBOARD_PROFILE": "modern",
            "INPUT_BACKEND": "recording", "LOG_LEVEL": "warn", "BRIDGE_STATE_DIR": str(directory / "state"),
        })
        base = [str(binary), "--env-file", str(directory / "absent.env"), "--backend", "recording"]

        def call(*arguments: str, check: bool = True) -> subprocess.CompletedProcess:
            return subprocess.run(base + list(arguments), env=environment, cwd=directory,
                                  capture_output=True, text=True, timeout=10, check=check)

        call("--version")
        assert "modern-us" in call("mappings", "list").stdout
        call("mappings", "validate")
        log = directory / "bridge.log"
        with log.open("w") as output:
            process = subprocess.Popen(base + ["start", "--fake-device", "--enable-input"],
                                       cwd=directory, env=environment, stdout=output, stderr=output)
            try:
                def status_when(predicate, timeout: float = 6) -> dict:
                    deadline = time.monotonic() + timeout
                    while time.monotonic() < deadline:
                        if process.poll() is not None:
                            raise RuntimeError("Recording bridge exited: " + log.read_text()[-4096:])
                        result = call("status", check=False)
                        if result.returncode == 0:
                            status = json.loads(result.stdout)
                            if predicate(status):
                                return status
                        time.sleep(0.05)
                    raise RuntimeError("Recording bridge status did not reach the expected state")

                status_when(lambda s: s["running"] and s["enabled"] and s["backend"] == "recording")
                assert json.loads(call("pause").stdout)["enabled"] is False
                assert json.loads(call("resume").stdout)["enabled"] is True
                # Inspect each completed session instead of relying on a total
                # event count: an unrelated failure must not mask a missing
                # watchdog/protocol check or an unprocessed simulator run.
                scenarios = (
                    ("typing", 6, None),
                    ("disconnect-held", 2, "transport"),
                    ("heartbeat-timeout", 2, "heartbeat_timeout"),
                    ("malformed", 0, "protocol"),
                )
                for scenario, event_count, reason in scenarios:
                    before = status_when(lambda s: not s["connected"])
                    expected_failures = dict(before["diagnostics"]["failures"])
                    if reason is not None:
                        expected_failures[reason] += 1
                    call("simulate", "--scenario", scenario, "--fragment")
                    status = status_when(lambda s: (
                        not s["connected"]
                        and s["diagnostics"]["attempts"] == before["diagnostics"]["attempts"] + 1
                        and s["diagnostics"]["authenticated_sessions"] == before["diagnostics"]["authenticated_sessions"] + 1
                        and s["diagnostics"]["failures"] == expected_failures
                        and (reason is None or s["diagnostics"]["last_failure"]["cleanup"] == "succeeded")
                    ))
                    assert status["events"] - before["events"] == event_count, scenario
                    assert status["diagnostics"]["current"]["phase"] == "idle", scenario
                    if reason is not None:
                        assert status["diagnostics"]["last_failure"]["reason"] == reason, scenario
                assert status["events"] == 10
                assert status["backend"] == "recording"
                call("stop")
                if process.wait(timeout=5) != 0:
                    raise RuntimeError("Recording bridge failed during shutdown")
            finally:
                if process.poll() is None:
                    call("stop", check=False)
                    try:
                        process.wait(timeout=3)
                    except subprocess.TimeoutExpired:
                        process.kill()
                        process.wait(timeout=3)
        print("CLI smoke passed: isolated recording backend, loopback simulator, pause/resume and cleanup")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, required=True)
    args = parser.parse_args()
    try:
        smoke(args.binary)
    except (OSError, ValueError, AssertionError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
