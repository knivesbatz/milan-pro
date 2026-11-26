"""Milan Pro brand: builds every file in assets/brand from the Blender renders (tools/brand/render.py).

    python tools/brand/compose.py                 all brand assets into assets/brand
    python tools/brand/compose.py --directions X  the three explored directions side by side, into X (a PNG)
    python tools/brand/compose.py --sheet X       contact sheet of the delivered assets, into X (a PNG)
    options: --cache DIR (renders are kept and reused there), --rerender (render again), --blender EXE

Needs Python 3 with Pillow and numpy, Blender (run headless: `-b --factory-startup`, never a window) and the
Windows system fonts (Trebuchet MS, Tahoma, Verdana, Franklin Gothic); no font or image ships with the brand.

The look is Windows XP era (2002-2006): XP icon rules (3/4 view, light from the upper left, rich gradients, a soft
dark outline, the 135 degree / 2 px / 75 % drop shadow, separately drawn 16-24 px sizes) and Luna-blue box-art
panels with soft white swooshes over a brushed-aluminium footer.
"""
import argparse
import glob
import io
import os
import struct
import subprocess
import sys
import tempfile

import numpy as np
from PIL import Image, ImageDraw, ImageEnhance, ImageFilter, ImageFont

HERE = os.path.dirname(os.path.abspath(__file__))
REPO = os.path.dirname(os.path.dirname(HERE))
BRAND = os.path.join(REPO, "assets", "brand")
FONTS = os.path.join(os.environ.get("WINDIR", r"C:\Windows"), "Fonts")
ICO_SIZES = (16, 20, 24, 32, 40, 48, 64, 96, 128, 256)
WIZARD = ((202, 386), (269, 515), (336, 643), (403, 772), (430, 824))
WIZARD_SMALL = (58, 77, 97, 116, 124)
TAG = "NONLINEAR EDITING SYSTEM"
LEGAL = "Copyright \u00a9 2026 Milan Pro Contributors. Open source under the MIT licence."

# ------------------------------------------------------------------------------------------------- Blender renders


def find_blender(explicit=None):
    if explicit:
        return explicit
    if os.environ.get("BLENDER"):
        return os.environ["BLENDER"]
    pats = [os.path.join(os.environ.get("LOCALAPPDATA", ""), "Programs", "Blender Foundation", "*", "blender.exe"),
            os.path.join(os.environ.get("ProgramFiles", r"C:\Program Files"), "Blender Foundation", "*", "blender.exe")]
    hits = sorted(h for p in pats for h in glob.glob(p))
    if not hits:
        sys.exit("Blender not found: set BLENDER or pass --blender")
    return hits[-1]


class Renders:
    def __init__(self, cache, blender, force):
        self.cache, self.blender, self.force = cache, blender, force
        os.makedirs(cache, exist_ok=True)

    def get(self, obj, size, variant="full"):
        out = os.path.join(self.cache, "%s-%s-%d.png" % (obj, variant, size))
        if self.force or not os.path.exists(out):
            cmd = [find_blender(self.blender), "-b", "--factory-startup", "--python", os.path.join(HERE, "render.py"),
                   "--", obj, out, str(size), variant]
            flags = getattr(subprocess, "CREATE_NO_WINDOW", 0)
            r = subprocess.run(cmd, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, creationflags=flags)
            if r.returncode or not os.path.exists(out):
                sys.stderr.write(r.stdout.decode(errors="replace")[-3000:])
                sys.exit("Blender render failed: %s %s" % (obj, variant))
        return Image.open(out).convert("RGBA")


# ------------------------------------------------------------------------------------------------- pixel helpers


def hexrgb(h):
    h = h.lstrip("#")
    return tuple(int(h[i:i + 2], 16) for i in (0, 2, 4))


def resize(im, w, h, method=Image.LANCZOS):
    return im.convert("RGBa").resize((max(1, w), max(1, h)), method).convert("RGBA")


def crop_alpha(im, thresh=6):
    bb = im.getchannel("A").point(lambda a: 255 if a > thresh else 0).getbbox()
    return im.crop(bb) if bb else im


def fit(im, w, h):
    im = crop_alpha(im)
    s = min(w / im.width, h / im.height)
    return resize(im, round(im.width * s), round(im.height * s))


def arr(im):
    return np.asarray(im, dtype=np.float32) / 255.0


def img(a, mode="RGBA"):
    return Image.fromarray(np.clip(a * 255.0 + 0.5, 0, 255).astype(np.uint8), mode)


def gradient(w, h, stops, horizontal=False, diag=None):
    """Multi-stop gradient as an RGB float array; diag=(dx, dy) runs it along that direction."""
    if diag:
        yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
        t = (xx + 0.5) / w * diag[0] + (yy + 0.5) / h * diag[1]
        t = (t - t.min()) / max(1e-6, t.max() - t.min())
    elif horizontal:
        t = np.tile((np.arange(w, dtype=np.float32) + 0.5) / w, (h, 1))
    else:
        t = np.tile(((np.arange(h, dtype=np.float32) + 0.5) / h)[:, None], (1, w))
    pos = np.array([s[0] for s in stops], np.float32)
    cols = np.array([hexrgb(s[1]) for s in stops], np.float32) / 255.0
    out = np.empty((h, w, 3), np.float32)
    for c in range(3):
        out[..., c] = np.interp(t, pos, cols[:, c])
    return out


