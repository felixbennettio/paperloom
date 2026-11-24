#!/usr/bin/env python3
"""Draws the Paperloom app icon (a page woven with threads) and writes every
size the apps need. Pure Python, no dependencies:

    python3 scripts/make_icons.py

Writes the Apple asset catalog icons and app/app-icon.png, the source the
Tauri CLI turns into the Windows, Linux and Android icons (`tauri icon`).
"""

import json
import math
import struct
import zlib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
APPICON = ROOT / "apple/Paperloom/Resources/Assets.xcassets/AppIcon.appiconset"
TAURI_ICON = ROOT / "app/app-icon.png"

INDIGO = (79, 70, 229)
TEAL = (13, 148, 136)
PAGE = (255, 255, 255)
FOLD = (199, 210, 254)
INK = (67, 56, 202)
THREAD = (20, 184, 166)


def rounded_rect(px, py, cx, cy, hw, hh, r):
    qx, qy = abs(px - cx) - hw + r, abs(py - cy) - hh + r
    outside = math.hypot(max(qx, 0.0), max(qy, 0.0))
    return outside + min(max(qx, qy), 0.0) - r


def segment(px, py, ax, ay, bx, by, half_width):
    abx, aby = bx - ax, by - ay
    t = max(0.0, min(1.0, ((px - ax) * abx + (py - ay) * aby) / (abx * abx + aby * aby)))
    return math.hypot(px - ax - t * abx, py - ay - t * aby) - half_width


def coverage(distance):
    return max(0.0, min(1.0, 0.5 - distance))


def render(size, mac):
    """RGBA pixels of the icon. `mac` draws the rounded tile with margins
    macOS expects; otherwise a full-bleed opaque square (iOS masks it)."""
    k = size / 1024.0
    # The page and threads are laid out on a 1024 grid and scaled up a
    # little on iOS, where the tile fills the whole square.
    zoom = 1.0 if mac else 1.18

    def tx(v):
        return (512 + (v - 512) * zoom) * k

    def ts(v):
        return v * zoom * k

    page = (tx(512), tx(515), ts(212), ts(285), ts(40))
    fold_x0, fold_y0, fold = tx(724 - 96), tx(230), ts(96)
    lines = [(380, 664), (450, 640), (520, 664), (590, 600), (660, 650)]
    threads = [430, 512, 594]
    line_w, thread_w = ts(13), ts(9)

    pixels = bytearray(size * size * 4)
    for y in range(size):
        py = y + 0.5
        for x in range(size):
            px = x + 0.5
            # Background tile with a diagonal indigo-to-teal gradient.
            if mac:
                a = coverage(rounded_rect(px, py, 512 * k, 512 * k, 412 * k, 412 * k, 185 * k))
            else:
                a = 1.0
            if a <= 0.0:
                continue
            t = max(0.0, min(1.0, (px + py) / (2 * size)))
            col = [INDIGO[i] + (TEAL[i] - INDIGO[i]) * t for i in range(3)]

            def over(color, alpha):
                for i in range(3):
                    col[i] = col[i] * (1 - alpha) + color[i] * alpha

            # The page, with its top-right corner folded.
            cut = (px - fold_x0) - (py - fold_y0)
            d_page = max(rounded_rect(px, py, *page), cut / math.sqrt(2))
            c_page = coverage(d_page)
            if c_page > 0:
                over(PAGE, c_page)
                d_fold = max(fold_x0 - px, py - (fold_y0 + fold), cut / math.sqrt(2))
                over(FOLD, coverage(d_fold))
                # Woven threads: vertical threads under the text lines,
                # except where they pass over (alternating).
                under = max((coverage(segment(px, py, tx(xv), tx(330), tx(xv), tx(710), thread_w))
                             for xv in threads), default=0.0)
                over(THREAD, under * 0.85 * c_page)
                for row, (ly, lx1) in enumerate(lines):
                    c = coverage(segment(px, py, tx(360), tx(ly), tx(lx1), tx(ly), line_w))
                    if c > 0:
                        over(INK, c * c_page)
                for col_i, xv in enumerate(threads):
                    for row, (ly, lx1) in enumerate(lines):
                        if (row + col_i) % 2 == 0 and xv < lx1:
                            c = coverage(segment(px, py, tx(xv), tx(ly - 22), tx(xv), tx(ly + 22), thread_w))
                            if c > 0:
                                over(THREAD, c * c_page)
            o = (y * size + x) * 4
            pixels[o:o + 4] = bytes((round(col[0]), round(col[1]), round(col[2]), round(a * 255)))
    return pixels


def downscale(pixels, size, factor):
    """Box filter on premultiplied alpha."""
    out_size = size // factor
    out = bytearray(out_size * out_size * 4)
    n = factor * factor
    for oy in range(out_size):
        for ox in range(out_size):
            acc = [0.0, 0.0, 0.0, 0.0]
            for dy in range(factor):
                row = ((oy * factor + dy) * size + ox * factor) * 4
                for dx in range(factor):
                    o = row + dx * 4
                    a = pixels[o + 3] / 255.0
                    acc[0] += pixels[o] * a
                    acc[1] += pixels[o + 1] * a
                    acc[2] += pixels[o + 2] * a
                    acc[3] += a
            o = (oy * out_size + ox) * 4
            alpha = acc[3] / n
            if alpha > 0:
                out[o:o + 4] = bytes((round(acc[0] / acc[3]), round(acc[1] / acc[3]),
                                      round(acc[2] / acc[3]), round(alpha * 255)))
    return out


def write_png(path, pixels, size, alpha=True):
    channels = 4 if alpha else 3
    raw = bytearray()
    for y in range(size):
        raw.append(0)
        row = pixels[y * size * 4:(y + 1) * size * 4]
        raw.extend(row if alpha else bytes(b for i, b in enumerate(row) if i % 4 != 3))

    def chunk(tag, data):
        body = tag + data
        return struct.pack(">I", len(data)) + body + struct.pack(">I", zlib.crc32(body) & 0xFFFFFFFF)

    color_type = 6 if alpha else 2
    png = b"\x89PNG\r\n\x1a\n"
    png += chunk(b"IHDR", struct.pack(">IIBBBBB", size, size, 8, color_type, 0, 0, 0))
    png += chunk(b"IDAT", zlib.compress(bytes(raw), 9))
    png += chunk(b"IEND", b"")
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(png)
    print(f"wrote {path.relative_to(ROOT)} ({size}x{size}, {channels} channels)")


def main():
    mac = render(1024, mac=True)
    ios = render(1024, mac=False)
    write_png(APPICON / "ios-1024.png", ios, 1024, alpha=False)  # App Store icons must be opaque
    write_png(TAURI_ICON, mac, 1024)
    images = [{"filename": "ios-1024.png", "idiom": "universal", "platform": "ios", "size": "1024x1024"}]
    for px in (16, 32, 64, 128, 256, 512, 1024):
        write_png(APPICON / f"mac-{px}.png", downscale(mac, 1024, 1024 // px) if px < 1024 else mac, px)
    for points in (16, 32, 128, 256, 512):
        for scale in (1, 2):
            images.append({"filename": f"mac-{points * scale}.png", "idiom": "mac",
                           "scale": f"{scale}x", "size": f"{points}x{points}"})
    (APPICON / "Contents.json").write_text(
        json.dumps({"images": images, "info": {"author": "xcode", "version": 1}}, indent=2) + "\n")


if __name__ == "__main__":
    main()
