#!/usr/bin/env python3
"""Writes the shipped data palettes of spec node UX.TOKENS.DATAPAL into `design/tokens/palettes/`.

Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/datapal/make_palettes.py          # (re)write the palette files
    python fixtures/ux/tokens/datapal/make_palettes.py --check  # fail if a shipped file differs

This is the authoring source of the palette values, not a test oracle: the independent checks
live in `make_fixtures.py`, which reads the files written here and never imports this module.

Every palette is original. Each one is a path through OKLCH (OKLab lightness, chroma, hue angle)
chosen for this project, converted to sRGB, reduced in chroma until it fits the gamut and
rounded to 8 bits. No step, sample or hex value is taken from another palette:

* `pal.step.cvd@1` (9 steps, the default) and `pal.step.tol@1` (11 steps) are diverging: a
  neutral grey centre, a blue arm (negative deviations) and an orange arm (positive deviations),
  lightness falling and chroma rising away from the centre. Blue and orange stay apart under the
  three dichromacies, unlike red and green (the spec's design method).
* `pal.cont.seq-a@1` and `pal.cont.seq-cvd@1` are sequential maps of this project's own: lightness
  rising strictly from dark to light, samples spaced evenly by CIEDE2000 arc length, `seq-a`
  sweeping hue from blue-violet through teal to yellow and `seq-cvd` on one straight OKLab line
  from blue to yellow, the axis that protanopia and deuteranopia keep. They were inspired by the
  published design goals of viridis (van der Walt, Smith and Firing; CC0) and cividis (Nuñez,
  Anderton and Renslow, 2018), and copy no sample of either.
* All four share one out-of-range pair, the out-of-tolerance region colours of the locked status
  palette's nonconforming set: `under` a muted olive and `over` a muted rose-brown. Mid lightness
  gives them 3:1 against both the light and the dark base surface; being warm keeps them
  CIEDE2000 20 from every theme's azure CONFORMS under all four vision conditions
  (REQ-UX.TOKENS-009); low chroma keeps them CIEDE2000 10 from every in-range colour of every
  shipped palette and from the neutral status colours under all four vision conditions. They
  were picked by a grid search in OKLCH over those rules. Under simulated dichromacy the two are
  only about 6 apart, so the stripe cue and the legend tell under-range from over-range.

OKLab is Bjorn Ottosson's published colour space (2020); only its conversion formulas are used.
"""

import hashlib
import json
import math
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
DESIGN = ROOT / "design"
PAL_DIR = "tokens/palettes"

# ------------------------------------------------------------------ OKLab <-> sRGB


def linear(v):
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def encode(c):
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1.0 / 2.4) - 0.055


def oklab_of_rgb(rgb):
    r, g, b = (linear(v) for v in rgb)
    l = 0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b
    m = 0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b
    s = 0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b
    l, m, s = (math.copysign(abs(x) ** (1.0 / 3.0), x) for x in (l, m, s))
    return (
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    )


def linear_of_oklch(L, C, H):
    a, b = C * math.cos(math.radians(H)), C * math.sin(math.radians(H))
    l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3
    m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3
    s = (L - 0.0894841775 * a - 1.2914855480 * b) ** 3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )


def rgb_of_oklch(L, C, H):
    """8-bit sRGB of an OKLCH colour, chroma reduced in steps of 0.001 until it fits the gamut."""
    while C >= 0.0:
        lin = linear_of_oklch(L, C, H)
        if all(-1e-6 <= x <= 1.0 + 1e-6 for x in lin):
            return tuple(round(255.0 * encode(min(1.0, max(0.0, x)))) for x in lin)
        C -= 0.001
    raise ValueError((L, C, H))


def hexs(rgb):
    return "#" + "".join(f"{v:02x}" for v in rgb)


def cie_lstar(rgb):
    y = 0.2126729 * linear(rgb[0]) + 0.7151522 * linear(rgb[1]) + 0.0721750 * linear(rgb[2])
    d = 6.0 / 29.0
    fy = y ** (1.0 / 3.0) if y > d ** 3 else y / (3.0 * d * d) + 4.0 / 29.0
    return 116.0 * fy - 16.0