def layer(rgb, alpha):
    """RGBA image from an RGB float array and an L image / float alpha array."""
    a = arr(alpha) if isinstance(alpha, Image.Image) else alpha
    return img(np.dstack([rgb, a]))


def paste_over(canvas, im, pos):
    tmp = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    tmp.paste(im, (int(pos[0]), int(pos[1])))
    return Image.alpha_composite(canvas, tmp)


def font(name, px):
    return ImageFont.truetype(os.path.join(FONTS, name), px)


# ------------------------------------------------------------------------------------------------- XP icon finish


def xp_outline(im, unit, strength=0.6):
    """XP icons carry a soft dark outline: the object's own edge colour, darkened, about 1 px at 48 px."""
    r = max(1, round(unit))
    a = im.getchannel("A")
    er = a.filter(ImageFilter.MinFilter(2 * r + 1))
    edge = np.clip(arr(a) - arr(er), 0, 1)
    if r > 1:
        edge = arr(img(edge, "L").filter(ImageFilter.GaussianBlur(r * 0.35)))
    px = arr(im)
    px[..., :3] *= (1.0 - strength * edge)[..., None]
    return img(px)


def xp_shadow(canvas, obj, pos, unit, opacity=0.75):
    """The XP drop shadow: angle 135 degrees, distance 2, size 2, black 75 % (at 48 px)."""
    off = max(1, round(2 * unit))
    sh = Image.new("L", canvas.size, 0)
    sh.paste(obj.getchannel("A"), (pos[0] + off, pos[1] + off))
    sh = sh.filter(ImageFilter.GaussianBlur(max(0.45, unit)))
    sh = sh.point(lambda v: int(v * opacity))
    black = Image.new("RGBA", canvas.size, (0, 0, 0, 0))
    black.putalpha(sh)
    canvas = Image.alpha_composite(canvas, black)
    return paste_over(canvas, obj, pos)


