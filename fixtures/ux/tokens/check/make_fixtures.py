#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.TOKENS.CHECK and the independent expected results.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/check/make_fixtures.py          # (re)write fixtures and expected files
    python fixtures/ux/tokens/check/make_fixtures.py --check  # fail if any file differs

Expected values, none produced by the Rust code under test:

* `expected-report.json`: the whole `color-report@1` release report of the shipped `design/`
  tree, computed by this script from the files: its own JSON reader (duplicate keys refused),
  DTCG walk and alias resolver; WCAG 2.2 relative luminance and contrast; sRGB to linear to XYZ
  (D65) to CIELAB; the colour-vision simulation of Machado, Oliveira and Fernandes (2009) at
  severity 1.0 (linear sRGB, clipped to [0, 1]); CIEDE2000 (Sharma, Wu and Dalal, 2005), checked
  against `ciede2000-sharma.csv`. Every value is rounded to 0.01 as the report does (the
  correctly rounded two-decimal form), and the script fails if any value lies within 1e-6 of a
  rounding boundary, where a last-bit difference between platforms could change the report.
* `expected.json`: per fixture the exit code and the findings, hand-labelled below (`LABELS`);
  the gate's own codes (`CONTRAST`, `CONTRAST-HC`, `STATUS-DISTANCE`) are also recomputed here
  and must agree with the labels.
* `ciede2000-sharma.csv`: the 34 test pairs published with Sharma, Wu and Dalal (2005), numbers
  only (L*, a*, b* of each colour and the expected difference).

