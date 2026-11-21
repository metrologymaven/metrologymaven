#!/usr/bin/env python3
"""Writes the fixtures of spec node VIZ.OVERLAY.STATUS and the independent expected results.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/viz/status-matrix/make_fixtures.py          # (re)write the fixtures
    python fixtures/viz/status-matrix/make_fixtures.py --check  # fail if any file differs

Expected values, none produced by the Rust code under test:

* `expected.json` `matrix`: the render spec (`status-render@1`) of every display state x
  provenance x theme (6 x 5 x 4), written from the leaf's hand-labelled tables below (glyph,
  pattern, message key and English text per state; badge per provenance) and the colours this
  script reads from the shipped `design/tokens/status.json` (UX.TOKENS.STATUS) and
  `design/tokens/palettes/` (UX.TOKENS.DATAPAL), never from the Rust tables.
* `expected.json` `cases`: claimed values, GD&T results with each interpretation, and the
  out-of-range region cues, hand-labelled.
* `expected.json` `skins`: the guard verdict of every skin fixture, hand-labelled and
  recomputed here with this script's own CIEDE2000 (Sharma, Wu and Dalal, 2005) under normal
  vision and the Machado, Oliveira and Fernandes (2009) dichromacies at severity 1.0.
* `expected.json` `patterns`: the pattern definitions of the leaf's design.

This module imports no other oracle and no authoring script.
"""

import hashlib
import json
import math
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
VIZ = HERE.parent
ROOT = HERE.parents[2]
DESIGN = ROOT / "design"

THEMES = ["light", "dark", "hc", "floor"]
STATES = ["conforms", "nonconforms", "indeterminate", "not-decidable", "not-evaluated", "stale"]
UPPER = {s: s.upper().replace("-", "_") for s in STATES}
NEUTRAL = ["indeterminate", "not-decidable", "not-evaluated"]
SLOTS = STATES + ["under-tolerance", "over-tolerance"]
SLOT_UPPER = dict(UPPER, **{"under-tolerance": "UNDER_TOLERANCE", "over-tolerance": "OVER_TOLERANCE"})
PROVENANCES = ["computed", "measured", "entered", "reported-by-source", "suggested"]
CLAIMS = {"entered", "reported-by-source", "suggested"}
VISIONS = ["normal", "protanopia", "deuteranopia", "tritanopia"]
MIN_DE = 20.0
MIN_NEUTRAL_DE = 10.0
MIN_STALE_NORMAL_DE = 10.0  # stale vs nonconforms under normal vision (maintainer, PR 355)

# Hand-labelled from the leaf's design.
TEXT = {
    "conforms": "Conforms",
    "nonconforms": "Does not conform",
    "indeterminate": "Indeterminate",
    "not-decidable": "Not decidable",
    "not-evaluated": "Not evaluated",
    "stale": "Stale",
}
BADGE_TEXT = {
    "computed": "Computed",
    "measured": "Measured",
    "entered": "Entered",
    "reported-by-source": "Reported by source",
    "suggested": "Suggested",
}
PATTERNS = {
    "pattern.conforms": {"kind": "solid"},
    "pattern.nonconforms": {"kind": "cross", "period": 6, "width": 1},
    "pattern.indeterminate": {"kind": "dots", "period": 6, "radius": 1},
    "pattern.not-decidable": {"kind": "hatch", "angle": 90, "period": 6, "width": 1, "dash": 0},
    "pattern.not-evaluated": {"kind": "hatch", "angle": 0, "period": 8, "width": 1, "dash": 0},
    "pattern.stale": {"kind": "hatch", "angle": 45, "period": 8, "width": 1, "dash": 3},
    "pattern.out-of-tolerance": {"kind": "cross", "period": 4, "width": 2},
    "pattern.under-tolerance": {"kind": "hatch", "angle": 135, "period": 4, "width": 2, "dash": 0, "refines": "pattern.out-of-tolerance"},
    "pattern.over-tolerance": {"kind": "hatch", "angle": 45, "period": 4, "width": 2, "dash": 0, "refines": "pattern.out-of-tolerance"},
}
assert len({json.dumps({k: v for k, v in p.items() if k != "refines"}, sort_keys=True) for p in PATTERNS.values()}) == 9
NUMBER = 1.25

# ------------------------------------------------------------------ colour maths

