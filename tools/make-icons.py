"""Milan Pro brand assets: the SVG master, the .ico, the PNGs and the installer wizard images.

    python tools/make-icons.py [preview.png]

Needs Pillow. Run from anywhere; writes into assets/brand/. The mark is the in-app Icon::Logo (src/ui/icons.rs):
a white "M" monogram on a navy tile with a blue ring, plus (64 px and up) a small timeline bar with a playhead.
Sizes up to 48 px come from hand-placed pixel grids (LAYOUTS) so stems and edges land on whole pixels.
"""
import io
import math
import struct
import sys
from pathlib import Path

from PIL import Image, ImageDraw, ImageFont

ROOT = Path(__file__).resolve().parent.parent
OUT = ROOT / "assets" / "brand"

# The in-app logo colours (theme.rs logo_bg / logo_border / logo_text), the tile shaded lighter at the top.
NAVY_TOP, NAVY_BOTTOM = (0x0B, 0x2E, 0x8A), (0x00, 0x17, 0x51)
RING = (0x25, 0x8E, 0xE4)
WHITE = (0xFF, 0xFF, 0xFF)
TRACK, CLIP = (0x12, 0x3F, 0x8F), (0x25, 0x8E, 0xE4)

# Master design on a 256 grid: tile inset/radius/ring, the M (x0..x1 outer stems, t..b, stem w, V tip at v) and the
# timeline bar (y0..y1, clip end, playhead x). The SVG is written from this.
DESIGN = dict(inset=8, r=48, ring=8, x0=60, x1=196, t=50, b=164, w=30, v=150,
              bar=(184, 198), clip=128, head=150)

# Hand-hinted small sizes: whole-pixel stems, tile and ring; no timeline bar.
LAYOUTS = {
    16: dict(inset=0, r=3, ring=1, x0=3, x1=13, t=3, b=13, w=2, v=12),
    20: dict(inset=0, r=4, ring=1, x0=4, x1=16, t=4, b=16, w=3, v=15),
    24: dict(inset=0, r=5, ring=1, x0=5, x1=19, t=5, b=19, w=3, v=17),
    32: dict(inset=1, r=6, ring=2, x0=8, x1=24, t=8, b=24, w=4, v=22),
    40: dict(inset=1, r=8, ring=2, x0=10, x1=30, t=10, b=30, w=5, v=27),
    48: dict(inset=2, r=9, ring=2, x0=12, x1=36, t=12, b=36, w=6, v=33),
}
ICO_SIZES = [16, 20, 24, 32, 40, 48, 64, 128, 256]


def layout(n):
    if n in LAYOUTS:
        return LAYOUTS[n]
    k = n / 256
    d = {key: DESIGN[key] * k for key in ("inset", "r", "ring", "t", "b", "v", "clip", "head")}
    d["x0"], d["w"] = round(DESIGN["x0"] * k), round(DESIGN["w"] * k)  # stems on whole pixels
    d["x1"] = n - d["x0"]
    d["bar"] = tuple(y * k for y in DESIGN["bar"])
    return d


def m_shapes(L):
    """The M as rectangles (stems) and quads (diagonals meeting in a point at v), in pixels. Diagonals are 0.9x
    the stem weight."""
    x0, x1, t, b, w, v = (L[k] for k in ("x0", "x1", "t", "b", "w", "v"))
    cx = (x0 + x1) / 2
    dx, dy = cx - x0, v - t
    hw = 0.9 * w * math.hypot(dx, dy) / dy  # horizontal width for that perpendicular weight
    k = hw * dy / dx  # the notch above the tip
    rects = [(x0, t, x0 + w, b), (x1 - w, t, x1, b)]
    quads = [[(x0, t), (x0 + hw, t), (cx, v - k), (cx, v)],
             [(x1, t), (x1 - hw, t), (cx, v - k), (cx, v)]]
    return rects, quads


def lerp(a, b, f):
    return tuple(round(a[i] + (b[i] - a[i]) * f) for i in range(3))


def vgradient(w, h, top, bottom):
    g = Image.new("RGB", (1, h))
    for y in range(h):
        g.putpixel((0, y), lerp(top, bottom, y / max(1, h - 1)))
    return g.resize((w, h))