Every fixture is a copy of the shipped `design/tokens/` tree and the palettes' originality
declarations with one mutation. This module imports no other oracle and no authoring script.
"""

import copy
import hashlib
import json
import math
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
DESIGN = ROOT / "design"

THEMES = ["light", "dark", "hc", "floor"]
VISIONS = ["normal", "protanopia", "deuteranopia", "tritanopia"]
MIN_STEP_DE = 10.0
CVD_MODEL = "Machado, Oliveira and Fernandes (2009)"
CVD_TABLE = "machado-2009-severity-1.0@1"

# ------------------------------------------------------------------ colour maths

MACHADO = {
    "protanopia": [[0.152286, 1.052583, -0.204868], [0.114503, 0.786281, 0.099216], [-0.003882, -0.048116, 1.051998]],
    "deuteranopia": [[0.367322, 0.860646, -0.227968], [0.280085, 0.672501, 0.047413], [-0.01182, 0.04294, 0.968881]],
    "tritanopia": [[1.255528, -0.076749, -0.178779], [-0.078411, 0.930809, 0.147602], [0.004733, 0.691367, 0.3039]],
}


def rgb(h):
    h = h.lower()
    assert len(h) in (7, 9) and h[0] == "#", h
    return [int(h[i : i + 2], 16) for i in (1, 3, 5)]


def to_linear(v):
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def rel_luminance(c):
    r, g, b = (to_linear(v) for v in c)
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def wcag_ratio(a, b):
    la, lb = rel_luminance(a), rel_luminance(b)
    hi, lo = (la, lb) if la >= lb else (lb, la)
    return (hi + 0.05) / (lo + 0.05)


def seen(c, vision):
    lin = [to_linear(v) for v in c]
    if vision == "normal":
        return lin
    m = MACHADO[vision]
    return [min(1.0, max(0.0, sum(m[i][j] * lin[j] for j in range(3)))) for i in range(3)]


def lab(lin):
    x = (0.4124564 * lin[0] + 0.3575761 * lin[1] + 0.1804375 * lin[2]) / 0.95047
    y = 0.2126729 * lin[0] + 0.7151522 * lin[1] + 0.0721750 * lin[2]
    z = (0.0193339 * lin[0] + 0.1191920 * lin[1] + 0.9503041 * lin[2]) / 1.08883
    eps = 216.0 / 24389.0
    kappa = 24389.0 / 27.0

    def f(t):
        return t ** (1.0 / 3.0) if t > eps else (kappa * t + 16.0) / 116.0

    return (116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z)))


def de2000(p, q):
    L1, a1, b1 = p
    L2, a2, b2 = q
    cm = (math.sqrt(a1 * a1 + b1 * b1) + math.sqrt(a2 * a2 + b2 * b2)) / 2.0
    g = 0.5 * (1.0 - math.sqrt(cm ** 7 / (cm ** 7 + 6103515625.0)))
    ap1, ap2 = (1.0 + g) * a1, (1.0 + g) * a2
    cp1, cp2 = math.sqrt(ap1 * ap1 + b1 * b1), math.sqrt(ap2 * ap2 + b2 * b2)

    def hp(b, a):
        if a == 0.0 and b == 0.0:
            return 0.0
        h = math.degrees(math.atan2(b, a))
        return h + 360.0 if h < 0.0 else h

    hp1, hp2 = hp(b1, ap1), hp(b2, ap2)
    if cp1 * cp2 == 0.0:
        dhp = 0.0
        hbar = hp1 + hp2
    else:
        dhp = hp2 - hp1
        if dhp > 180.0:
            dhp -= 360.0
        elif dhp < -180.0:
            dhp += 360.0
        if abs(hp1 - hp2) <= 180.0:
            hbar = (hp1 + hp2) / 2.0
        elif hp1 + hp2 < 360.0:
            hbar = (hp1 + hp2 + 360.0) / 2.0
        else:
            hbar = (hp1 + hp2 - 360.0) / 2.0
    dL, dC = L2 - L1, cp2 - cp1
    dH = 2.0 * math.sqrt(cp1 * cp2) * math.sin(math.radians(dhp / 2.0))
    lbar, cbar = (L1 + L2) / 2.0, (cp1 + cp2) / 2.0
    t = (
        1.0
        - 0.17 * math.cos(math.radians(hbar - 30.0))
        + 0.24 * math.cos(math.radians(2.0 * hbar))
        + 0.32 * math.cos(math.radians(3.0 * hbar + 6.0))
        - 0.20 * math.cos(math.radians(4.0 * hbar - 63.0))
    )
    d_theta = 30.0 * math.exp(-(((hbar - 275.0) / 25.0) ** 2))
    rc = 2.0 * math.sqrt(cbar ** 7 / (cbar ** 7 + 6103515625.0))
    sl = 1.0 + (0.015 * (lbar - 50.0) ** 2) / math.sqrt(20.0 + (lbar - 50.0) ** 2)
    sc = 1.0 + 0.045 * cbar
    sh = 1.0 + 0.015 * cbar * t
    rt = -math.sin(math.radians(2.0 * d_theta)) * rc
    u, v, w = dL / sl, dC / sc, dH / sh
    return math.sqrt(u * u + v * v + w * w + rt * v * w)


def de_under(a, b, vision):
    return de2000(lab(seen(a, vision)), lab(seen(b, vision)))


def lstar(c):
    return lab(seen(c, "normal"))[0]


SHARMA = [
    (50.0000, 2.6772, -79.7751, 50.0000, 0.0000, -82.7485, 2.0425),
    (50.0000, 3.1571, -77.2803, 50.0000, 0.0000, -82.7485, 2.8615),
    (50.0000, 2.8361, -74.0200, 50.0000, 0.0000, -82.7485, 3.4412),
    (50.0000, -1.3802, -84.2814, 50.0000, 0.0000, -82.7485, 1.0000),
    (50.0000, -1.1848, -84.8006, 50.0000, 0.0000, -82.7485, 1.0000),
    (50.0000, -0.9009, -85.5211, 50.0000, 0.0000, -82.7485, 1.0000),
    (50.0000, 0.0000, 0.0000, 50.0000, -1.0000, 2.0000, 2.3669),
    (50.0000, -1.0000, 2.0000, 50.0000, 0.0000, 0.0000, 2.3669),
    (50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0009, 7.1792),
    (50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0010, 7.1792),
    (50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0011, 7.2195),
    (50.0000, 2.4900, -0.0010, 50.0000, -2.4900, 0.0012, 7.2195),
    (50.0000, -0.0010, 2.4900, 50.0000, 0.0009, -2.4900, 4.8045),
    (50.0000, -0.0010, 2.4900, 50.0000, 0.0010, -2.4900, 4.8045),
    (50.0000, -0.0010, 2.4900, 50.0000, 0.0011, -2.4900, 4.7461),
    (50.0000, 2.5000, 0.0000, 50.0000, 0.0000, -2.5000, 4.3065),
    (50.0000, 2.5000, 0.0000, 73.0000, 25.0000, -18.0000, 27.1492),
    (50.0000, 2.5000, 0.0000, 61.0000, -5.0000, 29.0000, 22.8977),
    (50.0000, 2.5000, 0.0000, 56.0000, -27.0000, -3.0000, 31.9030),
    (50.0000, 2.5000, 0.0000, 58.0000, 24.0000, 15.0000, 19.4535),
    (50.0000, 2.5000, 0.0000, 50.0000, 3.1736, 0.5854, 1.0000),
    (50.0000, 2.5000, 0.0000, 50.0000, 3.2972, 0.0000, 1.0000),
    (50.0000, 2.5000, 0.0000, 50.0000, 1.8634, 0.5757, 1.0000),
    (50.0000, 2.5000, 0.0000, 50.0000, 3.2592, 0.3350, 1.0000),
    (60.2574, -34.0099, 36.2677, 60.4626, -34.1751, 39.4387, 1.2644),
    (63.0109, -31.0961, -5.8663, 62.8187, -29.7946, -4.0864, 1.2630),
    (61.2901, 3.7196, -5.3901, 61.4292, 2.2480, -4.9620, 1.8731),
    (35.0831, -44.1164, 3.7933, 35.0232, -40.0716, 1.5901, 1.8645),
    (22.7233, 20.0904, -46.6940, 23.0331, 14.9730, -42.5619, 2.0373),
    (36.4612, 47.8580, 18.3852, 36.2715, 50.5065, 21.2231, 1.4146),
    (90.8027, -2.0831, 1.4410, 91.1528, -1.6435, 0.0447, 1.4441),
    (90.9257, -0.5406, -0.9208, 88.6381, -0.8985, -0.7239, 1.5381),
    (6.7747, -0.2908, -2.4247, 5.8714, -0.0985, -2.2286, 0.6377),
    (2.0776, 0.0795, -1.1350, 0.9033, -0.0636, -0.5514, 0.9082),
]
for _r in SHARMA:
    assert abs(de2000(_r[0:3], _r[3:6]) - _r[6]) < 1e-4, _r
    assert abs(de2000(_r[3:6], _r[0:3]) - _r[6]) < 1e-4, _r

# ------------------------------------------------------------------ rounding


def r2(x):
    """The report's rounding: the correctly rounded two-decimal form, read back."""
    scaled = x * 100.0
    frac = scaled - math.floor(scaled)
    assert abs(frac - 0.5) > 1e-6, f"{x} lies on a rounding boundary"
    return float("%.2f" % x)


