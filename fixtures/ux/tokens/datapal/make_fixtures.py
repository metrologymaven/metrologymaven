#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.TOKENS.DATAPAL and the independent expected results.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/datapal/make_fixtures.py          # (re)write fixtures and expected.json
    python fixtures/ux/tokens/datapal/make_fixtures.py --check  # fail if any file differs

Expected values, none produced by the Rust code under test:

* The structural findings of every error-path fixture (`PAL-SCHEMA`, `PAL-STEPS`, `PAL-CENTRE`
  index, `PAL-SAMPLES`, `PAL-LICENCE`, `PAL-DEFAULT`, `PAL-IMMUTABLE`) are hand-labelled below
  (`LABELS`) from the leaf's rules, next to the one mutation each fixture makes to a copy of the
  shipped palettes. The perceptual findings (`PAL-LIGHTNESS`, `PAL-DELTA`, `PAL-OOT`,
  `PAL-CONTRAST`, `PAL-CLAIMED`, neutral centre) are hand-labelled too, and this script
  recomputes them with its own colour maths and fails if a label disagrees.
* For the shipped `design/tokens/palettes/`, this script reads the files and the token tree
  itself (`json` with a duplicate-key hook, its own DTCG walk and alias resolver) and computes
  the SHA-256 digest of the RFC 8785 form of `{colors, under, over}`, the CIELAB lightness of
  every colour, the CIEDE2000 difference (Sharma, Wu and Dalal, 2005) of every adjacent and
  mirrored step under normal vision and under protanopia, deuteranopia and tritanopia simulated
  with the matrices of Machado, Oliveira and Fernandes (2009) at severity 1.0 (linear sRGB,
  clipped to [0, 1]), the CIEDE2000 distance of the out-of-tolerance colours from CONFORMS in
  every theme, their WCAG 2.2 contrast on the base surface, the contrast of every stepped colour
  on the base surface (measured, not gated), and the least OKLab distance from every claimed
  role. It fails unless every gated value clears its rule by a margin, so a palette can only
  ship with a clear pass.

This module never imports `make_palettes.py` (the authoring script) or the oracles of
UX.TOKENS.COLOR and UX.TOKENS.STATUS.
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
PALS = "tokens/palettes"

THEMES = ["light", "dark", "hc", "floor"]
VISIONS = ["normal", "protanopia", "deuteranopia", "tritanopia"]
REQUIRED = ["pal.step.tol@1", "pal.step.cvd@1", "pal.cont.viridis@1", "pal.cont.cividis@1"]
DEFAULT = "pal.step.cvd@1"

MIN_STEP_DE = 10.0  # REQ-UX.TOKENS-007 [unverified threshold]
STEP_MARGIN = 0.5
MIN_OOT_DE = 20.0  # REQ-UX.TOKENS-009 [unverified threshold]
OOT_MARGIN = 1.0
MIN_CONTRAST = 3.0  # WCAG 2.2 SC 1.4.11
CONTRAST_MARGIN = 0.05
MIN_DE_OK = 0.10  # UX.TOKENS.COLOR floor: claimed roles vs measured data
DE_OK_MARGIN = 0.005
MAX_NEUTRAL_CHROMA = 0.02  # OKLab chroma of a neutral centre [unverified]
MIN_LSTAR_STEP = 0.02  # shipped continuous maps rise by at least this per sample

# ------------------------------------------------------------------ colour maths


def channels(h):
    assert len(h) == 7 and h[0] == "#", h
    return [int(h[i : i + 2], 16) for i in (1, 3, 5)]


def lin(v):
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(h):
    r, g, b = (lin(v) for v in channels(h))
    return 0.2126 * r + 0.7152 * g + 0.0722 * b


def contrast(a, b):
    la, lb = luminance(a), luminance(b)
    return (max(la, lb) + 0.05) / (min(la, lb) + 0.05)


def oklab(h):
    r, g, b = (lin(v) for v in channels(h))
    l_ = math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b)
    m_ = math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b)
    s_ = math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b)
    return (
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
    )


def chroma_ok(h):
    _, a, b = oklab(h)
    return math.hypot(a, b)


