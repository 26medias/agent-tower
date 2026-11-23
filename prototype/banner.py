"""Makes the README banner docs/images/banner.png (1280 x 320) from the pixel art of the app.

Run from the prototype folder:  python3 banner.py [path/to/logo.png]
With a logo (docs/images/logo.png), the logo is the mark at the left. Without it, a stack of
three floors of the app building is the mark.
The sky, the skyline and the letters are drawn on a 4x smaller grid and scaled up with
NEAREST, so that they have the same hard pixels as the art.
"""
import random
import sys
from pathlib import Path

from PIL import Image, ImageDraw

W, H = 1280, 320
P = 4  # one "pixel" of the drawn parts, in screen pixels
OUT = Path(__file__).resolve().parent.parent / "docs" / "images" / "banner.png"

# Night palette of the app (ui/src/app.css).
BG_TOP = (11, 17, 28)
BG_BOTTOM = (24, 35, 58)
ACCENT = (94, 224, 192)
ACCENT_DARK = (6, 36, 28)
DIM = (147, 162, 194)
WINDOW = (255, 197, 107)

# 5 x 7 bitmap font: the letters of the banner only.
FONT = {
    "A": ["01110", "10001", "10001", "11111", "10001", "10001", "10001"],
    "C": ["01110", "10001", "10000", "10000", "10000", "10001", "01110"],
    "E": ["11111", "10000", "10000", "11110", "10000", "10000", "11111"],
    "F": ["11111", "10000", "10000", "11110", "10000", "10000", "10000"],
    "G": ["01110", "10001", "10000", "10111", "10001", "10001", "01111"],
    "I": ["11111", "00100", "00100", "00100", "00100", "00100", "11111"],
    "K": ["10001", "10010", "10100", "11000", "10100", "10010", "10001"],
    "L": ["10000", "10000", "10000", "10000", "10000", "10000", "11111"],
    "M": ["10001", "11011", "10101", "10101", "10001", "10001", "10001"],
    "N": ["10001", "11001", "10101", "10011", "10001", "10001", "10001"],
    "O": ["01110", "10001", "10001", "10001", "10001", "10001", "01110"],
    "P": ["11110", "10001", "10001", "11110", "10000", "10000", "10000"],
    "R": ["11110", "10001", "10001", "11110", "10100", "10010", "10001"],
    "S": ["01111", "10000", "10000", "01110", "00001", "00001", "11110"],
    "T": ["11111", "00100", "00100", "00100", "00100", "00100", "00100"],
    "U": ["10001", "10001", "10001", "10001", "10001", "10001", "01110"],
    "W": ["10001", "10001", "10001", "10101", "10101", "10101", "01010"],
    "X": ["10001", "10001", "01010", "00100", "01010", "10001", "10001"],
    "Y": ["10001", "10001", "01010", "00100", "00100", "00100", "00100"],
    "-": ["00000", "00000", "00000", "01110", "00000", "00000", "00000"],
    " ": ["00000"] * 7,
}


def text_width(text, scale):
    return (len(text) * 6 - 1) * scale


