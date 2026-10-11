#!/usr/bin/env python3
"""Verify the pinned, archived source tree without relying on cc65's version string."""
import hashlib
import json
from pathlib import Path
import sys

REVISION = "555282497c3ecf8b313d87d5973093af19c35bd5"


def verify(root: Path) -> None:
    if (root / ".c64bridge-source-revision").read_text().strip() != REVISION:
        raise ValueError("Archived cc65 source revision does not match the pinned revision")
    manifest = json.loads((root / ".c64bridge-source-sha256.json").read_text())
    if manifest.get("revision") != REVISION or not manifest.get("files"):
        raise ValueError("Invalid cc65 source manifest")
    for name, expected in manifest["files"].items():
        path = root / name
        if path.resolve().is_relative_to(root.resolve()) is False:
            raise ValueError("cc65 source manifest contains an escaping path")
        if hashlib.sha256(path.read_bytes()).hexdigest() != expected:
            raise ValueError(f"cc65 source differs from the pinned archive: {name}")
    print("Verified archived cc65 source at " + REVISION)


if __name__ == "__main__":
    try:
        verify(Path(sys.argv[1]))
    except (OSError, ValueError, KeyError, IndexError) as error:
        sys.exit(str(error))