MACHADO = {
    "protanopia": ((0.152286, 1.052583, -0.204868), (0.114503, 0.786281, 0.099216), (-0.003882, -0.048116, 1.051998)),
    "deuteranopia": ((0.367322, 0.860646, -0.227968), (0.280085, 0.672501, 0.047413), (-0.011820, 0.042940, 0.968881)),
    "tritanopia": ((1.255528, -0.076749, -0.178779), (-0.078411, 0.930809, 0.147602), (0.004733, 0.691367, 0.303900)),
}


def simulate(h, vision):
    rgb = [lin(v) for v in channels(h)]
    if vision == "normal":
        return rgb
    m = MACHADO[vision]
    return [min(1.0, max(0.0, m[i][0] * rgb[0] + m[i][1] * rgb[1] + m[i][2] * rgb[2])) for i in range(3)]


def cielab(rgb):
    r, g, b = rgb
    x = (0.4124564 * r + 0.3575761 * g + 0.1804375 * b) / 0.95047
    y = 0.2126729 * r + 0.7151522 * g + 0.0721750 * b
    z = (0.0193339 * r + 0.1191920 * g + 0.9503041 * b) / 1.08883
    e = (6.0 / 29.0) ** 3

    def f(t):
        return t ** (1.0 / 3.0) if t > e else t * (841.0 / 108.0) + 4.0 / 29.0

    return (116.0 * f(y) - 16.0, 500.0 * (f(x) - f(y)), 200.0 * (f(y) - f(z)))


def lstar(h):
    return cielab(simulate(h, "normal"))[0]


def ciede2000(p, q):
    L1, a1, b1 = p
    L2, a2, b2 = q
    cbar = (math.hypot(a1, b1) + math.hypot(a2, b2)) / 2.0
    g = 0.5 * (1.0 - math.sqrt(cbar ** 7 / (cbar ** 7 + 25.0 ** 7)))
    a1p, a2p = a1 * (1.0 + g), a2 * (1.0 + g)
    c1p, c2p = math.hypot(a1p, b1), math.hypot(a2p, b2)
    h1p = math.degrees(math.atan2(b1, a1p)) % 360.0 if (a1p, b1) != (0.0, 0.0) else 0.0
    h2p = math.degrees(math.atan2(b2, a2p)) % 360.0 if (a2p, b2) != (0.0, 0.0) else 0.0
    if c1p * c2p == 0.0:
        dhp, hbarp = 0.0, h1p + h2p
    else:
        diff = h2p - h1p
        dhp = diff - 360.0 if diff > 180.0 else diff + 360.0 if diff < -180.0 else diff
        s = h1p + h2p
        hbarp = s / 2.0 if abs(h1p - h2p) <= 180.0 else (s + 360.0) / 2.0 if s < 360.0 else (s - 360.0) / 2.0
    dLp, dCp = L2 - L1, c2p - c1p
    dHp = 2.0 * math.sqrt(c1p * c2p) * math.sin(math.radians(dhp) / 2.0)
    lbarp, cbarp = (L1 + L2) / 2.0, (c1p + c2p) / 2.0
    t = (
        1.0
        - 0.17 * math.cos(math.radians(hbarp - 30.0))
        + 0.24 * math.cos(math.radians(2.0 * hbarp))
        + 0.32 * math.cos(math.radians(3.0 * hbarp + 6.0))
        - 0.20 * math.cos(math.radians(4.0 * hbarp - 63.0))
    )
    sl = 1.0 + 0.015 * (lbarp - 50.0) ** 2 / math.sqrt(20.0 + (lbarp - 50.0) ** 2)
    sc = 1.0 + 0.045 * cbarp
    sh = 1.0 + 0.015 * cbarp * t
    rt = -2.0 * math.sqrt(cbarp ** 7 / (cbarp ** 7 + 25.0 ** 7)) * math.sin(
        math.radians(60.0 * math.exp(-(((hbarp - 275.0) / 25.0) ** 2)))
    )
    x, y, z = dLp / sl, dCp / sc, dHp / sh
    return math.sqrt(x * x + y * y + z * z + rt * y * z)


# Spot checks of the formula against pairs of the Sharma, Wu and Dalal (2005) table.
for _p, _q, _want in [
    ((50.0, 2.6772, -79.7751), (50.0, 0.0, -82.7485), 2.0425),
    ((50.0, 2.49, -0.001), (50.0, -2.49, 0.0011), 7.2195),
    ((50.0, 2.5, 0.0), (73.0, 25.0, -18.0), 27.1492),
    ((60.2574, -34.0099, 36.2677), (60.4626, -34.1751, 39.4387), 1.2644),
    ((22.7233, 20.0904, -46.694), (23.0331, 14.973, -42.5619), 2.0373),
    ((2.0776, 0.0795, -1.135), (0.9033, -0.0636, -0.5514), 0.9082),
]:
    assert abs(ciede2000(_p, _q) - _want) < 1e-4, (_p, _q, ciede2000(_p, _q))