def render_mark(n, ss=16):
    """The mark at n x n px: drawn ss times larger, premultiplied, then box-averaged (exact pixel coverage)."""
    L = layout(n)
    S = n * ss
    im = Image.new("RGBa", (S, S), (0, 0, 0, 0))
    d = ImageDraw.Draw(im)

    def rr(x0, y0, x1, y1, r, fill, draw=d):
        draw.rounded_rectangle([x0 * ss, y0 * ss, x1 * ss - 1, y1 * ss - 1], radius=r * ss, fill=fill)

    i, ring, r = L["inset"], L["ring"], L["r"]
    rr(i, i, n - i, n - i, r, RING + (255,))
    mask = Image.new("L", (S, S), 0)
    rr(i + ring, i + ring, n - i - ring, n - i - ring, max(0, r - ring), 255, ImageDraw.Draw(mask))
    im.paste(vgradient(S, S, NAVY_TOP, NAVY_BOTTOM).convert("RGBa"), (0, 0), mask)
    rects, quads = m_shapes(L)
    for x0, y0, x1, y1 in rects:
        d.rectangle([x0 * ss, y0 * ss, x1 * ss - 1, y1 * ss - 1], fill=WHITE + (255,))
    for q in quads:
        d.polygon([(x * ss, y * ss) for x, y in q], fill=WHITE + (255,))
    if "bar" in L:
        y0, y1 = L["bar"]
        h = y1 - y0
        rr(L["x0"], y0, L["x1"], y1, h / 2, TRACK + (255,))
        rr(L["x0"], y0, L["clip"], y1, h / 2, CLIP + (255,))
        hx, pw = L["head"], max(1.0, h * 0.3)
        d.rectangle([(hx - pw / 2) * ss, (y0 - h * 0.45) * ss, (hx + pw / 2) * ss, (y1 + h * 0.45) * ss], fill=WHITE + (255,))
        d.polygon([((hx - h * 0.55) * ss, (y0 - h * 0.85) * ss), ((hx + h * 0.55) * ss, (y0 - h * 0.85) * ss),
                   (hx * ss, (y0 - h * 0.1) * ss)], fill=WHITE + (255,))
    return im.reduce(ss).convert("RGBA")


def svg():
    L = DESIGN
    i, r, ring = L["inset"], L["r"], L["ring"]
    rects, quads = m_shapes(L)
    y0, y1 = L["bar"]
    h = y1 - y0
    hx, pw = L["head"], h * 0.3
    f = lambda v: f"{v:.2f}".rstrip("0").rstrip(".")
    hexc = lambda c: "#%02X%02X%02X" % c
    out = [
        '<svg xmlns="http://www.w3.org/2000/svg" width="256" height="256" viewBox="0 0 256 256">',
        "  <title>Milan Pro</title>",
        f'  <defs><linearGradient id="tile" gradientUnits="userSpaceOnUse" x1="0" y1="0" x2="0" y2="256"><stop offset="0" stop-color="{hexc(NAVY_TOP)}"/>'
        f'<stop offset="1" stop-color="{hexc(NAVY_BOTTOM)}"/></linearGradient></defs>',
        f'  <rect x="{i}" y="{i}" width="{256 - 2 * i}" height="{256 - 2 * i}" rx="{r}" fill="{hexc(RING)}"/>',
        f'  <rect x="{i + ring}" y="{i + ring}" width="{256 - 2 * (i + ring)}" height="{256 - 2 * (i + ring)}" rx="{r - ring}" fill="url(#tile)"/>',
        '  <g fill="#FFFFFF">',
    ]
    out += [f'    <rect x="{a}" y="{b}" width="{c - a}" height="{e - b}"/>' for a, b, c, e in rects]
    out += ['    <polygon points="' + " ".join(f"{f(x)},{f(y)}" for x, y in q) + '"/>' for q in quads]
    out += [
        "  </g>",
        f'  <rect x="{L["x0"]}" y="{y0}" width="{L["x1"] - L["x0"]}" height="{h}" rx="{f(h / 2)}" fill="{hexc(TRACK)}"/>',
        f'  <rect x="{L["x0"]}" y="{y0}" width="{L["clip"] - L["x0"]}" height="{h}" rx="{f(h / 2)}" fill="{hexc(CLIP)}"/>',
        f'  <rect x="{f(hx - pw / 2)}" y="{f(y0 - h * 0.45)}" width="{f(pw)}" height="{f(h * 1.9)}" fill="#FFFFFF"/>',
        f'  <polygon points="{f(hx - h * 0.55)},{f(y0 - h * 0.85)} {f(hx + h * 0.55)},{f(y0 - h * 0.85)} {f(hx)},{f(y0 - h * 0.1)}" fill="#FFFFFF"/>',
        "</svg>",
    ]
    return "\n".join(out) + "\n"


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


def font(size, bold=True):
    for name in (["seguisb.ttf", "segoeuib.ttf"] if bold else ["segoeui.ttf"]):
        try:
            return ImageFont.truetype(f"C:/Windows/Fonts/{name}", size)
        except OSError:
            pass
    return ImageFont.load_default(size)


