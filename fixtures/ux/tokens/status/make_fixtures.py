#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.TOKENS.STATUS and the independent expected results.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/status/make_fixtures.py          # (re)write fixtures and expected.json
    python fixtures/ux/tokens/status/make_fixtures.py --check  # fail if any file differs

Expected values, none produced by the Rust code under test:

* The findings of every error-path fixture are hand-labelled below (`LABELS`) from the leaf's
  rules, next to the one mutation each fixture makes to a small valid tree.
* For the shipped `design/tokens/status.json`, this script reads the file and the token tree
  itself (`json` with a duplicate-key hook, its own DTCG walk and alias resolver) and computes:
  - the CIEDE2000 colour difference (Sharma, Wu and Dalal, 2005) between every status colour and
    CONFORMS in every theme under normal vision and under protanopia, deuteranopia and
    tritanopia simulated with the matrices of Machado, Oliveira and Fernandes (2009) at severity
    1.0, applied to linear sRGB and clipped to [0, 1];
  - the WCAG 2.2 contrast of every status colour on the base, raised and sunken surfaces;
  - the OKLCH hue and chroma of every status colour (the hue-family rule);
  - the OKLab distance from every claimed role (the floor of UX.TOKENS.COLOR's tokens-check).
  It fails unless every value clears its rule by a margin, so a palette can only ship with a
  clear pass.

This module never imports `fixtures/ux/tokens/color/make_tokens.py` (the authoring script) or
`fixtures/ux/tokens/color/make_fixtures.py`.
"""

import copy
import json
import math
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
DESIGN = ROOT / "design"

THEMES = ["light", "dark", "hc", "floor"]
STATES = ["conforms", "nonconforms", "indeterminate", "not-decidable", "not-evaluated", "stale"]
CONFORMING = ["conforms"]
NONCONFORMING = ["nonconforms", "out-of-tolerance", "stale"]
VISIONS = ["normal", "protanopia", "deuteranopia", "tritanopia"]

MIN_DE2000 = 20.0  # REQ-UX.TOKENS-009 / REQ-UX.TOKENS.STATUS-003 [unverified threshold]
DE2000_MARGIN = 1.0
MIN_DE_OK = 0.10  # UX.TOKENS.COLOR: claimed roles vs status roles, every theme
DE_OK_MARGIN = 0.005
CONTRAST_MARGIN = 0.05
# REQ-UX.TOKENS.STATUS-002, gated on distance (maintainer decision on PR 339): every NEUTRAL state
# is at least MIN_NEUTRAL_DE2000 from every coloured nonconforming-set state, under all four vision
# conditions, in every theme. Shipped values clear it by NEUTRAL_MARGIN.
NEUTRAL = ["indeterminate", "not-decidable", "not-evaluated"]
NC_COLOURED = ["nonconforms", "stale"]
MIN_NEUTRAL_DE2000 = 10.0
NEUTRAL_MARGIN = 0.5
# Hue-family advice (not gating): a colour with OKLCH chroma below ACHROMATIC has no hue; otherwise
# a family is every hue within FAMILY_HALF_WIDTH degrees of a member's hue [unverified rule].
ACHROMATIC = 0.03
FAMILY_HALF_WIDTH = 30.0

# ------------------------------------------------------------------ colour maths


def channels(hexv):
    assert len(hexv) == 7 and hexv[0] == "#", hexv
    return [int(hexv[i : i + 2], 16) for i in (1, 3, 5)]


def linear(v):
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(hexv):
    r, g, b = (linear(v) for v in channels(hexv))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = luminance(a), luminance(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)


def oklab(hexv):
    r, g, b = (linear(v) for v in channels(hexv))
    lms = (
        0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b,
        0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b,
        0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b,
    )
    l_, m_, s_ = (math.cbrt(x) for x in lms)
    return (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )


def oklch(hexv):
    _, a, b = oklab(hexv)
    return math.degrees(math.atan2(b, a)) % 360.0, math.hypot(a, b)


# Machado, Oliveira and Fernandes (2009), severity 1.0, on linear sRGB.
MACHADO = {
    "protanopia": ((0.152286, 1.052583, -0.204868), (0.114503, 0.786281, 0.099216), (-0.003882, -0.048116, 1.051998)),
    "deuteranopia": ((0.367322, 0.860646, -0.227968), (0.280085, 0.672501, 0.047413), (-0.011820, 0.042940, 0.968881)),
    "tritanopia": ((1.255528, -0.076749, -0.178779), (-0.078411, 0.930809, 0.147602), (0.004733, 0.691367, 0.303900)),
}
for _m in MACHADO.values():
    for _row in _m:
        assert abs(sum(_row) - 1.0) < 2e-6, _row  # white stays white


def simulate(hexv, vision):
    rgb = [linear(v) for v in channels(hexv)]
    if vision == "normal":
        return rgb
    m = MACHADO[vision]
    return [min(1.0, max(0.0, sum(m[i][j] * rgb[j] for j in range(3)))) for i in range(3)]


def cielab(rgb):
    """CIELAB (D65 white, 2 degree observer) of linear sRGB."""
    r, g, b = rgb
    x = 0.4124564 * r + 0.3575761 * g + 0.1804375 * b
    y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b
    z = 0.0193339 * r + 0.1191920 * g + 0.9503041 * b
    d = 6.0 / 29.0

    def f(t):
        return t ** (1.0 / 3.0) if t > d ** 3 else t / (3.0 * d * d) + 4.0 / 29.0

    fx, fy, fz = f(x / 0.95047), f(y / 1.0), f(z / 1.08883)
    return (116.0 * fy - 16.0, 500.0 * (fx - fy), 200.0 * (fy - fz))


def ciede2000(lab1, lab2):
    """Sharma, Wu and Dalal (2005), kL = kC = kH = 1."""
    L1, a1, b1 = lab1
    L2, a2, b2 = lab2
    c1, c2 = math.hypot(a1, b1), math.hypot(a2, b2)
    cbar7 = ((c1 + c2) / 2.0) ** 7
    g = 0.5 * (1.0 - math.sqrt(cbar7 / (cbar7 + 25.0 ** 7)))
    a1p, a2p = (1.0 + g) * a1, (1.0 + g) * a2
    c1p, c2p = math.hypot(a1p, b1), math.hypot(a2p, b2)

    def hue(b, a):
        return 0.0 if a == 0.0 and b == 0.0 else math.degrees(math.atan2(b, a)) % 360.0

    h1p, h2p = hue(b1, a1p), hue(b2, a2p)
    dl = L2 - L1
    dc = c2p - c1p
    if c1p * c2p == 0.0:
        dh = 0.0
    elif abs(h2p - h1p) <= 180.0:
        dh = h2p - h1p
    elif h2p - h1p > 180.0:
        dh = h2p - h1p - 360.0
    else:
        dh = h2p - h1p + 360.0
    dH = 2.0 * math.sqrt(c1p * c2p) * math.sin(math.radians(dh / 2.0))
    lbar = (L1 + L2) / 2.0
    cbarp = (c1p + c2p) / 2.0
    if c1p * c2p == 0.0:
        hbar = h1p + h2p
    elif abs(h1p - h2p) <= 180.0:
        hbar = (h1p + h2p) / 2.0
    elif h1p + h2p < 360.0:
        hbar = (h1p + h2p + 360.0) / 2.0
    else:
        hbar = (h1p + h2p - 360.0) / 2.0
    t = (
        1.0
        - 0.17 * math.cos(math.radians(hbar - 30.0))
        + 0.24 * math.cos(math.radians(2.0 * hbar))
        + 0.32 * math.cos(math.radians(3.0 * hbar + 6.0))
        - 0.20 * math.cos(math.radians(4.0 * hbar - 63.0))
    )
    dtheta = 30.0 * math.exp(-(((hbar - 275.0) / 25.0) ** 2))
    cbarp7 = cbarp ** 7
    rc = 2.0 * math.sqrt(cbarp7 / (cbarp7 + 25.0 ** 7))
    sl = 1.0 + 0.015 * (lbar - 50.0) ** 2 / math.sqrt(20.0 + (lbar - 50.0) ** 2)
    sc = 1.0 + 0.045 * cbarp
    sh = 1.0 + 0.015 * cbarp * t
    rt = -math.sin(math.radians(2.0 * dtheta)) * rc
    return math.sqrt((dl / sl) ** 2 + (dc / sc) ** 2 + (dH / sh) ** 2 + rt * (dc / sc) * (dH / sh))


# All 34 test pairs of the table published with Sharma, Wu and Dalal (2005).
SHARMA = [
    ((50.0000, 2.6772, -79.7751), (50.0000, 0.0000, -82.7485), 2.0425),
    ((50.0000, 3.1571, -77.2803), (50.0000, 0.0000, -82.7485), 2.8615),
    ((50.0000, 2.8361, -74.0200), (50.0000, 0.0000, -82.7485), 3.4412),
    ((50.0000, -1.3802, -84.2814), (50.0000, 0.0000, -82.7485), 1.0000),
    ((50.0000, -1.1848, -84.8006), (50.0000, 0.0000, -82.7485), 1.0000),
    ((50.0000, -0.9009, -85.5211), (50.0000, 0.0000, -82.7485), 1.0000),
    ((50.0000, 0.0000, 0.0000), (50.0000, -1.0000, 2.0000), 2.3669),
    ((50.0000, -1.0000, 2.0000), (50.0000, 0.0000, 0.0000), 2.3669),
    ((50.0000, 2.4900, -0.0010), (50.0000, -2.4900, 0.0009), 7.1792),
    ((50.0000, 2.4900, -0.0010), (50.0000, -2.4900, 0.0010), 7.1792),
    ((50.0000, 2.4900, -0.0010), (50.0000, -2.4900, 0.0011), 7.2195),
    ((50.0000, 2.4900, -0.0010), (50.0000, -2.4900, 0.0012), 7.2195),
    ((50.0000, -0.0010, 2.4900), (50.0000, 0.0009, -2.4900), 4.8045),
    ((50.0000, -0.0010, 2.4900), (50.0000, 0.0010, -2.4900), 4.8045),
    ((50.0000, -0.0010, 2.4900), (50.0000, 0.0011, -2.4900), 4.7461),
    ((50.0000, 2.5000, 0.0000), (50.0000, 0.0000, -2.5000), 4.3065),
    ((50.0000, 2.5000, 0.0000), (73.0000, 25.0000, -18.0000), 27.1492),
    ((50.0000, 2.5000, 0.0000), (61.0000, -5.0000, 29.0000), 22.8977),
    ((50.0000, 2.5000, 0.0000), (56.0000, -27.0000, -3.0000), 31.9030),
    ((50.0000, 2.5000, 0.0000), (58.0000, 24.0000, 15.0000), 19.4535),
    ((50.0000, 2.5000, 0.0000), (50.0000, 3.1736, 0.5854), 1.0000),
    ((50.0000, 2.5000, 0.0000), (50.0000, 3.2972, 0.0000), 1.0000),
    ((50.0000, 2.5000, 0.0000), (50.0000, 1.8634, 0.5757), 1.0000),
    ((50.0000, 2.5000, 0.0000), (50.0000, 3.2592, 0.3350), 1.0000),
    ((60.2574, -34.0099, 36.2677), (60.4626, -34.1751, 39.4387), 1.2644),
    ((63.0109, -31.0961, -5.8663), (62.8187, -29.7946, -4.0864), 1.2630),
    ((61.2901, 3.7196, -5.3901), (61.4292, 2.2480, -4.9620), 1.8731),
    ((35.0831, -44.1164, 3.7933), (35.0232, -40.0716, 1.5901), 1.8645),
    ((22.7233, 20.0904, -46.6940), (23.0331, 14.9730, -42.5619), 2.0373),
    ((36.4612, 47.8580, 18.3852), (36.2715, 50.5065, 21.2231), 1.4146),
    ((90.8027, -2.0831, 1.4410), (91.1528, -1.6435, 0.0447), 1.4441),
    ((90.9257, -0.5406, -0.9208), (88.6381, -0.8985, -0.7239), 1.5381),
    ((6.7747, -0.2908, -2.4247), (5.8714, -0.0985, -2.2286), 0.6377),
    ((2.0776, 0.0795, -1.1350), (0.9033, -0.0636, -0.5514), 0.9082),
]
assert len(SHARMA) == 34
for _a, _b, _want in SHARMA:
    assert abs(ciede2000(_a, _b) - _want) < 1e-4, (_a, _b, ciede2000(_a, _b))
    assert abs(ciede2000(_b, _a) - _want) < 1e-4
assert abs(contrast("#000000", "#ffffff") - 21.0) < 1e-12


def de2000(hex1, hex2, vision):
    return ciede2000(cielab(simulate(hex1, vision)), cielab(simulate(hex2, vision)))


# Cue keys: the six states, and the out-of-tolerance region (its stripe pattern; its colour comes
# from UX.TOKENS.DATAPAL).
CUE_KEYS = STATES + ["out-of-tolerance"]


def cues():
    out = {s: {"glyph": "state." + s, "text": "icon.state." + s} for s in STATES}
    out["out-of-tolerance"] = {"glyph": "pattern.out-of-tolerance", "text": "icon.pattern.out-of-tolerance"}
    return out


def neutral_findings(themes):
    """`STATUS-DISTANCE <theme> <state> <other>` of a palette's themes (used to confirm labels)."""
    out = set()
    for t, row in themes.items():
        for s in NEUTRAL:
            for x in NC_COLOURED:
                if s in row and x in row and min(de2000(row[s], row[x], v) for v in VISIONS) < MIN_NEUTRAL_DE2000:
                    out.add(f"STATUS-DISTANCE {t} {s} {x}")
    return out


def hue_gap(h1, h2):
    d = abs(h1 - h2) % 360.0
    return min(d, 360.0 - d)


# ------------------------------------------------------------------ reading files


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError(f"duplicate key in {keys}")
    return dict(pairs)


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_dups)