assert abs(contrast("#000000", "#ffffff") - 21.0) < 1e-12


def de(a, b, vision):
    return ciede2000(cielab(simulate(a, vision)), cielab(simulate(b, vision)))


def de_min(a, b):
    return min(de(a, b, v) for v in VISIONS)


def digest(colors, under, over):
    # RFC 8785 of an object whose values are ASCII strings and arrays of them: sorted keys, no
    # whitespace.
    text = '{"colors":[' + ",".join('"' + c + '"' for c in colors) + '],"over":"' + over + '","under":"' + under + '"}'
    return "sha256:" + hashlib.sha256(text.encode("ascii")).hexdigest()


# ------------------------------------------------------------------ reading files


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError(f"duplicate key in {keys}")
    return dict(pairs)


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_dups)


def flat_tokens(obj, path, out):
    if "$value" in obj:
        out[".".join(path)] = obj["$value"]
        return
    for k, v in obj.items():
        if not k.startswith("$"):
            flat_tokens(v, path + [k], out)


def resolved(root):
    """theme -> semantic name -> hex, following the theme binding and then aliases to a literal."""
    tokens = {}
    for p in sorted((root / "tokens").rglob("*.tokens.json")):
        flat_tokens(load_json(p), [], tokens)
    out = {}
    for t in THEMES:
        binds = load_json(root / "tokens" / "themes" / f"{t}.json")["bindings"]
        out[t] = {}
        for name, target in binds.items():
            cur = target.strip("{}")
            for _ in range(32):
                val = tokens[cur]
                if not val.startswith("{"):
                    break
                cur = val.strip("{}")
            out[t][name] = val
    return out


def is_claimed(name):
    segs = name.split(".")[1:]
    return name.startswith("sem.") and any(s.startswith("claimed") for s in segs) and not any(
        s.startswith("measured") for s in segs
    )


# ------------------------------------------------------------------ perceptual rules


def perceptual(pals, sem):
    """The perceptual findings of a palette set (`ref -> palette`) against `sem` (theme -> name -> hex)."""
    out = set()
    for t in THEMES:
        for n in ("sem.status.conforms", "sem.surface.base"):
            if n not in sem.get(t, {}):
                out.add(("PAL-OOT" if "status" in n else "PAL-CONTRAST") + f" {t} {n}")
    for ref, p in pals.items():
        cols, under, over = p["colors"], p["under"], p["over"]
        n = len(cols)
        stepped = p["kind"] == "stepped"
        pairs = [("under", 0), (n - 1, "over")]
        if stepped:
            pairs += [(i, i + 1) for i in range(n - 1)]
        odd = stepped and n % 2 == 1
        if odd:
            c = (n - 1) // 2
            pairs += [(c - k, c + k) for k in range(1, c + 1)]
            if chroma_ok(cols[c]) > MAX_NEUTRAL_CHROMA:
                out.add(f"PAL-CENTRE {ref}")
            for arm in (cols[c::-1], cols[c:]):
                ls = [lstar(x) for x in arm]
                d = [b - a for a, b in zip(ls, ls[1:])]
                if not (all(x > 0 for x in d) or all(x < 0 for x in d)):
                    out.add(f"PAL-LIGHTNESS {ref}")
        if not stepped:
            ls = [lstar(x) for x in cols]
            d = [b - a for a, b in zip(ls, ls[1:])]
            if not (all(x > 0 for x in d) or all(x < 0 for x in d)):
                out.add(f"PAL-LIGHTNESS {ref}")

        def col(k):
            return under if k == "under" else over if k == "over" else cols[k]

        for a, b in pairs:
            if de_min(col(a), col(b)) < MIN_STEP_DE:
                out.add(f"PAL-DELTA {ref} {a} {b}")
        for t in THEMES:
            row = sem.get(t, {})
            for side, x in (("under", under), ("over", over)):
                if "sem.status.conforms" in row and de_min(x, row["sem.status.conforms"]) < MIN_OOT_DE:
                    out.add(f"PAL-OOT {ref} {t} {side}")
                if "sem.surface.base" in row and contrast(x, row["sem.surface.base"]) < MIN_CONTRAST:
                    out.add(f"PAL-CONTRAST {ref} {t} {side}")
            claimed = [v for k, v in row.items() if is_claimed(k)]
            if any(math.dist(oklab(x), oklab(c)) < MIN_DE_OK for x in cols + [under, over] for c in claimed):
                out.add(f"PAL-CLAIMED {ref} {t}")
    return out


