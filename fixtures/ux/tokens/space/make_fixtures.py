#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.TOKENS.SPACE and the independent expected results.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/space/make_fixtures.py          # (re)write fixtures and expected.json
    python fixtures/ux/tokens/space/make_fixtures.py --check  # fail if any file differs

Expected values, none produced by the Rust code under test:

* Every fixture is a small valid design tree (`ok/`) with one mutation. Its findings are
  hand-labelled below (`CASES`) from the leaf's rules, as "<CODE> <subject>".
* An independent reading of the rules (`oracle`, its own DTCG walk, alias handling and exact
  decimal comparisons) must give exactly the hand labels for every fixture and no finding for
  the shipped `design/` tree; the script fails otherwise.

Rules, as pinned in crates/fid-design/src/space.rs:
  SPACE-GRID <name>             prim.space literal or sem.space literal not a whole multiple of 4 px
  SPACE-DENSITY <mode> <name>   a sem.space token unbound in a mode, an unknown or non-alias
                                binding, or a comfortable binding that differs from the default
  SIZE-TARGET <theme> <mode>    target below 24 px (44 px in floor) or missing
  SIZE-TARGET default           size.target.min missing or not the light/comfortable target
  MOTION-REDUCED <group>        no valid duration, cubicBezier easing or reduced-duration, or a
                                reduced-duration other than 0 ms
  MOTION-LONG <group>           a motion.panel.* duration above 250 ms
  ELEV-LEVELS elev              more than 5 levels, levels not 0..n-1, or none
  ELEV-NOSHADOW-CUE elev.<k>    no cue, bad cue, the same cue names as a lower level, a cue that
                                does not resolve to a colour in some theme, or in some theme the
                                same resolved (surface, border) colours as a lower level; at most
                                one per level
  SPACE-SCHEMA <file or name>   missing or invalid file, wrong format, wrong shape, a motion file
                                with no motion token, an elevation level name that is not a
                                decimal integer below 2^64 without leading zeros

