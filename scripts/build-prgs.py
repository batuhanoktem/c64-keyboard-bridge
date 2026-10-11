#!/usr/bin/env python3
"""Serialize C64 template builds and invoke every tool from the pinned cc65 tree."""
import os
from pathlib import Path
import platform
import runpy
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent.parent


def main() -> None:
    if platform.system() == "Windows":
        raise ValueError("Use the pinned Ubuntu C64 build artifact on Windows, or build cc65 manually under MSYS2")
    import fcntl
    (ROOT / "target").mkdir(exist_ok=True)
    with (ROOT / "target/c64-prgs.lock").open("a") as lock:
        deadline = time.monotonic() + 60
        while True:
            try:
                fcntl.flock(lock, fcntl.LOCK_EX | fcntl.LOCK_NB)
                break
            except BlockingIOError:
                if time.monotonic() >= deadline:
                    raise ValueError("Another PRG build is still running; retry when it completes")
                time.sleep(0.1)
        verifier = runpy.run_path(str(ROOT / "scripts/verify-prgs.py"))
        before = verifier["source_hash"]()
        (ROOT / "target/c64-prgs/prg-manifest.json").unlink(missing_ok=True)
        source = Path(subprocess.run(["bash", str(ROOT / "scripts/bootstrap-cc65.sh")],
                                     check=True, stdout=subprocess.PIPE, text=True).stdout.strip())
        environment = os.environ.copy()
        environment["CC65_HOME"] = str(source)
        environment["PATH"] = str(source / "bin") + os.pathsep + environment["PATH"]
        subprocess.run(["make", "-C", str(ROOT / "c64"), "BUILD=../target/c64-prgs", "-B",
                        "CC65=" + str(source / "bin/cc65"), "CA65=" + str(source / "bin/ca65"),
                        "CL65=" + str(source / "bin/cl65"), "all"], env=environment, check=True)
        if verifier["source_hash"]() != before:
            raise ValueError("C64 source changed during the build; rerun from a stable checkout")
        manifest = verifier["inspect"](ROOT / "target/c64-prgs")
        import json
        (ROOT / "target/c64-prgs/prg-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
        print("Verified both unconfigured C64 PRGs and their source/build manifest")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