# ------------------------------------------------------------------ the shipped palettes


def read_palettes(root):
    cat = load_json(root / PALS / "catalogue.json")
    return cat, {ref: load_json(root / PALS / f"{ref}.json") for ref in cat["palettes"]}


def shipped():
    cat, pals = read_palettes(DESIGN)
    # Structure, restated independently of the Rust check.
    assert sorted(cat) == ["default", "format", "palettes"], sorted(cat)
    assert cat["format"] == "palette-catalogue@1" and cat["default"] == DEFAULT
    assert sorted(cat["palettes"]) == sorted(REQUIRED), cat["palettes"]
    on_disk = sorted(p.name for p in (DESIGN / PALS).iterdir())
    assert on_disk == sorted(["catalogue.json"] + [r + ".json" for r in REQUIRED]), on_disk
    sem = resolved(DESIGN)
    fails = []
    out = {"default": DEFAULT, "palettes": {}, "digests": {}}
    for ref, p in sorted(pals.items()):
        pid, major = ref.split("@")
        keys = ["colors", "digest", "id", "kind", "licence", "over", "source", "under", "version"]
        if p["kind"] == "stepped":
            keys = sorted(keys + ["centre_index"])
        assert sorted(p) == keys, (ref, sorted(p))
        assert p["id"] == pid and p["version"] == int(major)
        assert p["kind"] == ("stepped" if pid.startswith("pal.step.") else "continuous")
        assert p["licence"] == "CC-BY-4.0" and p["source"].startswith("original: "), ref
        cols, under, over = p["colors"], p["under"], p["over"]
        for h in cols + [under, over]:
            assert h == h.lower() and len(channels(h)) == 3, h
        dg = digest(cols, under, over)
        assert p["digest"] == dg, ref
        out["digests"][ref] = dg
        n = len(cols)
        rec = {"kind": p["kind"], "count": n, "digest": dg, "lstar": [round(lstar(x), 9) for x in cols]}
        if p["kind"] == "stepped":
            assert n % 2 == 1 and 7 <= n <= 15 and p["centre_index"] == (n - 1) // 2, ref
            c = (n - 1) // 2
            rec["centre_chroma"] = round(chroma_ok(cols[c]), 9)
            if rec["centre_chroma"] > MAX_NEUTRAL_CHROMA:
                fails.append(f"{ref}: centre not neutral")
            labels = ["under"] + list(range(n)) + ["over"]
            seq = [under] + cols + [over]
            pairs = [(labels[i], labels[i + 1], seq[i], seq[i + 1]) for i in range(n + 1)]
            pairs += [(c - k, c + k, cols[c - k], cols[c + k]) for k in range(1, c + 1)]
            for arm in (cols[c::-1], cols[c:]):
                ls = [lstar(x) for x in arm]
                if not all(b < a - 1.0 for a, b in zip(ls, ls[1:])):
                    fails.append(f"{ref}: an arm's lightness does not fall by more than 1 L* per step")
            rec["contrast_on_base"] = {
                t: [round(contrast(x, sem[t]["sem.surface.base"]), 9) for x in cols] for t in THEMES
            }
        else:
            assert 256 <= n <= 4096, ref
            ls = [lstar(x) for x in cols]
            step = min(b - a for a, b in zip(ls, ls[1:]))
            rec["min_lstar_step"] = round(step, 9)
            if step < MIN_LSTAR_STEP:
                fails.append(f"{ref}: lightness rises by only {step:.4f} somewhere")
            oks = [math.dist(oklab(a), oklab(b)) for a, b in zip(cols, cols[1:])]
            rec["oklab_step"] = {"min": round(min(oks), 9), "max": round(max(oks), 9), "total": round(sum(oks), 9)}
            pairs = [("under", 0, under, cols[0]), (n - 1, "over", cols[-1], over)]
        rec["pairs"] = []
        for a, b, x, y in pairs:
            ds = {v: round(de(x, y, v), 9) for v in VISIONS}
            if min(ds.values()) < MIN_STEP_DE + STEP_MARGIN:
                fails.append(f"{ref} {a}/{b}: dE2000 {min(ds.values()):.3f}")
            rec["pairs"].append({"a": str(a), "b": str(b), "de2000": ds})
        rec["oot"], rec["oot_contrast"], rec["claimed_min"] = [], [], {}
        for t in THEMES:
            for side, x in (("under", under), ("over", over)):
                for v in VISIONS:
                    d = de(x, sem[t]["sem.status.conforms"], v)
                    if d < MIN_OOT_DE + OOT_MARGIN:
                        fails.append(f"{ref} {t} {side} {v}: dE2000 {d:.3f} from CONFORMS")
                    rec["oot"].append({"theme": t, "side": side, "vision": v, "de2000": round(d, 9)})
                r = contrast(x, sem[t]["sem.surface.base"])
                if r < MIN_CONTRAST + CONTRAST_MARGIN:
                    fails.append(f"{ref} {t} {side}: contrast {r:.3f}")
                rec["oot_contrast"].append({"theme": t, "side": side, "ratio": round(r, 9)})
            claimed = sorted(k for k in sem[t] if is_claimed(k))
            assert len(claimed) >= 3, claimed
            d = min(math.dist(oklab(x), oklab(sem[t][c])) for x in cols + [under, over] for c in claimed)
            if d < MIN_DE_OK + DE_OK_MARGIN:
                fails.append(f"{ref} {t}: OKLab {d:.4f} from a claimed role")
            rec["claimed_min"][t] = round(d, 9)
        out["palettes"][ref] = rec
    if perceptual(pals, sem):
        fails.append(f"perceptual findings on the shipped palettes: {sorted(perceptual(pals, sem))}")
    if fails:
        raise SystemExit("shipped palettes fail:\n  " + "\n  ".join(fails))
    out["values"] = {r: {"colors": p["colors"], "under": p["under"], "over": p["over"]} for r, p in pals.items()}
    out["sem"] = {t: {k: v for k, v in sorted(sem[t].items()) if k in SEM_NAMES or is_claimed(k)} for t in THEMES}
    return out


