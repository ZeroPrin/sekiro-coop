"""Draw the two MOCK placeholder images for the Karma mechanic (final art comes from the artist,
see docs/KARMA-ASSETS.md). Pure code, no game data.

usage: py -3.13 tools\\icons\\make_karma_placeholders.py [--font C:\\Windows\\Fonts\\YuGothB.ttc]
writes tools\\icons\\design\\karma-item.png   252x252  (inventory / pickup icon, hi cell size)
       tools\\icons\\design\\karma-status.png 92x90    (HUD status icon, hi cell size)
Straight (non-premultiplied) RGBA; fully transparent pixels are (0,0,0,0) like vanilla.
"""
import argparse
import os

import numpy as np
from PIL import Image, ImageDraw, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
DESIGN = os.path.join(HERE, "design")
GLYPH = "\u696d"  # 業 (gō, karma)
SS = 4  # supersampling


def glyph_layer(size, font_path, px, colour, offset=(0, 0)):
    """RGBA layer (size = (w, h)) with the glyph centred on its ink box."""
    w, h = size
    font = ImageFont.truetype(font_path, px)
    im = Image.new("L", size, 0)
    d = ImageDraw.Draw(im)
    x0, y0, x1, y1 = d.textbbox((0, 0), GLYPH, font=font)
    d.text(((w - (x1 - x0)) / 2 - x0 + offset[0], (h - (y1 - y0)) / 2 - y0 + offset[1]), GLYPH, font=font, fill=255)
    layer = Image.new("RGBA", size, colour)
    layer.putalpha(im)
    return layer


def finish(im: Image.Image, size):
    """Downsample with premultiplied alpha, return straight RGBA with clean zero pixels."""
    a = np.asarray(im).astype(np.float32) / 255.0
    pm = a.copy()
    pm[..., :3] *= pm[..., 3:4]
    small = np.stack([np.asarray(Image.fromarray(pm[..., c], mode="F").resize(size, Image.LANCZOS)) for c in range(4)], -1)
    small = np.clip(small, 0, 1)
    al = small[..., 3:4]
    rgb = np.where(al > 1e-4, small[..., :3] / np.maximum(al, 1e-4), 0)
    out = np.rint(np.concatenate([np.clip(rgb, 0, 1), al], -1) * 255).astype(np.uint8)
    out[out[..., 3] == 0] = 0
    return Image.fromarray(out, "RGBA")


def radial(size, cx, cy, r):
    w, h = size
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    return np.sqrt((xx - cx) ** 2 + (yy - cy) ** 2) / r


def item_icon(font):
    W = 252 * SS
    size = (W, W)
    c = W / 2
    r = 108 * SS  # roundel radius: ~18 px clear margin at 252 (vanilla item art: median margin 5-14 px)
    d = radial(size, c, c, r)
    img = Image.new("RGBA", size, (0, 0, 0, 0))
    # soft dark-red halo behind the roundel
    halo = np.clip(1.0 - (d - 0.92) / 0.14, 0, 1) * (d > 0.9)
    halo_rgba = np.zeros((W, W, 4), np.uint8)
    halo_rgba[..., 0], halo_rgba[..., 1], halo_rgba[..., 2] = 120, 20, 20
    halo_rgba[..., 3] = (halo * 150).astype(np.uint8)
    img.alpha_composite(Image.fromarray(halo_rgba, "RGBA"))
    # roundel body: dark, slightly lighter at the top-left
    body = np.zeros((W, W, 4), np.float32)
    shade = radial(size, c - 30 * SS, c - 36 * SS, r * 1.6)
    body[..., 0] = 46 - 30 * shade
    body[..., 1] = 22 - 14 * shade
    body[..., 2] = 26 - 16 * shade
    body[..., 3] = np.clip((1.0 - d) * r / SS, 0, 1) * 255  # 1 px anti-aliased edge
    img.alpha_composite(Image.fromarray(np.clip(body, 0, 255).astype(np.uint8), "RGBA"))
    # thin warm rim
    rim = np.clip(1.0 - np.abs(d - 0.95) * r / (2.2 * SS), 0, 1) * (d < 1.0)
    rim_rgba = np.zeros((W, W, 4), np.uint8)
    rim_rgba[..., 0], rim_rgba[..., 1], rim_rgba[..., 2] = 196, 150, 96
    rim_rgba[..., 3] = (rim * 220).astype(np.uint8)
    img.alpha_composite(Image.fromarray(rim_rgba, "RGBA"))
    # glyph: glow + light ivory fill
    glow = glyph_layer(size, font, 150 * SS, (230, 120, 70, 255)).filter(ImageFilter.GaussianBlur(7 * SS))
    img.alpha_composite(glow)
    img.alpha_composite(glyph_layer(size, font, 150 * SS, (244, 232, 206, 255)))
    return finish(img, (252, 252))


def status_icon(font):
    w, h = 92 * SS, 90 * SS
    size = (w, h)
    img = Image.new("RGBA", size, (0, 0, 0, 0))
    # smoky aura (vanilla status icons: a coloured cloud behind a kanji), ~3 px margin
    rng = np.random.default_rng(7)
    noise = Image.fromarray((rng.random((h // (6 * SS) + 1, w // (6 * SS) + 1)) * 255).astype(np.uint8), "L")
    noise = np.asarray(noise.resize(size, Image.BICUBIC).filter(ImageFilter.GaussianBlur(4 * SS))).astype(np.float32) / 255
    d = radial(size, w / 2, h / 2, 41 * SS)
    aura = np.clip((1.05 - d) * 1.8 + (noise - 0.5) * 1.1, 0, 1)
    aura_rgba = np.zeros((h, w, 4), np.float32)
    aura_rgba[..., 0] = 120 + 90 * noise
    aura_rgba[..., 1] = 30 + 40 * noise
    aura_rgba[..., 2] = 90 + 80 * noise
    aura_rgba[..., 3] = aura * 235
    img.alpha_composite(Image.fromarray(np.clip(aura_rgba, 0, 255).astype(np.uint8), "RGBA"))
    # glyph: black outline, red fill (like the vanilla 難 / 赤目 icons)
    px = 56 * SS
    outline = glyph_layer(size, font, px, (10, 6, 6, 255)).filter(ImageFilter.MaxFilter(2 * (2 * SS) + 1))
    img.alpha_composite(outline)
    img.alpha_composite(glyph_layer(size, font, px, (236, 36, 30, 255)))
    return finish(img, (92, 90))


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--font", default=r"C:\Windows\Fonts\YuGothB.ttc")
    ap.add_argument("--out", default=DESIGN)
    args = ap.parse_args()
    os.makedirs(args.out, exist_ok=True)
    item_icon(args.font).save(os.path.join(args.out, "karma-item.png"))
    status_icon(args.font).save(os.path.join(args.out, "karma-status.png"))
    print("wrote karma-item.png (252x252) and karma-status.png (92x90) to", args.out)


if __name__ == "__main__":
    main()
