#!/usr/bin/env python3
"""Check real Linux XTEST output only on a private, authenticated Xvfb display."""
import argparse
import ctypes
import ctypes.util
import json
import os
from pathlib import Path
import secrets
import shutil
import signal
import socket
import stat
import struct
import subprocess
import sys
import tempfile
import time


CONFIG_NAMES = {
    "C64_HOST", "C64_PORT", "C64_NETWORK_PASSWORD", "C64_ALLOWED_IPS",
    "BRIDGE_BIND_ADDRESS", "HOST_ADVERTISE_IP", "BRIDGE_PORT", "AUTO_CONNECT",
    "AUTO_DEPLOY", "KEYBOARD_LAYOUT", "KEYBOARD_PROFILE", "MAPPINGS_FILE",
    "INPUT_BACKEND", "LOG_LEVEL", "BRIDGE_STATE_DIR", "REPEAT_DELAY_MS",
    "REPEAT_INTERVAL_MS",
}


def stop_group(process: subprocess.Popen) -> None:
    """Stop only a process group created by this harness, including its children."""
    for signum, timeout in ((signal.SIGTERM, 1), (signal.SIGKILL, 1)):
        try:
            os.killpg(process.pid, signum)
        except ProcessLookupError:
            break
        try:
            process.wait(timeout=timeout)
            # An exited leader can leave a child in the owned process group.
            if signum == signal.SIGTERM:
                continue
            break
        except subprocess.TimeoutExpired:
            continue
    if process.poll() is None:
        raise RuntimeError("Owned process did not exit after bounded termination")


def run(arguments: list[str], environment: dict[str, str], timeout: float,
        *, check: bool = True, input_text: str | None = None) -> subprocess.CompletedProcess:
    process = subprocess.Popen(arguments, env=environment, stdin=subprocess.PIPE,
                               stdout=subprocess.PIPE, stderr=subprocess.PIPE,
                               text=True, start_new_session=True)
    try:
        stdout, stderr = process.communicate(input=input_text, timeout=timeout)
    except BaseException:
        stop_group(process)
        raise
    result = subprocess.CompletedProcess(arguments, process.returncode, stdout, stderr)
    if check and result.returncode:
        raise RuntimeError(f"{Path(arguments[0]).name} failed: {stderr[-2048:]}")
    return result


def command(binary: Path, directory: Path, layout: str, *arguments: str) -> list[str]:
    return [str(binary), "--env-file", str(directory / "absent.env"),
            "--backend", "x11", "--layout", layout, *arguments]


def private_environment(directory: Path, display: str, authority: Path) -> dict[str, str]:
    environment = os.environ.copy()
    for name in CONFIG_NAMES | {"WAYLAND_DISPLAY", "XDG_SESSION_TYPE"}:
        environment.pop(name, None)
    environment.update({
        "DISPLAY": display, "XAUTHORITY": str(authority), "C64_HOST": "127.0.0.1",
        "C64_NETWORK_PASSWORD": "", "AUTO_CONNECT": "false", "AUTO_DEPLOY": "false",
        "INPUT_BACKEND": "x11", "KEYBOARD_LAYOUT": "US", "KEYBOARD_PROFILE": "modern",
        "BRIDGE_STATE_DIR": str(directory / "state"), "LOG_LEVEL": "warn",
    })
    return environment


