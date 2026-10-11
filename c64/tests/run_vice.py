#!/usr/bin/env python3
"""Headless VICE baseline scanner and deterministic local-exit checks.

No ROM images are bundled. VICE must have legally obtained C64 ROMs installed.
The full production scanner runs at idle; exit uses a mocked matrix provider.
This does not emulate UCI or validate held keys, ghosting, or physical hardware.
"""
import os
from pathlib import Path
import shutil
import subprocess
import sys

root = Path(__file__).resolve().parents[1]
out = root / "build" / "vice"
out.mkdir(parents=True, exist_ok=True)
emulator = os.environ.get("VICE", "x64sc")
if not shutil.which(emulator):
    sys.exit("VICE x64sc is missing; install VICE and its C64 ROMs, or set VICE.")
flags = ["-t", "c64", "--cpu", "6502", "-Oirs", "--standard", "c99", "-I", str(root / "include")]
subprocess.run(["cc65", *flags, "-Dmain=bridge_main", "-Dscan_raw=test_scan_raw", "-o", str(out / "main.s"), str(root / "src/main.c")], check=True)
subprocess.run(["ca65", "-t", "c64", "-o", str(out / "main.o"), str(out / "main.s")], check=True)
sources = []
for original in [root / "src/bridge.c", root / "src/uci.c", root / "src/scanner.s", root / "tests/vice_checks.c", root / "tests/scan_flags.s"]:
    target = out / original.name
    shutil.copyfile(original, target)
    sources.append(str(target))
subprocess.run(["cl65", *flags, "-o", str(out / "CHECKS.PRG"), str(out / "main.o"), *sources], cwd=out, check=True)
commands = out / "commands.mon"
commands.write_text(f'load "{out / "CHECKS.PRG"}" 0\ng 080d\n')
result = subprocess.run([emulator, "-console", "+sound", "-warp", "-debugcart", "-limitcycles", "20000000", "-initbreak", "ready", "-moncommands", str(commands)], capture_output=True, text=True, timeout=30)
(out / "vice.log").write_text(result.stdout + result.stderr)
if result.returncode:
    print(result.stdout[-3000:] + result.stderr[-3000:], file=sys.stderr)
    sys.exit(f"VICE checks failed (exit {result.returncode}); inspect {out / 'vice.log'}")
print("VICE: production scanner idle matrix, interrupt flags, CIA DDR preservation, mocked one-second local exit, and KERNAL settings restoration passed. UCI and physical key input are unverified.")
