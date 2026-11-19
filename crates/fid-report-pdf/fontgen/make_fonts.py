#!/usr/bin/env python3
"""Generates the bundled report fonts of RPT.OUTPUT.PDF.TEXT and two test fonts.

The fonts are original work of the project (no third-party outlines, no font tools): each
glyph is a hand-written stroke skeleton (lines, elliptic arcs, dots) in this file, stroked
into closed polygons and written as a TrueType (glyf) font with the Python standard library.
Licence: the project licence, MIT OR Apache-2.0 (both on deny.toml's allowlist and on the
asset allowlist of house-rules/spdx-allowlist.toml).

Outputs (byte-stable: no timestamps, no platform maths; sin/cos are a fixed Taylor series
in IEEE-754 double arithmetic, so every platform gives the same bytes):

  crates/fid-report-pdf/assets/fonts/MavenReportSans-Regular.ttf
  crates/fid-report-pdf/assets/fonts/MavenReportSans-Bold.ttf
  crates/fid-report-pdf/assets/fonts/MavenReportMono-Regular.ttf
  crates/fid-report-pdf/assets/fonts/MavenReportMono-Bold.ttf
  fixtures/rpt/output/text/script-test.ttf     (stand-in I18N script font: 3 CJK glyphs,
                                                cmap formats 4 and 12)
  fixtures/rpt/output/text/composite-test.ttf  (one composite glyph, for subsetting)

Usage: python make_fonts.py            write the files
       python make_fonts.py --check    fail if any file differs from what would be written
"""

import hashlib
import math
import pathlib
import struct
import sys

ROOT = pathlib.Path(__file__).resolve().parents[3]
ASSETS = ROOT / "crates" / "fid-report-pdf" / "assets" / "fonts"
FIXT = ROOT / "fixtures" / "rpt" / "output" / "text"

UPEM = 1000
PI = 3.141592653589793


# ---------------------------------------------------------------------------------------
# Deterministic trigonometry (degrees). Only +, -, *, / on doubles: the same on every
# platform, unlike math.sin/math.cos which come from the C library.


def _reduce(deg):
    d = deg % 360.0
    if d > 180.0:
        d -= 360.0
    return d * PI / 180.0


def cosd(deg):
    x = _reduce(deg)
    x2 = x * x
    term = 1.0
    s = 1.0
    for k in range(1, 16):
        term = -term * x2 / ((2 * k - 1) * (2 * k))
        s += term
    return s


def sind(deg):
    x = _reduce(deg)
    x2 = x * x
    term = x
    s = x
    for k in range(1, 16):
        term = -term * x2 / ((2 * k) * (2 * k + 1))
        s += term
    return s


def rnd(v):
    # round half to even on a double: deterministic
    return int(round(v))


# ---------------------------------------------------------------------------------------
# Stroke primitives. A glyph is a list of primitives in font units (y up, baseline 0):
#   ("L", [(x, y), ...], hs)               polyline
#   ("A", cx, cy, rx, ry, a0, a1, hs)      elliptic arc from angle a0 to a1 (degrees)
#   ("E", cx, cy, rx, ry, hs)              full ellipse ring
#   ("D", x, y, r, hs)                     filled octagon dot (r None: from the stroke)
# hs scales the stroke half-width (superscripts use a thinner stroke).


def L(*pts):
    return ("L", list(pts), 1.0)


def A(cx, cy, rx, ry, a0, a1):
    return ("A", cx, cy, rx, ry, a0, a1, 1.0)


def E(cx, cy, rx, ry):
    return ("E", cx, cy, rx, ry, 1.0)


def D(x, y, r=None):
    return ("D", x, y, r, 1.0)


def apt(cx, cy, rx, ry, a):
    """A point on an arc's centre line."""
    return (cx + rx * cosd(a), cy + ry * sind(a))


def transform(prims, sx, sy, dx, dy, hs=1.0):
    """Scales by (sx, sy) about the origin, then shifts by (dx, dy)."""
    out = []
    for p in prims:
        k = p[0]
        if k == "L":
            out.append(("L", [(x * sx + dx, y * sy + dy) for (x, y) in p[1]], p[2] * hs))
        elif k == "D":
            r = p[3] * abs(sx) if p[3] is not None else None
            out.append(("D", p[1] * sx + dx, p[2] * sy + dy, r, p[4] * hs))
        else:
            cx, cy, rx, ry = p[1], p[2], p[3], p[4]
            ncx, ncy, nrx, nry = cx * sx + dx, cy * sy + dy, rx * abs(sx), ry * abs(sy)
            if k == "E":
                out.append(("E", ncx, ncy, nrx, nry, p[5] * hs))
            else:
                a0, a1 = p[5], p[6]
                if sx < 0 and sy < 0:
                    a0, a1 = a0 + 180, a1 + 180
                elif sx < 0:
                    a0, a1 = 180 - a0, 180 - a1
                elif sy < 0:
                    a0, a1 = -a0, -a1
                out.append(("A", ncx, ncy, nrx, nry, a0, a1, p[7] * hs))
    return out


def extent(prims):
    xs = []
    for p in prims:
        if p[0] == "L":
            xs += [x for (x, _) in p[1]]
        elif p[0] == "D":
            xs.append(p[1])
        else:
            xs += [p[1] - p[3], p[1] + p[3]]
    return (min(xs), max(xs)) if xs else (0, 0)


# ---------------------------------------------------------------------------------------
# Stroking into closed polygons.


def signed_area(pts):
    a = 0
    n = len(pts)
    for i in range(n):
        x0, y0 = pts[i]
        x1, y1 = pts[(i + 1) % n]
        a += x0 * y1 - x1 * y0
    return a


def clockwise(pts):
    return pts if signed_area(pts) < 0 else pts[::-1]


def counter_clockwise(pts):
    return pts if signed_area(pts) > 0 else pts[::-1]


def dedup(pts):
    out = []
    for p in pts:
        if not out or out[-1] != p:
            out.append(p)
    while len(out) > 1 and out[0] == out[-1]:
        out.pop()
    return out