# ------------------------------------------------------------------ reading a tree


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError(f"duplicate key in {keys}")
    return dict(pairs)


def read(files, rel):
    return json.loads(files[rel], object_pairs_hook=no_dups)


def tree_files(root):
    """Every file a fixture copies, as relative path -> text."""
    out = {}
    for base in ("tokens", "review/origin/tokens/palettes"):
        for p in sorted((root / base).rglob("*")):
            if p.is_file():
                out[str(p.relative_to(root)).replace("\\", "/")] = p.read_text(encoding="utf-8")
    return out


def tokens_of(files):
    flat = {}

    def walk(o, path):
        if "$value" in o:
            flat[".".join(path)] = o["$value"]
            return
        for k, v in o.items():
            if not k.startswith("$") and isinstance(v, dict):
                walk(v, path + [k])

    for rel in sorted(files):
        if rel.endswith(".tokens.json"):
            walk(read(files, rel), [])
    return flat


def resolve(files):
    """theme -> semantic name -> literal colour."""
    flat = tokens_of(files)
    out = {}
    for t in THEMES:
        binds = read(files, f"tokens/themes/{t}.json")["bindings"]
        row = {}
        for name, target in binds.items():
            cur = target[1:-1]
            for _ in range(32):
                v = flat.get(cur)
                if v is None or not v.startswith("{"):
                    break
                cur = v[1:-1]
            if v is not None and cur.startswith("prim."):
                row[name] = v
        out[t] = row
    return out


def required(tag, theme):
    if tag == "non-text":
        return 3.0
    if theme == "hc":
        return 7.0
    return 4.5 if tag == "text" else 3.0