def walk_tokens(obj, path=(), out=None):
    out = {} if out is None else out
    if "$value" in obj:
        out[".".join(path)] = obj["$value"]
        return out
    for k, v in obj.items():
        if not k.startswith("$"):
            walk_tokens(v, path + (k,), out)
    return out


def resolve_status(root):
    """theme -> sem.status.* name -> hex, by following theme bindings then aliases to a prim."""
    tokens = {}
    for p in sorted((root / "tokens").rglob("*.tokens.json")):
        for n, v in walk_tokens(load_json(p)).items():
            assert n not in tokens, n
            tokens[n] = v
    out = {}
    for t in THEMES:
        binds = load_json(root / "tokens" / "themes" / f"{t}.json")["bindings"]
        out[t] = {}
        for n in sorted(binds):
            if not n.startswith("sem.status."):
                continue
            cur = binds[n][1:-1]
            for _ in range(16):
                v = tokens[cur]
                if v.startswith("{"):
                    cur = v[1:-1]
                    continue
                assert cur.startswith("prim."), cur
                out[t][n] = v
                break
    return out, tokens


def theme_values(root, names):
    _, tokens = resolve_status(root)
    out = {}
    for t in THEMES:
        binds = load_json(root / "tokens" / "themes" / f"{t}.json")["bindings"]
        out[t] = {}
        for n in names:
            cur = binds[n][1:-1]
            while tokens[cur].startswith("{"):
                cur = tokens[cur][1:-1]
            out[t][n] = tokens[cur]
    return out


