#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.TOKENS.COLOR and the independent expected results.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/color/make_fixtures.py          # (re)write fixtures and expected.json
    python fixtures/ux/tokens/color/make_fixtures.py --check  # fail if any file differs

Two kinds of expected values, neither produced by the Rust code under test:

* The findings of every error-path fixture are hand-labelled below from the leaf's rules
  (`LABELS`), next to the one mutation each fixture makes to a small valid token set.
* The per-theme resolution of the shipped `design/tokens/` and of every fixture, and the WCAG 2.2
  contrast ratio of every declared pair in every theme, are computed here by an independent
  Python reading of the files (`json` with a duplicate-key hook, its own DTCG walk and alias
  resolver, and the WCAG relative-luminance formula). This script also fails if a shipped pair
  misses its threshold by less than MARGIN, so a value can only ship with a clear pass.

This module never imports `make_tokens.py` (the authoring script).
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
MARGIN = 0.05
# Maintainer decision on PR 330: claimed roles stay at least this far (OKLab, Euclidean) from
# every measured, brand-tier and status role in every theme. Shipped values must clear it by
# DE_MARGIN so that platform differences in cbrt cannot flip a verdict.
MIN_DE_OK = 0.10
DE_MARGIN = 0.005

# ------------------------------------------------------------------ WCAG 2.2


def linear(v):
    c = v / 255.0
    return c / 12.92 if c <= 0.04045 else ((c + 0.055) / 1.055) ** 2.4


def luminance(hexv):
    assert len(hexv) == 7 and hexv[0] == "#", hexv
    r, g, b = (int(hexv[i : i + 2], 16) for i in (1, 3, 5))
    return 0.2126 * linear(r) + 0.7152 * linear(g) + 0.0722 * linear(b)


def ratio(a, b):
    la, lb = luminance(a), luminance(b)
    hi, lo = max(la, lb), min(la, lb)
    return (hi + 0.05) / (lo + 0.05)


def required(tag, theme):
    # REQ-UX.TOKENS-002..004: text 4.5, large text 3, non-text 3; every text pair 7 in hc.
    if tag in ("text", "large-text") and theme == "hc":
        return 7.0
    return {"text": 4.5, "large-text": 3.0, "non-text": 3.0}[tag]


def oklab(hexv):
    """Bjorn Ottosson's OKLab (2020): linear sRGB -> LMS -> cube root -> Lab."""
    r, g, b = (linear(int(hexv[i : i + 2], 16)) for i in (1, 3, 5))
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


def de_ok(a, b):
    return math.dist(oklab(a), oklab(b))


assert abs(de_ok("#000000", "#ffffff") - 1.0) < 1e-6

# The WCAG 2.0/2.1 threshold 0.03928 and the 2.2 threshold 0.04045 agree on every 8-bit value.
assert all((v / 255 <= 0.04045) == (v / 255 <= 0.03928) for v in range(256))
assert abs(ratio("#000000", "#ffffff") - 21.0) < 1e-12

# ------------------------------------------------------------------ reading a token tree