def collector_guard(configuration: dict, directory: Path) -> None:
    """Never let the ctypes worker attach to a desktop or an unowned X server."""
    display = configuration["display"]
    number = int(display.removeprefix(":"))
    if display != f":{number}" or number < 100:
        raise RuntimeError("Collector requires a private display numbered at least 100")
    authority = Path(configuration["authority"])
    if authority.parent != directory or authority.name != "Xauthority":
        raise RuntimeError("Collector authority must belong to its private directory")
    if stat.S_IMODE(directory.stat().st_mode) != 0o700:
        raise RuntimeError("Collector directory must be private")
    if stat.S_IMODE(authority.stat().st_mode) != 0o600:
        raise RuntimeError("Collector authority must be private")
    if os.environ.get("DISPLAY") != display or os.environ.get("XAUTHORITY") != str(authority):
        raise RuntimeError("Collector environment does not identify its owned display")
    process_directory = Path("/proc") / str(configuration["server_pid"])
    if process_directory.stat().st_uid != os.getuid():
        raise RuntimeError("Collector does not own the X server")
    server_arguments = process_directory.joinpath("cmdline").read_bytes().split(b"\0")[:-1]
    expected = [os.fsencode(value) for value in configuration["server_arguments"]]
    if server_arguments != expected or expected[0] != os.fsencode(shutil.which("Xvfb")):
        raise RuntimeError("Collector server command does not match its owned Xvfb")
    if display.encode() not in expected or [b"-nolisten", b"tcp"] not in [
            expected[index:index + 2] for index in range(len(expected))]:
        raise RuntimeError("Collector server must have its private display and no TCP listener")
    if [b"-auth", os.fsencode(authority)] not in [
            expected[index:index + 2] for index in range(len(expected))]:
        raise RuntimeError("Collector server must require its private authority")


class XKeyEvent(ctypes.Structure):
    _fields_ = [
        ("type", ctypes.c_int), ("serial", ctypes.c_ulong),
        ("send_event", ctypes.c_int), ("display", ctypes.c_void_p),
        ("window", ctypes.c_ulong), ("root", ctypes.c_ulong),
        ("subwindow", ctypes.c_ulong), ("time", ctypes.c_ulong),
        ("x", ctypes.c_int), ("y", ctypes.c_int),
        ("x_root", ctypes.c_int), ("y_root", ctypes.c_int),
        ("state", ctypes.c_uint), ("keycode", ctypes.c_uint),
        ("same_screen", ctypes.c_int),
    ]


class XEvent(ctypes.Union):
    _fields_ = [("type", ctypes.c_int), ("key", XKeyEvent),
                ("pad", ctypes.c_long * 24)]


def xlib() -> ctypes.CDLL:
    library = ctypes.CDLL(ctypes.util.find_library("X11") or "libX11.so.6")
    pointer, integer, window = ctypes.c_void_p, ctypes.c_int, ctypes.c_ulong
    declarations = {
        "XOpenDisplay": ([ctypes.c_char_p], pointer),
        "XDefaultRootWindow": ([pointer], window),
        "XCreateSimpleWindow": ([pointer, window, integer, integer, ctypes.c_uint,
                                 ctypes.c_uint, ctypes.c_uint, window, window], window),
        "XSelectInput": ([pointer, window, ctypes.c_long], integer),
        "XMapWindow": ([pointer, window], integer),
        "XSetInputFocus": ([pointer, window, integer, window], integer),
        "XGetInputFocus": ([pointer, ctypes.POINTER(window), ctypes.POINTER(integer)], integer),
        "XSync": ([pointer, integer], integer),
        "XPending": ([pointer], integer),
        "XNextEvent": ([pointer, ctypes.POINTER(XEvent)], integer),
        "XLookupString": ([ctypes.POINTER(XKeyEvent), ctypes.c_char_p, integer,
                           ctypes.POINTER(window), pointer], integer),
        "XKeysymToKeycode": ([pointer, window], ctypes.c_ubyte),
        "XkbSetDetectableAutoRepeat": ([pointer, integer, ctypes.POINTER(integer)], integer),
        "XQueryKeymap": ([pointer, ctypes.c_char_p], integer),
        "XDestroyWindow": ([pointer, window], integer),
        "XCloseDisplay": ([pointer], integer),
    }
    for name, (arguments, result) in declarations.items():
        function = getattr(library, name)
        function.argtypes = arguments
        function.restype = result
    return library


