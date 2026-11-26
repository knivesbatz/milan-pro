"""Milan Pro brand assets, early-2000s "metallic/cyber" edition: the SVG master, the .ico, the PNG marks, the chrome
wordmark, the splash and About banner, and the Inno Setup wizard images. Everything is drawn procedurally with Pillow.

    python tools/make-icons.py [preview.png]

Run from anywhere; writes into assets/brand/ (see assets/brand/BRAND.md for the system). The mark is a brushed-gunmetal
plate with a faint grid and cyan circuit traces, a thin neon ring, a chrome "M" (horizon gradient, bevel, dark outline,
drop shadow) and a lens-flare glint with a hot-orange secondary. Sizes up to 48 px come from hand-placed pixel grids
(LAYOUTS) so the stems, the plate edge and the two-tone chrome split land on whole pixels; 64 px and up are reduced
from one 1024 px master render.
"""
import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageChops, ImageDraw, ImageFilter, ImageFont, ImageOps

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "brand"
FONTS = Path("C:/Windows/Fonts")

# Palette (BRAND.md): steel, gunmetal, electric cyan/blue, one hot accent.
WHITE = (0xFF, 0xFF, 0xFF)
INK = (0x07, 0x0A, 0x10)
STEEL_HI, STEEL, STEEL_LO = (0xE6, 0xEC, 0xF2), (0x9C, 0xA8, 0xB6), (0x3A, 0x44, 0x52)
GUN_TOP, GUN_MID, GUN_BOT = (0x3C, 0x46, 0x58), (0x1B, 0x21, 0x2C), (0x07, 0x09, 0x0F)
CYAN, BLUE, EDGE = (0x3C, 0xE6, 0xFF), (0x2A, 0x7F, 0xFF), (0x2E, 0xB8, 0xF0)
HOT = (0xFF, 0x8A, 0x1F)

# Chrome: sky-to-horizon gradient, a dark horizon band, a cooler darker ground reflection below it.
CHROME = [(0.00, WHITE), (0.10, (0xE2, 0xE8, 0xEE)), (0.40, (0x9A, 0xA6, 0xB4)), (0.49, (0x4A, 0x55, 0x62)),
          (0.51, (0x22, 0x2A, 0x34)), (0.56, (0x4E, 0x62, 0x78)), (0.80, (0x9F, 0xB8, 0xCC)), (1.00, (0xDC, 0xE8, 0xF2))]
PLATE = [(0.0, GUN_TOP), (0.55, GUN_MID), (1.0, GUN_BOT)]
EXTRUDE = [(0.0, STEEL_LO), (1.0, INK)]

# Master design on a 256 grid: plate inset/radius, ring radius/stroke, the M (x0..x1 outer stems, t..b, stem w, V tip v).
DESIGN = dict(inset=8, r=52, ring_r=80, ring_w=5, x0=58, x1=198, t=66, b=180, w=32, v=164)

# Hand-hinted small sizes: whole-pixel stems, plate and neon edge ("ring" = edge width).
LAYOUTS = {
    16: dict(inset=0, r=3, ring=1, x0=3, x1=13, t=3, b=13, w=2, v=12),
    20: dict(inset=0, r=4, ring=1, x0=4, x1=16, t=4, b=16, w=3, v=15),
    24: dict(inset=0, r=5, ring=1, x0=5, x1=19, t=5, b=19, w=3, v=17),
    32: dict(inset=1, r=6, ring=1, x0=8, x1=24, t=8, b=24, w=4, v=22),
    40: dict(inset=1, r=8, ring=1, x0=10, x1=30, t=10, b=30, w=5, v=27),
    48: dict(inset=2, r=9, ring=2, x0=12, x1=36, t=12, b=36, w=6, v=33),
}
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


# ----------------------------------------------------------------------------------------------- drawing helpers
def lerp(a, b, f):
    return tuple(round(a[i] + (b[i] - a[i]) * f) for i in range(3))


def luts(stops):
    out = [[0] * 256 for _ in range(3)]
    for i in range(256):
        p = i / 255
        c0, c1, f = stops[-1][1], stops[-1][1], 0.0
        for (p0, a), (p1, b) in zip(stops, stops[1:]):
            if p0 <= p <= p1:
                c0, c1, f = a, b, (p - p0) / (p1 - p0) if p1 > p0 else 0.0
                break
        for ch in range(3):
            out[ch][i] = round(c0[ch] + (c1[ch] - c0[ch]) * f)
    return out


def gradient(w, h, stops, horizontal=False):
    """An RGB gradient through colour stops, top-to-bottom (or left-to-right)."""
    g = Image.linear_gradient("L")
    if horizontal:
        g = g.transpose(Image.Transpose.ROTATE_90)
    g = g.resize((w, h), Image.BILINEAR)
    return Image.merge("RGB", [g.point(l) for l in luts(stops)])