def draw_text(img, text, x, y, scale, color, shadow=None, outline=None):
    """Draws the text with square pixels of `scale` screen pixels."""
    d = ImageDraw.Draw(img)

    def cells():
        for i, ch in enumerate(text):
            for r, row in enumerate(FONT[ch]):
                for c, bit in enumerate(row):
                    if bit == "1":
                        yield x + (i * 6 + c) * scale, y + r * scale

    if outline:
        for cx, cy in cells():
            d.rectangle([cx - scale // 3, cy - scale // 3, cx + scale + scale // 3 - 1, cy + scale + scale // 3 - 1], fill=outline)
    if shadow:
        for cx, cy in cells():
            d.rectangle([cx + scale // 2, cy + scale // 2, cx + scale + scale // 2 - 1, cy + scale + scale // 2 - 1], fill=shadow)
    for cx, cy in cells():
        d.rectangle([cx, cy, cx + scale - 1, cy + scale - 1], fill=color)


def sky():
    """The sky, the stars and the far skyline on the small grid."""
    w, h = W // P, H // P
    img = Image.new("RGB", (w, h))
    d = ImageDraw.Draw(img)
    for y in range(h):
        t = y / (h - 1)
        # Bands of 4 rows give a stepped, pixel-art gradient.
        t = round(t * 8) / 8
        d.line([(0, y), (w, y)], fill=tuple(round(a + (b - a) * t) for a, b in zip(BG_TOP, BG_BOTTOM)))
    rnd = random.Random(7)
    for _ in range(70):
        x, y = rnd.randrange(w), rnd.randrange(h - 30)
        c = rnd.choice([(70, 84, 120), (110, 124, 160), (200, 210, 235)])
        d.point((x, y), fill=c)
    # Far skyline: dark blocks with a few lit windows.
    x = 0
    while x < w:
        bw = rnd.randrange(5, 12)
        bh = rnd.randrange(8, 26)
        d.rectangle([x, h - bh, x + bw - 1, h - 1], fill=(17, 26, 44))
        for wy in range(h - bh + 2, h - 2, 3):
            for wx in range(x + 1, x + bw - 1, 2):
                if rnd.random() < 0.18:
                    d.point((wx, wy), fill=(120, 96, 60) if rnd.random() < 0.6 else WINDOW)
        x += bw + rnd.randrange(0, 3)
    return img.resize((W, H), Image.NEAREST)


def tower():
    """Three floors of the app building, stacked as in prototype/build.py."""
    floor = Image.open("web/floor-pixel.webp").convert("RGBA")  # 768 x 512
    wall = 150  # the wall height of one floor on this image
    n = 3
    img = Image.new("RGBA", (floor.width, floor.height + (n - 1) * wall), (0, 0, 0, 0))
    for i in range(n):
        img.alpha_composite(floor, (0, (n - 1 - i) * wall))
    return img.crop(img.getbbox())


def office():
    """The office room with six characters beside the desks (spots of prototype/build.py)."""
    room = Image.open("web/office-pixel.webp").convert("RGBA")  # 1152 x 768
    spots = [(617, 625), (925, 625), (1240, 625), (617, 845), (925, 845), (1240, 845)]
    ah = 120
    for i, (x, y) in enumerate(spots):
        a = Image.open(f"web/agent-{i}.webp").convert("RGBA")
        a = a.resize((round(a.width * ah / a.height), ah), Image.LANCZOS)
        x, y = round(x * 0.75), round(y * 0.75)
        room.alpha_composite(a, (x - a.width // 2, y - ah))
    return room.crop(room.getbbox())


def fit(img, height):
    return img.resize((round(img.width * height / img.height), height), Image.LANCZOS)


def main():
    logo = Image.open(sys.argv[1]) if len(sys.argv) > 1 else None
    canvas = sky().convert("RGBA")

    if logo is not None:
        # The logo of the project is the mark at the left.
        t = fit(logo.convert("RGBA").crop(logo.getbbox()), 248)
        canvas.alpha_composite(t, (36, (H - t.height) // 2))
    else:
        t = fit(tower(), 284)
        canvas.alpha_composite(t, (36, H - t.height - 10))

    o = fit(office(), 284)
    canvas.alpha_composite(o, (W - o.width - 14, (H - o.height) // 2 + 4))

    # The title and the tagline, between the tower and the office.
    left = 36 + t.width + 16
    right = W - o.width - 14
    title, tag = "AGENT TOWER", "A PIXEL OFFICE FOR AI AGENTS"
    ts = 8
    while text_width(title, ts) > right - left - 24:
        ts -= 1
    gs = 3
    while text_width(tag, gs) > right - left - 24:
        gs -= 1
    block_h = 7 * ts + 28 + 7 * gs
    y0 = (H - block_h) // 2 - 6
    cx = (left + right) // 2
    draw_text(canvas, title, cx - text_width(title, ts) // 2, y0, ts, ACCENT, shadow=ACCENT_DARK, outline=BG_TOP)
    draw_text(canvas, tag, cx - text_width(tag, gs) // 2, y0 + 7 * ts + 28, gs, DIM, outline=BG_TOP)

    OUT.parent.mkdir(parents=True, exist_ok=True)
    canvas.convert("RGB").save(OUT, optimize=True)
    print(OUT, canvas.size, f"title x{ts}", f"tagline x{gs}")


if __name__ == "__main__":
    main()
