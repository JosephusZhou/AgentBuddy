#!/usr/bin/env python3
"""Octopus icon generator: one geometry -> SVG + PNG (app icon + menu bar icon).

Layers (painter's order): ink (black) -> cut (white / transparent) -> ink2 (black again, pupils).
"""
import math, os, random, sys
import numpy as np
from PIL import Image, ImageDraw, ImageFilter

OUT = sys.argv[1] if len(sys.argv) > 1 else os.path.dirname(os.path.abspath(__file__))

# ---------------------------------------------------------------- geometry helpers
def catmull(pts, n=160):
    p = [np.array(q, float) for q in pts]
    p = [p[0] * 2 - p[1]] + p + [p[-1] * 2 - p[-2]]
    seg = []
    per = max(8, n // (len(p) - 3))
    for i in range(1, len(p) - 2):
        p0, p1, p2, p3 = p[i - 1], p[i], p[i + 1], p[i + 2]
        for t in np.linspace(0, 1, per, endpoint=False):
            seg.append(0.5 * ((2 * p1) + (-p0 + p2) * t + (2 * p0 - 5 * p1 + 4 * p2 - p3) * t * t
                              + (-p0 + 3 * p1 - 3 * p2 + p3) * t ** 3))
    seg.append(p[-2])
    return np.array(seg)


def stroke(center, width_fn):
    """Variable-width stroke polygon along a centerline (Nx2). width_fn(t)->full width."""
    c = np.array(center)
    d = np.diff(c, axis=0)
    L = np.r_[0, np.cumsum(np.hypot(d[:, 0], d[:, 1]))]
    t = L / L[-1]
    tang = np.gradient(c, axis=0)
    tang /= np.maximum(np.hypot(tang[:, 0], tang[:, 1])[:, None], 1e-9)
    nrm = np.stack([-tang[:, 1], tang[:, 0]], 1)
    w = np.array([width_fn(x) for x in t]) / 2
    left = c + nrm * w[:, None]
    right = c - nrm * w[:, None]
    cap = []
    r = w[-1]
    for a in np.linspace(0, math.pi, 10)[1:-1]:
        v = tang[-1] * math.sin(a) + nrm[-1] * math.cos(a)
        cap.append(c[-1] + v * r)
    poly = np.vstack([left, np.array(cap), right[::-1]])
    return poly, c, nrm, t, w


def ellipse(cx, cy, rx, ry, n=90, rot=0):
    a = np.linspace(0, 2 * math.pi, n, endpoint=False)
    x, y = rx * np.cos(a), ry * np.sin(a)
    cr, sr = math.cos(rot), math.sin(rot)
    return np.stack([cx + x * cr - y * sr, cy + x * sr + y * cr], 1)


def egg(cx, cy, rx, ry, k=0.16, shear=0.0, n=140):
    a = np.linspace(0, 2 * math.pi, n, endpoint=False)
    x = rx * np.sin(a) * (1 - k * np.cos(a))
    y = -ry * np.cos(a)
    return np.stack([cx + x + shear * (-y), cy + y], 1)


def rrect(cx, cy, hw, hh, r=None, n=8):
    r = r if r is not None else min(hw, hh)
    pts = []
    for (sx, sy, a0) in [(1, 1, 0), (-1, 1, 90), (-1, -1, 180), (1, -1, 270)]:
        for a in np.linspace(a0, a0 + 90, n):
            pts.append((cx + sx * (hw - r) + r * math.cos(math.radians(a)) * 1,
                        cy + sy * (hh - r) + r * math.sin(math.radians(a)) * 1))
    # correct corner placement
    pts = []
    corners = [(hw - r, hh - r, 0), (-(hw - r), hh - r, 90), (-(hw - r), -(hh - r), 180), (hw - r, -(hh - r), 270)]
    for ox, oy, a0 in corners:
        for a in np.linspace(a0, a0 + 90, n):
            pts.append((cx + ox + r * math.cos(math.radians(a)), cy + oy + r * math.sin(math.radians(a))))
    return np.array(pts)


def superellipse(cx, cy, half, n_exp=5.0, N=400):
    a = np.linspace(0, 2 * math.pi, N, endpoint=False)
    c, s = np.cos(a), np.sin(a)
    x = np.sign(c) * np.abs(c) ** (2 / n_exp)
    y = np.sign(s) * np.abs(s) ** (2 / n_exp)
    return np.stack([cx + half * x, cy + half * y], 1)

# ---------------------------------------------------------------- the octopus (design space ~1000 units)
def build(detail=True, bold=1.0, seed=7):
    ink, cut, ink2, cut2 = [], [], [], []

    # --- mantle (head bulb) and head
    ink.append(egg(512, 322, 190, 214, k=0.12, shear=0.09))
    ink.append(ellipse(500, 462, 156, 104))

    # --- eyes: socket bumps
    eyes = [(396, 474), (606, 470)]
    for (ex, ey) in eyes:
        ink.append(ellipse(ex, ey, 54, 52))
    # web / skirt where arms join
    ink.append(ellipse(500, 535, 130, 58))

    # --- arms
    arms = [
        # right side, inner -> outer       (w0)
        ([(535, 540), (548, 650), (522, 765), (545, 850), (600, 902), (646, 884), (648, 842)], 62),
        ([(568, 532), (606, 640), (612, 752), (662, 832), (735, 862), (778, 832), (756, 794), (718, 810)], 60),
        ([(594, 520), (660, 600), (704, 692), (768, 742), (834, 742), (862, 700), (836, 668), (802, 688)], 52),
        ([(616, 506), (696, 540), (770, 592), (846, 610), (898, 578), (888, 538), (854, 534)], 48),
        # left side
        ([(466, 540), (454, 652), (470, 772), (448, 852), (396, 906), (350, 888), (348, 846)], 58),
        ([(434, 532), (398, 642), (378, 754), (326, 824), (258, 848), (228, 812), (258, 786), (292, 804)], 56),
        ([(408, 520), (342, 606), (292, 692), (222, 726), (160, 714), (148, 672), (180, 654)], 52),
        ([(388, 506), (306, 546), (232, 598), (162, 612), (110, 578), (124, 540), (158, 538)], 48),
    ]
    rnd = random.Random(seed)
    for pts, w0 in arms:
        c = catmull(pts, 200)
        w0 *= bold
        wfn = lambda t, w0=w0: w0 * (1 - t) ** 0.78 + 5 * bold
        poly, cc, nrm, t, w = stroke(c, wfn)
        ink.append(poly)
        if detail:
            # suckers: alternating sides, shrinking with the arm
            k = 0.10
            flip = 1
            while k < 0.93:
                i = min(int(k * (len(cc) - 1)), len(cc) - 1)
                wi = w[i] * 2
                r = max(2.4, wi * 0.15)
                off = nrm[i] * wi * 0.20 * flip
                cut.append(ellipse(cc[i][0] + off[0], cc[i][1] + off[1], r, r * 0.92, n=20))
                flip = -flip
                k += max(0.032, 0.05 * (1 - k * 0.5))

    # --- eye details: white of the eye, dark iris, specular glint, heavy upper lid
    for (ex, ey) in eyes:
        cut.append(ellipse(ex, ey + 1, 40, 35, n=60))              # sclera
        ink2.append(ellipse(ex + 1, ey + 3, 27, 27, n=48))         # iris / pupil
        cut2.append(ellipse(ex - 8, ey - 6, 6.5, 6.5, n=20))       # glint
        lid = catmull([(ex - 46, ey - 8), (ex - 24, ey - 30), (ex + 20, ey - 33), (ex + 46, ey - 10)], 30)
        pl, *_ = stroke(lid, lambda t: 11 * math.sin(math.pi * t) ** 0.7 + 1.5)
        ink2.append(pl)

    if detail:
        # --- mantle highlights (crescent) + speckle texture
        hl = catmull([(432, 262), (440, 196), (474, 154), (516, 138)], 60)
        pl, *_ = stroke(hl, lambda t: 13 * math.sin(math.pi * t) ** 0.8 + 1)
        cut.append(pl)
        hl2 = catmull([(418, 312), (422, 284)], 20)
        pl, *_ = stroke(hl2, lambda t: 9 * math.sin(math.pi * t) + 1)
        cut.append(pl)
        pts = []
        tries = 0
        while len(pts) < 30 and tries < 4000:
            tries += 1
            x = rnd.uniform(380, 650)
            y = rnd.uniform(130, 440)
            # inside the mantle/head region, away from eyes + highlight
            if ((x - 520) / 150) ** 2 + ((y - 300) / 190) ** 2 > 0.86 and y < 420:
                continue
            if y >= 420 and ((x - 500) / 135) ** 2 + ((y - 440) / 55) ** 2 > 1:
                continue
            if any(math.hypot(x - ex, y - ey) < 64 for ex, ey in eyes):
                continue
            if x < 460 and y < 290:
                continue
            if any(math.hypot(x - px, y - py) < 24 for px, py, _ in pts):
                continue
            pts.append((x, y, rnd.uniform(2.4, 5.4)))
        for x, y, r in pts:
            cut.append(ellipse(x, y, r, r, n=16))
    return ink, cut, ink2, cut2


def bbox(polys):
    a = np.vstack(polys)
    return a.min(0), a.max(0)


def fit(polys_layers, size, target_h, cx, cy, ref):
    (x0, y0), (x1, y1) = bbox(ref)
    s = target_h / (y1 - y0)
    mx, my = (x0 + x1) / 2, (y0 + y1) / 2
    out = []
    for layer in polys_layers:
        out.append([(p - [mx, my]) * s + [cx, cy] for p in layer])
    return out, s

# ---------------------------------------------------------------- SVG writers
def d_of(poly):
    return "M" + " L".join(f"{x:.1f},{y:.1f}" for x, y in poly) + "Z"


def svg_app(layers, size=1024):
    ink, cut, ink2, cut2 = layers
    half = 412
    bg = superellipse(512, 512, half)
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 1024 1024">
  <defs>
    <filter id="shadow" x="-20%" y="-20%" width="140%" height="150%">
      <feDropShadow dx="0" dy="14" stdDeviation="18" flood-color="#000" flood-opacity="0.28"/>
    </filter>
  </defs>
  <path d="{d_of(bg)}" fill="#fff" stroke="#e4e4e7" stroke-width="2" filter="url(#shadow)"/>
  <g id="octopus">
    <path d="{' '.join(d_of(p) for p in ink)}" fill="#000" fill-rule="nonzero"/>
    <path d="{' '.join(d_of(p) for p in cut)}" fill="#fff"/>
    <path d="{' '.join(d_of(p) for p in ink2)}" fill="#000"/>
    <path d="{' '.join(d_of(p) for p in cut2)}" fill="#fff"/>
  </g>
</svg>
'''


def svg_menubar(layers, size=1024, color="#000"):
    ink, cut, ink2, cut2 = layers
    return f'''<svg xmlns="http://www.w3.org/2000/svg" width="{size}" height="{size}" viewBox="0 0 1024 1024">
  <mask id="m" maskUnits="userSpaceOnUse" x="0" y="0" width="1024" height="1024">
    <path d="{' '.join(d_of(p) for p in ink)}" fill="#fff"/>
    <path d="{' '.join(d_of(p) for p in cut)}" fill="#000"/>
    <path d="{' '.join(d_of(p) for p in ink2)}" fill="#fff"/>
    <path d="{' '.join(d_of(p) for p in cut2)}" fill="#000"/>
  </mask>
  <rect width="1024" height="1024" fill="{color}" mask="url(#m)"/>
</svg>
'''

# ---------------------------------------------------------------- PNG rasterisers (supersampled)
SS = 4


def raster_mask(layers, size):
    """Float alpha of the octopus shape with cutouts at `size` px (coords are in 1024 space)."""
    ink, cut, ink2, cut2 = layers
    big = size * SS
    k = big / 1024.0
    m = Image.new("L", (big, big), 0)
    d = ImageDraw.Draw(m)
    for group, val in ((ink, 255), (cut, 0), (ink2, 255), (cut2, 0)):
        for p in group:
            d.polygon([tuple(q * k) for q in p], fill=val)
    return m.resize((size, size), Image.LANCZOS)


def png_app(layers, size=1024):
    S = 1024
    big = S * SS
    # background squircle
    bgm = Image.new("L", (big, big), 0)
    ImageDraw.Draw(bgm).polygon([tuple(q * SS) for q in superellipse(512, 512, 412)], fill=255)
    bgm = bgm.resize((S, S), Image.LANCZOS)
    img = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    # soft shadow
    sh = Image.new("L", (S, S), 0)
    sh.paste(bgm.point(lambda v: int(v * 0.28)), (0, 14))
    sh = sh.filter(ImageFilter.GaussianBlur(18))
    img.paste(Image.new("RGBA", (S, S), (0, 0, 0, 255)), (0, 0), sh)
    # white tile + hairline border
    border = bgm.filter(ImageFilter.MinFilter(3))
    img.paste(Image.new("RGBA", (S, S), (228, 228, 231, 255)), (0, 0), bgm)
    inner = bgm.filter(ImageFilter.MinFilter(5))
    img.paste(Image.new("RGBA", (S, S), (255, 255, 255, 255)), (0, 0), inner)
    # octopus
    om = raster_mask(layers, S)
    img.paste(Image.new("RGBA", (S, S), (0, 0, 0, 255)), (0, 0), om)
    if size != S:
        img = img.resize((size, size), Image.LANCZOS)
    return img


def png_menubar(layers, size, color=(0, 0, 0)):
    om = raster_mask(layers, size)
    img = Image.new("RGBA", (size, size), color + (0,))
    img.putalpha(om)
    return img


# ---------------------------------------------------------------- main
def main():
    os.makedirs(OUT, exist_ok=True)
    # App icon: octopus ~ 58% of the 1024 canvas height, centred on the tile
    ink, cut, ink2, cut2 = build(detail=True, bold=1.0)
    (L_app, s_app) = fit([ink, cut, ink2, cut2], 1024, 590, 512, 520, ink)
    open(f"{OUT}/octopus-app-icon.svg", "w").write(svg_app(L_app))
    png_app(L_app, 1024).save(f"{OUT}/octopus-app-icon-1024.png")
    for sz in (512, 256, 128, 64, 32, 16):
        png_app(L_app, sz).save(f"{OUT}/octopus-app-icon-{sz}.png")

    # Menu bar icon: simplified, bolder, fills the square
    ink, cut, ink2, cut2 = build(detail=False, bold=1.28)
    (L_mb, s_mb) = fit([ink, cut, ink2, cut2], 1024, 960, 512, 512, ink)
    open(f"{OUT}/octopus-menubar-template.svg", "w").write(svg_menubar(L_mb))
    open(f"{OUT}/octopus-menubar-white.svg", "w").write(svg_menubar(L_mb, color="#fff"))
    png_menubar(L_mb, 1024).save(f"{OUT}/octopus-menubar-template-1024.png")
    for pt, name in ((22, "octopusTemplate@2x"), (11, None)):
        pass
    for px, name in ((22, "octopusTemplate"), (44, "octopusTemplate@2x"), (66, "octopusTemplate@3x")):
        png_menubar(L_mb, px).save(f"{OUT}/{name}.png")
    for px in (44,):
        png_menubar(L_mb, px, (255, 255, 255)).save(f"{OUT}/octopus-menubar-white@2x.png")

    # preview sheet for review
    sheet = Image.new("RGBA", (1400, 560), (255, 255, 255, 255))
    sheet.paste(png_app(L_app, 512), (20, 20), png_app(L_app, 512))
    mb = png_menubar(L_mb, 512)
    tile = Image.new("RGBA", (400, 220), (236, 236, 240, 255)); tile.paste(mb.resize((200, 200)), (100, 10), mb.resize((200, 200)))
    sheet.paste(tile, (560, 20))
    dk = Image.new("RGBA", (400, 220), (28, 28, 30, 255))
    mw = png_menubar(L_mb, 512, (255, 255, 255)).resize((200, 200)); dk.paste(mw, (100, 10), mw)
    sheet.paste(dk, (980, 20))
    # real-size menu bar strip (22pt @2x = 44px, shown at 2x zoom and 1x)
    strip_l = Image.new("RGBA", (400, 100), (236, 236, 240, 255))
    strip_d = Image.new("RGBA", (400, 100), (28, 28, 30, 255))
    for i, px in enumerate((44, 36, 28, 22, 16)):
        a = png_menubar(L_mb, px); b = png_menubar(L_mb, px, (255, 255, 255))
        strip_l.paste(a, (20 + i * 76, 28), a); strip_d.paste(b, (20 + i * 76, 28), b)
    sheet.paste(strip_l, (560, 270)); sheet.paste(strip_d, (980, 270))
    # small app icon sizes
    x = 560
    for sz in (128, 64, 32, 16):
        im = png_app(L_app, sz); sheet.paste(im, (x, 400), im); x += sz + 24
    sheet.convert("RGB").save(f"{OUT}/_preview.png")


main()
