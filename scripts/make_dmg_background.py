"""Renders the background of the macOS disk image window (stdlib only): a pale sky with a few
violet specks and an arrow from the app icon (left) to the Applications folder (right), so the
window says "drag BlackHole there" without any text to translate. Light, because Finder draws
the icon labels in dark text over a background picture.

Usage: make_dmg_background.py out.png SCALE      SCALE 1 = 660 x 400, 2 = Retina (1320 x 800)

The geometry matches scripts/make-dmg.sh: window 660 x 400 points, icons centred at
(170, 190) and (490, 190).
"""
import math
import random
import struct
import sys
import zlib

WIDTH, HEIGHT = 660, 400
ICON_Y = 190
# The arrow sits between the two 128 pt icons, a little above their centres.
ARROW_FROM, ARROW_TO = 258, 402


def seg_dist(px, py, ax, ay, bx, by):
    dx, dy = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
    return math.hypot(px - ax - t * dx, py - ay - t * dy)


def render(scale):
    w, h = WIDTH * scale, HEIGHT * scale
    rng = random.Random(7)
    stars = [(rng.uniform(0, w), rng.uniform(0, h), rng.uniform(0.4, 1.1) * scale, rng.uniform(0.15, 0.5))
             for _ in range(90)]
    y = ICON_Y * scale
    x0, x1 = ARROW_FROM * scale, ARROW_TO * scale
    head = 16 * scale
    stroke = 2.2 * scale
    segments = [(x0, y, x1, y), (x1 - head, y - head, x1, y), (x1 - head, y + head, x1, y)]
    rows = []
    for py in range(h):
        row = bytearray([0])
        for px in range(w):
            # Pale lavender, whiter towards the centre.
            r = math.hypot((px - w / 2) / w, (py - h * 0.45) / h)
            k = max(0.0, 1.0 - r * 1.6)
            col = [236 + 16 * k, 234 + 16 * k, 246 + 8 * k]
            # Arrow: anti-aliased round-capped strokes.
            d = min(seg_dist(px + 0.5, py + 0.5, *s) for s in segments) - stroke
            a = max(0.0, min(1.0, 0.5 - d)) * 0.7
            col = [c + (t - c) * a for c, t in zip(col, (70, 56, 130))]
            row.extend(int(max(0, min(255, c))) for c in col)
            row.append(255)
        rows.append(row)
    for sx, sy, sr, sa in stars:
        # Keep the icons' and labels' areas clean.
        if abs(sy - y) < 95 * scale and (abs(sx - 170 * scale) < 95 * scale or abs(sx - 490 * scale) < 95 * scale):
            continue
        for py in range(int(sy - 2 * sr), int(sy + 2 * sr) + 2):
            for px in range(int(sx - 2 * sr), int(sx + 2 * sr) + 2):
                if 0 <= px < w and 0 <= py < h:
                    a = max(0.0, 1.0 - math.hypot(px - sx, py - sy) / (sr + 0.5)) * sa
                    i = 1 + px * 4
                    for c, t in zip(range(3), (120, 100, 200)):
                        rows[py][i + c] = int(rows[py][i + c] + (t - rows[py][i + c]) * a)
    raw = zlib.compress(b"".join(bytes(r) for r in rows), 9)

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body))

    # pHYs: 72 dpi at scale 1, 144 at scale 2, so Finder lays the Retina image out at 660 x 400.
    ppm = int(round(72 * scale / 0.0254))
    out = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", w, h, 8, 6, 0, 0, 0))
    out += chunk(b"pHYs", struct.pack(">IIB", ppm, ppm, 1))
    return out + chunk(b"IDAT", raw) + chunk(b"IEND", b"")


def main(path, scale):
    with open(path, "wb") as f:
        f.write(render(scale))


main(sys.argv[1], int(sys.argv[2]) if len(sys.argv) > 2 else 1)