# ------------------------------------------------------------------ the shipped palette


def shipped():
    pal = load_json(DESIGN / "tokens" / "status.json")
    # Structure (restated independently of the Rust check).
    assert sorted(pal) == ["cues", "format", "formula", "locked", "min_delta_e2000", "sets", "themes"], sorted(pal)
    assert pal["format"] == "status-palette@1"
    assert pal["locked"] is True
    assert pal["formula"] == "ciede2000" and pal["min_delta_e2000"] == 20
    assert pal["sets"] == {"conforming": CONFORMING, "nonconforming": NONCONFORMING}, pal["sets"]
    assert sorted(pal["themes"]) == sorted(THEMES)
    for t in THEMES:
        assert sorted(pal["themes"][t]) == sorted(STATES), t
    # Never colour alone: one glyph and one text per state, all distinct (UX.ICONS.CLASS IDs).
    assert pal["cues"] == cues(), pal["cues"]
    assert len({c["glyph"] for c in pal["cues"].values()}) == len(CUE_KEYS)
    assert len({c["text"] for c in pal["cues"].values()}) == len(CUE_KEYS)

    # REQ-UX.TOKENS.STATUS-001: sem.status.<state> aliases exactly these values.
    resolved, _ = resolve_status(DESIGN)
    for t in THEMES:
        assert sorted(resolved[t]) == sorted("sem.status." + s for s in STATES), (t, sorted(resolved[t]))
        for s in STATES:
            assert resolved[t]["sem.status." + s] == pal["themes"][t][s], (t, s)
    # REQ-UX.TOKENS.STATUS-005: no status token in the brand tier.
    brand = load_json(DESIGN / "tokens" / "brand-tier.json")["tokens"]
    assert not [b for b in brand if b.startswith("sem.status.")], brand

    values = pal["themes"]
    distances, fails = [], []
    for t in THEMES:
        c = values[t]["conforms"]
        for s in STATES[1:]:
            for v in VISIONS:
                d = de2000(c, values[t][s], v)
                # REQ-UX.TOKENS.STATUS-004 covers the nonconforming set; the other states are held to
                # the same floor because VIZ.OVERLAY.STATUS's guard applies it to them too.
                if d < MIN_DE2000 + DE2000_MARGIN:
                    fails.append(f"{t} {s} {v}: dE2000 {d:.3f} < {MIN_DE2000} + {DE2000_MARGIN}")
                distances.append({"theme": t, "state": s, "vision": v, "de2000": round(d, 9)})

    # REQ-UX.TOKENS.STATUS-002, gating: neutral states keep their distance from the nonconforming set.
    neutral = []
    for t in THEMES:
        for s in NEUTRAL:
            for x in NC_COLOURED:
                for v in VISIONS:
                    d = de2000(values[t][s], values[t][x], v)
                    if d < MIN_NEUTRAL_DE2000 + NEUTRAL_MARGIN:
                        fails.append(f"{t} {s}/{x} {v}: dE2000 {d:.3f} < {MIN_NEUTRAL_DE2000} + {NEUTRAL_MARGIN}")
                    neutral.append({"theme": t, "state": s, "other": x, "vision": v, "de2000": round(d, 9)})

    # Hue families: advice only, recorded but never failing.
    hues, hue_advice = [], []
    for t in THEMES:
        lch = {s: oklch(values[t][s]) for s in STATES}
        fam_c = [lch[s][0] for s in CONFORMING if lch[s][1] >= ACHROMATIC]
        fam_n = [lch[s][0] for s in NONCONFORMING if s in lch and lch[s][1] >= ACHROMATIC]
        assert fam_c and fam_n, t
        for s in STATES:
            hues.append({"theme": t, "state": s, "hue": round(lch[s][0], 6), "chroma": round(lch[s][1], 9)})
        for s in ("indeterminate", "not-decidable", "not-evaluated"):
            h, ch = lch[s]
            if ch < ACHROMATIC:
                continue
            gap = min(hue_gap(h, x) for x in fam_c + fam_n)
            if gap < FAMILY_HALF_WIDTH:
                hue_advice.append(f"{t} {s}: hue {h:.1f} within {FAMILY_HALF_WIDTH} degrees of a family")

    # WCAG 2.2 contrast of every status colour as text on the plain surfaces (REQ-UX.TOKENS-002/004).
    surfaces = ["sem.surface.base", "sem.surface.raised", "sem.surface.sunken"]
    claimed = ["sem.border.value.claimed", "sem.surface.value.claimed", "sem.text.value.claimed"]
    other = theme_values(DESIGN, surfaces + claimed)
    contrasts = []
    for t in THEMES:
        need = 7.0 if t == "hc" else 4.5
        for s in STATES:
            for bg in surfaces:
                r = contrast(values[t][s], other[t][bg])
                if r < need + CONTRAST_MARGIN:
                    fails.append(f"{t} {s} on {bg}: contrast {r:.3f} < {need} + {CONTRAST_MARGIN}")
                contrasts.append({"theme": t, "state": s, "bg": bg, "ratio": round(r, 9), "required": need})
            for cl in claimed:
                d = math.dist(oklab(values[t][s]), oklab(other[t][cl]))
                if d < MIN_DE_OK + DE_OK_MARGIN:
                    fails.append(f"{t} {s} vs {cl}: OKLab {d:.4f} < {MIN_DE_OK} + {DE_OK_MARGIN}")
    if fails:
        raise SystemExit("shipped status palette fails:\n  " + "\n  ".join(fails))
    nc = [d["de2000"] for d in distances if d["state"] in NONCONFORMING]
    return {
        "exit": 0,
        "findings": [],
        "counts": {"states": 6, "themes": 4, "colours": 24},
        "values": values,
        "de2000": distances,
        "min_de2000_nonconforming": round(min(nc), 6),
        "min_de2000_all": round(min(d["de2000"] for d in distances), 6),
        "neutral_de2000": neutral,
        "min_neutral_de2000": round(min(d["de2000"] for d in neutral), 6),
        "hues": hues,
        "hue_advice": hue_advice,
        "contrasts": contrasts,
        "min_contrast_margin": round(min(c["ratio"] - c["required"] for c in contrasts), 6),
    }