def xp_icon(src, size, crisp=False):
    """An XP-finished icon of a render: fitted into size minus the shadow, outlined, shadowed."""
    unit = size / 48.0
    off = max(1, round(2 * unit))
    inner = size - off - (1 if size <= 64 else round(unit * 0.5))
    obj = fit(src, inner, inner)
    if crisp:
        rgb = obj.convert("RGB")
        rgb = ImageEnhance.Color(rgb).enhance(1.12)
        rgb = rgb.filter(ImageFilter.UnsharpMask(radius=0.8, percent=70, threshold=0))
        a = obj.getchannel("A").point(lambda v: 0 if v < 24 else min(255, int((v - 24) * 255 / 200)))
        obj = rgb.convert("RGBA")
        obj.putalpha(a)
    obj = xp_outline(obj, unit, 0.55 if size >= 64 else 0.5)
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    pos = ((size - off - obj.width) // 2, (size - off - obj.height) // 2)
    return xp_shadow(canvas, obj, pos, unit, 0.75 if size <= 48 else 0.6)


# ------------------------------------------------------------------------------------------------- 16-24 px icons
# Drawn, not reduced: at these sizes XP icons were redrawn straight-on and pixel-pushed. Each size places the reel,
# its windows, the hub and the play wedge on its own pixel grid (coordinates in pixels, y down).
TINY = {
    16: dict(cx=6.5, cy=6.5, R=6.5, back=1.0, ring=3.55, hole=1.45, hub=1.55, tri=((8.0, 7.5), (8.0, 15.0), (15.0, 11.25))),
    20: dict(cx=8.0, cy=8.0, R=8.0, back=1.25, ring=4.4, hole=1.8, hub=1.9, tri=((10.0, 9.5), (10.0, 19.0), (19.0, 14.25))),
    24: dict(cx=9.5, cy=9.5, R=9.5, back=1.5, ring=5.25, hole=2.15, hub=2.25, tri=((12.0, 11.0), (12.0, 23.0), (23.0, 17.0))),
}
SS = 16


def _mask(size, draw):
    m = Image.new("L", (size * SS, size * SS), 0)
    draw(ImageDraw.Draw(m))
    return m


def _circle(cx, cy, r):
    return lambda d: d.ellipse([(cx - r) * SS, (cy - r) * SS, (cx + r) * SS - 1, (cy + r) * SS - 1], fill=255)


def _down(m, size):
    return m.resize((size, size), Image.BOX)


def _ring(m, px=1.0):
    k = int(round(px * SS)) * 2 + 1
    er = m.filter(ImageFilter.MinFilter(k))
    return Image.fromarray(np.clip(np.asarray(m, np.int16) - np.asarray(er, np.int16), 0, 255).astype(np.uint8))


def _minus(a, b):
    return Image.fromarray(np.clip(np.asarray(a, np.int16) - np.asarray(b, np.int16), 0, 255).astype(np.uint8))


def tiny_icon(size):
    import math
    p = TINY[size]
    cx, cy, R = p["cx"], p["cy"], p["R"]
    canvas = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    paint = []  # (rgb array, alpha L image at native size)

    def add(rgb, m_ss):
        paint.append((rgb, _down(m_ss, size)))

    tl = (0.7, 1.0)  # light from the upper left: gradients run toward the lower right
    back = _mask(size, _circle(cx + p["back"], cy, R))
    front = _mask(size, _circle(cx, cy, R))
    add(gradient(size, size, [(0, "#3f72d8"), (1, "#0f2f86")], diag=tl), back)
    add(np.full((size, size, 3), np.array(hexrgb("#0b2468")) / 255.0, np.float32), _ring(back, 1.0))
    add(gradient(size, size, [(0, "#a9c8ff"), (0.35, "#4f86ee"), (1, "#1a49b8")], diag=tl), front)
    add(np.full((size, size, 3), np.array(hexrgb("#0c2a78")) / 255.0, np.float32), _ring(front, 1.0))
    holes = Image.new("L", front.size, 0)
    dh = ImageDraw.Draw(holes)
    for k in range(5):
        a = math.radians(90 + k * 72)
        _circle(cx + p["ring"] * math.cos(a), cy - p["ring"] * math.sin(a), p["hole"])(dh)
    add(gradient(size, size, [(0, "#14100c"), (1, "#4a3a2c")], diag=tl), holes)
    hub = _mask(size, _circle(cx, cy, p["hub"]))
    add(gradient(size, size, [(0, "#ffffff"), (0.5, "#c3cad4"), (1, "#7a8494")], diag=tl), hub)
    add(np.full((size, size, 3), np.array(hexrgb("#4a5260")) / 255.0, np.float32), _ring(hub, 0.75))
    tri = _mask(size, lambda d: d.polygon([(x * SS, y * SS) for x, y in p["tri"]], fill=255))
    ys = [y for _, y in p["tri"]]
    tg = np.zeros((size, size, 3), np.float32)
    y0, y1 = min(ys) / size, max(ys) / size
    t = np.clip(((np.arange(size) + 0.5) / size - y0) / (y1 - y0), 0, 1)
    stops = np.array([hexrgb("#ffdc90"), hexrgb("#ffa424"), hexrgb("#d86000")], np.float32) / 255.0
    for c in range(3):
        tg[..., c] = np.interp(t, [0, 0.45, 1], stops[:, c])[:, None]
    add(tg, tri)
    add(np.full((size, size, 3), np.array(hexrgb("#8a3c00")) / 255.0, np.float32), _ring(tri, 1.0))
    # a glint on the flange's upper left
    glint = _mask(size, _circle(cx - R * 0.5, cy - R * 0.52, max(0.8, R * 0.16)))
    paint.append((np.ones((size, size, 3), np.float32), Image.fromarray((np.asarray(_down(glint, size)) * 0.75).astype(np.uint8))))
    obj = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    for rgb, a in paint:
        obj = Image.alpha_composite(obj, layer(rgb, a))
    # shadow: 1 px down-right, soft, as the XP drop shadow reduces at this size
    union = obj.getchannel("A")
    sh = Image.new("L", (size, size), 0)
    sh.paste(union, (1, 1))
    sh = sh.filter(ImageFilter.GaussianBlur(0.5)).point(lambda v: int(v * 0.5))
    shadow = Image.new("RGBA", (size, size), (0, 0, 0, 0))
    shadow.putalpha(sh)
    return Image.alpha_composite(Image.alpha_composite(canvas, shadow), obj)


# ------------------------------------------------------------------------------------------------- wordmark

CHROME = [(0, "#ffffff"), (0.40, "#e6ebf2"), (0.49, "#9eabbd"), (0.53, "#6c7b92"), (0.64, "#aab8cb"), (1, "#f5f8fc")]
AMBER = [(0, "#ffe7a8"), (0.42, "#ffb43c"), (0.52, "#f08400"), (0.70, "#f79a1c"), (1, "#ffc75e")]


def styled_text(text, fnt, stops, K, stroke_px, stroke_col="#0b1f4f", bevel=1.0):
    """Photoshop-era type: gradient overlay, inner bevel lit from the upper left, dark stroke (all at K x)."""
    bb = fnt.getbbox(text)
    pad = int(stroke_px * K) + 4 * K
    w, h = bb[2] - bb[0] + 2 * pad, bb[3] - bb[1] + 2 * pad
    m = Image.new("L", (w, h), 0)
    ImageDraw.Draw(m).text((pad - bb[0], pad - bb[1]), text, font=fnt, fill=255)
    a = arr(m)
    rgb = gradient(w, h, stops)
    # restrict the gradient to the glyph box so the horizon sits mid-letter
    gy0, gy1 = pad, h - pad
    t = np.clip((np.arange(h, dtype=np.float32) - gy0) / max(1, gy1 - gy0), 0, 1)
    pos = np.array([s[0] for s in stops], np.float32)
    cols = np.array([hexrgb(s[1]) for s in stops], np.float32) / 255.0
    for c in range(3):
        rgb[..., c] = np.interp(t, pos, cols[:, c])[:, None]
    # bevel
    hb = arr(m.filter(ImageFilter.GaussianBlur(1.6 * K * bevel)))
    gy, gx = np.gradient(hb)
    shade = (-gx - gy) * 0.7071 * (6.0 * K * bevel)
    inner = np.clip(a - arr(m.filter(ImageFilter.MinFilter(int(3 * K * bevel) | 1))), 0, 1)
    inner = np.clip(inner + 0.35 * a, 0, 1)
    rgb = np.clip(rgb + (np.clip(shade, -1, 1) * inner)[..., None] * 0.55, 0, 1)
    fill = np.dstack([rgb, a])
    sk = int(round(stroke_px * K)) * 2 + 1
    st = arr(m.filter(ImageFilter.MaxFilter(sk)))
    st = arr(img(st, "L").filter(ImageFilter.GaussianBlur(K * 0.4)))
    stroke = np.dstack([np.broadcast_to(np.array(hexrgb(stroke_col), np.float32) / 255.0, (h, w, 3)), st])
    out = Image.alpha_composite(img(stroke), img(fill))
    return out


def wordmark(width, fontname="trebucbi.ttf", tag=True, tag_col="#1d3d7c", shadow=0.55, glow=False):
    """'Milan' in chrome and 'Pro' in amber, italic, stroked dark navy, with a drop shadow; tag line under it."""
    K = 3
    size = 200 * K
    f = font(fontname, size)
    milan = styled_text("Milan", f, CHROME, K, 5.5)
    pro = styled_text("Pro", f, AMBER, K, 5.5)
    gap = int(size * 0.17)
    pad = 8 * K
    trim = int(5.5 * K) + 4 * K  # styled_text padding beyond the stroke
    w = milan.width + pro.width + gap - trim
    h = max(milan.height, pro.height)
    W, H = w + 2 * pad, h + 2 * pad
    canvas = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    word = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    word = paste_over(word, milan, (pad, pad))
    word = paste_over(word, pro, (pad + milan.width + gap - trim, pad))
    if glow:
        g = word.getchannel("A").filter(ImageFilter.GaussianBlur(14 * K)).point(lambda v: int(v * 0.55))
        gl = Image.new("RGBA", (W, H), (255, 255, 255, 0))
        gl.putalpha(g)
        canvas = Image.alpha_composite(canvas, gl)
    if shadow:
        s2 = Image.new("L", (W, H), 0)
        s2.paste(word.getchannel("A"), (4 * K, 4 * K))
        s2 = s2.filter(ImageFilter.GaussianBlur(3.5 * K)).point(lambda v: int(v * shadow))
        sl = Image.new("RGBA", (W, H), (0, 0, 0, 0))
        sl.putalpha(s2)
        canvas = Image.alpha_composite(canvas, sl)
    canvas = crop_alpha(Image.alpha_composite(canvas, word), 2)
    word = resize(canvas, width, round(canvas.height * width / canvas.width))
    if not tag:
        return word
    # The descriptor: Tahoma Bold capitals, letter-spaced, set at the final pixel size (hinted, not reduced),
    # right-aligned under the word the way box art set it.
    tpx = max(9, round(width * 0.033))
    tf = font("tahomabd.ttf", tpx)
    spacing = max(1, round(tpx * 0.30))

    def adv(ch):
        return tf.getlength(ch) + spacing

    while sum(adv(c) for c in TAG) - spacing > width * 0.98 and spacing > 0:
        spacing -= 1
    tw = sum(adv(c) for c in TAG) - spacing
    y = word.height - round(width * 0.012)
    out = Image.new("RGBA", (width, y + tpx + 4), (0, 0, 0, 0))
    out = paste_over(out, word, (0, 0))
    d = ImageDraw.Draw(out)
    x0 = width - tw - round(width * 0.012)
    for dx, dy, col in ((1, 1, (0, 0, 0, 150) if tag_col.lower() == "#ffffff" else None), (0, 0, hexrgb(tag_col) + (255,))):
        if col is None:
            continue
        x = x0
        for ch in TAG:
            d.text((x + dx, y + dy), ch, font=tf, fill=col)
            x += adv(ch)
    return out


# ------------------------------------------------------------------------------------------------- panels


def brushed(w, h, seed=7):
    rng = np.random.default_rng(seed)
    n = rng.normal(0, 1, (h, w + 64)).astype(np.float32)
    k = 31
    c = np.cumsum(n, axis=1)
    n = (c[:, k:] - c[:, :-k])[:, :w] / np.sqrt(k)
    return n


def footer(w, h):
    base = gradient(w, h, [(0, "#f3f5f8"), (0.5, "#dde1e7"), (1, "#c4cad3")])
    base += brushed(w, h)[..., None] * 0.018
    base[0, :] = np.array(hexrgb("#5b6b86")) / 255.0
    base[1, :] = np.array(hexrgb("#ffffff")) / 255.0
    base[-1, :] = np.array(hexrgb("#8d96a4")) / 255.0
    return np.clip(base, 0, 1)


def swoosh(w, h, cx, cy, a, b, width, fade):
    """A soft band along an ellipse (centre cx, cy; radii a, b), fading out along x by `fade` (x0, x1)."""
    yy, xx = np.mgrid[0:h, 0:w].astype(np.float32)
    f = np.sqrt(((xx - cx) / a) ** 2 + ((yy - cy) / b) ** 2) - 1.0
    band = np.exp(-(f / width) ** 2)
    x0, x1 = fade
    ramp = np.clip((xx - x0) / (x1 - x0), 0, 1)
    ramp = np.sin(ramp * np.pi) ** 1.2
    return band * ramp


def blue_panel(w, h, foot=0, glow_at=None, glow_r=None):
    sky_h = h - foot
    rgb = gradient(w, sky_h, [(0, "#7aa7ee"), (0.38, "#3d73dc"), (0.75, "#1e4fbe"), (1, "#0f3392")])
    # soft white swooshes from the lower left up to the right
    s = (swoosh(w, sky_h, w * 0.78, sky_h * 1.55, w * 0.95, sky_h * 1.05, 0.020, (-w * 0.1, w * 1.05)) * 0.55
         + swoosh(w, sky_h, w * 0.80, sky_h * 1.62, w * 0.98, sky_h * 1.10, 0.060, (-w * 0.1, w * 1.1)) * 0.22
         + swoosh(w, sky_h, w * 0.30, sky_h * 1.9, w * 1.1, sky_h * 1.25, 0.012, (w * 0.2, w * 1.2)) * 0.35)
    if glow_at:
        yy, xx = np.mgrid[0:sky_h, 0:w].astype(np.float32)
        d = np.sqrt((xx - glow_at[0]) ** 2 + (yy - glow_at[1]) ** 2) / glow_r
        s = s + np.exp(-d ** 2 * 1.6) * 0.45
    rgb = rgb + (1 - rgb) * np.clip(s, 0, 1)[..., None]
    # a little brushed grain so the blue is not a dead fill
    rgb = rgb + brushed(w, sky_h, 3)[..., None] * 0.006
    if foot:
        rgb = np.vstack([rgb, footer(w, foot)])
        # shade the blue just above the footer
        for i, k in enumerate((0.30, 0.16, 0.07)):
            rgb[sky_h - 1 - i] *= 1 - k
    return img(np.dstack([np.clip(rgb, 0, 1), np.ones((h, w), np.float32)]))


def frame(im, col="#0a2260"):
    d = ImageDraw.Draw(im)
    d.rectangle([0, 0, im.width - 1, im.height - 1], outline=hexrgb(col) + (255,))
    return im


def text(im, xy, s, px=11, col="#1e2430", bold=False, anchor="la"):
    d = ImageDraw.Draw(im)
    d.text(xy, s, font=font("tahomabd.ttf" if bold else "tahoma.ttf", px), fill=hexrgb(col) + (255,), anchor=anchor)


def splash(mark, fontname="trebucbi.ttf"):
    """520 x 320. The app draws the progress groove at y 300-306 (x 24-496) and the version at the right of the
    footer's first line (src/ui/splash.rs); the art keeps both clear."""
    W, H, F = 520, 320, 52
    ic = xp_icon(mark, 178)
    im = blue_panel(W, H, F, glow_at=(112, 128), glow_r=150)
    im = paste_over(im, ic, (24, 40))
    wm = wordmark(282, fontname, tag=True, tag_col="#ffffff", shadow=0.6)
    im = paste_over(im, wm, (214, 132 - wm.height // 2))
    text(im, (24, 276), LEGAL, 11, "#2a3242")
    return frame(im)


def about_banner(mark, fontname="trebucbi.ttf"):
    W, H = 480, 120
    im = blue_panel(W, H, 0, glow_at=(62, 58), glow_r=90)
    im = paste_over(im, xp_icon(mark, 108), (12, 6))
    wm = wordmark(300, fontname, tag=True, tag_col="#ffffff", shadow=0.6)
    im = paste_over(im, wm, (142, 62 - wm.height // 2))
    d = ImageDraw.Draw(im)
    d.line([(0, H - 1), (W, H - 1)], fill=hexrgb("#0a2260") + (255,))
    return im


def readme_banner(mark, fontname="trebucbi.ttf"):
    W, H, F = 960, 260, 34
    im = blue_panel(W, H, F, glow_at=(190, 112), glow_r=200)
    im = paste_over(im, xp_icon(mark, 200), (88, 14))
    wm = wordmark(560, fontname, tag=True, tag_col="#ffffff", shadow=0.6)
    im = paste_over(im, wm, (318, 112 - wm.height // 2))
    text(im, (W // 2, H - F // 2 + 1), "Open-source nonlinear video editor for Windows  \u2022  " + LEGAL.split(". ")[0] + ".", 11, "#2a3242", anchor="mm")
    return frame(im)


def wizard(mark, w, h, fontname="trebucbi.ttf"):
    s = w / 202.0
    F = round(30 * s)
    icon = round(w * 0.70)
    im = blue_panel(w, h, F, glow_at=(w * 0.5, h * 0.06 + icon * 0.5), glow_r=w * 0.65)
    im = paste_over(im, xp_icon(mark, icon), ((w - icon) // 2, round(h * 0.06)))
    wm = wordmark(round(w * 0.86), fontname, tag=False, shadow=0.6)
    y = round(h * 0.06) + icon + round(10 * s)
    im = paste_over(im, wm, ((w - wm.width) // 2, y))
    tf = font("tahomabd.ttf", max(9, round(9 * s)))
    d = ImageDraw.Draw(im)
    d.text((w // 2, y + wm.height + round(6 * s)), "Nonlinear Editing System", font=tf, fill=(255, 255, 255, 255), anchor="ma")
    return frame(im).convert("RGB")


# ------------------------------------------------------------------------------------------------- ICO


def ico_bytes(images):
    """A Windows .ico: 32-bit BMP entries (with the AND mask) below 256 px, a PNG entry at 256 px."""
    entries, blobs = [], []
    for im in images:
        n = im.width
        if n >= 256:
            b = io.BytesIO()
            im.save(b, "PNG", optimize=True)
            data = b.getvalue()
        else:
            px = np.asarray(im.convert("RGBA"), np.uint8)
            bgra = px[::-1, :, [2, 1, 0, 3]].tobytes()
            row = ((n + 31) // 32) * 4
            mask = bytearray()
            for y in range(n - 1, -1, -1):
                rb = bytearray(row)
                for x in range(n):
                    if px[y, x, 3] == 0:
                        rb[x // 8] |= 0x80 >> (x % 8)
                mask += rb
            hdr = struct.pack("<IiiHHIIiiII", 40, n, n * 2, 1, 32, 0, len(bgra) + len(mask), 0, 0, 0, 0)
            data = hdr + bgra + bytes(mask)
        entries.append((n, data))
    out = io.BytesIO()
    out.write(struct.pack("<HHH", 0, 1, len(entries)))
    off = 6 + 16 * len(entries)
    for n, data in entries:
        d = 0 if n >= 256 else n
        out.write(struct.pack("<BBBBHHII", d, d, 0, 0, 1, 32, len(data), off))
        off += len(data)
    for _, data in entries:
        out.write(data)
    return out.getvalue()


def read_ico(path):
    """(size, kind) of every entry, parsed from the directory (kind: 'png' or 'bmp')."""
    b = open(path, "rb").read()
    _, typ, count = struct.unpack_from("<HHH", b, 0)
    assert typ == 1
    out = []
    for i in range(count):
        w, h, _, _, _, bpp, ln, off = struct.unpack_from("<BBBBHHII", b, 6 + 16 * i)
        kind = "png" if b[off:off + 8] == b"\x89PNG\r\n\x1a\n" else "bmp"
        out.append((w or 256, kind, bpp))
    return out


# ------------------------------------------------------------------------------------------------- SVG

SVG = """<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">
  <!-- Milan Pro: vector approximation of the rendered mark (tools/brand); blue film reel, orange play wedge. -->
  <defs>
    <linearGradient id="flange" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#a9c8ff"/><stop offset=".35" stop-color="#4f86ee"/><stop offset="1" stop-color="#1a49b8"/>
    </linearGradient>
    <linearGradient id="back" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#3f72d8"/><stop offset="1" stop-color="#0f2f86"/>
    </linearGradient>
    <linearGradient id="hub" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#ffffff"/><stop offset=".5" stop-color="#c3cad4"/><stop offset="1" stop-color="#7a8494"/>
    </linearGradient>
    <linearGradient id="wedge" x1="0" y1="0" x2="0" y2="1">
      <stop offset="0" stop-color="#ffdc90"/><stop offset=".45" stop-color="#ffa424"/><stop offset="1" stop-color="#d86000"/>
    </linearGradient>
    <linearGradient id="frame" x1="0" y1="0" x2="1" y2="0">
      <stop offset="0" stop-color="#3d7be0"/><stop offset="1" stop-color="#e9f2ff"/>
    </linearGradient>
    <filter id="shadow" x="-10%" y="-10%" width="130%" height="130%">
      <feGaussianBlur in="SourceAlpha" stdDeviation="4"/><feOffset dx="8" dy="8"/>
      <feComponentTransfer><feFuncA type="linear" slope=".6"/></feComponentTransfer>
      <feMerge><feMergeNode/><feMergeNode in="SourceGraphic"/></feMerge>
    </filter>
  </defs>
  <g filter="url(#shadow)">
    <g transform="translate(14 150) rotate(-14)">
      <rect x="0" y="0" width="96" height="34" rx="3" fill="#24201c" stroke="#0b0907" stroke-width="2"/>
      <rect x="8" y="9" width="22" height="16" fill="url(#frame)"/><rect x="36" y="9" width="22" height="16" fill="url(#frame)"/>
      <rect x="64" y="9" width="22" height="16" fill="url(#frame)"/>
    </g>
    <ellipse cx="140" cy="104" rx="84" ry="98" transform="rotate(14 140 104)" fill="url(#back)" stroke="#0b2468" stroke-width="4"/>
    <ellipse cx="118" cy="110" rx="84" ry="98" transform="rotate(14 118 110)" fill="url(#flange)" stroke="#0c2a78" stroke-width="5"/>
    <g fill="#2a221c" stroke="#0c2a78" stroke-width="2">
      <ellipse cx="126" cy="54" rx="20" ry="24"/><ellipse cx="72" cy="96" rx="19" ry="23"/>
      <ellipse cx="166" cy="104" rx="19" ry="23"/><ellipse cx="88" cy="160" rx="19" ry="23"/>
      <ellipse cx="146" cy="164" rx="19" ry="23"/>
    </g>
    <ellipse cx="118" cy="110" rx="22" ry="25" fill="url(#hub)" stroke="#4a5260" stroke-width="3"/>
    <ellipse cx="118" cy="110" rx="6" ry="7" fill="#4a5260"/>
    <path d="M136 132 L136 238 L232 186 Z" fill="url(#wedge)" stroke="#8a3c00" stroke-width="5" stroke-linejoin="round"/>
    <ellipse cx="74" cy="58" rx="16" ry="9" transform="rotate(-40 74 58)" fill="#ffffff" opacity=".55"/>
  </g>
</svg>
"""

# ------------------------------------------------------------------------------------------------- builds


def build(r):
    master = r.get("reel", 1024, "full")
    small = r.get("reel", 512, "small")
    icons = {}
    for n in ICO_SIZES:
        if n in TINY:
            icons[n] = tiny_icon(n)
        elif n <= 64:
            icons[n] = xp_icon(small, n, crisp=True)
        else:
            icons[n] = xp_icon(master, n)
    ico = os.path.join(BRAND, "milan-pro.ico")
    open(ico, "wb").write(ico_bytes([icons[n] for n in ICO_SIZES]))
    icons[256].save(os.path.join(BRAND, "milan-pro-256.png"), optimize=True)
    xp_icon(master, 512).save(os.path.join(BRAND, "milan-pro-512.png"), optimize=True)
    open(os.path.join(BRAND, "milan-pro.svg"), "w", newline="\n").write(SVG)
    wordmark(800).save(os.path.join(BRAND, "wordmark-800.png"), optimize=True)
    splash(master).convert("RGB").save(os.path.join(BRAND, "splash-520x320.png"), optimize=True)
    about_banner(master).convert("RGB").save(os.path.join(BRAND, "about-banner-480x120.png"), optimize=True)
    readme_banner(master).convert("RGB").save(os.path.join(BRAND, "readme-banner-960x260.png"), optimize=True)
    for w, h in WIZARD:
        wizard(master, w, h).save(os.path.join(BRAND, "installer", "wizard-%dx%d.png" % (w, h)), optimize=True)
    for n in WIZARD_SMALL:
        xp_icon(small if n <= 64 else master, n, crisp=n <= 64).save(
            os.path.join(BRAND, "installer", "wizard-small-%d.png" % n), optimize=True)
    # self-check
    got = read_ico(ico)
    assert [g[0] for g in got] == list(ICO_SIZES), got
    assert all((k == "png") == (n == 256) and bpp == 32 for n, k, bpp in got), got
    print("milan-pro.ico:", ", ".join("%d %s" % (n, k) for n, k, _ in got))


def directions(r, out):
    rows = [("A  Film reel + play wedge (chosen)", "reel", "trebucbi.ttf"),
            ("B  Clapperboard, chrome hinge", "clapper", "FRAHVIT.TTF"),
            ("C  Lens orb + filmstrip swoosh", "orb", "verdanaz.ttf")]
    W, RH = 1500, 330
    sheet = Image.new("RGBA", (W, RH * len(rows) + 40), hexrgb("#ece9d8") + (255,))
    text(sheet, (16, 12), "Milan Pro brand directions (XP era, 2002-2006): icon at 256 / 48 / 32 / 16 px, wordmark, splash",
         13, "#000000", bold=True)
    for i, (label, obj, fnt) in enumerate(rows):
        y = 40 + i * RH
        src = r.get(obj, 512, "full")
        text(sheet, (16, y + 4), label, 11, "#000000", bold=True)
        sheet = paste_over(sheet, xp_icon(src, 256), (16, y + 26))
        x = 290
        for n in (48, 32, 16):
            ic = tiny_icon(n) if (obj == "reel" and n in TINY) else xp_icon(src, n, crisp=True)
            sheet = paste_over(sheet, ic, (x, y + 40))
            blue = Image.new("RGBA", (n + 12, n + 12), hexrgb("#3a6ea5") + (255,))
            blue = paste_over(blue, ic, (6, 6))
            sheet = paste_over(sheet, blue, (x - 6, y + 110))
            x += n + 24
        wm = wordmark(420, fnt)
        sheet = paste_over(sheet, wm, (430, y + 60))
        sp = splash(src, fnt)
        sheet = paste_over(sheet, resize(sp, 520, 320), (W - 540, y + 4))
    sheet.convert("RGB").save(out)
    print("wrote", out)


def contact_sheet(out):
    """Every delivered asset at 1:1, and the icon at every ICO size on white, XP blue and the grey taskbar."""
    files = ["milan-pro-512.png", "milan-pro-256.png", "wordmark-800.png", "splash-520x320.png",
             "about-banner-480x120.png", "readme-banner-960x260.png"]
    ims = [(f, Image.open(os.path.join(BRAND, f)).convert("RGBA")) for f in files]
    wiz = [("installer/wizard-%dx%d.png" % s, Image.open(os.path.join(BRAND, "installer", "wizard-%dx%d.png" % s)).convert("RGBA")) for s in WIZARD]
    sm = [("installer/wizard-small-%d.png" % n, Image.open(os.path.join(BRAND, "installer", "wizard-small-%d.png" % n)).convert("RGBA")) for n in WIZARD_SMALL]
    ico = Image.open(os.path.join(BRAND, "milan-pro.ico"))
    icos = []
    for n in ICO_SIZES:
        ico.size = (n, n)
        icos.append(ico.copy().convert("RGBA"))
    W = 2000
    H = 6000
    sheet = Image.new("RGBA", (W, H), (255, 255, 255, 255))
    text(sheet, (16, 10), "Milan Pro brand assets (1:1) - tools/brand", 13, "#000000", bold=True)
    # ICO strips
    y = 36
    for name, bg in (("white", "#ffffff"), ("XP desktop #3A6EA5", "#3a6ea5"), ("taskbar grey #D4D0C8", "#d4d0c8"), ("Luna taskbar #245EDC", "#245edc")):
        strip = Image.new("RGBA", (W - 32, 280), hexrgb(bg) + (255,))
        x = 12
        for n, ic in zip(ICO_SIZES, icos):
            strip = paste_over(strip, ic, (x, 268 - n))
            text(strip, (x, 4), str(n), 11, "#000000" if bg in ("#ffffff", "#d4d0c8") else "#ffffff")
            x += n + 18
        text(strip, (W - 300, 4), name, 11, "#000000" if bg in ("#ffffff", "#d4d0c8") else "#ffffff", bold=True)
        sheet = paste_over(sheet, strip, (16, y))
        y += 290
    # 16 px icons enlarged x8 for inspection
    x = 16
    for n in (16, 20, 24, 32):
        ic = icos[ICO_SIZES.index(n)]
        big = Image.new("RGBA", (n, n), (255, 255, 255, 255))
        big = Image.alpha_composite(big, ic).resize((n * 8, n * 8), Image.NEAREST)
        sheet = paste_over(sheet, big, (x, y + 18))
        text(sheet, (x, y + 2), "%d px x8" % n, 11, "#000000")
        x += n * 8 + 20
    y += 18 + 32 * 8 + 16
    # the files
    x, rowh = 16, 0
    for name, im in ims + sm + wiz:
        if x + max(im.width, 8 + 6 * len(name)) > W - 16:
            x, y = 16, y + rowh + 26
            rowh = 0
        text(sheet, (x, y), name + " (%dx%d)" % im.size, 11, "#000000")
        checker = Image.new("RGBA", im.size, hexrgb("#e8e8e8") + (255,))
        sheet = paste_over(sheet, Image.alpha_composite(checker, im), (x, y + 16))
        x += max(im.width, 8 + 6 * len(name)) + 20
        rowh = max(rowh, im.height + 16)
    y += rowh + 30
    sheet = sheet.crop((0, 0, W, y))
    os.makedirs(os.path.dirname(out), exist_ok=True)
    sheet.convert("RGB").save(out)
    print("wrote", out)


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--cache", default=os.path.join(tempfile.gettempdir(), "milan-brand-renders"))
    ap.add_argument("--blender")
    ap.add_argument("--rerender", action="store_true")
    ap.add_argument("--directions")
    ap.add_argument("--sheet")
    a = ap.parse_args()
    r = Renders(a.cache, a.blender, a.rerender)
    if a.directions:
        directions(r, a.directions)
    elif a.sheet:
        contact_sheet(a.sheet)
    else:
        build(r)


if __name__ == "__main__":
    main()
