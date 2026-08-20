"""Generate RusticGU brand icons from a color master.

Keep cyan/gold color. Do not quantize to a 2-tone slate mark.
"""
from __future__ import annotations

import sys
from collections import deque
from pathlib import Path

from PIL import Image, ImageFilter, ImageOps

ROOT = Path(__file__).resolve().parents[1]
MASTER = 1024
FIELD_DARK = (0x07, 0x16, 0x1C, 255)
FIELD_LIGHT = (0xEA, 0xF3, 0xF4, 255)
DEFAULT_SOURCE = ROOT / "assets" / "brand" / "masters" / "icon-master-source.jpg"
DEFAULT_DARK = ROOT / "assets" / "brand" / "logo.png"
DEFAULT_MASTER = ROOT / "assets" / "brand" / "masters" / "icon-master-1024.png"
PNG_SIZES = [16, 20, 24, 32, 40, 48, 64, 96, 128, 256, 512]
ICO_SIZES = [256, 128, 64, 48, 32, 24, 16]


def _rgb_dist(a, b):
    return abs(a[0] - b[0]) + abs(a[1] - b[1]) + abs(a[2] - b[2])


def _luma(c):
    return 0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]


def _sat(c):
    mx, mn = max(c), min(c)
    return 0.0 if mx == 0 else (mx - mn) / float(mx)


def _sample_corner_field(rgb, patch=28):
    w, h = rgb.size
    samples = []
    px = rgb.load()
    for ox, oy in ((0, 0), (w - patch, 0), (0, h - patch), (w - patch, h - patch)):
        for y in range(oy, oy + patch):
            for x in range(ox, ox + patch):
                samples.append(px[x, y])
    samples.sort()
    return samples[len(samples) // 2]


def field_mask(img, threshold=22):
    rgb = img.convert("RGB")
    if rgb.size != (MASTER, MASTER):
        rgb = rgb.resize((MASTER, MASTER), Image.Resampling.LANCZOS)
    field = _sample_corner_field(rgb)
    field_luma, field_sat = _luma(field), _sat(field)
    px = rgb.load()
    w, h = rgb.size
    mask = Image.new("L", (w, h), 0)
    mx = mask.load()
    seen = bytearray(w * h)
    q = deque()

    def is_field_pixel(c):
        return (
            _rgb_dist(c, field) <= threshold
            and _luma(c) <= field_luma + 6
            and _sat(c) <= field_sat + 0.08
        )

    def try_push(x, y):
        i = y * w + x
        if seen[i] or not is_field_pixel(px[x, y]):
            return
        seen[i] = 1
        q.append((x, y))

    for x in range(w):
        try_push(x, 0)
        try_push(x, h - 1)
    for y in range(h):
        try_push(0, y)
        try_push(w - 1, y)
    while q:
        x, y = q.popleft()
        mx[x, y] = 255
        if x > 0:
            try_push(x - 1, y)
        if x + 1 < w:
            try_push(x + 1, y)
        if y > 0:
            try_push(x, y - 1)
        if y + 1 < h:
            try_push(x, y + 1)
    return mask


def paint_field(img, mask, color):
    rgba = img.convert("RGBA")
    if rgba.size != (MASTER, MASTER):
        rgba = rgba.resize((MASTER, MASTER), Image.Resampling.LANCZOS)
    if mask.size != rgba.size:
        mask = mask.resize(rgba.size, Image.Resampling.NEAREST)
    return Image.composite(Image.new("RGBA", rgba.size, color), rgba, mask)


def main(argv=None):
    args = list(sys.argv[1:] if argv is None else argv)
    src = Path(args[0]) if args else DEFAULT_SOURCE
    if not src.is_file() and DEFAULT_DARK.is_file():
        src = DEFAULT_DARK
    if not src.is_file() and DEFAULT_MASTER.is_file():
        src = DEFAULT_MASTER
    if not src.is_file():
        raise FileNotFoundError(f"Master image not found: {src}")
    print("source", src)
    raw = Image.open(src).convert("RGBA")
    if raw.size != (MASTER, MASTER):
        raw = raw.resize((MASTER, MASTER), Image.Resampling.LANCZOS)
    mask = field_mask(raw)
    coverage = mask.histogram()[255] / float(MASTER * MASTER)
    print(f"field coverage {coverage:.1%}")
    if coverage < 0.35 or coverage > 0.92:
        raise RuntimeError(f"field mask coverage {coverage:.1%} looks wrong")
    dark = paint_field(raw, mask, FIELD_DARK)
    subject = ImageOps.invert(mask).filter(ImageFilter.MaxFilter(7))
    light = paint_field(raw, ImageOps.invert(subject), FIELD_LIGHT)
    brand = ROOT / "assets" / "brand"
    brand.mkdir(parents=True, exist_ok=True)
    masters = brand / "masters"
    masters.mkdir(parents=True, exist_ok=True)
    dark.save(brand / "logo.png")
    dark.save(brand / "icon-1024.png")
    light.save(brand / "logo-light.png")
    for s in PNG_SIZES:
        dark.resize((s, s), Image.Resampling.LANCZOS).save(brand / f"icon-{s}.png")
    frames = [dark.resize((s, s), Image.Resampling.LANCZOS) for s in ICO_SIZES]
    frames[0].save(
        brand / "icon.ico",
        format="ICO",
        sizes=[(s, s) for s in ICO_SIZES],
        append_images=frames[1:],
    )
    dark.save(masters / "icon-master-1024.png")
    light.save(masters / "icon-master-light-1024.png")
    dark.resize((256, 256), Image.Resampling.LANCZOS).save(ROOT / "assets" / "icon.png")
    print("done")


if __name__ == "__main__":
    main()