def octagon(x, y, r):
    return [(rnd(x + r * cosd(22.5 + 45 * k)), rnd(y + r * sind(22.5 + 45 * k))) for k in range(8)]


def segment_rect(a, b, h):
    dx, dy = b[0] - a[0], b[1] - a[1]
    n = math.sqrt(dx * dx + dy * dy)
    if n == 0:
        return None
    nx, ny = -dy / n * h, dx / n * h
    return [
        (rnd(a[0] + nx), rnd(a[1] + ny)),
        (rnd(b[0] + nx), rnd(b[1] + ny)),
        (rnd(b[0] - nx), rnd(b[1] - ny)),
        (rnd(a[0] - nx), rnd(a[1] - ny)),
    ]


def ring_points(cx, cy, rx, ry, a0, a1, n):
    return [(rnd(cx + rx * cosd(a0 + (a1 - a0) * i / n)), rnd(cy + ry * sind(a0 + (a1 - a0) * i / n))) for i in range(n + 1)]


def stroke(prims, h0):
    contours = []
    for p in prims:
        k = p[0]
        if k == "L":
            h = h0 * p[2]
            pts = p[1]
            for a, b in zip(pts, pts[1:]):
                r = segment_rect(a, b, h)
                if r:
                    contours.append(clockwise(dedup(r)))
            for q in pts[1:-1]:
                contours.append(clockwise(octagon(q[0], q[1], h)))
        elif k == "D":
            h = h0 * p[4]
            r = p[3] if p[3] is not None else 1.35 * h
            contours.append(clockwise(octagon(p[1], p[2], r)))
        elif k == "E":
            _, cx, cy, rx, ry, hs = p
            h = h0 * hs
            outer = ring_points(cx, cy, rx + h, ry + h, 0, 360, 36)[:-1]
            irx, iry = max(rx - h, 1), max(ry - h, 1)
            inner = ring_points(cx, cy, irx, iry, 0, 360, 36)[:-1]
            contours.append(clockwise(dedup(outer)))
            contours.append(counter_clockwise(dedup(inner)))
        else:
            _, cx, cy, rx, ry, a0, a1, hs = p
            h = h0 * hs
            sweep = abs(a1 - a0)
            n = max(2, int((sweep + 9.999) // 10))
            outer = ring_points(cx, cy, rx + h, ry + h, a0, a1, n)
            inner = ring_points(cx, cy, max(rx - h, 1), max(ry - h, 1), a0, a1, n)
            contours.append(clockwise(dedup(outer + inner[::-1])))
    return [c for c in contours if len(c) >= 3]


# ---------------------------------------------------------------------------------------
# Glyph skeletons (proportional sans). Each entry: code point -> (advance, fn(h) -> prims).

G = {}


def glyph(cp, width):
    def deco(fn):
        G[cp] = (width, fn)
        return fn

    return deco


def simple(cp, width, fn):
    G[cp] = (width, fn)


T7 = 700  # cap height
X5 = 500  # x-height


def S_shape(cx, w, top, h):
    """An S between baseline and `top`, centred on cx, half-width w."""
    m = top / 2.0
    ry = m / 2.0 - h
    return [A(cx, top - m / 2.0, w, ry, 25, 270), A(cx, m / 2.0, w, ry, 90, -155)]


# Uppercase
simple(0x41, 640, lambda h: [L((40, 0), (320, 700), (600, 0)), L((140, 250), (500, 250))])
simple(0x42, 600, lambda h: [
    L((90, 0), (90, 700)),
    L((90, 700 - h), (330, 700 - h)), A(330, (700 - h + 360) / 2, 170, (700 - h - 360) / 2, 90, -90), L((90, 360), (330, 360)),
    L((90, h), (350, h)), A(350, (360 + h) / 2, 190, (360 - h) / 2, 90, -90)])
simple(0x43, 640, lambda h: [A(360, 350, 270, 350 - h, 45, 315)])
simple(0x44, 650, lambda h: [L((90, 0), (90, 700)), L((90, 700 - h), (300, 700 - h)), L((90, h), (300, h)), A(300, 350, 260, 350 - h, 90, -90)])
simple(0x45, 560, lambda h: [L((500, 700 - h), (90, 700 - h), (90, h), (500, h)), L((90, 350), (460, 350))])
simple(0x46, 540, lambda h: [L((500, 700 - h), (90, 700 - h), (90, 0)), L((90, 370), (440, 370))])
simple(0x47, 680, lambda h: [A(370, 350, 270, 350 - h, 40, 325), L((400, 320), (620, 320), (620, 60))])
simple(0x48, 680, lambda h: [L((90, 0), (90, 700)), L((590, 0), (590, 700)), L((90, 360), (590, 360))])
simple(0x49, 240, lambda h: [L((120, 0), (120, 700))])
simple(0x4A, 500, lambda h: [L((410, 700), (410, 220)), A(240, 220, 170, 220 - h, 0, -180)])
simple(0x4B, 620, lambda h: [L((90, 0), (90, 700)), L((560, 700), (90, 260)), L((250, 410), (580, 0))])
simple(0x4C, 520, lambda h: [L((90, 700), (90, h), (490, h))])
simple(0x4D, 780, lambda h: [L((90, 0), (90, 700), (390, 150), (690, 700), (690, 0))])
simple(0x4E, 680, lambda h: [L((90, 0), (90, 700), (590, 0), (590, 700))])
simple(0x4F, 720, lambda h: [E(360, 350, 270, 350 - h)])


def _P(h):
    return [L((90, 0), (90, 700)), L((90, 700 - h), (340, 700 - h)),
            A(340, (700 - h + 330) / 2, 190, (700 - h - 330) / 2, 90, -90), L((340, 330), (90, 330))]


simple(0x50, 600, _P)
simple(0x51, 720, lambda h: [E(360, 350, 270, 350 - h), L((400, 170), (640, -40))])
simple(0x52, 620, lambda h: _P(h) + [L((300, 330), (560, 0))])
simple(0x53, 600, lambda h: S_shape(300, 210, 700, h))
simple(0x54, 600, lambda h: [L((40, 700 - h), (560, 700 - h)), L((300, 700), (300, 0))])
simple(0x55, 680, lambda h: [L((90, 700), (90, 250)), A(340, 250, 250, 250 - h, 180, 360), L((590, 250), (590, 700))])
simple(0x56, 640, lambda h: [L((40, 700), (320, 0), (600, 700))])
simple(0x57, 900, lambda h: [L((40, 700), (240, 0), (450, 600), (660, 0), (860, 700))])
simple(0x58, 620, lambda h: [L((50, 700), (570, 0)), L((570, 700), (50, 0))])
simple(0x59, 620, lambda h: [L((40, 700), (310, 350), (580, 700)), L((310, 350), (310, 0))])
simple(0x5A, 600, lambda h: [L((70, 700 - h), (530, 700 - h), (70, h), (530, h))])

# Lowercase
simple(0x61, 560, lambda h: [E(270, 250, 190, 250 - h), L((460, 500), (460, 0))])
simple(0x62, 580, lambda h: [L((90, 760), (90, 0)), E(310, 250, 200, 250 - h)])
simple(0x63, 520, lambda h: [A(290, 250, 210, 250 - h, 40, 320)])
simple(0x64, 580, lambda h: [E(270, 250, 200, 250 - h), L((490, 760), (490, 0))])
simple(0x65, 560, lambda h: [A(285, 250, 210, 250 - h, 0, 320), L((75, 250), (495, 250))])
simple(0x66, 340, lambda h: [L((130, 0), (130, 560)), A(250, 560, 120, 200 - h, 180, 40), L((40, 480), (300, 480))])
simple(0x67, 580, lambda h: [E(270, 250, 190, 250 - h), L((460, 500), (460, 0)), A(270, 0, 190, 180, 0, -165)])
simple(0x68, 580, lambda h: [L((90, 760), (90, 0)), A(290, 300, 200, 200 - h, 180, 0), L((490, 300), (490, 0))])
simple(0x69, 220, lambda h: [L((110, 0), (110, 500)), D(110, 680)])
simple(0x6A, 240, lambda h: [L((150, 500), (150, -100), (60, -190)), D(150, 680)])
simple(0x6B, 540, lambda h: [L((90, 760), (90, 0)), L((470, 500), (90, 180)), L((220, 290), (500, 0))])
simple(0x6C, 220, lambda h: [L((110, 0), (110, 760))])
simple(0x6D, 860, lambda h: [
    L((90, 500), (90, 0)), A(240, 330, 150, 170 - h, 180, 0), L((390, 330), (390, 0)),
    A(540, 330, 150, 170 - h, 180, 0), L((690, 330), (690, 0))])


def _n(h):
    return [L((90, 500), (90, 0)), A(290, 300, 200, 200 - h, 180, 0), L((490, 300), (490, 0))]


simple(0x6E, 580, _n)
simple(0x6F, 580, lambda h: [E(290, 250, 210, 250 - h)])
simple(0x70, 580, lambda h: [L((90, 500), (90, -200)), E(310, 250, 200, 250 - h)])
simple(0x71, 580, lambda h: [E(270, 250, 200, 250 - h), L((490, 500), (490, -200))])
simple(0x72, 380, lambda h: [L((90, 500), (90, 0)), A(260, 300, 170, 200 - h, 180, 60)])
simple(0x73, 500, lambda h: S_shape(250, 170, 500, h))
simple(0x74, 360, lambda h: [L((150, 640), (150, h), (320, h)), L((40, 480), (320, 480))])


def _u(h):
    return [L((90, 500), (90, 200)), A(290, 200, 200, 200 - h, 180, 360), L((490, 500), (490, 0))]


simple(0x75, 580, _u)
simple(0x76, 520, lambda h: [L((40, 500), (260, 0), (480, 500))])
simple(0x77, 760, lambda h: [L((40, 500), (200, 0), (380, 420), (560, 0), (720, 500))])
simple(0x78, 520, lambda h: [L((50, 500), (470, 0)), L((470, 500), (50, 0))])


def _y(h):
    return [L((40, 500), (280, 0)), L((480, 500), (200, -200))]


simple(0x79, 520, _y)
simple(0x7A, 500, lambda h: [L((60, 500 - h), (440, 500 - h), (60, h), (440, h))])

# Digits (tabular: all 560)
DIG = {}
DIG[0x30] = lambda h: [E(280, 350, 200, 350 - h)]
DIG[0x31] = lambda h: [L((150, 560), (310, 700), (310, 0)), L((150, h), (460, h))]


def _two(h):
    a = A(280, 490, 190, 210 - h, 160, -20)
    e = apt(280, 490, 190, 210 - h, -20)
    return [a, L(e, (90, h), (480, h))]


DIG[0x32] = _two
DIG[0x33] = lambda h: [A(270, 525, 180, 175 - h, 150, -90), A(280, 175, 200, 175 - h, 90, -150)]
DIG[0x34] = lambda h: [L((400, 0), (400, 700), (60, 200), (500, 200))]


def _five(h):
    s = apt(280, 220, 200, 220 - h, 140)
    return [L((460, 700 - h), (130, 700 - h), s), A(280, 220, 200, 220 - h, 140, -150)]


DIG[0x35] = _five
DIG[0x36] = lambda h: [E(285, 220, 195, 220 - h), A(330, 350, 240, 350 - h, 200, 60)]
DIG[0x37] = lambda h: [L((60, 700 - h), (500, 700 - h), (200, 0))]
DIG[0x38] = lambda h: [E(280, 530, 170, 170 - h), E(280, 185, 200, 185 - h)]
DIG[0x39] = lambda h: [E(275, 480, 195, 220 - h), A(230, 350, 240, 350 - h, 20, -120)]
for _cp, _fn in DIG.items():
    simple(_cp, 560, _fn)

# ASCII punctuation and symbols
simple(0x20, 280, lambda h: [])
simple(0x21, 260, lambda h: [L((130, 700), (130, 220)), D(130, 40)])
simple(0x22, 360, lambda h: [L((110, 700), (110, 500)), L((250, 700), (250, 500))])
simple(0x23, 620, lambda h: [L((240, 680), (200, 20)), L((420, 680), (380, 20)), L((90, 460), (560, 460)), L((60, 240), (530, 240))])
simple(0x24, 560, lambda h: S_shape(280, 190, 700, h) + [L((280, 760), (280, -60))])
simple(0x25, 760, lambda h: [E(190, 530, 110, 160), E(570, 170, 110, 160), L((600, 700), (160, 0))])
simple(0x26, 680, lambda h: [E(250, 545, 120, 145), A(280, 200, 200, 200 - h, 60, 330), L((220, 410), (620, 0))])
simple(0x27, 200, lambda h: [L((100, 700), (100, 500))])
simple(0x28, 320, lambda h: [A(400, 330, 260, 600, 140, 220)])
simple(0x29, 320, lambda h: [A(-80, 330, 260, 600, 40, -40)])
simple(0x2A, 480, lambda h: [L((240, 700), (240, 420)), L((110, 640), (370, 480)), L((370, 640), (110, 480))])
simple(0x2B, 600, lambda h: [L((300, 90), (300, 510)), L((90, 300), (510, 300))])
simple(0x2C, 240, lambda h: [L((140, 90), (80, -120))])
simple(0x2D, 360, lambda h: [L((60, 300), (300, 300))])
simple(0x2E, 240, lambda h: [D(120, 40)])
simple(0x2F, 400, lambda h: [L((360, 760), (40, -100))])
simple(0x3A, 240, lambda h: [D(120, 40), D(120, 440)])
simple(0x3B, 240, lambda h: [D(120, 440), L((140, 90), (80, -120))])
simple(0x3C, 600, lambda h: [L((510, 520), (90, 300), (510, 80))])
simple(0x3D, 600, lambda h: [L((90, 390), (510, 390)), L((90, 210), (510, 210))])
simple(0x3E, 600, lambda h: [L((90, 520), (510, 300), (90, 80))])


def _q(h):
    e = apt(260, 520, 180, 180 - h, -60)
    return [A(260, 520, 180, 180 - h, 160, -60), L(e, (260, 300), (260, 200)), D(260, 40)]


simple(0x3F, 520, _q)
simple(0x40, 900, lambda h: [E(440, 300, 130, 170), L((570, 470), (570, 170), (660, 120)), A(450, 300, 380, 400, -20, 300)])
simple(0x5B, 320, lambda h: [L((260, 760), (110, 760), (110, -100), (260, -100))])
simple(0x5C, 400, lambda h: [L((40, 760), (360, -100))])
simple(0x5D, 320, lambda h: [L((60, 760), (210, 760), (210, -100), (60, -100))])
simple(0x5E, 560, lambda h: [L((90, 380), (280, 700), (470, 380))])
simple(0x5F, 560, lambda h: [L((0, -80), (560, -80))])
simple(0x60, 300, lambda h: [L((90, 720), (210, 580))])
simple(0x7B, 360, lambda h: [L((280, 760), (180, 700), (180, 420), (90, 330), (180, 240), (180, -40), (280, -100))])
simple(0x7C, 240, lambda h: [L((120, 760), (120, -200))])
simple(0x7D, 360, lambda h: [L((80, 760), (180, 700), (180, 420), (270, 330), (180, 240), (180, -40), (80, -100))])
simple(0x7E, 600, lambda h: [A(200, 300, 110, 70, 180, 0), A(420, 300, 110, 70, 180, 360)])

# Accents, placed about (cx, base): base 760 above capitals, 560 above lowercase.


def acc(kind, cx, base):
    if kind == "grave":
        return [L((cx - 70, base + 170), (cx + 50, base + 30))]
    if kind == "acute":
        return [L((cx + 70, base + 170), (cx - 50, base + 30))]
    if kind == "circ":
        return [L((cx - 120, base + 30), (cx, base + 170), (cx + 120, base + 30))]
    if kind == "tilde":
        return [A(cx - 60, base + 90, 60, 70, 180, 0), A(cx + 60, base + 90, 60, 70, 180, 360)]
    if kind == "diaer":
        return [D(cx - 90, base + 90), D(cx + 90, base + 90)]
    if kind == "ring":
        return [E(cx, base + 100, 70, 70)]
    if kind == "cedilla":
        return [L((cx, 0), (cx + 30, -80), (cx - 50, -180))]
    raise ValueError(kind)


def with_acc(cp, base_cp, kind, cx=None, upper=None):
    w, fn = G[base_cp]
    if cx is None:
        cx = w / 2.0
    if upper is None:
        upper = 0x41 <= base_cp <= 0x5A
    base = 760 if upper else 560
    G[cp] = (w, lambda h, fn=fn, kind=kind, cx=cx, base=base: fn(h) + acc(kind, cx, base))


def dotless_i(h):
    return [L((110, 0), (110, 500))]


def sup(cp_base, s, dx, dy):
    """The base glyph's skeleton scaled by s and shifted, with a thinner stroke."""
    _, fn = G[cp_base]
    return lambda h: transform(fn(h), s, s, dx, dy, 0.7)


# Latin-1 supplement
simple(0xA0, 280, lambda h: [])
simple(0xA1, 260, lambda h: [D(130, 460), L((130, 280), (130, -200))])
simple(0xA2, 560, lambda h: [A(290, 250, 190, 230, 40, 320), L((290, 580), (290, -80))])
simple(0xA3, 560, lambda h: [L((470, h), (80, h)), L((80, h), (170, 120), (170, 450)), A(310, 470, 140, 190, 180, 10), L((70, 330), (380, 330))])
simple(0xA4, 600, lambda h: [E(300, 330, 150, 150), L((140, 490), (190, 440)), L((460, 490), (410, 440)), L((140, 170), (190, 220)), L((460, 170), (410, 220))])
simple(0xA5, 620, lambda h: [L((40, 700), (310, 350), (580, 700)), L((310, 350), (310, 0)), L((130, 300), (490, 300)), L((130, 160), (490, 160))])
simple(0xA6, 240, lambda h: [L((120, 760), (120, 380)), L((120, 180), (120, -200))])
simple(0xA7, 540, lambda h: [E(270, 340, 140, 120), A(270, 560, 150, 110, 10, 250), A(270, 120, 150, 110, -170, 70)])
simple(0xA8, 400, lambda h: [D(130, 640), D(270, 640)])
simple(0xA9, 800, lambda h: [E(400, 350, 330, 340), A(410, 350, 140, 160, 45, 315)])
simple(0xAA, 380, lambda h: [E(170, 520, 90, 110), L((260, 630), (260, 410)), L((80, 330), (300, 330))])
simple(0xAB, 520, lambda h: [L((260, 440), (100, 260), (260, 80)), L((440, 440), (280, 260), (440, 80))])
simple(0xAC, 600, lambda h: [L((90, 360), (510, 360), (510, 180))])
simple(0xAD, 360, lambda h: [L((60, 300), (300, 300))])
simple(0xAE, 800, lambda h: [
    E(400, 350, 330, 340), L((300, 170), (300, 530), (420, 530)), A(420, 450, 90, 80, 90, -90),
    L((420, 370), (300, 370)), L((400, 370), (500, 170))])
simple(0xAF, 400, lambda h: [L((60, 680), (340, 680))])
simple(0xB0, 400, lambda h: [E(200, 560, 110, 120)])
simple(0xB1, 600, lambda h: [L((300, 150), (300, 570)), L((90, 360), (510, 360)), L((90, h), (510, h))])
simple(0xB2, 380, sup(0x32, 0.55, 30, 330))
simple(0xB3, 380, sup(0x33, 0.55, 30, 330))
simple(0xB4, 300, lambda h: [L((210, 720), (90, 580))])
simple(0xB5, 580, lambda h: [L((90, 500), (90, -200)), A(290, 200, 200, 200 - h, 180, 360), L((490, 500), (490, 0))])
simple(0xB6, 600, lambda h: [L((300, 700), (300, 0)), L((450, 700), (450, 0)), L((450, 700 - h), (250, 700 - h)), A(250, 500, 150, 200 - h, 90, 270)])
simple(0xB7, 240, lambda h: [D(120, 330)])
simple(0xB8, 300, lambda h: [L((150, 0), (180, -80), (100, -180))])
simple(0xB9, 380, sup(0x31, 0.55, 30, 330))
simple(0xBA, 380, lambda h: [E(190, 540, 110, 120), L((80, 330), (300, 330))])
simple(0xBB, 520, lambda h: [L((80, 440), (240, 260), (80, 80)), L((260, 440), (420, 260), (260, 80))])
simple(0xBC, 800, lambda h: sup(0x31, 0.5, 20, 350)(h) + [L((600, 700), (200, 0))] + sup(0x34, 0.5, 480, 0)(h))
simple(0xBD, 800, lambda h: sup(0x31, 0.5, 20, 350)(h) + [L((600, 700), (200, 0))] + sup(0x32, 0.5, 480, 0)(h))
simple(0xBE, 800, lambda h: sup(0x33, 0.5, 20, 350)(h) + [L((600, 700), (200, 0))] + sup(0x34, 0.5, 480, 0)(h))
simple(0xBF, 520, lambda h: transform(_q(h), -1, -1, 520, 460))

for _cp, _b, _k in [
    (0xC0, 0x41, "grave"), (0xC1, 0x41, "acute"), (0xC2, 0x41, "circ"), (0xC3, 0x41, "tilde"),
    (0xC4, 0x41, "diaer"), (0xC5, 0x41, "ring"),
    (0xC8, 0x45, "grave"), (0xC9, 0x45, "acute"), (0xCA, 0x45, "circ"), (0xCB, 0x45, "diaer"),
    (0xCC, 0x49, "grave"), (0xCD, 0x49, "acute"), (0xCE, 0x49, "circ"), (0xCF, 0x49, "diaer"),
    (0xD1, 0x4E, "tilde"),
    (0xD2, 0x4F, "grave"), (0xD3, 0x4F, "acute"), (0xD4, 0x4F, "circ"), (0xD5, 0x4F, "tilde"), (0xD6, 0x4F, "diaer"),
    (0xD9, 0x55, "grave"), (0xDA, 0x55, "acute"), (0xDB, 0x55, "circ"), (0xDC, 0x55, "diaer"),
    (0xDD, 0x59, "acute"),
    (0xE0, 0x61, "grave"), (0xE1, 0x61, "acute"), (0xE2, 0x61, "circ"), (0xE3, 0x61, "tilde"),
    (0xE4, 0x61, "diaer"), (0xE5, 0x61, "ring"),
    (0xE8, 0x65, "grave"), (0xE9, 0x65, "acute"), (0xEA, 0x65, "circ"), (0xEB, 0x65, "diaer"),
    (0xF1, 0x6E, "tilde"),
    (0xF2, 0x6F, "grave"), (0xF3, 0x6F, "acute"), (0xF4, 0x6F, "circ"), (0xF5, 0x6F, "tilde"), (0xF6, 0x6F, "diaer"),
    (0xF9, 0x75, "grave"), (0xFA, 0x75, "acute"), (0xFB, 0x75, "circ"), (0xFC, 0x75, "diaer"),
    (0xFD, 0x79, "acute"), (0xFF, 0x79, "diaer"),
]:
    with_acc(_cp, _b, _k)
for _cp, _k in [(0xEC, "grave"), (0xED, "acute"), (0xEE, "circ"), (0xEF, "diaer")]:
    G[_cp] = (220, lambda h, k=_k: dotless_i(h) + acc(k, 110, 560))
G[0xC7] = (640, lambda h: G[0x43][1](h) + acc("cedilla", 360, 0))
G[0xE7] = (520, lambda h: G[0x63][1](h) + acc("cedilla", 290, 0))
simple(0xC6, 900, lambda h: [L((40, 0), (420, 700)), L((420, 700 - h), (840, 700 - h)), L((450, 700), (450, h), (840, h)), L((450, 360), (800, 360)), L((200, 250), (450, 250))])
simple(0xD0, 650, lambda h: G[0x44][1](h) + [L((20, 360), (250, 360))])
simple(0xD7, 600, lambda h: [L((120, 120), (480, 480)), L((480, 120), (120, 480))])
simple(0xD8, 720, lambda h: [E(360, 350, 270, 350 - h), L((620, 760), (100, -60))])
simple(0xDE, 600, lambda h: [L((90, 0), (90, 700)), L((90, 560), (320, 560)), A(320, 385, 190, 175 - h, 90, -90), L((320, 210), (90, 210))])
simple(0xDF, 600, lambda h: [L((90, 0), (90, 520)), A(260, 520, 170, 190, 180, 0), L((430, 520), (300, 400)), A(300, 190, 200, 190 - h, 90, -150)])
simple(0xE6, 860, lambda h: [E(250, 250, 170, 230), L((420, 500), (420, 0)), A(620, 250, 190, 230, 0, 320), L((430, 250), (810, 250))])
simple(0xF0, 580, lambda h: [E(290, 230, 200, 230 - h), L((480, 240), (260, 720)), L((260, 620), (460, 680))])
simple(0xF7, 600, lambda h: [L((90, 300), (510, 300)), D(300, 480), D(300, 120)])
simple(0xF8, 580, lambda h: [E(290, 250, 210, 250 - h), L((520, 560), (60, -60))])
simple(0xFE, 580, lambda h: [L((90, 760), (90, -200)), E(310, 250, 200, 230)])

# A few general punctuation and technical symbols used in reports
simple(0x2009, 200, lambda h: [])
simple(0x202F, 200, lambda h: [])
simple(0x2013, 560, lambda h: [L((40, 300), (520, 300))])
simple(0x2014, 1000, lambda h: [L((40, 300), (960, 300))])
simple(0x2018, 240, lambda h: [L((140, 700), (100, 540))])
simple(0x2019, 240, lambda h: [L((140, 700), (100, 540))])
simple(0x201C, 400, lambda h: [L((140, 700), (100, 540)), L((280, 700), (240, 540))])
simple(0x201D, 400, lambda h: [L((140, 700), (100, 540)), L((280, 700), (240, 540))])
simple(0x2022, 400, lambda h: [D(200, 330, 100)])
simple(0x2026, 900, lambda h: [D(150, 40), D(450, 40), D(750, 40)])
simple(0x2212, 600, lambda h: [L((90, 300), (510, 300))])
simple(0x2264, 600, lambda h: [L((510, 600), (90, 400), (510, 200)), L((90, 60), (510, 60))])
simple(0x2265, 600, lambda h: [L((90, 600), (510, 400), (90, 200)), L((90, 60), (510, 60))])
simple(0x2300, 720, lambda h: [E(360, 330, 220, 220), L((620, 630), (100, 30))])


def notdef(h):
    return [L((60, 0), (60, 700), (440, 700), (440, 0), (60, 0))]


# ---------------------------------------------------------------------------------------
# TrueType writing


def encode_simple(contours):
    if not contours:
        return b""
    pts = [p for c in contours for p in c]
    xs = [p[0] for p in pts]
    ys = [p[1] for p in pts]
    out = struct.pack(">hhhhh", len(contours), min(xs), min(ys), max(xs), max(ys))
    end = -1
    for c in contours:
        end += len(c)
        out += struct.pack(">H", end)
    out += struct.pack(">H", 0)
    flags = bytearray()
    xb = bytearray()
    yb = bytearray()
    px = py = 0
    for (x, y) in pts:
        f = 0x01
        dx, dy = x - px, y - py
        if dx == 0:
            f |= 0x10
        elif -255 <= dx <= 255:
            f |= 0x02 | (0x10 if dx > 0 else 0)
            xb.append(abs(dx))
        else:
            xb += struct.pack(">h", dx)
        if dy == 0:
            f |= 0x20
        elif -255 <= dy <= 255:
            f |= 0x04 | (0x20 if dy > 0 else 0)
            yb.append(abs(dy))
        else:
            yb += struct.pack(">h", dy)
        flags.append(f)
        px, py = x, y
    out += bytes(flags) + bytes(xb) + bytes(yb)
    return out


def pad4(b):
    return b + b"\0" * ((4 - len(b) % 4) % 4)


def checksum(b):
    b = pad4(b)
    s = 0
    for i in range(0, len(b), 4):
        s = (s + struct.unpack(">I", b[i:i + 4])[0]) & 0xFFFFFFFF
    return s


def assemble(tables):
    """tables: dict tag(bytes) -> data. head must have checkSumAdjustment zero."""
    tags = sorted(tables)
    n = len(tags)
    es = 0
    while (1 << (es + 1)) <= n:
        es += 1
    sr = (1 << es) * 16
    header = struct.pack(">IHHHH", 0x00010000, n, sr, es, n * 16 - sr)
    offset = 12 + 16 * n
    dirs = b""
    body = b""
    for t in tags:
        d = tables[t]
        dirs += struct.pack(">4sIII", t, checksum(d), offset + len(body), len(d))
        body += pad4(d)
    font = bytearray(header + dirs + body)
    adj = (0xB1B0AFBA - checksum(bytes(font))) & 0xFFFFFFFF
    head_off = offset + sum(len(pad4(tables[t])) for t in tags[: tags.index(b"head")])
    font[head_off + 8:head_off + 12] = struct.pack(">I", adj)
    return bytes(font)


def name_table(records):
    """records: list of (nameID, text), platform 3 / encoding 1 / language 0x409."""
    records = sorted(records)
    strings = b""
    recs = b""
    for nid, text in records:
        s = text.encode("utf-16-be")
        recs += struct.pack(">HHHHHH", 3, 1, 0x409, nid, len(s), len(strings))
        strings += s
    return struct.pack(">HHH", 0, len(records), 6 + 12 * len(records)) + recs + strings


def cmap4(mapping):
    """mapping: sorted list of (code point <= 0xFFFF, gid)."""
    segs = []
    for cp, gid in mapping:
        if segs and segs[-1][1] + 1 == cp and segs[-1][2] + (cp - segs[-1][0]) == gid:
            segs[-1][1] = cp
        else:
            segs.append([cp, cp, gid])
    segs.append([0xFFFF, 0xFFFF, None])
    n = len(segs)
    es = 0
    while (1 << (es + 1)) <= n:
        es += 1
    sr = 2 * (1 << es)
    ends = b"".join(struct.pack(">H", s[1]) for s in segs)
    starts = b"".join(struct.pack(">H", s[0]) for s in segs)
    deltas = b"".join(struct.pack(">H", 1 if s[2] is None else (s[2] - s[0]) & 0xFFFF) for s in segs)
    ranges = b"\0\0" * n
    body = struct.pack(">HHHH", 2 * n, sr, es, 2 * n - sr) + ends + b"\0\0" + starts + deltas + ranges
    return struct.pack(">HHH", 4, 6 + len(body), 0) + body


def cmap12(mapping):
    groups = []
    for cp, gid in mapping:
        if groups and groups[-1][1] + 1 == cp and groups[-1][2] + (cp - groups[-1][0]) == gid:
            groups[-1][1] = cp
        else:
            groups.append([cp, cp, gid])
    body = b"".join(struct.pack(">III", *g) for g in groups)
    return struct.pack(">HHIII", 12, 0, 16 + len(body), 0, len(groups)) + body


def cmap_table(mapping, with12):
    subs = [(3, 1, cmap4([m for m in mapping if m[0] <= 0xFFFF]))]
    if with12:
        subs.append((3, 10, cmap12(mapping)))
    head = struct.pack(">HH", 0, len(subs))
    off = 4 + 8 * len(subs)
    recs = b""
    data = b""
    for p, e, d in subs:
        recs += struct.pack(">HHI", p, e, off + len(data))
        data += d
    return head + recs + data


def build_font(family, ps_name, style, bold, mono, glyphs, with12=False, composite=None):
    """glyphs: list of (code point or None for .notdef, advance, glyf bytes, xmin)."""
    n = len(glyphs)
    glyf = b""
    loca = []
    max_pts = max_ctr = 0
    xmin = ymin = 32767
    xmax = ymax = -32768
    for _, _, data, _ in glyphs:
        loca.append(len(glyf))
        if data and struct.unpack(">h", data[0:2])[0] >= 0:
            nc = struct.unpack(">h", data[0:2])[0]
            ends = struct.unpack(">%dH" % nc, data[10:10 + 2 * nc])
            max_pts = max(max_pts, ends[-1] + 1)
            max_ctr = max(max_ctr, nc)
        if data:
            bx0, by0, bx1, by1 = struct.unpack(">hhhh", data[2:10])
            xmin, ymin, xmax, ymax = min(xmin, bx0), min(ymin, by0), max(xmax, bx1), max(ymax, by1)
        glyf += pad4(data)
    loca.append(len(glyf))
    hmtx = b"".join(struct.pack(">Hh", adv, lsb) for _, adv, _, lsb in glyphs)
    head = struct.pack(
        ">IIIIHHqqhhhhHHhhh", 0x00010000, 0x00010000, 0, 0x5F0F3CF5, 0x000B, UPEM, 0, 0,
        xmin, ymin, xmax, ymax, 1 if bold else 0, 8, 2, 1, 0)
    advs = [g[1] for g in glyphs]
    hhea = struct.pack(
        ">IhhhHhhhhhhhhhhhH", 0x00010000, 950, -250, 0, max(advs), min(g[3] for g in glyphs), 0,
        xmax, 1, 0, 0, 0, 0, 0, 0, 0, n)
    comp_pts, comp_ctr, comp_elems, comp_depth = composite or (0, 0, 0, 0)
    maxp = struct.pack(">IHHHHHHHHHHHHHH", 0x00010000, n, max_pts, max_ctr, comp_pts, comp_ctr,
                       2, 0, 0, 0, 0, 0, 0, comp_elems, comp_depth)
    mapping = sorted((cp, gid) for gid, (cp, _, _, _) in enumerate(glyphs) if cp is not None)
    first = min(cp for cp, _ in mapping)
    last = min(max(cp for cp, _ in mapping), 0xFFFF)
    avg = sum(a for a in advs if a) // max(1, len([a for a in advs if a]))
    os2 = struct.pack(
        ">HhHHH" + "h" * 10 + "h" + "10s" + "IIII" + "4s" + "HHH" + "hhh" + "HH" + "II" + "hhHHH",
        4, avg, 700 if bold else 400, 5, 0,
        650, 600, 0, 75, 650, 600, 0, 350, 50, 300,
        0, b"\0" * 10, 3, 0, 0, 0, b"NONE",
        0x20 if bold else 0x40, first, last,
        800, -200, 200, 950, 250, 1, 0,
        500, 700, 0, 32, 1)
    licence = ("Licensed under MIT OR Apache-2.0, at your option, like the rest of Metrology Maven. "
               "Original outlines generated by make_fonts.py.")
    name = name_table([
        (0, "Copyright 2026 The Metrology Maven contributors"),
        (1, family),
        (2, style),
        (3, ps_name + ";1.000"),
        (4, family + " " + style),
        (5, "Version 1.000"),
        (6, ps_name),
        (13, licence),
    ])
    post = struct.pack(">IihhIIIII", 0x00030000, 0, -100, 50, 1 if mono else 0, 0, 0, 0, 0)
    loca_b = b"".join(struct.pack(">I", o) for o in loca)
    tables = {
        b"OS/2": os2,
        b"cmap": cmap_table(mapping, with12),
        b"glyf": glyf,
        b"head": head,
        b"hhea": hhea,
        b"hmtx": hmtx,
        b"loca": loca_b,
        b"maxp": maxp,
        b"name": name,
        b"post": post,
    }
    return assemble(tables)


def outline(prims, h):
    contours = stroke(prims, h)
    data = encode_simple(contours)
    lsb = min(p[0] for c in contours for p in c) if contours else 0
    return data, lsb


def make_family_font(family, ps_name, style, bold, mono):
    h = 60 if bold else 38
    glyphs = []
    nd_prims = notdef(h)
    if mono:
        nd_prims = transform(nd_prims, 1, 1, 50, 0)
    data, lsb = outline(nd_prims, h)
    glyphs.append((None, 600 if mono else 500, data, lsb))
    for cp in sorted(G):
        width, fn = G[cp]
        prims = fn(h)
        adv = width
        if mono:
            adv = 600
            if prims:
                x0, x1 = extent(prims)
                span = x1 - x0
                s = min(1.0, 420.0 / span) if span > 0 else 1.0
                c = (x0 + x1) / 2.0
                prims = transform(prims, s, 1, 300 - c * s, 0)
        data, lsb = outline(prims, h)
        glyphs.append((cp, adv, data, lsb))
    return build_font(family, ps_name, style, bold, mono, glyphs)


def make_script_test_font():
    """Three box-like CJK glyphs (U+65E5, U+672C, U+8A9E) and digit one, cmap formats 4 and 12."""
    h = 40
    glyphs = [(None, 1000, *outline([L((100, -80), (100, 820), (900, 820), (900, -80), (100, -80))], h))]
    shapes = {
        0x31: (500, [L((250, 0), (250, 700))]),
        0x65E5: (1000, [L((250, -50), (250, 780), (750, 780), (750, -50), (250, -50)), L((250, 365), (750, 365))]),
        0x672C: (1000, [L((100, 560), (900, 560)), L((500, 820), (500, -60)), L((500, 560), (120, 80)),
                        L((500, 560), (880, 80)), L((330, 160), (670, 160))]),
        0x8A9E: (1000, [L((80, 760), (380, 760)), L((80, 560), (380, 560)), L((80, 380), (380, 380)),
                        L((100, 180), (100, -40), (360, -40), (360, 180), (100, 180)),
                        L((480, 760), (900, 760)), L((480, 400), (900, 400)), L((480, 180), (480, -40), (880, -40), (880, 180), (480, 180))]),
    }
    for cp in sorted(shapes):
        adv, prims = shapes[cp]
        data, lsb = outline(prims, h)
        glyphs.append((cp, adv, data, lsb))
    return build_font("Maven Script Test", "MavenScriptTest-Regular", "Regular", False, False, glyphs, with12=True)


def make_composite_test_font():
    """.notdef, 'A', 'B' (simple) and 'C' = 'A' + 'B' shifted (composite, args as words)."""
    h = 40
    nd, nd_lsb = outline(notdef(h), h)
    a, a_lsb = outline([L((40, 0), (320, 700), (600, 0))], h)
    b, b_lsb = outline([L((100, 0), (100, 700))], h)
    ax0, ay0, ax1, ay1 = struct.unpack(">hhhh", a[2:10])
    bx0, by0, bx1, by1 = struct.unpack(">hhhh", b[2:10])
    shift = 600
    bbox = (min(ax0, bx0 + shift), min(ay0, by0), max(ax1, bx1 + shift), max(ay1, by1))
    comp = struct.pack(">hhhhh", -1, *bbox)
    comp += struct.pack(">HHhh", 0x0001 | 0x0002 | 0x0020, 1, 0, 0)  # gid 1 = 'A'
    comp += struct.pack(">HHhh", 0x0001 | 0x0002, 2, shift, 0)  # gid 2 = 'B'
    glyphs = [(None, 500, nd, nd_lsb), (0x41, 640, a, a_lsb), (0x42, 240, b, b_lsb), (0x43, 900, comp, bbox[0])]
    na = struct.unpack(">H", a[10:12])[0] + 1
    nb = struct.unpack(">H", b[10:12])[0] + 1
    ca = struct.unpack(">h", a[0:2])[0]
    cb = struct.unpack(">h", b[0:2])[0]
    return build_font("Maven Composite Test", "MavenCompositeTest-Regular", "Regular", False, False, glyphs,
                      composite=(na + nb, ca + cb, 2, 1))


def outputs():
    return {
        ASSETS / "MavenReportSans-Regular.ttf": make_family_font("Maven Report Sans", "MavenReportSans-Regular", "Regular", False, False),
        ASSETS / "MavenReportSans-Bold.ttf": make_family_font("Maven Report Sans", "MavenReportSans-Bold", "Bold", True, False),
        ASSETS / "MavenReportMono-Regular.ttf": make_family_font("Maven Report Mono", "MavenReportMono-Regular", "Regular", False, True),
        ASSETS / "MavenReportMono-Bold.ttf": make_family_font("Maven Report Mono", "MavenReportMono-Bold", "Bold", True, True),
        FIXT / "script-test.ttf": make_script_test_font(),
        FIXT / "composite-test.ttf": make_composite_test_font(),
    }


def main(argv):
    check = "--check" in argv[1:]
    bad = 0
    for path, data in outputs().items():
        rel = path.relative_to(ROOT).as_posix()
        if check:
            if not path.exists() or path.read_bytes() != data:
                print("differs: " + rel)
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            print("wrote %s (%d bytes)" % (rel, len(data)))
    # The Rust manifest (bundled fonts) and the tests (test fonts) pin each file's SHA-256.
    crate = ROOT / "crates" / "fid-report-pdf"
    pins = (crate / "src" / "text" / "manifest.rs").read_text(encoding="utf-8")
    pins += (crate / "tests" / "text.rs").read_text(encoding="utf-8")
    for path, data in outputs().items():
        digest = "sha256:" + hashlib.sha256(data).hexdigest()
        if digest not in pins:
            print("digest of %s is not pinned in manifest.rs / tests/text.rs: %s" % (path.name, digest))
            bad += 1
    if bad:
        print("%d problem(s); run make_fonts.py and update the pinned digests" % bad)
        return 1
    if check:
        print("fonts ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