def radial(w, h, power=1.0, gain=1.0):
    """An L glow: 255 at the centre falling to 0 at the edge of the w x h ellipse."""
    g = ImageOps.invert(Image.radial_gradient("L")).resize((w, h), Image.BILINEAR)
    return g.point(lambda v: min(255, round(255 * gain * (v / 255) ** power)))


def solid(colour, mask):
    im = Image.new("RGBA", mask.size, colour + (0,))
    im.putalpha(mask)
    return im


def scaled(mask, f):
    return mask.point(lambda v: round(v * f))


def stamp(canvas, small, colour, cx, cy):
    """Alpha-composite a small L mask (centred on cx, cy, may hang over the edge) in a colour."""
    layer = Image.new("L", canvas.size, 0)
    layer.paste(small, (round(cx - small.width / 2), round(cy - small.height / 2)))
    canvas.alpha_composite(solid(colour, layer))


def flare(canvas, cx, cy, size, streak=4.5, hot=True):
    """A lens-flare glint: hot white core, cyan halo, a long horizontal streak and an orange secondary."""
    stamp(canvas, radial(round(size * 2.6), round(size * 2.6), 2.2, 0.55), CYAN, cx, cy)
    stamp(canvas, radial(round(size * streak), max(3, round(size * 0.22)), 1.3), (0xD8, 0xF6, 0xFF), cx, cy)
    stamp(canvas, radial(max(3, round(size * 0.16)), round(size * 1.6), 1.3, 0.8), (0xD8, 0xF6, 0xFF), cx, cy)
    stamp(canvas, radial(size, size, 3.0), WHITE, cx, cy)
    if hot:
        stamp(canvas, radial(round(size * 0.5), round(size * 0.5), 1.8, 0.9), HOT, cx + size * 1.15, cy + size * 0.55)
        stamp(canvas, radial(round(size * 0.28), round(size * 0.28), 1.6, 0.8), (0xFF, 0xD0, 0x80), cx - size * 0.8,
              cy - size * 0.35)


def chrome(mask, bevel=2, outline=2, extrude=0, stops=CHROME):
    """Chrome through an L mask: extrusion, dark outline, horizon gradient body, light top-left / dark bottom-right
    bevel rims. The gradient spans the mask's own vertical extent."""
    w, h = mask.size
    out = Image.new("RGBA", (w, h), (0, 0, 0, 0))
    for i in range(extrude, 0, -1):
        out.alpha_composite(solid(lerp(STEEL_LO, INK, i / extrude), ImageChops.offset(mask, i, i)))
    if outline:
        out.alpha_composite(solid(INK, mask.filter(ImageFilter.MaxFilter(2 * outline + 1))))
    x0, y0, x1, y1 = mask.getbbox()
    fill = Image.new("RGB", (w, h), INK)
    fill.paste(gradient(w, y1 - y0, stops), (0, y0))
    body = fill.convert("RGBA")
    body.putalpha(mask)
    out.alpha_composite(body)
    hi = ImageChops.subtract(mask, ImageChops.offset(mask, bevel, bevel))
    lo = ImageChops.subtract(mask, ImageChops.offset(mask, -bevel, -bevel))
    out.alpha_composite(solid(WHITE, scaled(hi, 0.92)))
    out.alpha_composite(solid(INK, scaled(lo, 0.75)))
    return out


def font(name, size):
    for n in ([name] if isinstance(name, str) else name):
        try:
            return ImageFont.truetype(str(FONTS / n), size)
        except OSError:
            pass
    return ImageFont.load_default(size)


def spaced_text(draw, xy, text, f, spacing, fill=255):
    x, y = xy
    for ch in text:
        draw.text((x, y), ch, font=f, fill=fill)
        x += draw.textlength(ch, font=f) + spacing
    return x


def text_width(draw, text, f, spacing):
    return sum(draw.textlength(ch, font=f) for ch in text) + spacing * (len(text) - 1)


# ----------------------------------------------------------------------------------------------- the mark
def layout(n):
    if n in LAYOUTS:
        return LAYOUTS[n]
    k = n / 256
    d = {key: DESIGN[key] * k for key in DESIGN}
    d["x0"], d["w"] = round(DESIGN["x0"] * k), round(DESIGN["w"] * k)
    d["x1"] = n - d["x0"]
    return d


def m_shapes(L):
    """The M as rectangles (stems) and quads (diagonals meeting in a point at v), in pixels. Diagonals are 0.9x
    the stem weight."""
    x0, x1, t, b, w, v = (L[k] for k in ("x0", "x1", "t", "b", "w", "v"))
    cx = (x0 + x1) / 2
    dx, dy = cx - x0, v - t
    hw = 0.9 * w * math.hypot(dx, dy) / dy
    k = hw * dy / dx
    rects = [(x0, t, x0 + w, b), (x1 - w, t, x1, b)]
    quads = [[(x0, t), (x0 + hw, t), (cx, v - k), (cx, v)],
             [(x1, t), (x1 - hw, t), (cx, v - k), (cx, v)]]
    return rects, quads


