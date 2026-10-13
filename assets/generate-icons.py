#!/usr/bin/env python3
"""Regenerate the original icon's PNG, ICO and ICNS with only Python's standard library."""
from pathlib import Path
import math
import struct
import zlib

ROOT = Path(__file__).resolve().parent


def icon(size):
    pixels = bytearray(size * size * 4)

    def rect(x, y, width, height, color, radius=0):
        for yy in range(max(0, int(y * size / 512)), min(size, math.ceil((y + height) * size / 512))):
            for xx in range(max(0, int(x * size / 512)), min(size, math.ceil((x + width) * size / 512))):
                px, py = (xx + .5) * 512 / size, (yy + .5) * 512 / size
                cx = max(x + radius, min(px, x + width - radius))
                cy = max(y + radius, min(py, y + height - radius))
                if radius == 0 or (px - cx) ** 2 + (py - cy) ** 2 <= radius ** 2:
                    offset = (yy * size + xx) * 4
                    pixels[offset:offset + 4] = bytes((*color, 255))

    rect(16, 16, 480, 480, (40, 36, 66), 112)
    rect(65, 154, 382, 221, (184, 176, 161), 30)
    rect(83, 174, 346, 179, (101, 94, 99), 17)
    for y, keys in [(195, [(105, 39), (156, 39), (207, 39), (258, 39), (309, 39), (360, 46)]),
                    (238, [(105, 46), (163, 39), (214, 39), (265, 39), (316, 39), (367, 39)]),
                    (281, [(105, 51), (168, 188), (368, 38)])]:
        for x, width in keys:
            rect(x, y, width, 31, (238, 231, 218), 3)
    rect(202, 94, 76, 12, (145, 167, 238), 6)
    for n in range(23):
        rect(255 + n, 77 + n, 12, 12, (145, 167, 238), 6)
        rect(255 + n, 111 - n, 12, 12, (145, 167, 238), 6)
    rect(387, 381, 24, 24, (131, 212, 178), 12)

    def chunk(kind, data):
        return struct.pack('>I', len(data)) + kind + data + struct.pack('>I', zlib.crc32(kind + data))

    rows = b''.join(b'\0' + pixels[y * size * 4:(y + 1) * size * 4] for y in range(size))
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', size, size, 8, 6, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(rows, 9)) + chunk(b'IEND', b''))


def main():
    images = {size: icon(size) for size in (32, 128, 256, 512, 1024)}
    (ROOT / 'c64-keyboard-bridge.png').write_bytes(images[256])
    data = images[256]
    (ROOT / 'c64-keyboard-bridge.ico').write_bytes(struct.pack('<HHH', 0, 1, 1)
                                      + struct.pack('<BBBBHHII', 0, 0, 0, 0, 1, 32, len(data), 22) + data)
    pieces = b''.join(kind + struct.pack('>I', len(images[size]) + 8) + images[size]
                      for kind, size in [(b'icp5', 32), (b'ic07', 128), (b'ic08', 256),
                                         (b'ic09', 512), (b'ic10', 1024)])
    (ROOT / 'c64-keyboard-bridge.icns').write_bytes(b'icns' + struct.pack('>I', len(pieces) + 8) + pieces)


if __name__ == '__main__':
    main()