# ------------------------------------------------------------------ fixtures

P = "prim.color.fix."
# Synthetic colours for the fixture trees (not the shipped palette).
FIX = {
    "light": {"conforms": "#00607a", "nonconforms": "#b01020", "indeterminate": "#002000", "not-decidable": "#000040", "not-evaluated": "#202020", "stale": "#8a4000"},
    "dark": {"conforms": "#40c0e0", "nonconforms": "#ffa0a0", "indeterminate": "#d0e000", "not-decidable": "#c0f020", "not-evaluated": "#f0f0f0", "stale": "#f08020"},
    "hc": {"conforms": "#004050", "nonconforms": "#900010", "indeterminate": "#101000", "not-decidable": "#201000", "not-evaluated": "#000000", "stale": "#603000"},
    "floor": {"conforms": "#00607a", "nonconforms": "#b01020", "indeterminate": "#002000", "not-decidable": "#000040", "not-evaluated": "#202020", "stale": "#8a4000"},
}


def prim_name(theme, state):
    return f"{P}{state}-{theme}"


def base():
    """A small valid tree: status.json plus the token files sem.status.* resolves through."""
    return {
        "status": {
            "format": "status-palette@1",
            "locked": True,
            "formula": "ciede2000",
            "min_delta_e2000": 20,
            "sets": {"conforming": list(CONFORMING), "nonconforming": list(NONCONFORMING)},
            "cues": cues(),
            "themes": copy.deepcopy(FIX),
        },
        # sem name -> theme -> prim name
        "bind": {"sem.status." + s: {t: prim_name(t, s) for t in THEMES} for s in STATES},
        "brand": [],
    }


