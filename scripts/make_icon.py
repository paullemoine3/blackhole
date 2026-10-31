"""Renders BlackHole's app icon (an accretion disc around a black hole) as a PNG, stdlib only."""
import math
import struct
import sys
import zlib

SIZE = 1024


def pixel(x, y):
    cx = cy = SIZE / 2
    dx, dy = x - cx, y - cy
    r = math.hypot(dx, dy) / (SIZE / 2)
    # Rounded-square mask (macOS icon shape).
    n = 5.0
    sq = (abs(dx) / (SIZE * 0.46)) ** n + (abs(dy) / (SIZE * 0.46)) ** n
    if sq > 1:
        return (0, 0, 0, 0)
    alpha = int(255 * min(1.0, (1 - sq) * 40))
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


def main(path):
    rows = []
    for y in range(SIZE):
        row = bytearray([0])
        for x in range(SIZE):
            row.extend(pixel(x, y))
        rows.append(bytes(row))
    raw = zlib.compress(b"".join(rows), 6)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    png = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
    png += chunk(b"IDAT", raw) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(png)


main(sys.argv[1])