# ------------------------------------------------------------------ palettes

UNDER = (0.54, 0.06, 90.0)  # muted olive
OVER = (0.53, 0.06, 30.0)  # muted rose-brown

# Stepped: (OKLab L, chroma, hue) from the blue end through the neutral centre to the orange end.
STEPPED = {
    "pal.step.cvd": [
        (0.40, 0.150, 255.0),
        (0.56, 0.140, 252.0),
        (0.70, 0.110, 250.0),
        (0.84, 0.060, 250.0),
        (0.97, 0.000, 0.0),
        (0.86, 0.065, 70.0),
        (0.74, 0.120, 62.0),
        (0.62, 0.150, 55.0),
        (0.45, 0.130, 50.0),
    ],
    "pal.step.tol": [
        (0.36, 0.150, 266.0),
        (0.48, 0.160, 262.0),
        (0.60, 0.140, 256.0),
        (0.72, 0.100, 248.0),
        (0.85, 0.050, 238.0),
        (0.97, 0.000, 0.0),
        (0.875, 0.060, 80.0),
        (0.765, 0.105, 62.0),
        (0.65, 0.145, 50.0),
        (0.54, 0.160, 42.0),
        (0.42, 0.140, 38.0),
    ],
}

# Continuous: control points (OKLab L, chroma, hue), joined by straight segments in OKLab
# (L, a, b). The samples are then spaced evenly along the path by CIEDE2000 arc length, so
# equal steps in the data give roughly equal colour differences (perceptual uniformity).
CONTINUOUS = {
    "pal.cont.seq-a": [(0.32, 0.100, 262.0), (0.625, 0.110, 195.0), (0.93, 0.170, 108.0)],
    # One straight segment from blue to yellow: it passes close to grey without a kink.
    "pal.cont.seq-cvd": [(0.33, 0.085, 258.0), (0.92, 0.110, 92.0)],
}
DENSE = 4096
SAMPLES = 256
MIN_LSTAR_STEP = 0.05  # every sample is lighter than the one before by at least this (CIELAB L*)

SOURCES = {
    "pal.step.cvd": "original: generated for this project by fixtures/ux/tokens/datapal/make_palettes.py "
    "in OKLCH, a neutral centre with blue and orange arms kept apart under colour-vision deficiency",
    "pal.step.tol": "original: generated for this project by fixtures/ux/tokens/datapal/make_palettes.py "
    "in OKLCH, a finer stepped tolerance map with a neutral centre and blue and orange arms",
    "pal.cont.seq-a": "original: generated for this project by fixtures/ux/tokens/datapal/make_palettes.py "
    "along an OKLab path from blue-violet through teal to yellow, spaced by CIEDE2000; inspired only by "
    "the design goals of viridis (Stefan van der Walt, Nathaniel Smith and Eric Firing; CC0), no sample copied",
    "pal.cont.seq-cvd": "original: generated for this project by fixtures/ux/tokens/datapal/make_palettes.py "
    "on one OKLab line from blue to yellow, spaced by CIEDE2000; inspired only by the design goals of "
    "cividis (Jamie R. Nuñez, Christopher R. Anderton and Ryan S. Renslow, 2018), no sample copied",
}
LICENCE = "CC-BY-4.0"  # project-made data (LICENSE.md)
DEFAULT = "pal.step.cvd@1"
ORDER = ["pal.step.cvd@1", "pal.step.tol@1", "pal.cont.seq-a@1", "pal.cont.seq-cvd@1"]


def cielab_of_rgb_float(lin):
    r, g, b = lin
    x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047
    y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b
    z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883
    d = 6.0 / 29.0

    def f(t):
        return t ** (1.0 / 3.0) if t > d ** 3 else t / (3.0 * d * d) + 4.0 / 29.0

    return (116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z)))