# ------------------------------------------------------------------ fixtures

SEM_NAMES = ["sem.status.conforms", "sem.surface.base", "sem.text.value.measured"]


def token_tree(sem):
    """A small valid token tree holding the shipped values of the roles palettes-check reads."""
    names = sorted({k for t in THEMES for k in sem[t] if k in SEM_NAMES or is_claimed(k)})
    prim, semf, files = {}, {}, {}
    for n in names:
        role = n[len("sem.") :].replace(".", "-")
        for t in THEMES:
            prim.setdefault(role, {})[t] = {"$value": sem[t][n]}
        cur = semf
        segs = n.split(".")
        for s in segs[:-1]:
            cur = cur.setdefault(s, {})
        cur[segs[-1]] = {"$value": "{prim.color." + role + ".light}"}
    semf["sem"]["$type"] = "color"
    files["tokens/primitive/color.tokens.json"] = {"prim": {"$type": "color", "color": prim}}
    files["tokens/semantic/color.tokens.json"] = semf
    for t in THEMES:
        files[f"tokens/themes/{t}.json"] = {
            "format": "fid-design-theme@1",
            "theme": t,
            "bindings": {n: "{prim.color." + n[4:].replace(".", "-") + "." + t + "}" for n in names},
        }
    files["tokens/pairs.json"] = {"format": "fid-design-pairs@1", "pairs": []}
    files["tokens/brand-tier.json"] = {"format": "fid-design-brand-tier@1", "tokens": []}
    return files


def base_state():
    cat, pals = read_palettes(DESIGN)
    return {"catalogue": copy.deepcopy(cat), "pals": copy.deepcopy(pals), "extra": {}, "baseline": None}


def redigest(p):
    p["digest"] = digest(p["colors"], p["under"], p["over"])


def edit(ref, fn, rehash=True):
    def go(m):
        fn(m["pals"][ref])
        if rehash:
            redigest(m["pals"][ref])

    return go


def both(*fns):
    def go(m):
        for f in fns:
            f(m)

    return go