MACHADO = {
    "protanopia": ((0.152286, 1.052583, -0.204868), (0.114503, 0.786281, 0.099216), (-0.003882, -0.048116, 1.051998)),
    "deuteranopia": ((0.367322, 0.860646, -0.227968), (0.280085, 0.672501, 0.047413), (-0.01182, 0.04294, 0.968881)),
    "tritanopia": ((1.255528, -0.076749, -0.178779), (-0.078411, 0.930809, 0.147602), (0.004733, 0.691367, 0.3039)),
}


def rgb(h):
    assert len(h) == 7 and h[0] == "#" and h == h.lower(), h
    return [int(h[i : i + 2], 16) for i in (1, 3, 5)]


def lin(v):
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def lab(h, vision):
    x = [lin(v) for v in rgb(h)]
    if vision != "normal":
        m = MACHADO[vision]
        x = [min(1.0, max(0.0, sum(m[i][j] * x[j] for j in range(3)))) for i in range(3)]
    X = (0.4124564 * x[0] + 0.3575761 * x[1] + 0.1804375 * x[2]) / 0.95047
    Y = 0.2126729 * x[0] + 0.7151522 * x[1] + 0.0721750 * x[2]
    Z = (0.0193339 * x[0] + 0.1191920 * x[1] + 0.9503041 * x[2]) / 1.08883
    f = lambda t: t ** (1 / 3) if t > (6 / 29) ** 3 else t * 841 / 108 + 4 / 29
    return (116 * f(Y) - 16, 500 * (f(X) - f(Y)), 200 * (f(Y) - f(Z)))


def de2000(p, q):
    L1, a1, b1 = p
    L2, a2, b2 = q
    cm = (math.hypot(a1, b1) + math.hypot(a2, b2)) / 2
    g = 0.5 * (1 - math.sqrt(cm ** 7 / (cm ** 7 + 25 ** 7)))
    a1, a2 = a1 * (1 + g), a2 * (1 + g)
    c1, c2 = math.hypot(a1, b1), math.hypot(a2, b2)
    h1 = math.degrees(math.atan2(b1, a1)) % 360 if (a1, b1) != (0, 0) else 0.0
    h2 = math.degrees(math.atan2(b2, a2)) % 360 if (a2, b2) != (0, 0) else 0.0
    if c1 * c2 == 0:
        dh, hb = 0.0, h1 + h2
    else:
        dh = h2 - h1
        dh = dh - 360 if dh > 180 else dh + 360 if dh < -180 else dh
        hb = (h1 + h2) / 2 if abs(h1 - h2) <= 180 else (h1 + h2 + 360) / 2 if h1 + h2 < 360 else (h1 + h2 - 360) / 2
    lb, cb = (L1 + L2) / 2, (c1 + c2) / 2
    t = 1 - 0.17 * math.cos(math.radians(hb - 30)) + 0.24 * math.cos(math.radians(2 * hb)) + 0.32 * math.cos(math.radians(3 * hb + 6)) - 0.2 * math.cos(math.radians(4 * hb - 63))
    sl = 1 + 0.015 * (lb - 50) ** 2 / math.sqrt(20 + (lb - 50) ** 2)
    sc, sh = 1 + 0.045 * cb, 1 + 0.015 * cb * t
    rt = -2 * math.sqrt(cb ** 7 / (cb ** 7 + 25 ** 7)) * math.sin(math.radians(60 * math.exp(-(((hb - 275) / 25) ** 2))))
    x, y, z = (L2 - L1) / sl, (c2 - c1) / sc, 2 * math.sqrt(c1 * c2) * math.sin(math.radians(dh / 2)) / sh
    return math.sqrt(x * x + y * y + z * z + rt * y * z)


assert abs(de2000((50, 2.6772, -79.7751), (50, 0, -82.7485)) - 2.0425) < 1e-4
assert abs(de2000((50, 2.5, 0), (73, 25, -18)) - 27.1492) < 1e-4
assert abs(de2000((2.0776, 0.0795, -1.135), (0.9033, -0.0636, -0.5514)) - 0.9082) < 1e-4


def de_min(a, b):
    return min(de2000(lab(a, v), lab(b, v)) for v in VISIONS)


# ------------------------------------------------------------------ the locked palette


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    assert len(keys) == len(set(keys)), keys
    return dict(pairs)


def load(p):
    return json.loads(p.read_text(encoding="utf-8"), object_pairs_hook=no_dups)


def locked():
    status = load(DESIGN / "tokens/status.json")
    assert status["min_delta_e2000"] == MIN_DE
    cat = load(DESIGN / "tokens/palettes/catalogue.json")
    ends = {(p["under"], p["over"]) for p in (load(DESIGN / f"tokens/palettes/{r}.json") for r in cat["palettes"])}
    assert len(ends) == 1, ends  # one out-of-range pair shared by every palette
    under, over = ends.pop()
    return {t: dict(status["themes"][t], **{"under-tolerance": under, "over-tolerance": over}) for t in THEMES}