def collect(configuration_path: Path) -> None:
    """All potentially blocking Xlib calls live in this killable subprocess."""
    configuration = json.loads(configuration_path.read_text())
    directory = configuration_path.parent
    collector_guard(configuration, directory)
    library = xlib()
    display = library.XOpenDisplay(configuration["display"].encode())
    if not display:
        raise RuntimeError("Collector cannot authenticate to its owned Xvfb")
    window = library.XCreateSimpleWindow(display, library.XDefaultRootWindow(display),
                                         0, 0, 320, 120, 0, 0, 0)
    process = None
    try:
        library.XSelectInput(display, window, (1 << 0) | (1 << 1))
        library.XMapWindow(display, window)
        library.XSetInputFocus(display, window, 2, 0)
        library.XSync(display, 0)
        focus, revert = ctypes.c_ulong(), ctypes.c_int()
        library.XGetInputFocus(display, ctypes.byref(focus), ctypes.byref(revert))
        if focus.value != window:
            raise RuntimeError("Private collector did not receive focus")
        if configuration.get("failure") == "stall":
            collect_stall(library, display, window, directory)
            return
        arguments = command(Path(configuration["binary"]), directory,
                            configuration["configured_layout"], "test-input", "--countdown", "3")
        # Inherit the collector's owned group so the supervisor can kill both.
        with tempfile.TemporaryFile(mode="w+") as output:
            process = subprocess.Popen(arguments, stdout=output, stderr=output)
            events, text = [], []

            def drain() -> None:
                while library.XPending(display):
                    event = XEvent()
                    library.XNextEvent(display, ctypes.byref(event))
                    if event.type not in (2, 3):
                        continue
                    if event.key.window != window or event.key.send_event:
                        raise RuntimeError("Unexpected synthetic event source in private collector")
                    events.append((event.type, event.key.keycode))
                    if len(events) > 64:
                        raise RuntimeError("Private collector exceeded its event bound")
                    if event.type == 2:
                        buffer, symbol = ctypes.create_string_buffer(32), ctypes.c_ulong()
                        length = library.XLookupString(ctypes.byref(event.key), buffer, 32,
                                                       ctypes.byref(symbol), None)
                        text.append(buffer.raw[:length].decode("ascii"))

            deadline = time.monotonic() + 8
            while process.poll() is None:
                drain()
                if time.monotonic() >= deadline:
                    raise RuntimeError("CLI test-input exceeded its 8-second worker deadline")
                time.sleep(0.005)
            library.XSync(display, 0)
            drain()
            keymap = ctypes.create_string_buffer(32)
            library.XQueryKeymap(display, keymap)
            if any(keymap.raw):
                raise RuntimeError("Keys remain held on the private Xvfb server")
            output.seek(0)
            diagnostic = output.read(4096)
            if configuration["mismatch"]:
                if process.returncode == 0 or events:
                    raise RuntimeError("Configured-layout mismatch did not refuse before input")
                if "layout" not in diagnostic.lower():
                    raise RuntimeError("Mismatch failure did not report its layout reason")
            else:
                if process.returncode:
                    raise RuntimeError("CLI input failed: " + diagnostic[-2048:])
                if "".join(text) != "c64 bridge\r":
                    raise RuntimeError(f"Unexpected private-window text: {''.join(text)!r}")
                if len(events) != 22:
                    raise RuntimeError(f"Expected 22 balanced events, received {len(events)}")
                for index in range(0, len(events), 2):
                    if events[index][0] != 2 or events[index + 1] != (3, events[index][1]):
                        raise RuntimeError("Private-window press/release order is unbalanced")
            print(json.dumps({"layout": configuration["configured_layout"],
                              "mismatch": configuration["mismatch"], "events": len(events),
                              "held_keys": 0, "text": "".join(text)}))
    finally:
        if process is not None and process.poll() is None:
            process.kill()
            process.wait(timeout=1)
        library.XDestroyWindow(display, window)
        library.XCloseDisplay(display)


