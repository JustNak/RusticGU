"""Generate RusticGU brand icons from dark + light masters.

Produces:
  - assets/brand/logo.png (dark theme), logo-light.png (light theme)
  - assets/brand/icon-*.png, icon.ico
  - assets/icon.png
  - assets/brand/masters/icon-master-1024.png
  - assets/brand/masters/icon-master-light-1024.png

No extension-icon outputs (RusticGU has no browser extension).

Pipeline:
  1. Load square masters (dark glyph-on-field, optional already-correct light).
  2. Quantize to Default Light primary palette (#171717 / #fafafa).
  3. Full-bleed slate corners (no white / no alpha holes).
  4. Export PNG sizes + multi-size ICO from the dark master.
  5. Keep the provided light master when it is already the correct theme
     variant — do not invert a correct light file.

Usage:
  python scripts/gen_icons.py
  python scripts/gen_icons.py path/to/logo.png
  python scripts/gen_icons.py path/to/logo.png path/to/logo-light.png
"""
from __future__ import annotations

import sys
from pathlib import Path

from PIL import Image

ROOT = Path(__file__).resolve().parents[1]

# gpui-component Default Light primary tokens (dark-theme mark: light glyph on dark field)
BG = (0x17, 0x17, 0x17, 255)  # #171717
GLYPH = (0xFA, 0xFA, 0xFA, 255)  # #fafafa

# Light-theme mark: inverted field / glyph
LIGHT_BG = GLYPH
LIGHT_GLYPH = BG

MASTER = 1024

DEFAULT_DARK = ROOT / "assets" / "brand" / "logo.png"
DEFAULT_LIGHT = ROOT / "assets" / "brand" / "logo-light.png"
DEFAULT_MASTER = ROOT / "assets" / "brand" / "masters" / "icon-master-1024.png"


def to_brand_palette(
    img: Image.Image,
    *,
    dark_pixel: tuple[int, int, int, int] = BG,
    light_pixel: tuple[int, int, int, int] = GLYPH,
) -> Image.Image:
    """Force full-bleed 2-tone brand colors, keeping dark/light pixel roles."""
    rgba = img.convert("RGBA")
    if rgba.size != (MASTER, MASTER):
        rgba = rgba.resize((MASTER, MASTER), Image.Resampling.LANCZOS)

    rgb = rgba.convert("RGB")
    # Luminance threshold preserves the source polarity:
    # dark pixels → dark_pixel, light pixels → light_pixel.
    out = Image.new("RGBA", (MASTER, MASTER), dark_pixel)
    px_in = rgb.load()
    px_out = out.load()
    for y in range(MASTER):
        for x in range(MASTER):
            r, g, b = px_in[x, y]
            yv = 0.2126 * r + 0.7152 * g + 0.0722 * b
            px_out[x, y] = light_pixel if yv > 90 else dark_pixel
    return out


def invert_mark(master: Image.Image) -> Image.Image:
    """Swap field/glyph for light-theme chrome (dark glyph on light field)."""
    return to_brand_palette(master, dark_pixel=LIGHT_BG, light_pixel=LIGHT_GLYPH)


def _sized(master: Image.Image, s: int) -> Image.Image:
    return master.resize((s, s), Image.Resampling.LANCZOS)


def _looks_like_light_field(img: Image.Image) -> bool:
    """True when the canvas is already a light-theme mark (bright field)."""
    rgb = img.convert("RGB").resize((32, 32), Image.Resampling.BOX)
    px = rgb.load()
    acc = 0.0
    n = 0
    for y in range(32):
        for x in range(32):
            r, g, b = px[x, y]
            acc += 0.2126 * r + 0.7152 * g + 0.0722 * b
            n += 1
    return (acc / n) > 128


def load_dark(src: Path) -> Image.Image:
    print("source dark", src)
    return to_brand_palette(Image.open(src), dark_pixel=BG, light_pixel=GLYPH)


def load_light(src: Path | None, dark: Image.Image) -> Image.Image:
    if src is None or not src.is_file():
        print("light master missing; inverting dark")
        return invert_mark(dark)
    raw = Image.open(src)
    if not _looks_like_light_field(raw):
        print("light source is not a light field; inverting dark instead", src)
        return invert_mark(dark)
    print("source light", src)
    # Already-correct light mark: keep dark glyph / light field; do not invert.
    return to_brand_palette(raw, dark_pixel=LIGHT_GLYPH, light_pixel=LIGHT_BG)


def main(argv: list[str] | None = None) -> None:
    args = list(sys.argv[1:] if argv is None else argv)
    dark_src = Path(args[0]) if args else DEFAULT_DARK
    light_src = Path(args[1]) if len(args) > 1 else DEFAULT_LIGHT
    if not dark_src.is_file() and DEFAULT_MASTER.is_file():
        dark_src = DEFAULT_MASTER

    if not dark_src.is_file():
        raise FileNotFoundError(
            f"Master image not found: {dark_src}\n"
            "Pass a path: python scripts/gen_icons.py path/to/logo.png [logo-light.png]"
        )

    brand = ROOT / "assets" / "brand"
    brand.mkdir(parents=True, exist_ok=True)

    master = load_dark(dark_src)
    light = load_light(light_src if light_src != dark_src else None, master)

    master.save(brand / "logo.png")
    master.save(brand / "icon-1024.png")
    print("wrote", brand / "logo.png")

    light.save(brand / "logo-light.png")
    print("wrote", brand / "logo-light.png")

    for s in [16, 20, 24, 32, 40, 48, 64, 96, 128, 256, 512]:
        out = brand / f"icon-{s}.png"
        _sized(master, s).save(out)
        print("wrote", out)

    ico_sizes = [256, 128, 64, 48, 32, 24, 16]
    frames = [_sized(master, s) for s in ico_sizes]
    ico_path = brand / "icon.ico"
    frames[0].save(
        ico_path,
        format="ICO",
        sizes=[(s, s) for s in ico_sizes],
        append_images=frames[1:],
    )
    print("wrote", ico_path)

    masters_dir = brand / "masters"
    masters_dir.mkdir(parents=True, exist_ok=True)
    master.save(masters_dir / "icon-master-1024.png")
    print("wrote", masters_dir / "icon-master-1024.png")
    light.save(masters_dir / "icon-master-light-1024.png")
    print("wrote", masters_dir / "icon-master-light-1024.png")

    master.resize((256, 256), Image.Resampling.LANCZOS).save(ROOT / "assets" / "icon.png")
    print("wrote", ROOT / "assets" / "icon.png")
    print("done")


if __name__ == "__main__":
    main()
