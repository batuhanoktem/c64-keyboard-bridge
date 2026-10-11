#!/usr/bin/env python3
"""Validate unconfigured PRGs and bind transferred build outputs to checked-in C64 source."""
import argparse
import hashlib
import json
from pathlib import Path
import sys

ROOT = Path(__file__).resolve().parent.parent
CC65_REVISION = "555282497c3ecf8b313d87d5973093af19c35bd5"
NAMES = ("KEYBRIDGE.PRG", "NETWORKTEST.PRG")
MARKER = b"KBCFGv1\0"
UNCONFIGURED = MARKER + bytes((1, 64)) + bytes(34) + bytes((12,)) + b"C64-Ultimate" + bytes(7)


def source_hash() -> str:
    digest = hashlib.sha256()
    files = [ROOT / "c64/Makefile"]
    for folder in ("src", "include"):
        files.extend(p for p in (ROOT / "c64" / folder).rglob("*") if p.is_file())
    for path in sorted(files, key=lambda item: item.relative_to(ROOT).as_posix()):
        digest.update(path.relative_to(ROOT).as_posix().encode() + b"\0")
        digest.update(path.read_bytes())
        digest.update(b"\0")
    return digest.hexdigest()


def inspect(directory: Path) -> dict:
    hashes = {}
    for name in NAMES:
        data = (directory / name).read_bytes()
        if not 66 < len(data) <= 65537 or data[:2] != b"\x01\x08":
            raise ValueError(f"{name}: invalid C64 PRG load address or size")
        if data.count(MARKER) != 1:
            raise ValueError(f"{name}: configuration marker is missing or ambiguous")
        offset = data.index(MARKER)
        if data[offset:offset + 64] != UNCONFIGURED:
            raise ValueError(f"{name}: release template was configured or has an unknown config format")
        hashes[name] = hashlib.sha256(data).hexdigest()
    return {"schema": 1, "cc65_revision": CC65_REVISION, "c64_source_sha256": source_hash(), "files": hashes}


def verify(directory: Path) -> dict:
    actual = inspect(directory)
    recorded = json.loads((directory / "prg-manifest.json").read_text())
    if recorded != actual:
        raise ValueError("PRG manifest does not match the current C64 source, pinned cc65 or PRG bytes")
    return actual


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("directory", type=Path)
    parser.add_argument("--write-manifest", action="store_true", help="Only use immediately after a fresh pinned build")
    args = parser.parse_args()
    if args.write_manifest:
        manifest = inspect(args.directory)
        (args.directory / "prg-manifest.json").write_text(json.dumps(manifest, indent=2) + "\n")
    else:
        verify(args.directory)
    print("Verified both unconfigured C64 PRGs and their source/build manifest")


if __name__ == "__main__":
    try:
        main()
    except (OSError, ValueError, KeyError) as error:
        sys.exit(str(error))