def de_normal(a, b):
    return de2000(lab(a, "normal"), lab(b, "normal"))


LOCKED_PAL = None  # set in main()


def violation(pal):
    """The guard's first violation, as (slot, theme), in theme then slot order. Distances to
    CONFORMS are taken from the locked CONFORMS."""
    for t in THEMES:
        row = pal[t]
        conforms = LOCKED_PAL[t]["conforms"]
        if row["conforms"] != conforms:
            return "conforms", t
        for slot in SLOTS[1:]:
            if de_min(row[slot], conforms) < MIN_DE:
                return slot, t
            if slot in NEUTRAL and any(de_min(row[slot], row[x]) < MIN_NEUTRAL_DE for x in ("nonconforms", "stale")):
                return slot, t
            if slot == "stale" and (row[slot] == row["nonconforms"] or de_normal(row[slot], row["nonconforms"]) < MIN_STALE_NORMAL_DE):
                return slot, t
    return None


# ------------------------------------------------------------------ render specs


def render(theme, state, colours, provenance=None, claimed=False, declared=False, uncharacterized=False, interpretation=None):
    """The render spec, hand-labelled from the leaf's rules: any evidence fact or a claim
    provenance gives the claimed role and the word "claimed"; each of declared and
    uncharacterized input adds its own label."""
    styled = claimed or declared or uncharacterized or provenance in CLAIMS
    value = None
    if provenance is not None:
        value = {
            "number": NUMBER,
            "badge": {
                "provenance": provenance,
                "glyph": f"badge.{provenance}",
                "text_key": f"icon.badge.{provenance}",
                "text": BADGE_TEXT[provenance],
            },
            "role": "sem.text.value.claimed" if styled else "sem.text.value.measured",
        }
    labels = []
    if styled:
        labels.append({"key": "value.claimed", "text": "claimed"})
    if declared:
        labels.append({"key": "label.declared-input", "text": "declared input"})
    if uncharacterized:
        labels.append({"key": "label.uncharacterized-input", "text": "uncharacterized input"})
    interp = None
    if interpretation == "provisional":
        interp = {"provisional": True, "label_key": "gdt.interpretation.provisional", "text": "interpretation: provisional", "cue": "badge.provisional"}
    elif interpretation == "signed_off":
        interp = {"provisional": False, "label_key": None, "text": "interpretation: signed_off", "cue": None}
    return {
        "format": "status-render@1",
        "theme": theme,
        "state": state,
        "glyph": f"state.{state}",
        "pattern": f"pattern.{state}",
        "text_key": f"icon.state.{state}",
        "text": TEXT[state],
        "color": colours[theme][state],
        "value": value,
        "labels": labels,
        "interpretation": interp,
    }


def digest(obj):
    """sha256: of the RFC 8785 form (ASCII strings, the number 1.25, sorted keys, no spaces)."""
    text = json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    return "sha256:" + hashlib.sha256(text.encode("ascii")).hexdigest()


# ------------------------------------------------------------------ skins

AZURE = None  # set from the locked light CONFORMS


def skin(themes):
    return {"format": "status-skin@1", "themes": themes}


def skins(pal):
    light = pal["light"]
    return {
        # AC-2: NONCONFORMS drawn in the conforming azure.
        "skin-azure-nonconform.json": (skin({"light": {"nonconforms": light["conforms"]}}), ["NONCONFORMS", "light"]),
        # AC-3: INDETERMINATE a lighter azure close to CONFORMS (CIEDE2000 below 20, not equal).
        "skin-near-azure-indeterminate.json": (skin({"light": {"indeterminate": "#1a7590"}}), ["INDETERMINATE", "light"]),
        # The over-range region colour drawn in a teal near the dark theme's CONFORMS.
        "skin-azure-over-tolerance.json": (skin({"dark": {"over-tolerance": "#1fa8cc"}}), ["OVER_TOLERANCE", "dark"]),
        # REQ-003: stale drawn in the nonconforms colour (both far from CONFORMS).
        "skin-stale-as-nonconforms.json": (skin({"hc": {"stale": pal["hc"]["nonconforms"]}}), ["STALE", "hc"]),
        # REQ-003: not-evaluated drawn in the stale colour.
        "skin-not-evaluated-as-stale.json": (skin({"floor": {"not-evaluated": pal["floor"]["stale"]}}), ["NOT_EVALUATED", "floor"]),
        # The reviewer's inversion (PR 355): CONFORMS red and NONCONFORMS azure. Skins may not set
        # CONFORMS at all, so the file is refused as SkinSchema.
        "skin-inversion.json": (
            skin({"light": {"conforms": light["nonconforms"], "nonconforms": light["conforms"]}}),
            "schema",
        ),
        # REQ-003: stale a red-orange within 10 of NONCONFORMS under normal vision.
        "skin-stale-near-nonconforms.json": (skin({"light": {"stale": "#b9361f"}}), ["STALE", "light"]),
        # A valid skin: a slightly darker NONCONFORMS red in light, which every rule still allows.
        "skin-darker-red.json": (skin({"light": {"nonconforms": "#b82628"}}), None),
    }