def with_baseline(fn):
    def go(m):
        fn(m)
        m["baseline"] = True

    return go


def drop_pal(ref):
    def go(m):
        m["catalogue"]["palettes"].remove(ref)
        del m["pals"][ref]

    return go


def set_key(k, v):
    def go(p):
        p[k] = v

    return go


def del_key(k):
    def go(p):
        del p[k]

    return go


def setcol(i, h):
    def go(p):
        p["colors"][i] = h

    return go


CAT = "tokens/palettes/catalogue.json"
TOL, CVD, VIR, CIV = REQUIRED


def add_v2(m):
    p = copy.deepcopy(m["pals"][TOL])
    p["version"] = 2
    p["colors"][3] = "#70abe0"
    redigest(p)
    m["pals"]["pal.step.tol@2"] = p
    m["catalogue"]["palettes"].append("pal.step.tol@2")


def swap(i, j):
    def go(p):
        p["colors"][i], p["colors"][j] = p["colors"][j], p["colors"][i]

    return go


# name -> (mutation, exit code, ["CODE subject", ...]); hand-labelled from the leaf's rules.
LABELS = {
    "ok": (lambda m: None, 0, []),
    # The shipped digests as a baseline: nothing changed.
    "ok-baseline": (with_baseline(lambda m: None), 0, []),
    # REQ-004: a new major version beside the old one is allowed.
    "new-major": (with_baseline(add_v2), 0, []),
    # AC-2: an 8-step stepped palette (the default palette without its centre step).
    "even-steps": (edit(CVD, lambda p: p["colors"].pop(4)), 1, ["PAL-STEPS " + CVD]),
    # 5 steps: odd, but fewer than 7 (the default palette's four outermost steps dropped).
    "five-steps": (
        edit(CVD, both(lambda p: p.__setitem__("colors", p["colors"][2:7]), set_key("centre_index", 2))),
        1,
        ["PAL-STEPS " + CVD],
    ),
    # AC-3: one colour of pal.step.tol@1 differs from the baseline digest (the file's own digest
    # was updated, so only the baseline catches it).
    "changed-v1": (with_baseline(edit(TOL, setcol(3, "#71aae0"))), 1, ["PAL-IMMUTABLE " + TOL]),
    # REQ-004: the old version must stay in the catalogue.
    "removed-v1": (
        with_baseline(both(add_v2, drop_pal(TOL))),
        1,
        ["PAL-IMMUTABLE " + TOL, "PAL-SCHEMA " + CAT],
    ),
    # The file's digest does not match its colours.
    "digest-mismatch": (edit(VIR, set_key("digest", "sha256:" + "0" * 64), rehash=False), 1, ["PAL-SCHEMA " + VIR]),
    "centre-off": (edit(TOL, set_key("centre_index", 4)), 1, ["PAL-CENTRE " + TOL]),
    # A tinted centre (a pale green instead of the neutral grey).
    "centre-tinted": (edit(CVD, setcol(4, "#e4f5e4")), 1, ["PAL-CENTRE " + CVD]),
    "samples": (edit(CIV, lambda p: p["colors"].pop()), 1, ["PAL-SAMPLES " + CIV]),
    "licence-nc": (edit(CIV, set_key("licence", "CC-BY-NC-4.0"), rehash=False), 1, ["PAL-LICENCE " + CIV]),
    # CC-BY-4.0 is only for project-made ("original: ...") palettes.
    "licence-third-party": (
        edit(VIR, set_key("source", "matplotlib viridis data"), rehash=False),
        1,
        ["PAL-LICENCE " + VIR],
    ),
    "default-wrong": (lambda m: m["catalogue"].__setitem__("default", TOL), 1, ["PAL-DEFAULT " + CAT]),
    "default-missing": (lambda m: m["catalogue"].pop("default"), 1, ["PAL-DEFAULT " + CAT]),
    "required-missing": (drop_pal(CIV), 1, ["PAL-SCHEMA " + CAT]),
    "unlisted-file": (lambda m: m["extra"].__setitem__("tokens/palettes/notes.json", {}), 1, ["PAL-SCHEMA tokens/palettes/notes.json"]),
    "no-catalogue": (lambda m: m.__setitem__("catalogue", None), 1, ["PAL-SCHEMA " + CAT]),
    "bad-hex": (edit(TOL, set_key("under", "#B18E15"), rehash=False), 1, ["PAL-SCHEMA " + TOL]),
    "extra-key": (edit(CVD, set_key("note", "x"), rehash=False), 1, ["PAL-SCHEMA " + CVD]),
    "wrong-kind": (edit(CIV, del_key("kind"), rehash=False), 1, ["PAL-SCHEMA " + CIV]),
    "id-mismatch": (edit(VIR, set_key("version", 2), rehash=False), 1, ["PAL-SCHEMA " + VIR]),
    # Perceptual rules (labels confirmed by this script's own maths).
    # Two blue steps swapped: the blue arm's lightness is no longer monotonic.
    "lightness-step": (
        edit(TOL, swap(1, 2)),
        1,
        ["PAL-LIGHTNESS " + TOL],
    ),
    # Two samples of a continuous map swapped.
    "lightness-cont": (edit(VIR, swap(100, 101)), 1, ["PAL-LIGHTNESS " + VIR]),
    # The second orange step nearly equal to the first.
    "delta": (edit(CVD, setcol(6, "#e9c197")), 1, ["PAL-DELTA " + CVD + " 5 6"]),
    # Under-range drawn in the blue arm's colour: it is CONFORMS-like and next to the blue end.
    "oot-blue": (
        edit(CVD, set_key("under", "#2a76c3")),
        1,
        ["PAL-OOT " + CVD + f" {t} under" for t in THEMES],
    ),
    # Over-range too light to see on a white surface.
    "contrast": (
        edit(TOL, set_key("over", "#ffe680")),
        1,
        ["PAL-CONTRAST " + TOL + f" {t} over" for t in ("light", "hc", "floor")],
    ),
    # Under-range drawn in the claimed border colour of the light theme: too near the claimed
    # roles of light and floor (the same values), and, like every magenta, under 20 from an azure
    # CONFORMS once colour vision is simulated.
    "claimed": (
        edit(CIV, set_key("under", "#ab3f92")),
        1,
        ["PAL-CLAIMED " + CIV + f" {t}" for t in ("light", "floor")]
        + ["PAL-OOT " + CIV + f" {t} under" for t in THEMES],
    ),
}