def de2000(p, q):
    """CIEDE2000 (Sharma, Wu and Dalal, 2005), used here only to space the samples."""
    L1, a1, b1 = p
    L2, a2, b2 = q
    cb = (math.hypot(a1, b1) + math.hypot(a2, b2)) / 2.0
    g = 0.5 * (1.0 - math.sqrt(cb ** 7 / (cb ** 7 + 25.0 ** 7)))
    a1, a2 = a1 * (1.0 + g), a2 * (1.0 + g)
    c1, c2 = math.hypot(a1, b1), math.hypot(a2, b2)
    h1 = math.degrees(math.atan2(b1, a1)) % 360.0 if c1 else 0.0
    h2 = math.degrees(math.atan2(b2, a2)) % 360.0 if c2 else 0.0
    if c1 * c2 == 0.0:
        dh, hb = 0.0, h1 + h2
    else:
        dh = h2 - h1
        dh = dh - 360.0 if dh > 180.0 else dh + 360.0 if dh < -180.0 else dh
        hb = (h1 + h2) / 2.0 if abs(h1 - h2) <= 180.0 else (h1 + h2 + 360.0) / 2.0 if h1 + h2 < 360.0 else (h1 + h2 - 360.0) / 2.0
    dH = 2.0 * math.sqrt(c1 * c2) * math.sin(math.radians(dh / 2.0))
    lb, cbp = (L1 + L2) / 2.0, (c1 + c2) / 2.0
    t = 1 - 0.17 * math.cos(math.radians(hb - 30)) + 0.24 * math.cos(math.radians(2 * hb)) + 0.32 * math.cos(math.radians(3 * hb + 6)) - 0.20 * math.cos(math.radians(4 * hb - 63))
    sl = 1.0 + 0.015 * (lb - 50.0) ** 2 / math.sqrt(20.0 + (lb - 50.0) ** 2)
    sc, sh = 1.0 + 0.045 * cbp, 1.0 + 0.015 * cbp * t
    rt = -2.0 * math.sqrt(cbp ** 7 / (cbp ** 7 + 25.0 ** 7)) * math.sin(math.radians(60.0 * math.exp(-(((hb - 275.0) / 25.0) ** 2))))
    x, y, z = (L2 - L1) / sl, (c2 - c1) / sc, dH / sh
    return math.sqrt(x * x + y * y + z * z + rt * y * z)


def fit_lab(L, a, b):
    """Linear sRGB of an OKLab colour, chroma reduced (hue kept) until it fits the gamut."""
    C, H = math.hypot(a, b), math.degrees(math.atan2(b, a))
    while True:
        lin = linear_of_oklch(L, C, H)
        if all(-1e-6 <= x <= 1.0 + 1e-6 for x in lin) or C <= 0.0:
            return [min(1.0, max(0.0, x)) for x in lin]
        C = max(0.0, C - 0.001)


def continuous(points):
    labs = [(L, C * math.cos(math.radians(H)), C * math.sin(math.radians(H))) for L, C, H in points]
    seg = len(labs) - 1
    dense = []
    for i in range(DENSE + 1):
        u = i * seg / DENSE
        j = min(int(u), seg - 1)
        f = u - j
        p, q = labs[j], labs[j + 1]
        dense.append(fit_lab(*(p[k] + (q[k] - p[k]) * f for k in range(3))))
    cie = [cielab_of_rgb_float(x) for x in dense]
    arc = [0.0]
    for p, q in zip(cie, cie[1:]):
        arc.append(arc[-1] + de2000(p, q))
    out, j = [], 0
    for i in range(SAMPLES):
        want = arc[-1] * i / (SAMPLES - 1)
        while j < DENSE and arc[j + 1] < want:
            j += 1
        span = arc[j + 1] - arc[j] if j < DENSE else 1.0
        f = 0.0 if j >= DENSE or span == 0.0 else (want - arc[j]) / span
        a, b = dense[j], dense[min(j + 1, DENSE)]
        lin = [a[k] + (b[k] - a[k]) * f for k in range(3)]
        rgb = tuple(round(255.0 * encode(x)) for x in lin)
        if out and cie_lstar(rgb) < cie_lstar(out[-1]) + MIN_LSTAR_STEP:
            # Rounding to 8 bits broke the strict lightness rise: take the nearest 8-bit
            # neighbour (OKLab distance) that keeps it.
            target = oklab_of_rgb(rgb)
            cands = []
            for dr in range(-2, 3):
                for dg in range(-2, 3):
                    for db in range(-2, 3):
                        c = (rgb[0] + dr, rgb[1] + dg, rgb[2] + db)
                        if all(0 <= x <= 255 for x in c) and cie_lstar(c) >= cie_lstar(out[-1]) + MIN_LSTAR_STEP:
                            cands.append((math.dist(oklab_of_rgb(c), target), c))
            if not cands:
                raise SystemExit(f"cannot keep the lightness rising at sample {i}")
            rgb = min(cands)[1]
        out.append(rgb)
    return [hexs(c) for c in out]