def canon(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    global LOCKED_PAL
    pal = locked()
    LOCKED_PAL = pal
    assert violation(pal) is None, violation(pal)
    for t in THEMES:
        assert round(de_normal(pal[t]["stale"], pal[t]["nonconforms"]), 1) >= 18.0, t  # 18.0 (hc) to 19.3
    matrix = []
    for s in STATES:
        for p in PROVENANCES:
            for t in THEMES:
                matrix.append({"state": s, "provenance": p, "theme": t, "render": render(t, s, pal, provenance=p)})
    cases = {
        "claimed-computed": render("light", "conforms", pal, provenance="computed", claimed=True),
        "status-only": render("dark", "stale", pal),
        "gdt-provisional": render("hc", "nonconforms", pal, provenance="computed", interpretation="provisional"),
        "gdt-signed-off": render("floor", "conforms", pal, provenance="measured", interpretation="signed_off"),
        # The scene status `kernel:INDETERMINATE:-du`: declared and uncharacterized input.
        "evidence-indeterminate-du": render("light", "indeterminate", pal, provenance="computed", declared=True, uncharacterized=True),
        # `kernel:CONFORMS:-d-` without a number: the labels still show.
        "evidence-declared-status-only": render("dark", "conforms", pal, declared=True),
        # `kernel:NOT_DECIDABLE(u_missing):---` with an entered number.
        "not-decidable-entered": render("hc", "not-decidable", pal, provenance="entered"),
    }
    digests = [
        {"state": m["state"], "provenance": m["provenance"], "theme": m["theme"], "digest": digest(m["render"])}
        for m in matrix
        if (m["state"], m["provenance"], m["theme"]) in {("conforms", "measured", "light"), ("stale", "suggested", "dark"), ("nonconforms", "computed", "hc")}
    ]
    assert len(digests) == 3
    regions = {
        t: {
            "under": {"pattern": "pattern.under-tolerance", "text_key": "icon.pattern.under-tolerance", "text": "Below tolerance", "color": pal[t]["under-tolerance"]},
            "over": {"pattern": "pattern.over-tolerance", "text_key": "icon.pattern.over-tolerance", "text": "Above tolerance", "color": pal[t]["over-tolerance"]},
        }
        for t in THEMES
    }
    files = {}
    skin_expect = {}
    for name, (obj, want) in skins(pal).items():
        if want == "schema":
            assert any("conforms" in row for row in obj["themes"].values()), name
            skin_expect[name] = {"schema": True, "violation": None}
            files[f"../{name}"] = canon(obj)
            continue
        merged = {t: dict(pal[t]) for t in THEMES}
        for t, row in obj["themes"].items():
            merged[t].update(row)
        got = violation(merged)
        got = [SLOT_UPPER[got[0]], got[1]] if got else None
        assert got == want, (name, got, want)
        skin_expect[name] = {"schema": False, "violation": want}
        files[f"../{name}"] = canon(obj)
    files["expected.json"] = canon(
        {
            "locked": pal,
            "matrix": matrix,
            "cases": cases,
            "digests": digests,
            "regions": regions,
            "skins": skin_expect,
            "patterns": PATTERNS,
            "guard": {"min_delta_e2000": MIN_DE, "min_neutral_delta_e2000": MIN_NEUTRAL_DE, "min_stale_delta_e2000_normal": MIN_STALE_NORMAL_DE},
        }
    )
    bad = []
    for rel, text in sorted(files.items()):
        p = (HERE / rel).resolve()
        if check:
            if not p.exists() or p.read_bytes() != text.encode("utf-8"):
                bad.append(rel)
        else:
            p.write_bytes(text.encode("utf-8"))
    if bad:
        print("out of date (run make_fixtures.py):\n  " + "\n  ".join(bad), file=sys.stderr)
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
