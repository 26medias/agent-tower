"""Cleans the generated art and writes web images plus a stacked preview for each style."""
from PIL import Image
import numpy as np

S = 0.5  # web scale
# Measured on the 1536x1024 sources: wall height, facade slope, sticker centre and width.
STYLES = {
    "pixel": dict(h=300, slope=0.404, cx=1098, cy=689, sw=170),
    "lowpoly": dict(h=336, slope=0.41, cx=1114, cy=694, sw=190),
}


def clean(im):
    a = np.array(im.convert("RGBA"))
    a[a[..., 3] < 128] = 0
    return Image.fromarray(a)


def trim(im):
    return im.crop(im.getbbox())


logos = clean(Image.open("art/logos.png"))
stickers = [trim(logos.crop(b)) for b in [(0, 0, 512, 512), (0, 512, 512, 1024), (512, 512, 1024, 1024)]]
stickers.append(trim(clean(Image.open("art/logo-gear.png"))))
for i, s in enumerate(stickers):
    s.thumbnail((256, 256))
    s.save(f"web/sticker-{i}.webp", quality=90)


def skew(st, w, slope):
    st = st.resize((w, round(st.height * w / st.width)))
    extra = round(slope * w)
    return st.transform((w, st.height + extra), Image.AFFINE, (1, 0, 0, slope, 1, -extra), Image.BICUBIC)


for name, g in STYLES.items():
    floor = clean(Image.open(f"art/floor-{name}.png"))
    small = floor.resize((round(1536 * S), round(1024 * S)), Image.LANCZOS)
    small.save(f"web/floor-{name}.webp", quality=88)
    n = 4
    canvas = Image.new("RGBA", (1536, 1024 + (n - 1) * g["h"]), (16, 24, 38, 255))
    for i in range(n):
        y = (n - 1 - i) * g["h"]
        canvas.alpha_composite(floor, (0, y))
        sk = skew(stickers[i], g["sw"], g["slope"])
        canvas.alpha_composite(sk, (g["cx"] - sk.width // 2, y + g["cy"] - sk.height // 2))
    canvas.resize((canvas.width // 2, canvas.height // 2)).convert("RGB").save(f"art/preview-{name}.jpg", quality=85)

# --- Office interior and agent sprites (pixel style) ---
office = clean(Image.open("art/office-pixel.png"))
office.resize((1152, 768), Image.LANCZOS).save("web/office-pixel.webp", quality=88)
sheet = clean(Image.open("art/agents-pixel.png"))
alpha = np.array(sheet)[..., 3]
cols = alpha.max(axis=0) > 0
runs, start = [], None
for x, on in enumerate(list(cols) + [False]):
    if on and start is None:
        start = x
    elif not on and start is not None:
        if x - start > 40:
            runs.append((start, x))
        start = None
print("agent runs", runs)
agents = []
for i, (x0, x1) in enumerate(runs):
    a = trim(sheet.crop((x0, 0, x1, sheet.height)))
    a = a.resize((round(a.width * 300 / a.height), 300), Image.NEAREST if False else Image.LANCZOS)
    a.save(f"web/agent-{i}.webp", quality=90)
    agents.append(a)
    print(i, a.size)
# Feet anchors on the 1536x1024 office image: beside each desk.
SPOTS = [(617, 625), (925, 625), (1240, 625), (617, 845), (925, 845), (1240, 845)]
AH = 160
prev = Image.new("RGBA", office.size, (16, 24, 38, 255))
prev.alpha_composite(office)
for a, (x, y) in zip(agents, SPOTS):
    s = a.resize((round(a.width * AH / a.height), AH), Image.LANCZOS)
    prev.alpha_composite(s, (x - s.width // 2, y - AH))
prev.resize((768, 512)).convert("RGB").save("art/preview-office.jpg", quality=85)

# --- Modular office: empty room and one desk sprite, placed by the page ---
clean(Image.open("art/room-empty.png")).resize((1152, 768), Image.LANCZOS).save("web/room-pixel.webp", quality=88)
# desk-back2.png: no chair, monitor seen from the back, so that the agent stands behind the desk.
clean(Image.open("art/desk-back2.png")).resize((512, 512), Image.LANCZOS).save("web/desk-back.webp", quality=90)