The oracle resolves elevation cues on its own: every `tokens/**/*.tokens.json` token, the
`tokens/themes/<theme>.json` bindings for `sem.*` names, `{alias}` hops (at most 16), ending at
a literal `#rrggbb[aa]` of a `prim.*` token.
"""

import copy
import json
import pathlib
import re
import shutil
import sys
from decimal import Decimal, InvalidOperation

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
DESIGN = ROOT / "design"
THEMES = ["light", "dark", "hc", "floor"]
DENSITIES = ["comfortable", "compact"]


# ------------------------------------------------------------------ the valid base tree

def base():
    prim = {"$type": "dimension"}
    for n in [0, 1, 2, 3, 4, 6, 8]:
        prim[str(n)] = {"$value": f"{4 * n}px"}
    sem_defaults = {"gap": {"sm": 2, "md": 4}, "inset": {"sm": 2, "md": 3}}
    sem = {"$type": "dimension"}
    for g, roles in sem_defaults.items():
        sem[g] = {r: {"$value": "{prim.space.%d}" % n} for r, n in roles.items()}

    def density(mode, steps, targets):
        return {
            "bindings": {f"sem.space.{g}.{r}": "{prim.space.%d}" % steps[(g, r)]
                         for g, roles in sem_defaults.items() for r in roles},
            "density": mode,
            "format": "fid-design-density@1",
            "target_min": targets,
        }

    comfortable = density("comfortable", {(g, r): n for g, roles in sem_defaults.items() for r, n in roles.items()},
                          {"dark": "32px", "floor": "48px", "hc": "32px", "light": "32px"})
    compact = density("compact", {("gap", "sm"): 1, ("gap", "md"): 2, ("inset", "sm"): 1, ("inset", "md"): 2},
                      {"dark": "24px", "floor": "44px", "hc": "24px", "light": "24px"})

    def mot(d, easing):
        return {"duration": {"$type": "duration", "$value": d},
                "easing": {"$type": "cubicBezier", "$value": easing},
                "reduced-duration": {"$type": "duration", "$value": "0ms"}}

    motion = {"motion": {"feedback": {"press": mot("100ms", [0.2, 0, 0, 1])},
                         "panel": {"close": mot("250ms", [0.4, 0, 1, 1]), "open": mot("200ms", [0, 0, 0.2, 1])}}}

    def level(blur, surface, border=None):
        cue = {"surface": surface}
        if border:
            cue["border"] = border
        return {"$extensions": {"org.metrologymaven.elevation": cue},
                "$value": {"blur": blur, "color": "#00000029", "offsetX": "0px", "offsetY": "1px", "spread": "0px"}}

    elev = {"elev": {"$type": "shadow",
                     "0": level("0px", "sem.surface.base"),
                     "1": level("2px", "sem.surface.raised"),
                     "2": level("8px", "sem.surface.raised", "sem.border.strong")}}
    greys = {"0": "#ffffff", "50": "#f5f5f5", "100": "#eeeeee", "200": "#cccccc", "600": "#555555",
             "1000": "#000000"}
    primitive = {"prim": {"color": {"$type": "color", "neutral": {k: {"$value": v} for k, v in greys.items()}}}}
    # Each theme binds the five roles to five different greys, so no two cue sets look alike.
    binds = {
        "light": {"base": "0", "raised": "50", "sunken": "100", "subtle": "200", "strong": "600"},
        "dark": {"base": "1000", "raised": "600", "sunken": "200", "subtle": "100", "strong": "50"},
        "hc": {"base": "0", "raised": "50", "sunken": "100", "subtle": "600", "strong": "1000"},
        "floor": {"base": "0", "raised": "50", "sunken": "100", "subtle": "200", "strong": "600"},
    }
    role = {"base": "sem.surface.base", "raised": "sem.surface.raised", "sunken": "sem.surface.sunken",
            "subtle": "sem.border.subtle", "strong": "sem.border.strong"}

    def grey(step):
        return "{prim.color.neutral.%s}" % step

    light = binds["light"]
    colours = {"sem": {"$type": "color",
                       "border": {r: {"$value": grey(light[r])} for r in ["strong", "subtle"]},
                       "surface": {r: {"$value": grey(light[r])} for r in ["base", "raised", "sunken"]}}}
    themes = {f"tokens/themes/{t}.json": {"bindings": {role[r]: grey(s) for r, s in b.items()},
                                         "format": "fid-design-theme@1", "theme": t}
              for t, b in binds.items()}
    return {
        **themes,
        "tokens/primitive/color.tokens.json": primitive,
        "tokens/space.tokens.json": {"prim": {"space": prim}, "sem": {"space": sem}},
        "tokens/size.tokens.json": {"size": {"$type": "dimension", "target": {"min": {"$value": "32px"}}}},
        "tokens/motion.tokens.json": motion,
        "tokens/elevation.tokens.json": elev,
        "tokens/density/comfortable.json": comfortable,
        "tokens/density/compact.json": compact,
        "tokens/semantic/color.tokens.json": colours,
    }


def text_of(v):
    return v if isinstance(v, str) else json.dumps(v, indent=2, sort_keys=True) + "\n"


# ------------------------------------------------------------------ mutations and hand labels

def m_set(path, keys, value):
    def f(t):
        cur = t[path]
        for k in keys[:-1]:
            cur = cur[k]
        cur[keys[-1]] = value
    return f


def m_del(path, keys):
    def f(t):
        cur = t[path]
        for k in keys[:-1]:
            cur = cur[k]
        del cur[keys[-1]]
    return f


def m_drop_file(path):
    def f(t):
        del t[path]
    return f


def m_raw(path, text):
    def f(t):
        t[path] = text
    return f


def m_six_levels(t):
    e = t["tokens/elevation.tokens.json"]["elev"]
    cues = [("sem.surface.sunken", None), ("sem.surface.sunken", "sem.border.subtle"),
            ("sem.surface.base", "sem.border.subtle")]
    for i, (s, b) in enumerate(cues, start=3):
        lv = copy.deepcopy(e["1"])
        lv["$extensions"]["org.metrologymaven.elevation"] = {"surface": s, "border": b} if b else {"surface": s}
        e[str(i)] = lv


SPACE = "tokens/space.tokens.json"
COMF = "tokens/density/comfortable.json"
COMP = "tokens/density/compact.json"
MOTION = "tokens/motion.tokens.json"
ELEV = "tokens/elevation.tokens.json"
SIZE = "tokens/size.tokens.json"
HC = "tokens/themes/hc.json"

CASES = {
    "ok": (lambda t: None, []),
    # AC-2: a 6 px step; the bindings that alias it are reported once, at the primitive.
    "odd-space": (m_set(SPACE, ["prim", "space", "1", "$value"], "6px"), ["SPACE-GRID prim.space.1"]),
    "fraction-space": (m_set(SPACE, ["prim", "space", "2", "$value"], "8.5px"), ["SPACE-GRID prim.space.2"]),
    "sem-literal-off-grid": (m_set(SPACE, ["sem", "space", "gap", "sm", "$value"], "10px"),
                             # The comfortable binding no longer equals the default either.
                             ["SPACE-DENSITY comfortable sem.space.gap.sm", "SPACE-GRID sem.space.gap.sm"]),
    # AC-3: floor target 32 px.
    "small-target": (m_set(COMF, ["target_min", "floor"], "32px"), ["SIZE-TARGET floor comfortable"]),
    "target-hc-low": (m_set(COMP, ["target_min", "hc"], "20px"), ["SIZE-TARGET hc compact"]),
    "target-missing-theme": (m_del(COMP, ["target_min", "dark"]), ["SIZE-TARGET dark compact"]),
    "target-default": (m_set(SIZE, ["size", "target", "min", "$value"], "24px"), ["SIZE-TARGET default"]),
    # AC-4: no reduced-duration.
    "motion-no-reduced": (m_del(MOTION, ["motion", "panel", "open", "reduced-duration"]),
                          ["MOTION-REDUCED motion.panel.open"]),
    "motion-reduced-nonzero": (m_set(MOTION, ["motion", "feedback", "press", "reduced-duration", "$value"], "50ms"),
                               ["MOTION-REDUCED motion.feedback.press"]),
    "motion-bad-easing": (m_set(MOTION, ["motion", "feedback", "press", "easing", "$value"], [1.5, 0, 0, 1]),
                          ["MOTION-REDUCED motion.feedback.press"]),
    "motion-long": (m_set(MOTION, ["motion", "panel", "open", "duration", "$value"], "300ms"),
                    ["MOTION-LONG motion.panel.open"]),
    "motion-long-fraction": (m_set(MOTION, ["motion", "panel", "close", "duration", "$value"], "250.5ms"),
                             ["MOTION-LONG motion.panel.close"]),
    "density-missing": (m_del(COMP, ["bindings", "sem.space.gap.md"]), ["SPACE-DENSITY compact sem.space.gap.md"]),
    "density-differs": (m_set(COMF, ["bindings", "sem.space.gap.md"], "{prim.space.6}"),
                        ["SPACE-DENSITY comfortable sem.space.gap.md"]),
    "density-not-alias": (m_set(COMP, ["bindings", "sem.space.gap.md"], "8px"),
                          ["SPACE-DENSITY compact sem.space.gap.md"]),
    "density-unknown-name": (m_set(COMP, ["bindings", "sem.space.gap.xl"], "{prim.space.2}"),
                             ["SPACE-DENSITY compact sem.space.gap.xl"]),
    "density-wrong-format": (m_set(COMP, ["format"], "fid-design-density@2"), ["SPACE-SCHEMA tokens/density/compact.json"]),
    "elev-six": (m_six_levels, ["ELEV-LEVELS elev"]),
    "elev-gap": (m_del(ELEV, ["elev", "1"]), ["ELEV-LEVELS elev"]),
    "elev-no-cue": (m_del(ELEV, ["elev", "2", "$extensions"]), ["ELEV-NOSHADOW-CUE elev.2"]),
    "elev-same-cue": (m_set(ELEV, ["elev", "2", "$extensions", "org.metrologymaven.elevation"],
                            {"surface": "sem.surface.raised"}), ["ELEV-NOSHADOW-CUE elev.2"]),
    "elev-unknown-cue": (m_set(ELEV, ["elev", "1", "$extensions", "org.metrologymaven.elevation", "surface"],
                               "sem.surface.missing"), ["ELEV-NOSHADOW-CUE elev.1"]),
    "elev-border-as-surface": (m_set(ELEV, ["elev", "1", "$extensions", "org.metrologymaven.elevation", "surface"],
                                     "sem.border.subtle"), ["ELEV-NOSHADOW-CUE elev.1"]),
    # REQ-004 in high contrast: hc binds the raised surface to the page colour, so elev.1 (a
    # surface cue only) looks like elev.0 without shadows; elev.2 keeps its border and passes.
    "elev-hc-collision": (m_set(HC, ["bindings", "sem.surface.raised"], "{prim.color.neutral.0}"),
                          ["ELEV-NOSHADOW-CUE elev.1"]),
    "elev-cue-unresolved": (m_del(HC, ["bindings", "sem.border.strong"]), ["ELEV-NOSHADOW-CUE elev.2"]),
    "elev-huge-level": (m_set(ELEV, ["elev", "18446744073709551616"],
                              {"$value": {"blur": "0px", "color": "#00000029", "offsetX": "0px", "offsetY": "1px",
                                          "spread": "0px"}}),
                        ["SPACE-SCHEMA elev.18446744073709551616"]),
    "motion-empty": (m_raw(MOTION, "{}\n"), ["SPACE-SCHEMA tokens/motion.tokens.json"]),
    "missing-file": (m_drop_file(MOTION), ["SPACE-SCHEMA tokens/motion.tokens.json"]),
    "duplicate-key": (m_raw(SIZE, '{"size": {"$type": "dimension", "target": {"min": {"$value": "32px"}}}, '
                                  '"size": {}}\n'),
                      ["SIZE-TARGET default", "SPACE-SCHEMA tokens/size.tokens.json"]),
}


# ------------------------------------------------------------------ the independent oracle

class Dup(Exception):
    pass


def no_dups(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise Dup(k)
        out[k] = v
    return out


def load(tree, rel, out):
    """Parsed file or None (a SPACE-SCHEMA finding when missing or invalid)."""
    if rel not in tree:
        out.append(f"SPACE-SCHEMA {rel}")
        return None
    try:
        return json.loads(tree[rel], object_pairs_hook=no_dups)
    except (ValueError, Dup):
        out.append(f"SPACE-SCHEMA {rel}")
        return None


def walk(v, path=(), ty=None):
    """DTCG walk: {name: (type, value)}; a token is an object with $value."""
    out = {}
    if not isinstance(v, dict):
        return out
    ty = v.get("$type", ty)
    if "$value" in v:
        out[".".join(path)] = (ty, v["$value"])
        return out
    for k, x in v.items():
        if not k.startswith("$"):
            out.update(walk(x, path + (k,), ty))
    return out


def alias(s):
    if isinstance(s, str) and s.startswith("{") and s.endswith("}") and "{" not in s[1:-1]:
        return s[1:-1]
    return None


def px(s):
    """Exact px value as Decimal, or None."""
    if not isinstance(s, str) or not s.endswith("px"):
        return None
    try:
        d = Decimal(s[:-2])
    except InvalidOperation:
        return None
    return d if d.is_finite() and d >= 0 else None


def on_grid(s):
    d = px(s)
    return d is not None and d == d.to_integral_value() and int(d) % 4 == 0


def ms(s):
    if not isinstance(s, str) or not s.endswith("ms"):
        return None
    try:
        d = Decimal(s[:-2])
    except InvalidOperation:
        return None
    return d if d.is_finite() and d >= 0 else None


def oracle(tree):
    out = []
    raw = load(tree, SPACE, out)
    space = walk(raw) if raw is not None else {}
    prim = {n: v for n, v in space.items() if n.startswith("prim.space.")}
    sem = {n: v for n, v in space.items() if n.startswith("sem.space.")}
    for n, (_, v) in prim.items():
        if alias(v) is not None or not isinstance(v, str):
            out.append(f"SPACE-SCHEMA {n}")
        elif not on_grid(v):
            out.append(f"SPACE-GRID {n}")
    for n, (_, v) in sem.items():
        a = alias(v)
        if a is None and not on_grid(v):
            out.append(f"SPACE-GRID {n}")
        elif a is None or a not in prim:
            out.append(f"SPACE-SCHEMA {n}")
    light = None
    for mode in DENSITIES:
        rel = f"tokens/density/{mode}.json"
        d = load(tree, rel, out)
        if d is None:
            continue
        if sorted(d) != ["bindings", "density", "format", "target_min"] or d.get("format") != "fid-design-density@1" \
                or d.get("density") != mode:
            out.append(f"SPACE-SCHEMA {rel}")
        b = d.get("bindings", {})
        for n in sem:
            if n not in b:
                out.append(f"SPACE-DENSITY {mode} {n}")
        for n, v in b.items():
            if n not in sem:
                out.append(f"SPACE-DENSITY {mode} {n}")
            elif alias(v) not in prim:
                out.append(f"SPACE-DENSITY {mode} {n}")
            elif mode == "comfortable" and v != sem[n][1]:
                out.append(f"SPACE-DENSITY {mode} {n}")
        t = d.get("target_min", {})
        for theme in THEMES:
            need = 44 if theme == "floor" else 24
            v = px(t.get(theme))
            if v is None or v != v.to_integral_value() or v < need:
                out.append(f"SIZE-TARGET {theme} {mode}")
            elif theme == "light" and mode == "comfortable":
                light = t.get(theme)
    rawsize = load(tree, SIZE, out)
    size = walk(rawsize) if rawsize is not None else {}
    default = size.get("size.target.min")
    if default is None or default[0] != "dimension":
        out.append("SIZE-TARGET default")
    elif light is not None and default[1] != light:
        out.append("SIZE-TARGET default")
    rawm = load(tree, MOTION, out)
    motion = walk(rawm) if rawm is not None else {}
    groups = {}
    for n, tv in motion.items():
        g, _, part = n.rpartition(".")
        groups.setdefault(g, {})[part] = tv
    if rawm is not None and not groups:
        out.append(f"SPACE-SCHEMA {MOTION}")
    for g, parts in groups.items():
        dur = parts.get("duration")
        red = parts.get("reduced-duration")
        eas = parts.get("easing")
        bad = dur is None or dur[0] != "duration" or ms(dur[1]) is None
        bad = bad or red is None or red[0] != "duration" or ms(red[1]) != 0
        ok_e = (eas is not None and eas[0] == "cubicBezier" and isinstance(eas[1], list) and len(eas[1]) == 4
                and all(isinstance(x, (int, float)) and not isinstance(x, bool) for x in eas[1])
                and 0 <= eas[1][0] <= 1 and 0 <= eas[1][2] <= 1)
        if bad or not ok_e:
            out.append(f"MOTION-REDUCED {g}")
        if g.startswith("motion.panel.") and dur is not None and ms(dur[1]) is not None and ms(dur[1]) > 250:
            out.append(f"MOTION-LONG {g}")
    rawe = load(tree, ELEV, out)
    rawc = load(tree, "tokens/semantic/color.tokens.json", out)
    colours = walk(rawc) if rawc is not None else {}
    levels = {}
    for k, v in (rawe or {}).get("elev", {}).items():
        if k.startswith("$"):
            continue
        if re.fullmatch(r"0|[1-9][0-9]*", k) and int(k) < 2 ** 64:
            levels[int(k)] = v
        else:
            out.append(f"SPACE-SCHEMA elev.{k}")
    keys = sorted(levels)
    if len(keys) > 5 or keys != list(range(len(keys))) or not keys:
        out.append("ELEV-LEVELS elev")
    every = {}
    for rel, text in tree.items():
        if rel.startswith("tokens/") and rel.endswith(".tokens.json"):
            try:
                every.update(walk(json.loads(text, object_pairs_hook=no_dups)))
            except (ValueError, Dup):
                pass
    bindings = {}
    for theme in THEMES:
        try:
            bindings[theme] = json.loads(tree.get(f"tokens/themes/{theme}.json", "{}")).get("bindings", {})
        except ValueError:
            bindings[theme] = {}

    def resolve(name, theme):
        for _ in range(17):
            if name.startswith("sem."):
                v = bindings[theme].get(name)
            else:
                v = every.get(name, (None, None))[1]
            a = alias(v)
            if a is not None:
                name = a
            elif name.startswith("prim.") and isinstance(v, str) and re.fullmatch(r"#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})", v):
                v = v.lower()
                return v if len(v) == 9 else v + "ff"
            else:
                return None
        return None

    seen = set()
    looks = {theme: [] for theme in THEMES}
    for k in keys:
        cue = levels[k].get("$extensions", {}).get("org.metrologymaven.elevation")
        s = cue.get("surface") if isinstance(cue, dict) else None
        b = cue.get("border") if isinstance(cue, dict) else None
        good = (isinstance(cue, dict) and set(cue) <= {"surface", "border"} and (s or b)
                and (s is None or (s.startswith("sem.surface.") and s in colours))
                and (b is None or (b.startswith("sem.border.") and b in colours)))
        if not good or (s, b) in seen:
            out.append(f"ELEV-NOSHADOW-CUE elev.{k}")
            continue
        seen.add((s, b))
        mine = {}
        for theme in THEMES:
            rs = resolve(s, theme) if s else "none"
            rb = resolve(b, theme) if b else "none"
            if rs is None or rb is None or (rs, rb) in looks[theme]:
                out.append(f"ELEV-NOSHADOW-CUE elev.{k}")
                break
            mine[theme] = (rs, rb)
        else:
            for theme, look in mine.items():
                looks[theme].append(look)
    return sorted(out)


# ------------------------------------------------------------------ output

def build():
    files = {}
    expected = {}
    for name, (mutate, labels) in CASES.items():
        tree = base()
        mutate(tree)
        texts = {rel: text_of(v) for rel, v in tree.items()}
        got = oracle(texts)
        if got != sorted(labels):
            raise SystemExit(f"{name}: oracle {got} != hand labels {sorted(labels)}")
        for rel, t in texts.items():
            files[f"{name}/{rel}"] = t
        expected[name] = {"exit": 1 if labels else 0, "findings": sorted(labels)}
    shipped = {}
    for p in sorted((DESIGN / "tokens").rglob("*")):
        if p.is_file():
            shipped[p.relative_to(DESIGN).as_posix()] = p.read_text(encoding="utf-8")
    got = oracle(shipped)
    if got:
        raise SystemExit(f"shipped design tree: {got}")
    expected_doc = {"fixtures": expected, "shipped": {"exit": 0, "findings": []}}
    files["expected.json"] = json.dumps(expected_doc, indent=2, sort_keys=True) + "\n"
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    stale = []
    existing = {p.relative_to(HERE).as_posix() for p in HERE.rglob("*") if p.is_file()} - {"make_fixtures.py"}
    for rel, text in files.items():
        path = HERE / rel
        if check:
            if not path.is_file() or path.read_bytes() != text.encode("utf-8"):
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(text.encode("utf-8"))
    extra = sorted(existing - set(files))
    if check:
        stale += extra
    else:
        for rel in extra:
            (HERE / rel).unlink()
        for d in sorted((p for p in HERE.rglob("*") if p.is_dir()), reverse=True):
            if not any(d.iterdir()):
                shutil.rmtree(d)
    if stale:
        print("stale: " + ", ".join(stale))
        return 1
    print(f"{len(CASES)} fixtures {'up to date' if check else 'written'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
