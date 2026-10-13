#!/usr/bin/env python3
"""Fetch and verify the official portable NSIS 3.13 compiler, without installing it globally."""
import hashlib
from pathlib import Path
import subprocess
import sys
import tempfile
import urllib.request
import zipfile

ROOT = Path(__file__).resolve().parent.parent
VERSION = "3.13"
URL = "https://downloads.sourceforge.net/project/nsis/NSIS%203/3.13/nsis-3.13.zip"
# SHA-256 of the official SourceForge portable ZIP, retrieved 2026-10-12.
SHA256 = "ba63dffc4410ee89193e1cb5a41989991bd77c61068da17e3156d136b7b0b3d8"


def bootstrap():
    destination = ROOT / "target/tools" / f"nsis-{VERSION}"
    archive_path = ROOT / "target/tools" / f"nsis-{VERSION}.zip"
    archive_path.parent.mkdir(parents=True, exist_ok=True)
    if not archive_path.is_file() or hashlib.sha256(archive_path.read_bytes()).hexdigest() != SHA256:
        with tempfile.NamedTemporaryFile(dir=archive_path.parent, delete=False) as temporary:
            temporary_path = Path(temporary.name)
            try:
                request = urllib.request.Request(URL, headers={"User-Agent": "C64KeyboardBridge-release-builder"})
                with urllib.request.urlopen(request, timeout=60) as response:
                    data = response.read(8 * 1024 * 1024 + 1)
                if len(data) > 8 * 1024 * 1024 or hashlib.sha256(data).hexdigest() != SHA256:
                    raise ValueError("Official NSIS 3.13 archive checksum mismatch; no compiler was executed")
                temporary.write(data)
                temporary.flush()
            except BaseException:
                temporary_path.unlink(missing_ok=True)
                raise
        temporary_path.replace(archive_path)
    with zipfile.ZipFile(archive_path) as archive:
        for entry in archive.infolist():
            path = ROOT / "target/tools" / entry.filename
            if not path.resolve().is_relative_to(destination.resolve()):
                raise ValueError("Unexpected path in pinned NSIS archive")
            if (entry.external_attr >> 16) & 0o170000 == 0o120000:
                raise ValueError("Unexpected symlink in pinned NSIS archive")
        archive.extractall(ROOT / "target/tools")
    compiler = destination / "makensis.exe"
    result = subprocess.run([str(compiler), "/VERSION"], check=True, capture_output=True, text=True)
    if result.stdout.strip() != "v3.13":
        raise ValueError("Pinned NSIS compiler reported an unexpected version")
    return compiler


if __name__ == "__main__":
    try:
        print(bootstrap())
    except (OSError, ValueError, subprocess.SubprocessError) as error:
        sys.exit(str(error))