def digest(colors, under, over):
    """SHA-256 of the RFC 8785 form of {colors, over, under} (ASCII strings only)."""
    body = json.dumps({"colors": colors, "over": over, "under": under}, sort_keys=True, separators=(",", ":"))
    return "sha256:" + hashlib.sha256(body.encode("utf-8")).hexdigest()


def palette(pid):
    under, over = hexs(rgb_of_oklch(*UNDER)), hexs(rgb_of_oklch(*OVER))
    if pid in STEPPED:
        colors = [hexs(rgb_of_oklch(*p)) for p in STEPPED[pid]]
        kind = "stepped"
    else:
        colors = continuous(CONTINUOUS[pid])
        kind = "continuous"
    out = {
        "id": pid,
        "version": 1,
        "kind": kind,
        "colors": colors,
        "under": under,
        "over": over,
        "source": SOURCES[pid],
        "licence": LICENCE,
        "digest": digest(colors, under, over),
    }
    if kind == "stepped":
        out["centre_index"] = (len(colors) - 1) // 2
    return out


def canon(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


AUTHOR = (
    "Claude Code (Claude Opus 5.5), agent author of UX.TOKENS.DATAPAL; values generated by "
    "fixtures/ux/tokens/datapal/make_palettes.py from OKLCH paths chosen for this project"
)
REFS = [
    "The UX.TOKENS, UX.TOKENS.DATAPAL, UX.TOKENS.STATUS and UX.SYSTEM.PRINC spec nodes in this repository",
    "The OKLab colour space conversion formulas published by Bjorn Ottosson (2020)",
    "The published design goals, as inspiration only and not the data, of the viridis colour map (Stefan van der Walt, Nathaniel Smith and Eric Firing; CC0) and of cividis (Jamie R. Nuñez, Christopher R. Anderton and Ryan S. Renslow, 2018)",
    "The CIEDE2000 colour-difference formula (Sharma, Wu and Dalal, 2005), used only to verify the values",
    "The colour-vision-deficiency simulation matrices of Machado, Oliveira and Fernandes (2009) at severity 1.0, used only to verify the values",
    "WCAG 2.2 success criterion 1.4.11 (the 3:1 non-text contrast threshold only)",
]


def files():
    out = {}
    out[f"{PAL_DIR}/catalogue.json"] = {"format": "palette-catalogue@1", "default": DEFAULT, "palettes": ORDER}
    for ref in ORDER:
        out[f"{PAL_DIR}/{ref}.json"] = palette(ref.split("@")[0])
    for rel in list(out):
        out[f"review/origin/{rel}.origin.json"] = {
            "artefact": rel,
            "author": AUTHOR,
            "date": "2026-11-20",
            "references": REFS,
            "statement_version": "originality-statement@1",
        }
    return {k: canon(v) for k, v in out.items()}


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    for rel, text in sorted(files().items()):
        p = DESIGN / rel
        if check:
            if not p.exists() or p.read_bytes() != text.encode("utf-8"):
                bad.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(text.encode("utf-8"))
    if bad:
        print("out of date (run make_palettes.py):\n  " + "\n  ".join(bad), file=sys.stderr)
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