def nest(flat):
    out = {}
    for name, tok in flat.items():
        cur = out
        segs = name.split(".")
        for s in segs[:-1]:
            cur = cur.setdefault(s, {})
        cur[segs[-1]] = tok
    for top in out.values():
        top["$type"] = "color"
    return out


def render(m):
    files = {}
    if m["status"] is not None:
        files["tokens/status.json"] = m["status"]
    prim = {prim_name(t, s): {"$value": FIX[t][s]} for t in THEMES for s in STATES}
    files["tokens/primitive/color.tokens.json"] = nest(prim)
    sem = {n: {"$value": "{" + b["light"] + "}"} for n, b in m["bind"].items() if "light" in b}
    files["tokens/semantic/color.tokens.json"] = nest(sem)
    for t in THEMES:
        files[f"tokens/themes/{t}.json"] = {
            "format": "fid-design-theme@1",
            "theme": t,
            "bindings": {n: "{" + b[t] + "}" for n, b in m["bind"].items() if t in b},
        }
    files["tokens/pairs.json"] = {"format": "fid-design-pairs@1", "pairs": []}
    files["tokens/brand-tier.json"] = {"format": "fid-design-brand-tier@1", "tokens": m["brand"]}
    return files


def mutate(fn):
    m = base()
    fn(m)
    return m


