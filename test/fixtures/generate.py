#!/usr/bin/env python3
"""Generate small test PNG images for smoke tests.
Pure Python — no dependencies. Creates minimal valid PNGs."""

import struct
import zlib
import os

def make_png(width, height, r, g, b):
    """Create a minimal PNG with a solid color."""
    def chunk(chunk_type, data):
        c = chunk_type + data
        return struct.pack('>I', len(data)) + c + struct.pack('>I', zlib.crc32(c) & 0xffffffff)

    header = b'\x89PNG\r\n\x1a\n'
    ihdr = chunk(b'IHDR', struct.pack('>IIBBBBB', width, height, 8, 2, 0, 0, 0))

    # Raw pixel data: filter byte 0 + RGB for each row
    raw = b''
    for _ in range(height):
        raw += b'\x00' + bytes([r, g, b]) * width

    idat = chunk(b'IDAT', zlib.compress(raw))
    iend = chunk(b'IEND', b'')
    return header + ihdr + idat + iend


def main():
    out = os.path.dirname(os.path.abspath(__file__))

    images = [
        ('red_800x600.png',    800, 600, 220, 40,  40),
        ('green_800x600.png',  800, 600, 40,  180, 40),
        ('blue_800x600.png',   800, 600, 40,  40,  220),
        ('white_400x300.png',  400, 300, 240, 240, 240),
        ('dark_1920x1080.png', 1920, 1080, 20, 20, 25),
    ]

    for name, w, h, r, g, b in images:
        path = os.path.join(out, name)
        with open(path, 'wb') as f:
            f.write(make_png(w, h, r, g, b))
        print(f'  {name} ({w}x{h})')

    print(f'Generated {len(images)} test images in {out}')


if __name__ == '__main__':
    main()
