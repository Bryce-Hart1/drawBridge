# Claude — Date 11/03/2026
# Draws app-icon.png: a pixel-art gate on a black squircle, in the app's two
# colors. `npx tauri icon app-icon.png` turns it into every size the bundle needs.
import struct
import zlib

SIZE = 1024
INK = (17, 17, 17, 255)
PAPER = (236, 236, 232, 255)
CLEAR = (0, 0, 0, 0)

GATE = [
    "................",
    ".##..##..##..##.",
    ".##############.",
    ".##############.",
    ".#####....#####.",
    ".####......####.",
    ".###..#..#..###.",
    ".###..#..#..###.",
    ".###..#..#..###.",
    ".###..#..#..###.",
    ".###........###.",
    "................",
    "...##########...",
    "..############..",
    "................",
    "................",
]

CELL = 40
LEFT = (SIZE - CELL * 16) // 2
TOP = (SIZE - CELL * 16) // 2 + 30
MARGIN, RADIUS = 100, 190


def inside_squircle(x, y):
    lo, hi = MARGIN, SIZE - MARGIN
    if not (lo <= x < hi and lo <= y < hi):
        return False
    cx = min(max(x, lo + RADIUS), hi - RADIUS)
    cy = min(max(y, lo + RADIUS), hi - RADIUS)
    return (x - cx) ** 2 + (y - cy) ** 2 <= RADIUS ** 2


rows = []
for y in range(SIZE):
    row = bytearray([0])  # filter: none
    for x in range(SIZE):
        px = CLEAR
        if inside_squircle(x, y):
            px = INK
            gx, gy = (x - LEFT) // CELL, (y - TOP) // CELL
            if 0 <= gx < 16 and 0 <= gy < 16 and GATE[gy][gx] == "#":
                px = PAPER
        row += bytes(px)
    rows.append(bytes(row))


def chunk(kind, data):
    return struct.pack(">I", len(data)) + kind + data + struct.pack(">I", zlib.crc32(kind + data) & 0xFFFFFFFF)


png = b"\x89PNG\r\n\x1a\n"
png += chunk(b"IHDR", struct.pack(">IIBBBBB", SIZE, SIZE, 8, 6, 0, 0, 0))
png += chunk(b"IDAT", zlib.compress(b"".join(rows), 9))
png += chunk(b"IEND", b"")
open("app-icon.png", "wb").write(png)