def drop(path):
    def fn(m):
        cur = m["status"]
        for k in path[:-1]:
            cur = cur[k]
        del cur[path[-1]]

    return fn


def put(path, value):
    def fn(m):
        cur = m["status"]
        for k in path[:-1]:
            cur = cur[k]
        cur[path[-1]] = value

    return fn


def both(*fns):
    def fn(m):
        for f in fns:
            f(m)

    return fn


S = "tokens/status.json"

# name -> (mutation, exit code, ["CODE subject", ...]); hand-labelled from the leaf's rules.
LABELS = {
    "ok": (lambda m: None, 0, []),
    # AC-2: no high-contrast colour for stale.
    "missing-hc": (drop(["themes", "hc", "stale"]), 1, ["STATUS-MISSING hc stale"]),
    # AC-3: locked = false.
    "unlocked": (put(["locked"], False), 1, ["STATUS-UNLOCKED " + S]),
    # A whole theme missing: one finding per state.
    "missing-theme": (drop(["themes", "floor"]), 1, [f"STATUS-MISSING floor {s}" for s in STATES]),
    # Stale moved into the conforming set: both sets are wrong.
    "stale-conforming": (
        both(put(["sets", "conforming"], ["conforms", "stale"]), put(["sets", "nonconforming"], ["nonconforms", "out-of-tolerance"])),
        1,
        ["STATUS-SET conforming", "STATUS-SET nonconforming"],
    ),
    # The nonconforming set lists a member twice and lacks out-of-tolerance.
    "set-duplicate": (put(["sets", "nonconforming"], ["nonconforms", "stale", "stale"]), 1, ["STATUS-SET nonconforming"]),
    # A status token in the brand tier (REQ-UX.TOKENS.STATUS-005).
    "brand-status": (lambda m: m.__setitem__("brand", ["sem.status.conforms"]), 1, ["STATUS-SET sem.status.conforms"]),
    # The threshold and the formula differ from the spec's defaults.
    "threshold": (
        both(put(["min_delta_e2000"], 10), put(["formula"], "cie76")),
        1,
        ["STATUS-THRESHOLD formula", "STATUS-THRESHOLD min_delta_e2000"],
    ),
    # Shape errors.
    "wrong-format": (put(["format"], "status-palette@2"), 1, ["STATUS-SCHEMA " + S]),
    "extra-key": (put(["comment"], "x"), 1, ["STATUS-SCHEMA " + S]),
    "locked-string": (put(["locked"], "true"), 1, ["STATUS-SCHEMA " + S]),
    "bad-hex": (
        both(put(["themes", "dark", "conforms"], "#40C0E0"), put(["themes", "light", "stale"], "#8a40")),
        1,
        ["STATUS-SCHEMA dark conforms", "STATUS-SCHEMA light stale"],
    ),
    "unknown-state": (put(["themes", "light", "pending"], "#123456"), 1, ["STATUS-SCHEMA light pending"]),
    "unknown-theme": (put(["themes", "print"], dict(FIX["light"])), 1, ["STATUS-SCHEMA print"]),
    "no-file": (lambda m: m.__setitem__("status", None), 1, ["STATUS-SCHEMA " + S]),
    # REQ-UX.TOKENS.STATUS-001: the semantic tokens alias exactly these values.
    "alias-mismatch": (put(["themes", "dark", "nonconforms"], "#ff0000"), 1, ["STATUS-ALIAS dark nonconforms"]),
    "alias-unbound": (lambda m: m["bind"]["sem.status.stale"].pop("hc"), 1, ["STATUS-ALIAS hc stale"]),
    "alias-extra": (
        lambda m: m["bind"].__setitem__("sem.status.pending", {t: prim_name(t, "stale") for t in THEMES}),
        1,
        ["STATUS-ALIAS sem.status.pending"],
    ),
    # Never colour alone: every state names its glyph and text.
    "cue-missing": (drop(["cues", "stale"]), 1, ["STATUS-CUE stale"]),
    "cue-wrong-glyph": (put(["cues", "indeterminate", "glyph"], "state.not-decidable"), 1, ["STATUS-CUE indeterminate"]),
    "cue-no-text": (drop(["cues", "conforms", "text"]), 1, ["STATUS-CUE conforms"]),
    "cue-no-oot": (drop(["cues", "out-of-tolerance"]), 1, ["STATUS-CUE out-of-tolerance"]),
    # REQ-UX.TOKENS.STATUS-002: indeterminate drawn in the stale colour (dark) is 0 from stale; the
    # tokens follow so that only the distance rule fires.
    "neutral-near-stale": (
        lambda m: (
            m["status"]["themes"]["dark"].__setitem__("indeterminate", FIX["dark"]["stale"]),
            m["bind"]["sem.status.indeterminate"].__setitem__("dark", prim_name("dark", "stale")),
        ),
        1,
        ["STATUS-DISTANCE dark indeterminate stale"],
    ),
}