def m_mask(L, size, ss=1, grow=0):
    """L mask of the M at ss x scale, optionally grown by `grow` px (for outlines at small sizes)."""
    mask = Image.new("L", (size * ss, size * ss), 0)
    d = ImageDraw.Draw(mask)
    rects, quads = m_shapes(L)
    for x0, y0, x1, y1 in rects:
        d.rectangle([(x0 - grow) * ss, (y0 - grow) * ss, (x1 + grow) * ss - 1, (y1 + grow) * ss - 1], fill=255)
    for q in quads:
        d.polygon([(x * ss, y * ss) for x, y in q], fill=255)
    if grow:
        for q in quads:  # grow the diagonals by stroking their outline
            d.line([(x * ss, y * ss) for x, y in q + q[:1]], fill=255, width=round(2 * grow * ss))
    return mask


def render_small(n, ss=16):
    """Hand-hinted mark at n <= 48 px: neon edge, gunmetal plate, two-tone chrome M (plus horizon band and dark
    outline from 32 px). Drawn ss times larger then box-averaged so edges are exact pixel coverage."""
    L = layout(n)
    S = n * ss
    im = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)
    i, e, r = L["inset"], L["ring"], L["r"]
    d.rounded_rectangle([i * ss, i * ss, (n - i) * ss - 1, (n - i) * ss - 1], radius=r * ss, fill=EDGE + (255,))
    inner = Image.new("L", (S, S), 0)
    ImageDraw.Draw(inner).rounded_rectangle([(i + e) * ss, (i + e) * ss, (n - i - e) * ss - 1, (n - i - e) * ss - 1],
                                            radius=max(0, r - e) * ss, fill=255)
    plate = gradient(S, S, PLATE).convert("RGBA")
    if n >= 32:  # a soft light at the top of the plate and a 1 px steel bevel line under the edge
        stamp(plate, radial(S, S // 2, 1.5, 0.45), (0x8A, 0x9A, 0xB2), S / 2, 0)
        ImageDraw.Draw(plate).rectangle([(i + e + 1) * ss, (i + e) * ss, (n - i - e - 1) * ss - 1, (i + e + 1) * ss - 1],
                                        fill=(0x5E, 0x6C, 0x80, 255))
    im.paste(plate, (0, 0), inner)
    t, b = L["t"], L["b"]
    hgt = b - t
    band = hgt // 2
    top, bottom = (0xEE, 0xF2, 0xF6), (0x8C, 0x9E, 0xB4)
    if n >= 32:
        stops = [(0, WHITE), (band / hgt, (0xB8, 0xC2, 0xCE)), (band / hgt, (0x2E, 0x3A, 0x48)),
                 ((band + 1) / hgt, (0x2E, 0x3A, 0x48)), ((band + 1) / hgt, (0x6E, 0x86, 0xA0)), (1, (0xC8, 0xD8, 0xE6))]
        im.alpha_composite(solid(INK, m_mask(L, n, ss, grow=1)))
    else:
        stops = [(0, top), (band / hgt, top), (band / hgt, bottom), (1, bottom)]
    fill = Image.new("RGB", (S, S), INK)
    fill.paste(gradient(S, hgt * ss, stops), (0, t * ss))
    body = fill.convert("RGBA")
    body.putalpha(m_mask(L, n, ss))
    im.alpha_composite(body)
    if n >= 40:  # a one-pixel glint on the left stem's shoulder
        d.rectangle([L["x0"] * ss, t * ss, (L["x0"] + 1) * ss - 1, (t + 1) * ss - 1], fill=WHITE + (255,))
    return im.reduce(ss)


def render_hd(S=1024):
    """The master mark at S px (RGBA, transparent outside the plate)."""
    L = layout(S)
    k = S / 256
    i, r = L["inset"], L["r"]
    plate_mask = Image.new("L", (S, S), 0)
    ImageDraw.Draw(plate_mask).rounded_rectangle([i, i, S - i - 1, S - i - 1], radius=r, fill=255)

    # Brushed gunmetal plate with a soft top light.
    base = gradient(S, S, PLATE)
    noise = Image.effect_noise((S // 28, S), 40).resize((S, S), Image.BILINEAR)
    base = Image.blend(base, ImageChops.soft_light(base, Image.merge("RGB", (noise, noise, noise))), 0.55)
    im = base.convert("RGBA")
    stamp(im, radial(round(S * 1.3), round(S * 0.8), 1.6, 0.42), (0x8A, 0x9A, 0xB2), S / 2, S * 0.12)

    # Grid and circuit traces.
    grid = Image.new("RGBA", (S, S), (0, 0, 0, 0))
    g = ImageDraw.Draw(grid)
    step = S / 12
    for q in range(1, 12):
        p = round(q * step)
        g.line([(p, 0), (p, S)], fill=WHITE + (16,), width=max(1, round(k * 0.6)))
        g.line([(0, p), (S, p)], fill=WHITE + (16,), width=max(1, round(k * 0.6)))
    traces = [[(0.08, 0.30), (0.08, 0.16), (0.16, 0.08), (0.34, 0.08)],
              [(0.92, 0.70), (0.92, 0.84), (0.84, 0.92), (0.66, 0.92)],
              [(0.66, 0.08), (0.78, 0.08), (0.84, 0.14)],
              [(0.16, 0.92), (0.28, 0.92), (0.34, 0.86)]]
    for tr in traces:
        pts = [(x * S, y * S) for x, y in tr]
        g.line(pts, fill=CYAN + (120,), width=max(1, round(k * 0.9)), joint="curve")
        for x, y in (pts[0], pts[-1]):
            g.ellipse([x - k * 1.6, y - k * 1.6, x + k * 1.6, y + k * 1.6], fill=CYAN + (200,))
    im.alpha_composite(grid)
    im.alpha_composite(solid(CYAN, scaled(grid.getchannel("A").filter(ImageFilter.GaussianBlur(k * 1.5)), 0.5)))

    # Plate bevel: light rim top-left, dark rim bottom-right, then the outer edge in electric blue.
    rim = round(k * 1.6)
    hi = ImageChops.subtract(plate_mask, ImageChops.offset(plate_mask, rim, rim)).filter(ImageFilter.GaussianBlur(k * 0.4))
    lo = ImageChops.subtract(plate_mask, ImageChops.offset(plate_mask, -rim, -rim)).filter(ImageFilter.GaussianBlur(k * 0.4))
    im.alpha_composite(solid(STEEL, scaled(hi, 0.9)))
    im.alpha_composite(solid(INK, scaled(lo, 0.8)))
    edge = ImageChops.subtract(plate_mask, ImageOps.invert(ImageOps.invert(plate_mask).filter(ImageFilter.MaxFilter(round(k) * 2 + 1))))
    im.alpha_composite(solid(EDGE, scaled(edge, 0.85)))

    # Neon ring with glow, behind the M.
    c, rr, rw = S / 2, L["ring_r"], L["ring_w"]
    ring = Image.new("L", (S, S), 0)
    ImageDraw.Draw(ring).ellipse([c - rr, c - rr, c + rr, c + rr], outline=255, width=round(rw))
    im.alpha_composite(solid(CYAN, scaled(ring.filter(ImageFilter.GaussianBlur(k * 4)), 0.9)))
    im.alpha_composite(solid((0xC8, 0xF6, 0xFF), ring))
    im.alpha_composite(solid(WHITE, scaled(ImageChops.subtract(ring, ImageChops.offset(ring, 0, round(rw * 0.6))), 0.7)))

    # The chrome M: drop shadow, then chrome.
    mask = m_mask(L, S)
    shadow = ImageChops.offset(mask, round(k * 2.5), round(k * 4)).filter(ImageFilter.GaussianBlur(k * 3))
    im.alpha_composite(solid(INK, scaled(shadow, 0.75)))
    im.alpha_composite(chrome(mask, bevel=round(k * 1.6), outline=round(k * 1.4)))
    # Specular: a soft sheen on the upper left stem and the glint on the right shoulder.
    stamp(im, radial(round(k * 46), round(k * 70), 2.0, 0.45), WHITE, L["x0"] + L["w"] * 0.5, L["t"] + k * 26)
    flare(im, L["x1"] - L["w"] * 0.5, L["t"] + k * 2, round(k * 14))

    im.putalpha(ImageChops.multiply(im.getchannel("A"), plate_mask))
    return im


_HD = {}


def render_mark(n):
    """The mark at n x n px: hand-hinted up to 48, reduced from the 1024 master above."""
    if n in LAYOUTS:
        return render_small(n)
    if "hd" not in _HD:
        _HD["hd"] = render_hd()
    return _HD["hd"].resize((n, n), Image.LANCZOS)


def svg():
    """A vector approximation of the mark (gradients and strokes; no bevel filters) for docs and the web."""
    L = DESIGN
    i, r = L["inset"], L["r"]
    rects, quads = m_shapes(L)
    f = lambda v: f"{v:.2f}".rstrip("0").rstrip(".")
    hexc = lambda c: "#%02X%02X%02X" % c
    stops = lambda name: "".join(f'<stop offset="{f(p)}" stop-color="{hexc(c)}"/>' for p, c in name)
    pts = " ".join(f"{f(x)},{f(y)}" for q in quads for x, y in q)
    left, right = quads
    path = (f"M{f(rects[0][0])},{f(rects[0][1])} H{f(rects[0][2])} V{f(rects[0][3])} H{f(rects[0][0])} Z "
            f"M{f(rects[1][0])},{f(rects[1][1])} H{f(rects[1][2])} V{f(rects[1][3])} H{f(rects[1][0])} Z "
            + " ".join("M" + " L".join(f"{f(x)},{f(y)}" for x, y in q) + " Z" for q in quads))
    gx, gy = L["x1"] - L["w"] / 2, L["t"] + 2
    return "\n".join([
        '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">',
        "  <title>Milan Pro</title>",
        "  <defs>",
        f'    <linearGradient id="plate" x1="0" y1="0" x2="0" y2="1">{stops(PLATE)}</linearGradient>',
        f'    <linearGradient id="chrome" gradientUnits="userSpaceOnUse" x1="0" y1="{L["t"]}" x2="0" y2="{L["b"]}">{stops(CHROME)}</linearGradient>',
        f'    <radialGradient id="glint"><stop offset="0" stop-color="#FFFFFF"/><stop offset="0.35" stop-color="{hexc(CYAN)}" stop-opacity="0.8"/><stop offset="1" stop-color="{hexc(CYAN)}" stop-opacity="0"/></radialGradient>',
        f'    <radialGradient id="top"><stop offset="0" stop-color="#8A9AB2" stop-opacity="0.45"/><stop offset="1" stop-color="#8A9AB2" stop-opacity="0"/></radialGradient>',
        '    <pattern id="grid" width="21.33" height="21.33" patternUnits="userSpaceOnUse"><path d="M21.33 0V21.33H0" fill="none" stroke="#FFFFFF" stroke-opacity="0.07" stroke-width="0.6"/></pattern>',
        "  </defs>",
        f'  <rect x="{i}" y="{i}" width="{256 - 2 * i}" height="{256 - 2 * i}" rx="{r}" fill="url(#plate)" stroke="{hexc(EDGE)}" stroke-width="2"/>',
        f'  <rect x="{i}" y="{i}" width="{256 - 2 * i}" height="{256 - 2 * i}" rx="{r}" fill="url(#grid)"/>',
        f'  <ellipse cx="128" cy="30" rx="166" ry="100" fill="url(#top)"/>',
        f'  <g fill="none" stroke="{hexc(CYAN)}" stroke-opacity="0.5" stroke-width="1" stroke-linejoin="round">',
        "    <path d=\"M20,77 V41 L41,20 H87\"/><path d=\"M236,179 V215 L215,236 H169\"/><path d=\"M169,20 H200 L215,36\"/><path d=\"M41,236 H72 L87,220\"/>",
        "  </g>",
        f'  <circle cx="128" cy="128" r="{L["ring_r"]}" fill="none" stroke="{hexc(CYAN)}" stroke-opacity="0.35" stroke-width="{L["ring_w"] * 3}"/>',
        f'  <circle cx="128" cy="128" r="{L["ring_r"]}" fill="none" stroke="#C8F6FF" stroke-width="{L["ring_w"]}"/>',
        f'  <path d="{path}" fill="{hexc(INK)}" opacity="0.6" transform="translate(2.5 4)"/>',
        f'  <path d="{path}" fill="url(#chrome)" stroke="{hexc(INK)}" stroke-width="2.8" stroke-linejoin="miter"/>',
        f'  <polygon points="{pts}" fill="none"/>',
        f'  <circle cx="{f(gx)}" cy="{f(gy)}" r="20" fill="url(#glint)"/>',
        f'  <ellipse cx="{f(gx)}" cy="{f(gy)}" rx="40" ry="2.2" fill="#D8F6FF" opacity="0.8"/>',
        f'  <circle cx="{f(gx + 16)}" cy="{f(gy + 8)}" r="3.5" fill="{hexc(HOT)}" opacity="0.9"/>',
        "</svg>",
    ]) + "\n"


def ico(images):
    """A .ico: 32-bit BMP entries below 256 px (what every Windows component reads), PNG at 256."""
    blobs = []
    for im in images:
        n = im.width
        if n >= 256:
            buf = io.BytesIO()
            im.save(buf, "PNG", optimize=True)
            blobs.append(buf.getvalue())
            continue
        px = im.load()
        xor = bytearray()
        andm = bytearray()
        stride = ((n + 31) // 32) * 4
        for y in range(n - 1, -1, -1):  # bottom-up
            row = bytearray(stride)
            for x in range(n):
                r, g, b, a = px[x, y]
                xor += bytes((b, g, r, a))
                if a == 0:
                    row[x // 8] |= 0x80 >> (x % 8)
            andm += row
        hdr = struct.pack("<IiiHHIIiiII", 40, n, n * 2, 1, 32, 0, len(xor) + len(andm), 0, 0, 0, 0)
        blobs.append(hdr + bytes(xor) + bytes(andm))
    out = struct.pack("<HHH", 0, 1, len(images))
    off = 6 + 16 * len(images)
    for im, blob in zip(images, blobs):
        n = im.width
        out += struct.pack("<BBBBHHII", n % 256, n % 256, 0, 0, 1, 32, len(blob), off)
        off += len(blob)
    return out + b"".join(blobs)


# ----------------------------------------------------------------------------------------------- wordmark and panels
def wordmark(width, tagline=True):
    """"MILAN PRO" in extruded, bevelled chrome (Arial Black) over a cyan glow line with a flare, and the
    "NONLINEAR EDITING SYSTEM" tag in letter-spaced Agency FB. Returns RGBA scaled to `width`."""
    fs = 220
    f = font(["ariblk.ttf", "arialbd.ttf"], fs)
    sp = fs * 0.03
    probe = ImageDraw.Draw(Image.new("L", (1, 1)))
    tw = text_width(probe, "MILAN PRO", f, sp)
    pad = fs * 0.35
    W, H = round(tw + 2 * pad), round(fs * 1.75)
    mask = Image.new("L", (W, H), 0)
    d = ImageDraw.Draw(mask)
    spaced_text(d, (pad, fs * 0.08), "MILAN PRO", f, sp)
    im = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    bbox = mask.getbbox()
    base_y = bbox[3]
    # Glow line under the type, flaring at its right end.
    ly = base_y + fs * 0.11
    stamp(im, radial(round(tw * 1.1), round(fs * 0.22), 1.2, 0.8), CYAN, W / 2, ly)
    stamp(im, radial(round(tw * 1.0), max(2, round(fs * 0.035)), 0.8), (0xE0, 0xFA, 0xFF), W / 2, ly)
    flare(im, bbox[2] - fs * 0.05, ly, round(fs * 0.13), streak=7)
    shadow = ImageChops.offset(mask, 6, 10).filter(ImageFilter.GaussianBlur(10))
    im.alpha_composite(solid(INK, scaled(shadow, 0.7)))
    im.alpha_composite(chrome(mask, bevel=4, outline=4, extrude=9))
    if tagline:
        tf = font(["AGENCYB.TTF", "bahnschrift.ttf", "trebucbd.ttf"], round(fs * 0.30))
        tag = "NONLINEAR EDITING SYSTEM"
        tsp = fs * 0.07
        ttw = text_width(d, tag, tf, tsp)
        ty = ly + fs * 0.10
        tag_mask = Image.new("L", (W, H), 0)
        spaced_text(ImageDraw.Draw(tag_mask), ((W - ttw) / 2, ty), tag, tf, tsp)
        im.alpha_composite(solid(CYAN, scaled(tag_mask.filter(ImageFilter.GaussianBlur(5)), 0.6)))
        im.alpha_composite(solid((0xCF, 0xEE, 0xFA), tag_mask))
    im = im.crop(im.getbbox())
    return im.resize((width, round(im.height * width / im.width)), Image.LANCZOS)


def panel(W, H, ss, grid_step=0.09, traces=()):
    """A brushed gunmetal panel at ss x scale: gradient, horizontal brushing, top light, grid, cyan traces."""
    base = gradient(W, H, [(0, (0x30, 0x38, 0x48)), (0.5, GUN_MID), (1, GUN_BOT)])
    noise = Image.effect_noise((max(8, W // 40), H), 38).resize((W, H), Image.BILINEAR)
    base = Image.blend(base, ImageChops.soft_light(base, Image.merge("RGB", (noise, noise, noise))), 0.5)
    im = base.convert("RGBA")
    stamp(im, radial(round(W * 1.2), round(H * 0.7), 1.6, 0.4), (0x8A, 0x9A, 0xB2), W * 0.5, 0)
    grid = Image.new("RGBA", (W, H), (0, 0, 0, 0))
    g = ImageDraw.Draw(grid)
    step = min(W, H) * grid_step
    lw = max(1, round(ss * 0.5))
    x = step
    while x < W:
        g.line([(x, 0), (x, H)], fill=WHITE + (14,), width=lw)
        x += step
    y = step
    while y < H:
        g.line([(0, y), (W, y)], fill=WHITE + (14,), width=lw)
        y += step
    for tr in traces:
        pts = [(x * W, y * H) for x, y in tr]
        g.line(pts, fill=CYAN + (130,), width=max(1, round(ss * 0.9)), joint="curve")
        for x, y in (pts[0], pts[-1]):
            g.ellipse([x - ss * 1.5, y - ss * 1.5, x + ss * 1.5, y + ss * 1.5], fill=CYAN + (220,))
    im.alpha_composite(grid)
    im.alpha_composite(solid(CYAN, scaled(grid.getchannel("A").filter(ImageFilter.GaussianBlur(ss * 1.5)), 0.5)))
    return im


def finish(im, w, h, scanlines=True, frame=True):
    """Reduce a panel to 1x, add scanlines every 3rd row and a steel bevel frame; returns RGB."""
    out = im.convert("RGB").resize((w, h), Image.LANCZOS)
    d = ImageDraw.Draw(out, "RGBA")
    if scanlines:
        for y in range(0, h, 3):
            d.line([(0, y), (w, y)], fill=(0, 0, 0, 38))
    if frame:
        d.rectangle([0, 0, w - 1, h - 1], outline=(0x5A, 0x66, 0x78, 255))
        d.line([(1, 1), (w - 2, 1)], fill=(0xB4, 0xBE, 0xCA, 255))
        d.line([(1, 1), (1, h - 2)], fill=(0xB4, 0xBE, 0xCA, 255))
        d.line([(1, h - 2), (w - 2, h - 2)], fill=(0x14, 0x18, 0x20, 255))
        d.line([(w - 2, 1), (w - 2, h - 2)], fill=(0x14, 0x18, 0x20, 255))
    return out


def place_mark(im, size, x, y, ss):
    """The mark at `size` px with a soft drop shadow onto the panel."""
    mark = render_mark(1024).resize((size, size), Image.LANCZOS)
    layer = Image.new("L", im.size, 0)
    layer.paste(mark.getchannel("A"), (x + ss * 3, y + ss * 6))
    im.alpha_composite(solid(INK, scaled(layer.filter(ImageFilter.GaussianBlur(ss * 6)), 0.65)))
    im.alpha_composite(mark, (x, y))


def horizon(im, y, ss, x0=0.0, x1=1.0):
    """A thin cyan light streak across the panel with a flare at its right end."""
    W = im.width
    cx, span = W * (x0 + x1) / 2, W * (x1 - x0)
    stamp(im, radial(round(span), round(ss * 10), 1.3, 0.7), CYAN, cx, y)
    stamp(im, radial(round(span * 0.9), max(2, round(ss * 1.2)), 0.8), (0xE0, 0xFA, 0xFF), cx, y)
    flare(im, W * x1 - span * 0.12, y, round(ss * 9), streak=6)


def timeline_strip(im, x0, y, x1, h, ss):
    """A groove with three steel-blue clip blocks and a playhead: the NLE nod."""
    d = ImageDraw.Draw(im)
    d.rounded_rectangle([x0, y, x1, y + h], radius=h * 0.25, fill=(0x0A, 0x0D, 0x14, 255), outline=(0x3A, 0x44, 0x52, 255), width=ss)
    span = x1 - x0
    for a, b, c in ((0.02, 0.36, (0x2A, 0x7F, 0xFF)), (0.38, 0.62, EDGE), (0.64, 0.82, (0x1F, 0x5E, 0x9A))):
        d.rounded_rectangle([x0 + span * a, y + ss * 2, x0 + span * b, y + h - ss * 2], radius=h * 0.15, fill=c + (255,))
        d.line([(x0 + span * a + ss, y + ss * 3), (x0 + span * b - ss, y + ss * 3)], fill=(0xDC, 0xF0, 0xFF, 160), width=ss)
    px = x0 + span * 0.62
    layer = Image.new("RGBA", im.size, (0, 0, 0, 0))
    ImageDraw.Draw(layer).line([(px, y - h * 0.4), (px, y + h * 1.4)], fill=HOT + (255,), width=max(1, ss))
    glow = layer.getchannel("A").filter(ImageFilter.GaussianBlur(ss * 2))
    im.alpha_composite(solid(HOT, glow))
    im.alpha_composite(layer)


def splash(w=520, h=320, ss=3):
    W, H = w * ss, h * ss
    traces = [[(0.03, 0.55), (0.03, 0.25), (0.10, 0.12), (0.30, 0.12)], [(0.97, 0.45), (0.97, 0.75), (0.90, 0.88), (0.70, 0.88)],
              [(0.55, 0.06), (0.62, 0.06), (0.65, 0.12)], [(0.40, 0.95), (0.52, 0.95), (0.56, 0.86)]]
    im = panel(W, H, ss, traces=traces)
    m = round(h * 0.56) * ss
    place_mark(im, m, round(w * 0.065) * ss, round(h * 0.14) * ss, ss)
    wm = wordmark(round(w * 0.56) * ss)
    im.alpha_composite(wm, (round(w * 0.39) * ss, round(h * 0.26) * ss))
    horizon(im, round(h * 0.78) * ss, ss, 0.03, 0.97)
    timeline_strip(im, round(w * 0.065) * ss, round(h * 0.86) * ss, round(w * 0.935) * ss, round(h * 0.055) * ss, ss)
    return finish(im, w, h)


def about_banner(w=480, h=120, ss=3):
    W, H = w * ss, h * ss
    traces = [[(0.02, 0.80), (0.02, 0.30), (0.06, 0.10), (0.20, 0.10)], [(0.98, 0.20), (0.98, 0.70), (0.94, 0.90), (0.80, 0.90)]]
    im = panel(W, H, ss, grid_step=0.2, traces=traces)
    place_mark(im, round(h * 0.8) * ss, round(h * 0.1) * ss, round(h * 0.1) * ss, ss)
    wm = wordmark(round(w * 0.64) * ss)
    im.alpha_composite(wm, (round(w * 0.26) * ss, round((H - wm.height) / 2)))
    return finish(im, w, h)


def readme_banner(w=960, h=260, ss=2):
    W, H = w * ss, h * ss
    traces = [[(0.02, 0.60), (0.02, 0.25), (0.06, 0.10), (0.22, 0.10)], [(0.98, 0.40), (0.98, 0.75), (0.94, 0.90), (0.78, 0.90)],
              [(0.30, 0.95), (0.45, 0.95), (0.48, 0.86)], [(0.60, 0.05), (0.72, 0.05), (0.75, 0.14)]]
    im = panel(W, H, ss, grid_step=0.12, traces=traces)
    place_mark(im, round(h * 0.76) * ss, round(w * 0.07) * ss, round(h * 0.12) * ss, ss)
    wm = wordmark(round(w * 0.6) * ss)
    im.alpha_composite(wm, (round(w * 0.31) * ss, round((H - wm.height) / 2)))
    return finish(im, w, h)


def wizard_image(w, h, ss=4):
    """Left panel of the Welcome/Finished pages: the mark, the chrome wordmark and a timeline strip on gunmetal."""
    W, H = w * ss, h * ss
    traces = [[(0.06, 0.62), (0.06, 0.52), (0.12, 0.47), (0.40, 0.47)], [(0.94, 0.66), (0.94, 0.76), (0.88, 0.80), (0.60, 0.80)],
              [(0.10, 0.03), (0.10, 0.08), (0.16, 0.12)], [(0.90, 0.97), (0.90, 0.92), (0.84, 0.88)]]
    im = panel(W, H, ss, grid_step=0.12, traces=traces)
    m = round(w * 0.58) * ss
    mx, my = round((W - m) / 2), round(h * 0.09) * ss
    place_mark(im, m, mx, my, ss)
    wm = wordmark(round(w * 0.84) * ss)
    im.alpha_composite(wm, (round((W - wm.width) / 2), my + m + round(h * 0.04) * ss))
    horizon(im, round(h * 0.66) * ss, ss, 0.0, 1.0)
    timeline_strip(im, round(w * 0.08) * ss, round(h * 0.90) * ss, round(w * 0.92) * ss, round(h * 0.03) * ss, ss)
    return finish(im, w, h, frame=False)


def preview(path, marks):
    """A check sheet: every .ico size at 1x and 4x on light and dark, the 512, the wordmark, splash, banners,
    the wizard side image and the small wizard mark."""
    sheet = Image.new("RGB", (1760, 1560), (0xF3, 0xF3, 0xF3))
    sheet.paste((0x20, 0x20, 0x20), (0, 300, 1760, 600))
    for y in (16, 316):
        x = 20
        for im in marks:
            if im.width > 64:
                continue
            sheet.paste(im, (x, y), im)
            big = im.resize((im.width * 4, im.width * 4), Image.NEAREST)
            sheet.paste(big, (x, y + 70), big)
            x += im.width * 4 + 20
        sheet.paste(marks[-1], (x, y + 10), marks[-1])
        x += 276
        m128 = marks[-2]
        sheet.paste(m128, (x, y + 10), m128)
    sheet.paste((0x10, 0x12, 0x18), (0, 600, 1760, 1560))
    big = render_mark(512)
    sheet.paste(big, (20, 620), big)
    wm = wordmark(600)
    sheet.paste(wm, (560, 640), wm)
    sheet.paste(splash(), (560, 640 + wm.height + 30))
    wiz = wizard_image(202, 386)
    sheet.paste(wiz, (1200, 620))
    small = render_mark(58)
    sheet.paste(small, (1430, 620), small)
    sheet.paste(about_banner(), (1200, 1030))
    sheet.paste(readme_banner(), (20, 1240))
    sheet.save(path)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "installer").mkdir(exist_ok=True)
    (OUT / "milan-pro.svg").write_text(svg(), encoding="utf-8", newline="\n")
    marks = [render_mark(n) for n in ICO_SIZES]
    (OUT / "milan-pro.ico").write_bytes(ico(marks))
    marks[-1].save(OUT / "milan-pro-256.png", optimize=True)
    render_mark(512).save(OUT / "milan-pro-512.png", optimize=True)
    wordmark(800).save(OUT / "wordmark-800.png", optimize=True)
    splash().save(OUT / "splash-520x320.png", optimize=True)
    about_banner().save(OUT / "about-banner-480x120.png", optimize=True)
    readme_banner().save(OUT / "readme-banner-960x260.png", optimize=True)
    # Inno Setup 6.7 image areas at 100/125/150/175/200% DPI (help: WizardImageFile, WizardSmallImageFile).
    for w, h in ((202, 386), (269, 515), (336, 643), (403, 772), (430, 824)):
        wizard_image(w, h).save(OUT / "installer" / f"wizard-{w}x{h}.png", optimize=True)
    for n in (58, 77, 97, 116, 124):
        render_mark(n).save(OUT / "installer" / f"wizard-small-{n}.png", optimize=True)
    if len(sys.argv) > 1:
        preview(sys.argv[1], marks)
    # Self-check: the .ico reads back with every size, and the 16 px stems are solid, opaque steel on whole pixels.
    with Image.open(OUT / "milan-pro.ico") as back:
        assert sorted(s[0] for s in back.info["sizes"]) == ICO_SIZES, back.info["sizes"]
    m16 = marks[0].load()
    for x in (3, 4, 11, 12):
        for y in range(4, 12):
            r, g, b, a = m16[x, y]
            assert a == 255 and min(r, g, b) >= 0x80 and max(r, g, b) - min(r, g, b) <= 0x30, ("16 px stems are blurry", x, y, m16[x, y])


if __name__ == "__main__":
    main()