PERCEPTUAL = ("PAL-LIGHTNESS", "PAL-DELTA", "PAL-OOT", "PAL-CONTRAST", "PAL-CLAIMED")


def canon(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def all_files(sem, digests):
    out = {}
    expected = {"fixtures": {}}
    tree = token_tree(sem)
    for name, (fn, code, findings) in sorted(LABELS.items()):
        m = base_state()
        fn(m)
        for rel, obj in tree.items():
            out[f"{name}/{rel}"] = obj
        if m["catalogue"] is not None:
            out[f"{name}/{CAT}"] = m["catalogue"]
        for ref, p in m["pals"].items():
            out[f"{name}/{PALS}/{ref}.json"] = p
        for rel, obj in m["extra"].items():
            out[f"{name}/{rel}"] = obj
        baseline = None
        if m["baseline"]:
            baseline = f"{name}/baseline.json"
            out[baseline] = {"format": "palette-digests@1", "digests": digests}
        expected["fixtures"][name] = {
            "exit": code,
            "findings": sorted(set(findings)),
            "baseline": "baseline.json" if baseline else None,
        }
        # Confirm the perceptual labels (and the neutral-centre part of PAL-CENTRE) with this
        # script's own maths, on palettes whose colours are all valid hex.
        valid = {
            r: p
            for r, p in m["pals"].items()
            if all(isinstance(h, str) and h == h.lower() for h in p["colors"] + [p["under"], p["over"]])
            and p.get("kind") in ("stepped", "continuous")
        }
        got = {f for f in perceptual(valid, sem) if f.startswith(PERCEPTUAL)}
        want = {f for f in findings if f.startswith(PERCEPTUAL)}
        assert got == want, (name, sorted(got), sorted(want))
        if name == "centre-tinted":
            assert "PAL-CENTRE " + CVD in perceptual(valid, sem), name
    return {k: canon(v) for k, v in out.items()}, expected


def main():
    check = "--check" in sys.argv[1:]
    ship = shipped()
    sem = ship["sem"]
    files, expected = all_files(sem, ship["digests"])
    expected["shipped"] = ship
    files["expected.json"] = canon(expected)
    files["shipped-digests.json"] = canon({"format": "palette-digests@1", "digests": ship["digests"]})
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