def collect_stall(library: ctypes.CDLL, display, window: int, directory: Path) -> None:
    """Observe the surviving server; never infer release from its disappearance."""
    # Repeat KeyPress events must not manufacture KeyRelease events that could
    # falsely satisfy the cleanup-order assertion. This affects this collector
    # connection only; server and bridge repeat settings remain unchanged.
    supported = ctypes.c_int()
    if not library.XkbSetDetectableAutoRepeat(display, 1, ctypes.byref(supported)) \
            or not supported.value:
        raise RuntimeError("Private stall collector requires detectable auto-repeat")
    modifier = library.XKeysymToKeycode(display, 0xffe1)  # Shift_L
    ordinary = library.XKeysymToKeycode(display, 0x41)  # A
    if not modifier or not ordinary or modifier == ordinary:
        raise RuntimeError("Private server cannot resolve distinct Shift_L and A keycodes")
    (directory / "collector-ready").touch(mode=0o600)
    events, held = [], set()

    def drain() -> None:
        while library.XPending(display):
            event = XEvent()
            library.XNextEvent(display, ctypes.byref(event))
            if event.type not in (2, 3):
                continue
            if event.key.window != window or event.key.send_event:
                raise RuntimeError("Unexpected event source during private-server stall")
            events.append((event.type, event.key.keycode))
            if len(events) > 64:
                raise RuntimeError("Private stall collector exceeded its event bound")
            if event.type == 2:
                held.add(event.key.keycode)
            else:
                held.discard(event.key.keycode)
            if len(held) == 2 and not (directory / "keys-observed").exists():
                keymap = ctypes.create_string_buffer(32)
                library.XQueryKeymap(display, keymap)
                if sum(byte.bit_count() for byte in keymap.raw) != 2:
                    raise RuntimeError("Private server did not confirm two bridge-owned held keys")
                (directory / "keys-observed").touch(mode=0o600)
    deadline = time.monotonic() + 15
    while not (directory / "cleanup-retried").exists():
        drain()
        if time.monotonic() >= deadline:
            raise RuntimeError("Private stall collector exceeded its 15-second deadline")
        time.sleep(0.005)
    # XSync orders the query after the supervisor's acknowledged retry.
    library.XSync(display, 0)
    drain()
    keymap = ctypes.create_string_buffer(32)
    library.XQueryKeymap(display, keymap)
    if held or any(keymap.raw):
        raise RuntimeError("Keys remain held after surviving-server cleanup retry")
    if len(events) < 4 or events[:2] != [(2, modifier), (2, ordinary)]:
        raise RuntimeError("Private stall did not observe its modifier/ordinary presses")
    if any(code not in (modifier, ordinary) for _kind, code in events):
        raise RuntimeError("Private stall collector received an unexpected key")
    # Failed requests can be buffered, and cleanup retry can repeat releases.
    # Require release order without asserting that each key-up appeared only once.
    try:
        ordinary_release = events.index((3, ordinary))
        modifier_release = events.index((3, modifier))
    except ValueError as error:
        raise RuntimeError("Private stall did not observe both owned-key releases") from error
    if ordinary_release >= modifier_release:
        raise RuntimeError("Private stall released its modifier before its ordinary key")
    print(json.dumps({"failure": "stall", "held_before_failure": 2,
                      "held_after_retry": 0, "observed_events": len(events)}))


