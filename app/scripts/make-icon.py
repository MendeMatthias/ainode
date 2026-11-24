#!/usr/bin/env python3
"""Render AInode's app icon (1024x1024 PNG) with the standard library only.

The core from the app, as an object: a near-black squircle, an engraved tick
ring, a thin dashed ring, an obsidian disc, and one green node at the centre
with its glow. Everything is analytic (distance fields with smoothstep edges),
so it is crisp at every size `tauri icon` derives from it.

    python3 -I scripts/make-icon.py src-tauri/icons/icon-source.png
"""
import math
import struct
import sys
import zlib

N = 1024
C = N / 2

BG_TOP = (14, 18, 16)
BG_BOT = (2, 3, 2)
GREEN = (74, 222, 128)
DEEP = (16, 185, 129)


def smooth(e0, e1, x):
    t = min(1.0, max(0.0, (x - e0) / (e1 - e0)))
    return t * t * (3 - 2 * t)


def mix(a, b, t):
    return tuple(a[i] + (b[i] - a[i]) * t for i in range(3))


def squircle_sdf(x, y):
    # macOS Big Sur grid: 824 px body centred in 1024, superellipse n=5.
    a = 412.0
    nx, ny = abs(x - C) / a, abs(y - C) / a
    r = (nx ** 5 + ny ** 5) ** (1 / 5)
    return (r - 1.0) * a  # ~signed distance in px near the edge


def render():
    rows = []
    for j in range(N):
        y = j + 0.5
        row = bytearray()
        for i in range(N):
            x = i + 0.5
            sd = squircle_sdf(x, y)
            body = 1.0 - smooth(-1.0, 1.0, sd)
            if body <= 0.0:
                # soft drop shadow below the body
                sh = math.exp(-max(0.0, squircle_sdf(x, y - 14)) / 22.0) * 0.35
                row += bytes((0, 0, 0, int(255 * sh * 0.6)))
                continue
            dx, dy = x - C, y - C
            r = math.hypot(dx, dy)
            ang = math.atan2(dy, dx)

            col = mix(BG_TOP, BG_BOT, smooth(100, 924, y))
            # faint green field behind the core
            col = mix(col, (6, 30, 16), 0.55 * math.exp(-((r / 300) ** 2)))

            # engraved tick ring: 72 ticks
            tick = 0.5 + 0.5 * math.cos(ang * 72)
            band = smooth(318, 321, r) * (1 - smooth(336, 339, r))
            col = mix(col, (60, 90, 70), band * smooth(0.82, 0.95, tick) * 0.9)

            # bright outer ring with glow
            ring = math.exp(-((r - 300) / 2.2) ** 2)
            glow = math.exp(-((r - 300) / 18) ** 2) * 0.35
            col = mix(col, GREEN, min(1.0, ring + glow))

            # dashed inner ring
            dash = smooth(0.2, 0.5, 0.5 + 0.5 * math.cos(ang * 48))
            dring = math.exp(-((r - 262) / 1.6) ** 2) * dash
            col = mix(col, DEEP, dring * 0.85)

            # obsidian disc with a top-left sheen and a dark rim
            disc = 1 - smooth(229, 232, r)
            if disc > 0:
                sheen = math.exp(-(((x - (C - 70)) / 150) ** 2 + ((y - (C - 90)) / 120) ** 2))
                d = mix((7, 9, 8), (30, 36, 32), sheen * 0.9)
                d = mix(d, (2, 3, 2), smooth(170, 231, r) * 0.8)
                col = mix(col, d, disc)
                col = mix(col, (40, 70, 50), math.exp(-((r - 230) / 1.2) ** 2) * 0.8)

            # the node: green core with glow
            node = 1 - smooth(46, 49, r)
            nglow = math.exp(-(r / 95) ** 2) * 0.55
            col = mix(col, GREEN, min(1.0, nglow))
            col = mix(col, (190, 255, 215), node * (1 - smooth(0, 46, r)) * 0.7)
            col = mix(col, GREEN, node * 0.85)

            # hairline top highlight on the body edge
            col = mix(col, (60, 70, 64), math.exp(-((sd + 2) / 1.5) ** 2) * smooth(500, 120, y) * 0.8)

            row += bytes((int(col[0]), int(col[1]), int(col[2]), int(255 * body)))
        rows.append(bytes(row))
    return rows


def png(rows, path):
    raw = b"".join(b"\x00" + r for r in rows)

    def chunk(t, d):
        c = struct.pack(">I", len(d)) + t + d
        return c + struct.pack(">I", zlib.crc32(t + d) & 0xFFFFFFFF)

    data = b"\x89PNG\r\n\x1a\n" + chunk(b"IHDR", struct.pack(">IIBBBBB", N, N, 8, 6, 0, 0, 0))
    data += chunk(b"IDAT", zlib.compress(raw, 9)) + chunk(b"IEND", b"")
    with open(path, "wb") as f:
        f.write(data)


if __name__ == "__main__":
    out = sys.argv[1] if len(sys.argv) > 1 else "icon-source.png"
    png(render(), out)
    print(out)
