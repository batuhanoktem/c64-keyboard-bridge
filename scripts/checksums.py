#!/usr/bin/env python3
"""Write checksums for the archive set assembled from the native CI jobs."""
import hashlib
from pathlib import Path
import sys

directory = Path(sys.argv[1])
files = sorted(p for p in directory.iterdir() if p.name.endswith((".tar.gz", ".zip")))
if not files:
    sys.exit("No release archives found")
(directory / "SHA256SUMS").write_text("".join(
    f"{hashlib.sha256(path.read_bytes()).hexdigest()}  {path.name}\n" for path in files))
print(f"Checksummed {len(files)} release archives")