def canon(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def all_files():
    out = {}
    expected = {"fixtures": {}}
    for name, (fn, code, findings) in sorted(LABELS.items()):
        for rel, obj in render(mutate(fn)).items():
            out[f"{name}/{rel}"] = canon(obj)
        expected["fixtures"][name] = {"exit": code, "findings": sorted(set(findings))}
        st = mutate(fn)["status"]
        if st is not None and isinstance(st.get("themes"), dict):
            want = {f for f in findings if f.startswith("STATUS-DISTANCE")}
            good = {t: {k: v for k, v in row.items() if isinstance(v, str) and len(v) == 7 and v == v.lower()} for t, row in st["themes"].items() if t in THEMES}
            assert neutral_findings(good) == want, (name, neutral_findings(good), want)
    return out, expected


def main():
    check = "--check" in sys.argv[1:]
    files, expected = all_files()
    expected["shipped"] = shipped()
    files["expected.json"] = canon(expected)
    bad = []
    for rel, text in sorted(files.items()):
        p = HERE / rel
        if check:
            if not p.exists() or p.read_bytes() != text.encode("utf-8"):
                bad.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(text.encode("utf-8"))
    if check:
        on_disk = {
            str(p.relative_to(HERE)).replace("\\", "/")
            for name in LABELS
            for p in (HERE / name).rglob("*")
            if p.is_file()
        }
        bad += sorted(on_disk - set(files))
    if bad:
        print("out of date (run make_fixtures.py):\n  " + "\n  ".join(bad), file=sys.stderr)
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