def composite(fg, alpha, bg):
    return [(alpha * f + (255 - alpha) * b + 127) // 255 for f, b in zip(fg, bg)]


def report(files):
    """The gate's own part of the report: pairs, status distances, stepped palettes, findings."""
    sem = resolve(files)
    pairs = read(files, "tokens/pairs.json")["pairs"]
    findings = []
    out_pairs = []
    for t in THEMES:
        for p in pairs:
            fg, bg, tag = p["fg"], p["bg"], p["tag"]
            need = required(tag, t)
            a, b = sem[t].get(fg), sem[t].get(bg)
            assert a is not None and b is not None, (t, fg, bg)
            assert len(b) == 7, "no fixture has a background with alpha"
            ca = composite(rgb(a), int(a[7:9], 16), rgb(b)) if len(a) == 9 else rgb(a)
            ratio = wcag_ratio(ca, rgb(b))
            if ratio < need:
                code = "CONTRAST-HC" if t == "hc" and tag != "non-text" else "CONTRAST"
                findings.append(f"{code} {t} {fg}/{bg} {'%.2f' % ratio}")
            out_pairs.append(
                {"theme": t, "fg": fg, "bg": bg, "tag": tag, "ratio": r2(ratio), "required": need, "pass": ratio >= need}
            )
    st = read(files, "tokens/status.json")
    need = st["min_delta_e2000"]
    out_status = []
    for t in THEMES:
        row = st["themes"][t]
        for s in st["sets"]["nonconforming"]:
            if s not in row:
                continue
            for c in st["sets"]["conforming"]:
                for v in VISIONS:
                    d = de_under(rgb(row[s]), rgb(row[c]), v)
                    if d < need:
                        findings.append(f"STATUS-DISTANCE {v} {t} {s}")
                    out_status.append(
                        {"theme": t, "state": s, "other": c, "vision": v, "de2000": r2(d), "required": need, "pass": d >= need}
                    )
    cat = read(files, "tokens/palettes/catalogue.json")
    out_pals = []
    for ref in sorted(cat["palettes"]):
        p = read(files, f"tokens/palettes/{ref}.json")
        if p["kind"] != "stepped":
            continue
        cols = [rgb(h) for h in p["colors"]]
        ls = [lstar(c) for c in cols]
        n = len(ls)
        mid = (n - 1) // 2

        def strict(xs):
            d = [y - x for x, y in zip(xs, xs[1:])]
            return all(e > 0 for e in d) or all(e < 0 for e in d)

        mono = n % 2 == 1 and strict(ls[mid::-1]) and strict(ls[mid:])
        adj = []
        for i in range(n - 1):
            for v in VISIONS:
                d = de_under(cols[i], cols[i + 1], v)
                adj.append({"step": i, "vision": v, "de2000": r2(d), "pass": d >= MIN_STEP_DE})
        out_pals.append(
            {
                "ref": ref,
                "default": cat["default"] == ref,
                "lstar": [r2(x) for x in ls],
                "lstar_monotonic": mono,
                "adjacent": adj,
                "required": MIN_STEP_DE,
            }
        )
    return out_pairs, out_status, out_pals, sorted(set(findings))


def table_digest():
    text = json.dumps(MACHADO, sort_keys=True, separators=(",", ":"))
    return "sha256:" + hashlib.sha256(text.encode("ascii")).hexdigest()


def shipped_report():
    files = tree_files(DESIGN)
    pairs, status, pals, findings = report(files)
    assert findings == [], findings
    # Margins: every shipped value clears its threshold by more than the 0.01 rounding.
    for p in pairs:
        assert p["ratio"] - p["required"] >= 0.02, p
    for s in status:
        assert s["de2000"] - s["required"] >= 0.02, s
    for p in pals:
        assert p["lstar_monotonic"], p["ref"]
        for a in p["adjacent"]:
            assert a["de2000"] - MIN_STEP_DE >= 0.02, (p["ref"], a)
    return {
        "format": "color-report@1",
        "wcag": "2.2",
        "formula": "ciede2000 (Sharma, Wu and Dalal 2005)",
        "cvd": {
            "model": CVD_MODEL,
            "table": CVD_TABLE,
            "table_digest": table_digest(),
            "severity": 1,
            "visions": VISIONS,
        },
        "pairs": pairs,
        "status": status,
        "palettes": pals,
        "gates": {"palettes-check": 0, "status-check": 0, "tokens-check": 0},
        "findings": [],
        "ok": True,
    }


# ------------------------------------------------------------------ fixtures


def dump(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def edit_json(rel, fn):
    def go(files):
        obj = read(files, rel)
        fn(obj)
        files[rel] = dump(obj)

    return go


def chain(*fns):
    def go(files):
        for f in fns:
            f(files)

    return go


def add_prim(hue, step, value):
    def fn(o):
        o["prim"]["color"].setdefault(hue, {})[step] = {"$value": value}

    return edit_json("tokens/primitive/color.tokens.json", fn)


def bind(theme, name, target):
    def fn(o):
        o["bindings"][name] = "{" + target + "}"

    return edit_json(f"tokens/themes/{theme}.json", fn)


def sem_default(path, target):
    def fn(o):
        cur = o
        for k in path[:-1]:
            cur = cur[k]
        cur[path[-1]]["$value"] = "{" + target + "}"

    return edit_json("tokens/semantic/color.tokens.json", fn)


def status_colour(theme, state, value):
    def fn(o):
        o["themes"][theme][state] = value

    return edit_json("tokens/status.json", fn)


LOW = "#767d7d"  # 4.20:1 on white
HC6 = "#636363"  # 6.01:1 on white
CLASH = "#a896b4"  # 11.8 from the dark theme's CONFORMS under simulated deuteranopia, 9.8 under protanopia

PAIR = "sem.text.muted/sem.surface"

# name -> (mutation, exit code, ["CODE subject", ...]); hand-labelled from the leaf's rules.
LABELS = {
    "ok": (lambda files: None, 0, []),
    # AC-2: the light theme's muted text at 4.20:1 on the base surface (lower on raised and sunken).
    "low-contrast": (
        chain(
            add_prim("neutral", "650", LOW),
            bind("light", "sem.text.muted", "prim.color.neutral.650"),
            sem_default(["sem", "text", "muted"], "prim.color.neutral.650"),
        ),
        1,
        [
            f"CONTRAST light {PAIR}.base 4.20",
            f"CONTRAST light {PAIR}.raised 3.91",
            f"CONTRAST light {PAIR}.sunken 3.58",
        ],
    ),
    # AC-3: the high-contrast theme's muted text at about 6:1.
    "hc-6-to-1": (
        chain(add_prim("neutral", "750", HC6), bind("hc", "sem.text.muted", "prim.color.neutral.750")),
        1,
        [
            f"CONTRAST-HC hc {PAIR}.base 6.01",
            f"CONTRAST-HC hc {PAIR}.raised 6.01",
            f"CONTRAST-HC hc {PAIR}.sunken 5.59",
        ],
    ),
    # AC-4: the dark theme's NONCONFORMS a lavender that normal vision tells from the azure
    # CONFORMS (36) but simulated deuteranopia (11.8) and protanopia (9.8) do not.
    "deutan-clash": (
        chain(
            add_prim("lavender", "400", CLASH),
            bind("dark", "sem.status.nonconforms", "prim.color.lavender.400"),
            status_colour("dark", "nonconforms", CLASH),
        ),
        1,
        ["STATUS-DISTANCE deuteranopia dark nonconforms", "STATUS-DISTANCE protanopia dark nonconforms"],
    ),
}

OWN = ("CONTRAST ", "CONTRAST-HC ", "STATUS-DISTANCE ")


def main():
    check = "--check" in sys.argv[1:]
    base = tree_files(DESIGN)
    files = {}
    expected = {"fixtures": {}}
    for name, (fn, code, findings) in sorted(LABELS.items()):
        tree = dict(base)
        fn(tree)
        for rel, text in tree.items():
            files[f"{name}/{rel}"] = text
        got = report(tree)[3]
        own = sorted(f for f in findings if f.startswith(OWN))
        assert got == own, (name, got, own)
        expected["fixtures"][name] = {"exit": code, "findings": sorted(set(findings))}
    files["expected.json"] = dump(expected)
    files["expected-report.json"] = dump(shipped_report())
    files["ciede2000-sharma.csv"] = "l1,a1,b1,l2,a2,b2,de2000\n" + "".join(
        ",".join("%.4f" % x for x in row) + "\n" for row in SHARMA
    )
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