def wizard_image(w, h, ss=4):
    """Left panel of the Welcome/Finished pages: the VEGAS 23-style dark look with the mark, name and a timeline."""
    W, H = w * ss, h * ss
    im = vgradient(W, H, (0x10, 0x24, 0x52), (0x14, 0x14, 0x16)).convert("RGBA")
    d = ImageDraw.Draw(im)
    # Timeline motif in the lower part: three tracks of clips, a ruler line and a playhead.
    top, th, gap = H * 0.70, H * 0.045, H * 0.012
    d.line([(0, top - gap * 2), (W, top - gap * 2)], fill=(0x2C, 0x3A, 0x55), width=max(1, ss))
    clips = [[(0.00, 0.42, (0x2B, 0x5F, 0xB8)), (0.45, 0.95, (0x25, 0x8E, 0xE4)), (0.98, 1.4, (0x2B, 0x5F, 0xB8))],
             [(0.10, 0.62, (0x1F, 0x6E, 0x7A)), (0.66, 1.4, (0x1F, 0x6E, 0x7A))],
             [(-0.1, 0.30, (0x3A, 0x45, 0x63)), (0.34, 0.80, (0x3A, 0x45, 0x63))]]
    for k, row in enumerate(clips):
        y = top + k * (th + gap)
        for a, b, c in row:
            d.rounded_rectangle([a * W, y, b * W - ss, y + th], radius=th * 0.18, fill=c + (255,))
    px = W * 0.56
    d.rectangle([px - ss, top - gap * 4, px + ss, top + 3 * (th + gap)], fill=WHITE + (255,))
    d.polygon([(px - th * 0.4, top - gap * 4), (px + th * 0.4, top - gap * 4), (px, top - gap * 4 + th * 0.5)],
              fill=WHITE + (255,))
    # The mark, then the name.
    m = round(w * 0.46)
    mark = render_mark(m, 8).resize((m * ss, m * ss), Image.LANCZOS)
    im.alpha_composite(mark, (round((W - m * ss) / 2), round(H * 0.17)))
    ty = H * 0.17 + m * ss + H * 0.05
    for text, size, colour, bold in (("Milan Pro", 0.15, WHITE, True), ("VIDEO EDITOR", 0.055, (0x8F, 0xB8, 0xE8), False)):
        f = font(round(w * size * ss), bold)
        tw = d.textlength(text, font=f)
        if not bold:  # letter-spaced caption
            sp = w * 0.012 * ss
            x = (W - tw - sp * (len(text) - 1)) / 2
            for ch in text:
                d.text((x, ty), ch, font=f, fill=colour + (255,))
                x += d.textlength(ch, font=f) + sp
        else:
            d.text(((W - tw) / 2, ty), text, font=f, fill=colour + (255,))
        ty += f.size * 1.35
    return im.convert("RGB").resize((w, h), Image.LANCZOS)


def preview(path, marks):
    """A check sheet: every .ico size at 1x and 4x on a light and a dark taskbar colour."""
    sheet = Image.new("RGB", (1400, 560), (0xF3, 0xF3, 0xF3))
    sheet.paste((0x20, 0x20, 0x20), (0, 280, 1400, 560))
    for row, y in ((0, 20), (1, 300)):
        x = 20
        for im in marks:
            if im.width > 64:
                continue
            sheet.paste(im, (x, y), im)
            big = im.resize((im.width * 4, im.width * 4), Image.NEAREST)
            sheet.paste(big, (x, y + 60), big)
            x += im.width * 4 + 24
        big = marks[-1]
        sheet.paste(big, (x, y), big)
    sheet.save(path)


def main():
    OUT.mkdir(parents=True, exist_ok=True)
    (OUT / "installer").mkdir(exist_ok=True)
    (OUT / "milan-pro.svg").write_text(svg(), encoding="utf-8", newline="\n")
    marks = [render_mark(n) for n in ICO_SIZES]
    (OUT / "milan-pro.ico").write_bytes(ico(marks))
    marks[-1].save(OUT / "milan-pro-256.png", optimize=True)
    render_mark(512, 8).save(OUT / "milan-pro-512.png", optimize=True)
    # Inno Setup 6.7 image areas at 100/125/150/175/200% DPI (help: WizardImageFile, WizardSmallImageFile).
    for w, h in ((202, 386), (269, 515), (336, 643), (403, 772), (430, 824)):
        wizard_image(w, h).save(OUT / "installer" / f"wizard-{w}x{h}.png", optimize=True)
    for n in (58, 77, 97, 116, 124):
        render_mark(n, 8).save(OUT / "installer" / f"wizard-small-{n}.png", optimize=True)
    if len(sys.argv) > 1:
        preview(sys.argv[1], marks)
    # Self-check: the .ico reads back with every size, and the 16 px stems are solid white on whole pixels.
    with Image.open(OUT / "milan-pro.ico") as back:
        assert sorted(s[0] for s in back.info["sizes"]) == ICO_SIZES, back.info["sizes"]
    m16 = marks[0].load()
    assert all(m16[x, y][:3] == WHITE for x in (3, 4, 11, 12) for y in range(4, 12)), "16 px stems are blurry"


if __name__ == "__main__":
    main()