def stalled_server(binary: Path, directory: Path, environment: dict[str, str],
                   configuration: dict, server: subprocess.Popen) -> None:
    """Fail closed on a live X11 outage, then verify release on the same server."""
    with socket.socket() as reservation:
        reservation.bind(("127.0.0.1", 0))
        port = reservation.getsockname()[1]
    environment = environment.copy()
    environment.update({"C64_ALLOWED_IPS": "127.0.0.1",
                        "BRIDGE_BIND_ADDRESS": "127.0.0.1",
                        "HOST_ADVERTISE_IP": "127.0.0.1", "BRIDGE_PORT": str(port)})
    configuration_path = directory / "stall-collector.json"
    configuration_path.write_text(json.dumps(configuration | {"failure": "stall"}))
    configuration_path.chmod(0o600)
    collector, bridge, peer = None, None, None
    stopped = False
    base = command(binary, directory, "US")

    def status() -> dict:
        return json.loads(run(base + ["status"], environment, 3).stdout)

    def wait_for(predicate, timeout: float, description: str) -> None:
        deadline = time.monotonic() + timeout
        while not predicate():
            if collector.poll() is not None or bridge.poll() is not None:
                raise RuntimeError("Owned stall-test process exited before " + description)
            if time.monotonic() >= deadline:
                raise RuntimeError("Private stall test timed out waiting for " + description)
            time.sleep(0.02)

    sequence = 0

    def frame(kind: int, payload: bytes) -> None:
        nonlocal sequence
        peer.sendall(b"KB\x01" + bytes([kind]) + struct.pack("<HH", len(payload), sequence)
                     + payload)
        sequence = (sequence + 1) & 0xffff

    with tempfile.TemporaryFile(mode="w+") as bridge_output, \
            tempfile.TemporaryFile(mode="w+") as collector_output:
        try:
            collector = subprocess.Popen(
                [sys.executable, str(Path(__file__).resolve()), "--collector",
                 str(configuration_path)], env=environment, stdout=collector_output,
                stderr=collector_output, start_new_session=True)
            bridge = subprocess.Popen(base + ["start", "--enable-input"], env=environment,
                                      stdout=bridge_output, stderr=bridge_output,
                                      start_new_session=True)
            wait_for(lambda: (directory / "collector-ready").exists()
                     and (directory / "state" / "runtime.json").exists(), 5, "ready processes")
            if not status()["enabled"]:
                raise RuntimeError("Private X11 bridge did not enable input")
            peer = socket.create_connection(("127.0.0.1", port), timeout=2)
            identity = b"PRIVATE-XVFB"
            token = (directory / "state" / "pairing.key").read_bytes()
            if len(token) != 16:
                raise RuntimeError("Private stall test token has an invalid length")
            frame(1, token + b"\x01\x00" + bytes([len(identity)]) + identity)
            # Authentication acknowledgment precedes synchronization and transitions.
            acknowledgment = bytearray()
            acknowledgment_deadline = time.monotonic() + 2
            while len(acknowledgment) < 9:
                remaining = acknowledgment_deadline - time.monotonic()
                if remaining <= 0:
                    raise RuntimeError("Private HELLO acknowledgment exceeded two seconds")
                peer.settimeout(remaining)
                chunk = peer.recv(9 - len(acknowledgment))
                if not chunk:
                    raise RuntimeError("Private bridge closed before HELLO acknowledgment")
                acknowledgment.extend(chunk)
            peer.settimeout(2)
            if acknowledgment != b"KB\x01\x02\x01\x00\x00\x00\x01":
                raise RuntimeError("Private bridge returned an unexpected HELLO acknowledgment")
            frame(5, bytes(8))
            frame(3, bytes([15, 1]))  # Left Shift, then A with the matching physical mask.
            frame(3, bytes([10, 1]))
            wait_for(lambda: (directory / "keys-observed").exists(), 2, "two held keys")
            baseline = status()["diagnostics"]["failures"]
            stopped = True
            os.kill(server.pid, signal.SIGSTOP)
            started = time.monotonic()
            frame(4, bytes([10, 1]))
            frame(4, bytes([15, 0]))
            deadline = started + 5
            while True:
                # Valid device traffic continues; the injected failure is only X11 transport.
                frame(6, bytes(8))
                observed = status()
                observed_at = time.monotonic()
                if observed_at >= deadline:
                    raise RuntimeError("Established X11 stall did not disable input within 5 seconds")
                failure = observed["diagnostics"]["last_failure"]
                if not observed["enabled"] and failure is not None:
                    if failure["reason"] != "layout" or failure["cleanup"] != "failed":
                        raise RuntimeError("Stalled X11 did not retain its layout/cleanup failure")
                    expected = dict(baseline)
                    expected["layout"] += 1
                    expected["cleanup"] += 1
                    if observed["diagnostics"]["failures"] != expected:
                        raise RuntimeError("An unrelated session failure obscured the X11 stall: "
                                           + json.dumps(observed["diagnostics"]["failures"]))
                    failed_after = observed_at - started
                    break
                time.sleep(0.05)
            os.kill(server.pid, signal.SIGCONT)
            stopped = False
            paused = json.loads(run(base + ["pause"], environment, 3).stdout)
            if paused["enabled"] or paused["diagnostics"]["last_failure"] != failure:
                raise RuntimeError("Cleanup retry changed the retained failure or enabled input")
            (directory / "cleanup-retried").touch(mode=0o600)
            collector.wait(timeout=5)
            collector_output.seek(0)
            output = collector_output.read(4096)
            if collector.returncode:
                raise RuntimeError("Private stall collector failed: " + output[-2048:])
            observation = json.loads(output)
            frame(8, b"\x00")
            print(f"Established private X11 stall: disabled in {failed_after:.3f} seconds, "
                  f"cleanup failure retained, {observation['held_after_retry']} held keys after "
                  "same-server resume and pause retry")
        finally:
            try:
                if stopped:
                    try:
                        os.kill(server.pid, signal.SIGCONT)
                    except ProcessLookupError:
                        pass
            finally:
                try:
                    if peer is not None:
                        peer.close()
                    if bridge is not None and bridge.poll() is None:
                        try:
                            run(base + ["stop"], environment, 3, check=False)
                        finally:
                            stop_group(bridge)
                finally:
                    if collector is not None:
                        stop_group(collector)