def no_dups(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError(f"duplicate key in {keys}")
    return dict(pairs)


def load_json(path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_dups)


def walk_tokens(obj, path=(), inherited=None, out=None):
    """DTCG walk: name -> (type, value)."""
    out = {} if out is None else out
    typ = obj.get("$type", inherited)
    if "$value" in obj:
        out[".".join(path)] = (typ, obj["$value"])
        return out
    for k, v in obj.items():
        if not k.startswith("$"):
            walk_tokens(v, path + (k,), typ, out)
    return out


def read_tree(root):
    tokens = {}
    for p in sorted((root / "tokens").rglob("*.tokens.json")):
        for name, tv in walk_tokens(load_json(p)).items():
            assert name not in tokens, name
            tokens[name] = tv
    themes = {}
    for t in THEMES:
        p = root / "tokens" / "themes" / f"{t}.json"
        themes[t] = load_json(p)["bindings"] if p.exists() else None
    pairs = load_json(root / "tokens" / "pairs.json")["pairs"]
    brand = load_json(root / "tokens" / "brand-tier.json")["tokens"]
    return tokens, themes, pairs, brand


def alias(v):
    if isinstance(v, str) and v.startswith("{") and v.endswith("}"):
        return v[1:-1]
    return None


def effective_type(tokens, name):
    """Declared type, else the first declared type along the token-file alias chain."""
    for _ in range(17):
        if name not in tokens:
            return None
        t, v = tokens[name]
        if t is not None:
            return t
        name = alias(v)
        if name is None:
            return None
    return None


def is_colour(tokens, name):
    t = effective_type(tokens, name)
    return t == "color" if t is not None else name.split(".")[0] in ("prim", "sem", "comp")


def colour_names(tokens, tier):
    return sorted(n for n in tokens if n.split(".")[0] == tier and is_colour(tokens, n))


def resolve(tokens, binds, theme, name):
    """Returns (hex, prim) or None. Semantic tokens follow the theme; others their alias."""
    sem = set(colour_names(tokens, "sem"))
    seen = []
    cur = name
    while cur not in seen and len(seen) <= 16:
        seen.append(cur)
        if cur in sem:
            b = binds.get(theme)
            if b is None or cur not in b:
                return None
            cur = alias(b[cur])
            if cur is None:
                return None
            continue
        if cur not in tokens or (len(seen) > 1 and not is_colour(tokens, cur)):
            return None
        _, v = tokens[cur]
        nxt = alias(v)
        if nxt is None:
            return (v, cur) if cur.startswith("prim.") else None
        cur = nxt
    return None


def summarise(root):
    tokens, binds, pairs, brand = read_tree(root)
    counts, values = {}, {}
    for t in THEMES:
        c = {}
        values[t] = {}
        for tier, key in (("sem", "semantic"), ("comp", "component")):
            ok = bad = 0
            for n in colour_names(tokens, tier):
                r = resolve(tokens, binds, t, n)
                if r is None:
                    bad += 1
                else:
                    ok += 1
                    values[t][n] = r
            c[key] = [ok, bad]
        counts[t] = c
    return tokens, binds, pairs, brand, counts, values


# ------------------------------------------------------------------ the shipped tokens


def shipped():
    tokens, binds, pairs, brand, counts, values = summarise(DESIGN)
    for t in THEMES:
        assert counts[t]["semantic"][1] == 0 and counts[t]["component"][1] == 0, (t, counts[t])
    # Independent restatement of the structural rules for the shipped set.
    for n in colour_names(tokens, "comp"):
        a = alias(tokens[n][1])
        assert a is not None and a.startswith("sem."), n
    assert len(brand) <= 12 and all(b.startswith("sem.brand.") for b in brand), brand

    def side(n, word):
        return any(seg.startswith(word) for seg in n.split(".")[1:])

    claimed = [n for n in colour_names(tokens, "sem") if side(n, "claimed")]
    measured = [n for n in colour_names(tokens, "sem") if side(n, "measured")]
    status = [n for n in colour_names(tokens, "sem") if n.startswith("sem.status.")]
    distances = []
    for t in THEMES:
        for c in claimed:
            for o in measured + brand + status:
                d = de_ok(values[t][c][0], values[t][o][0])
                assert d >= MIN_DE_OK + DE_MARGIN, (t, c, o, d)
                if o not in status:
                    assert values[t][c][1].split(".")[2] != values[t][o][1].split(".")[2], (t, c, o)
                distances.append({"theme": t, "claimed": c, "other": o, "de_ok": round(d, 9)})
    out_pairs = []
    fails = []
    for t in THEMES:
        for p in pairs:
            r = ratio(values[t][p["fg"]][0], values[t][p["bg"]][0])
            need = required(p["tag"], t)
            if r < need + MARGIN:
                fails.append(f"{t} {p['fg']}/{p['bg']} {r:.3f} < {need} + {MARGIN}")
            out_pairs.append({"theme": t, "fg": p["fg"], "bg": p["bg"], "tag": p["tag"], "ratio": round(r, 9), "required": need})
    if fails:
        raise SystemExit("shipped pairs below threshold:\n  " + "\n  ".join(fails))
    return {
        "exit": 0,
        "findings": [],
        "counts": counts,
        "values": {t: {n: v[0] for n, v in sorted(values[t].items())} for t in THEMES},
        "pairs": out_pairs,
        "claimed_distances": distances,
        "min_margin": round(min(p["ratio"] - p["required"] for p in out_pairs), 6),
    }


# ------------------------------------------------------------------ fixtures

P = "prim.color."


def base():
    every = lambda target, **over: {t: over.get(t, target) for t in THEMES}  # noqa: E731
    return {
        "prim": {
            "neutral": {"0": "#ffffff", "500": "#6e6e6e", "900": "#222222"},
            "blue": {"600": "#1f5fbf"},
            "red": {"700": "#a01010"},
            "green": {"700": "#106010"},
            "magenta": {"700": "#8a1f70"},
        },
        "sem": {
            "sem.text.primary": every(P + "neutral.900", dark=P + "neutral.0"),
            "sem.text.muted": every(P + "neutral.500"),
            "sem.text.value.measured": every(P + "neutral.900", dark=P + "neutral.0"),
            "sem.text.value.claimed": every(P + "magenta.700"),
            "sem.surface.base": every(P + "neutral.0", dark=P + "neutral.900"),
            "sem.status.conforms": every(P + "green.700"),
            "sem.status.nonconforms": every(P + "red.700"),
            "sem.brand.accent": every(P + "blue.600"),
            "sem.focus.ring": every(P + "blue.600"),
        },
        "default_override": {},
        "comp": {
            "comp.button.bg.primary": "{sem.brand.accent}",
            "comp.input.fg.placeholder": "{sem.text.muted}",
            "comp.value.fg.claimed": "{sem.text.value.claimed}",
            "comp.value.fg.measured": "{sem.text.value.measured}",
        },
        "pairs": [
            ("sem.text.primary", "sem.surface.base", "text"),
            ("sem.text.muted", "sem.surface.base", "text"),
            ("sem.brand.accent", "sem.surface.base", "non-text"),
        ],
        "brand": ["sem.brand.accent"],
        "drop_theme": None,
        "extra": {},
    }


def nest(flat):
    out = {}
    for name, tok in sorted(flat.items()):
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
    prim = {f"{P}{h}.{s}": {"$value": v} for h, steps in m["prim"].items() for s, v in steps.items()}
    files["tokens/primitive/color.tokens.json"] = nest(prim)
    sem = {}
    for n, b in m["sem"].items():
        sem[n] = {"$value": m["default_override"].get(n, "{" + b["light"] + "}")}
    files["tokens/semantic/color.tokens.json"] = nest(sem)
    files["tokens/component/ui.tokens.json"] = nest({n: {"$value": v} for n, v in m["comp"].items()})
    for t in THEMES:
        if t == m["drop_theme"]:
            continue
        files[f"tokens/themes/{t}.json"] = {
            "format": "fid-design-theme@1",
            "theme": t,
            "bindings": {n: "{" + b[t] + "}" for n, b in m["sem"].items() if t in b},
        }
    files["tokens/pairs.json"] = {
        "format": "fid-design-pairs@1",
        "pairs": [{"fg": f, "bg": b, "tag": g} for f, b, g in m["pairs"]],
    }
    files["tokens/brand-tier.json"] = {"format": "fid-design-brand-tier@1", "tokens": m["brand"]}
    files.update(m["extra"])
    return files


def mutate(fn):
    m = copy.deepcopy(base())
    fn(m)
    return m


def set_all(m, name, target):
    m["sem"][name] = {t: target for t in THEMES}


def brand_size(m):
    for i in range(1, 13):
        set_all(m, f"sem.brand.b{i:02d}", P + "blue.600")
        m["brand"].append(f"sem.brand.b{i:02d}")


# name -> (mutation, hand-labelled exit code, hand-labelled "CODE subject" findings).
LABELS = {
    "ok": (lambda m: None, 0, []),
    # AC-2: sem.text.muted has no dark binding. comp.input.fg.placeholder aliases it, but the
    # finding names only the first broken link.
    "missing-dark": (lambda m: m["sem"]["sem.text.muted"].pop("dark"), 1, ["TOK-UNRESOLVED dark sem.text.muted"]),
    # AC-3: a component token aliases a primitive directly.
    "comp-to-prim": (
        lambda m: m["comp"].__setitem__("comp.button.border.primary", "{" + P + "blue.600}"),
        1,
        ["TOK-COMP-SKIPS-SEM comp.button.border.primary"],
    ),
    "comp-literal": (
        lambda m: m["comp"].__setitem__("comp.button.fg.primary", "#ffffff"),
        1,
        ["TOK-COMP-SKIPS-SEM comp.button.fg.primary"],
    ),
    # AC-4: the brand-tier token aliases sem.status.nonconforms.
    "brand-alias-status": (
        lambda m: set_all(m, "sem.brand.accent", "sem.status.nonconforms"),
        1,
        ["TOK-BRAND-STATUS sem.brand.accent"],
    ),
    # The other direction: a status token aliases the brand tier (dark theme only).
    "status-alias-brand": (
        lambda m: m["sem"]["sem.status.conforms"].__setitem__("dark", "sem.brand.accent"),
        1,
        ["TOK-BRAND-STATUS sem.brand.accent"],
    ),
    "alias-cycle": (
        lambda m: (
            m["sem"]["sem.focus.ring"].__setitem__("hc", "sem.brand.accent"),
            m["sem"]["sem.brand.accent"].__setitem__("hc", "sem.focus.ring"),
        ),
        1,
        ["TOK-ALIAS-CYCLE hc sem.brand.accent"],
    ),
    "unknown-target": (
        lambda m: m["sem"]["sem.text.muted"].__setitem__("dark", P + "neutral.555"),
        1,
        ["TOK-UNRESOLVED dark sem.text.muted"],
    ),
    "name-syntax": (lambda m: set_all(m, "sem.shade.base", P + "neutral.0"), 1, ["TOK-NAME-SYNTAX sem.shade.base"]),
    "brand-size": (brand_size, 1, ["TOK-BRAND-SIZE tokens/brand-tier.json"]),
    "brand-scope": (lambda m: m["brand"].append("sem.text.primary"), 1, ["TOK-BRAND-SIZE sem.text.primary"]),
    "pair-unknown": (
        lambda m: m["pairs"].append(("sem.text.ghost", "sem.surface.base", "text")),
        1,
        ["TOK-PAIR-UNKNOWN sem.text.ghost"],
    ),
    # Claimed values never look measured. A component that aliases the claimed role inherits the
# broken chain, so it is named too.
    "claimed-same-ramp": (
        lambda m: m["sem"]["sem.text.value.claimed"].__setitem__("floor", P + "neutral.500"),
        1,
        ["TOK-CLAIMED-AS-MEASURED floor sem.text.value.claimed"],
    ),
    "claimed-via-measured": (
        lambda m: m["sem"]["sem.text.value.claimed"].__setitem__("floor", "sem.text.value.measured"),
        1,
        ["TOK-CLAIMED-AS-MEASURED floor comp.value.fg.claimed", "TOK-CLAIMED-AS-MEASURED floor sem.text.value.claimed"],
    ),
    "claimed-via-brand": (
        lambda m: m["sem"]["sem.text.value.claimed"].__setitem__("hc", "sem.brand.accent"),
        1,
        ["TOK-CLAIMED-AS-MEASURED hc comp.value.fg.claimed", "TOK-CLAIMED-AS-MEASURED hc sem.text.value.claimed"],
    ),
    "comp-claimed-measured": (
        lambda m: m["comp"].__setitem__("comp.value.fg.claimed", "{sem.text.value.measured}"),
        1,
        [
            "TOK-CLAIMED-AS-MEASURED comp.value.fg.claimed",
            "TOK-CLAIMED-AS-MEASURED dark comp.value.fg.claimed",
            "TOK-CLAIMED-AS-MEASURED floor comp.value.fg.claimed",
            "TOK-CLAIMED-AS-MEASURED hc comp.value.fg.claimed",
            "TOK-CLAIMED-AS-MEASURED light comp.value.fg.claimed",
        ],
    ),
    "claimed-missing": (
        lambda m: (m["sem"].pop("sem.text.value.claimed"), m["comp"].pop("comp.value.fg.claimed")),
        1,
        ["TOK-CLAIMED-AS-MEASURED sem.text.value.claimed"],
    ),
    "schema-default": (
        lambda m: m["default_override"].__setitem__("sem.text.primary", "{" + P + "neutral.500}"),
        1,
        ["TOK-SCHEMA sem.text.primary"],
    ),
    # Review of PR 330: an untyped component alias straight to a primitive is still in scope.
    "untyped-comp-to-prim": (
        lambda m: m["extra"].__setitem__(
            "tokens/component/badge.tokens.json",
            {"comp": {"badge": {"border": {"primary": {"$value": "{" + P + "blue.600}"}}}}},
        ),
        1,
        ["TOK-COMP-SKIPS-SEM comp.badge.border.primary"],
    ),
    # A colour component aliasing a semantic token typed as something else, which holds a prim.
    "comp-to-noncolour-sem": (
        lambda m: (
            m["extra"].__setitem__(
                "tokens/semantic/size.tokens.json",
                {"sem": {"text": {"size-ref": {"$type": "dimension", "$value": "{" + P + "blue.600}"}}}},
            ),
            m["comp"].__setitem__("comp.button.fg.size", "{sem.text.size-ref}"),
        ),
        1,
        ["TOK-COMP-SKIPS-SEM comp.button.fg.size"],
    ),
    # A claimed role 0.0x from a status colour in OKLab, though on its own ramp.
    "claimed-near-status": (
        lambda m: (
            m["prim"].__setitem__("plum", {"700": "#a31220"}),
            m["sem"]["sem.text.value.claimed"].__setitem__("light", P + "plum.700"),
        ),
        1,
        ["TOK-CLAIMED-AS-MEASURED light sem.text.value.claimed"],
    ),
    # A claimed role on the brand ramp (same value as the brand accent).
    "claimed-brand-ramp": (
        lambda m: m["sem"]["sem.text.value.claimed"].__setitem__("floor", P + "blue.600"),
        1,
        ["TOK-CLAIMED-AS-MEASURED floor sem.text.value.claimed"],
    ),
    # Claimed/measured roles are matched by segment prefix, not only by the last segment.
    "claimed-prefix": (
        lambda m: m["comp"].__setitem__("comp.badge.claimed-bg", "{sem.text.value.measured}"),
        1,
        [
            "TOK-CLAIMED-AS-MEASURED comp.badge.claimed-bg",
            "TOK-CLAIMED-AS-MEASURED dark comp.badge.claimed-bg",
            "TOK-CLAIMED-AS-MEASURED floor comp.badge.claimed-bg",
            "TOK-CLAIMED-AS-MEASURED hc comp.badge.claimed-bg",
            "TOK-CLAIMED-AS-MEASURED light comp.badge.claimed-bg",
        ],
    ),
    "missing-theme": (lambda m: m.__setitem__("drop_theme", "hc"), 1, ["TOK-SCHEMA tokens/themes/hc.json"]),
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
    return out, expected


def main():
    check = "--check" in sys.argv[1:]
    files, expected = all_files()
    # Write (or compare) the fixture trees first, then compute counts from the files on disk.
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
    for name in LABELS:
        expected["fixtures"][name]["counts"] = summarise(HERE / name)[4]
    expected["shipped"] = shipped()
    text = canon(expected)
    p = HERE / "expected.json"
    if check:
        if not p.exists() or p.read_text(encoding="utf-8") != text:
            bad.append("expected.json")
    else:
        p.write_text(text, encoding="utf-8", newline="\n")
    if bad:
        print("out of date (run make_fixtures.py):\n  " + "\n  ".join(bad), file=sys.stderr)
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
