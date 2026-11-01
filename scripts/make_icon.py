"""Renders BlackHole's app icon (an accretion disc around a black hole), stdlib only.

Usage: make_icon.py out.png          1024px PNG (macOS iconset source)
       make_icon.py out.ico          multi-size Windows icon
"""
import math
import struct
import sys
import zlib


def pixel(x, y, size):
    cx = cy = size / 2
    dx, dy = x - cx, y - cy
    r = math.hypot(dx, dy) / (size / 2)
    # Rounded-square mask (macOS icon shape).
    n = 5.0
    sq = (abs(dx) / (size * 0.46)) ** n + (abs(dy) / (size * 0.46)) ** n
    if sq > 1:
        return (0, 0, 0, 0)
    alpha = int(255 * min(1.0, (1 - sq) * max(8.0, 40 * size / 1024)))
    bg = (0x14, 0x12, 0x20)
    glow = max(0.0, 1 - abs(r - 0.52) / 0.2) ** 2
    ring = max(0.0, 1 - abs(r - 0.5) / 0.05)
    tilt = 0.5 + 0.5 * math.cos(math.atan2(dy, dx) - 0.8)
    a = glow * (0.35 + 0.65 * tilt)
    col = [bg[i] + (c - bg[i]) * a for i, c in enumerate((0x9A, 0x7C, 0xFF))]
    col = [c + (255 - c) * ring * 0.8 for c in col]
    if r < 0.36:
        col = [c * (r / 0.36) ** 3 for c in col]
    return (*(int(max(0, min(255, c))) for c in col), alpha)


def png(size):
    rows = []
    for y in range(size):
        row = bytearray([0])
        for x in range(size):
            row.extend(pixel(x, y, size))
        rows.append(bytes(row))
    raw = zlib.compress(b"".join(rows), 6)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, 6, 0, 0, 0))
    return out + chunk(b"IDAT", raw) + chunk(b"IEND", b"")


def ico(sizes):
    images = [png(s) for s in sizes]
    head = struct.pack("<HHH", 0, 1, len(sizes))
    offset = 6 + 16 * len(sizes)
    entries = b""
    for s, data in zip(sizes, images):
        entries += struct.pack("<BBBBHHII", s % 256, s % 256, 0, 0, 1, 32, len(data), offset)
        offset += len(data)
    return head + entries + b"".join(images)


def main(path):
    data = ico([16, 24, 32, 48, 64, 128, 256]) if path.endswith(".ico") else png(1024)
    with open(path, "wb") as f:
        f.write(data)


main(sys.argv[1])
