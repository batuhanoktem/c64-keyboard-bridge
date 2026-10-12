#!/usr/bin/env python3
"""Write checksums for the archive set assembled from the native CI jobs."""
import hashlib
from pathlib import Path
import sys

directory = Path(sys.argv[1])
files = sorted(p for p in directory.iterdir() if p.is_file() and
               p.name.endswith((".tar.gz", ".zip", ".dmg", ".exe", ".deb", ".metadata.json")))
if not files:
    sys.exit("No release artifacts found")
(directory / "SHA256SUMS").write_text("".join(
    f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in files))
print(f"Checksummed {len(files)} release artifacts")