def smoke(binary: Path) -> None:
    binary = binary.resolve(strict=True)
    required = ("Xvfb", "xauth", "setxkbmap", "xdpyinfo")
    missing = [name for name in required if not shutil.which(name)]
    if missing:
        raise RuntimeError("Missing " + ", ".join(missing)
                           + "; install xvfb xauth x11-xkb-utils x11-utils libx11-6")
    server = None
    owned_paths = []
    with tempfile.TemporaryDirectory(prefix="c64bridge-x11-smoke-") as temporary:
        directory = Path(temporary)
        authority = directory / "Xauthority"
        authority.touch(mode=0o600)
        number = next((candidate for candidate in (100 + secrets.randbelow(19000) for _ in range(32))
                       if not Path(f"/tmp/.X11-unix/X{candidate}").exists()
                       and not Path(f"/tmp/.X{candidate}-lock").exists()), None)
        if number is None:
            raise RuntimeError("Cannot reserve a distinct private Xvfb display")
        display = f":{number}"
        environment = private_environment(directory, display, authority)
        cookie = secrets.token_hex(16)
        run([shutil.which("xauth"), "-f", str(authority), "source", "-"], environment, 3,
            input_text=f"add {display} MIT-MAGIC-COOKIE-1 {cookie}\n")
        server_arguments = [shutil.which("Xvfb"), display, "-screen", "0", "640x480x24",
                            "-nolisten", "tcp", "-auth", str(authority), "-noreset"]
        with (directory / "Xvfb.log").open("w") as log:
            try:
                server = subprocess.Popen(server_arguments, env=environment, stdout=log,
                                          stderr=log, start_new_session=True)
                deadline = time.monotonic() + 5
                socket_path = Path(f"/tmp/.X11-unix/X{number}")
                while not socket_path.is_socket():
                    if server.poll() is not None:
                        raise RuntimeError("Private Xvfb exited: "
                                           + (directory / "Xvfb.log").read_text()[-2048:])
                    if time.monotonic() >= deadline:
                        raise RuntimeError("Private Xvfb did not become ready within 5 seconds")
                    time.sleep(0.02)
                lock_path = Path(f"/tmp/.X{number}-lock")
                if int(lock_path.read_text().strip()) != server.pid:
                    raise RuntimeError("Private Xvfb lock does not identify its owned process")
                for path in (socket_path, lock_path):
                    metadata = path.stat()
                    if metadata.st_uid != os.getuid():
                        raise RuntimeError("Private Xvfb resources have an unexpected owner")
                    owned_paths.append((path, (metadata.st_dev, metadata.st_ino)))
                # A socket can exist before a loaded CI server accepts its first
                # authenticated query. Retry only this read-only readiness probe;
                # never retry a typing or stalled-server acceptance operation.
                for attempt in range(3):
                    try:
                        extensions = run([shutil.which("xdpyinfo"), "-queryExtensions"], environment, 3)
                        break
                    except (subprocess.TimeoutExpired, subprocess.CalledProcessError):
                        if server.poll() is not None or attempt == 2:
                            raise
                        time.sleep(.1)
                if not all(name in extensions.stdout for name in ("XTEST", "XKEYBOARD")):
                    raise RuntimeError("Private Xvfb lacks XTEST or XKB")
                unauthorized = environment.copy()
                unauthorized["XAUTHORITY"] = str(directory / "missing-authority")
                denied = run([shutil.which("xdpyinfo")], unauthorized, 3, check=False)
                if denied.returncode == 0:
                    raise RuntimeError("Private Xvfb unexpectedly admits unauthenticated clients")
                for actual, configured in (("us", "US"), ("gb", "UK")):
                    run([shutil.which("setxkbmap"), "-layout", actual, "-variant", "", "-option", ""],
                        environment, 3)
                    probe = json.loads(run(command(binary, directory, configured, "permissions"),
                                           environment, 3).stdout)
                    if not (probe["available"] and probe["authorized"] and probe["layout_matches"]):
                        raise RuntimeError(f"Private {configured} layout was not admitted: {probe['detail']}")
                    for mismatch in (False, True):
                        configuration = {
                            "binary": str(binary), "display": display, "authority": str(authority),
                            "server_pid": server.pid, "server_arguments": server_arguments,
                            "configured_layout": ("UK" if configured == "US" else "US")
                            if mismatch else configured, "mismatch": mismatch,
                        }
                        configuration_path = directory / "collector.json"
                        configuration_path.write_text(json.dumps(configuration))
                        configuration_path.chmod(0o600)
                        result = run([sys.executable, str(Path(__file__).resolve()), "--collector",
                                      str(configuration_path)], environment, 12)
                        observation = json.loads(result.stdout)
                        print(f"Private {configured} XTEST: "
                              + ("mismatch refused without events" if mismatch
                                 else f"{observation['events']} balanced events, phrase/Return, no held keys"))
                run([shutil.which("setxkbmap"), "-layout", "us", "-variant", "", "-option", ""],
                    environment, 3)
                stalled_server(binary, directory, environment,
                               configuration | {"configured_layout": "US", "mismatch": False},
                               server)
                stop_group(server)
                server = None
                started = time.monotonic()
                absent = run(command(binary, directory, "US", "test-input", "--countdown", "3"),
                             environment, 3, check=False)
                if absent.returncode == 0 or "connect" not in absent.stderr.lower():
                    raise RuntimeError("Missing owned X server did not produce a connection failure")
                print(f"Missing private display refused in {time.monotonic() - started:.3f} seconds")
            finally:
                if server is not None:
                    stop_group(server)
                # A hard-killed X server can leave its own socket/lock behind.
                # Never remove a replacement resource belonging to a later server.
                for path, identity in owned_paths:
                    try:
                        metadata = path.stat()
                        if (metadata.st_dev, metadata.st_ino) == identity:
                            path.unlink()
                    except FileNotFoundError:
                        pass
    print("X11 smoke passed: authenticated private Xvfb only; desktop/Wayland acceptance remains separate")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--binary", type=Path, help="Linux c64bridge executable")
    parser.add_argument("--collector", type=Path, help=argparse.SUPPRESS)
    arguments = parser.parse_args()
    if (arguments.binary is None) == (arguments.collector is None):
        parser.error("provide --binary")
    if sys.platform != "linux":
        print("X11 smoke skipped: this harness requires Linux and its private Xvfb server")
        return
    if arguments.collector:
        collect(arguments.collector.resolve(strict=True))
    else:
        def interrupted(_signum, _frame):
            raise KeyboardInterrupt()

        signal.signal(signal.SIGTERM, interrupted)
        smoke(arguments.binary)


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError, RuntimeError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
    except KeyboardInterrupt:
        sys.exit("X11 smoke interrupted; owned process groups cleaned up")
