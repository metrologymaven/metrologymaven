#!/usr/bin/env python3
"""Writes the shipped colour tokens of spec node UX.TOKENS.COLOR into `design/tokens/`.

Python stdlib only. Run from any directory:

    python fixtures/ux/tokens/color/make_tokens.py          # (re)write the token files
    python fixtures/ux/tokens/color/make_tokens.py --check  # fail if a shipped file differs
    python fixtures/ux/tokens/color/make_tokens.py --derive # print the derived primitive ramps

This is the authoring source of the colour values, not a test oracle: the independent checks
live in `make_fixtures.py`, which reads the files written here and never imports this module.

Every file is written in one canonical form (UTF-8, keys sorted, two-space indent, LF, one
trailing newline), so the same source always gives byte-identical files.

The primitive ramps are original: each hue is a fixed OKLCH hue angle and chroma chosen for
this project, sampled at fixed OKLab lightness steps and reduced in chroma until it fits the
sRGB gamut. No ramp, step value or hex value is taken from another product's palette.
OKLab is Bjorn Ottosson's published colour space (2020); only its conversion formulas are used.
"""

import json
import math
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
DESIGN = ROOT / "design"

THEMES = ["light", "dark", "hc", "floor"]

# ------------------------------------------------------------------ primitive ramps

# Step name -> OKLab lightness.
STEPS = [
    ("50", 0.975),
    ("100", 0.945),
    ("200", 0.890),
    ("300", 0.810),
    ("400", 0.720),
    ("500", 0.630),
    ("600", 0.545),
    ("700", 0.465),
    ("800", 0.385),
    ("900", 0.305),
    ("950", 0.230),
]

# Hue name -> (OKLCH hue angle in degrees, peak chroma). Chosen for this project.
HUES = {
    "neutral": (255.0, 0.014),
    "blue": (259.0, 0.165),
    "teal": (192.0, 0.105),
    "green": (148.0, 0.150),
    "amber": (78.0, 0.150),
    "orange": (52.0, 0.170),
    "red": (26.0, 0.190),
    "violet": (296.0, 0.170),
    "magenta": (338.0, 0.170),
    # Added by UX.TOKENS.STATUS for the locked status palette (tokens/status.json).
    "azure": (225.0, 0.150),
    "yellow": (95.0, 0.150),
    "olive": (115.0, 0.130),
}


def oklab_to_linear_srgb(L, a, b):
    l_ = L + 0.3963377774 * a + 0.2158037573 * b
    m_ = L - 0.1055613458 * a - 0.0638541728 * b
    s_ = L - 0.0894841775 * a - 1.2914855480 * b
    l, m, s = l_ ** 3, m_ ** 3, s_ ** 3
    return (
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s,
    )


def encode(c):
    c = min(1.0, max(0.0, c))
    return 12.92 * c if c <= 0.0031308 else 1.055 * c ** (1 / 2.4) - 0.055


def in_gamut(rgb):
    return all(-1e-9 <= c <= 1 + 1e-9 for c in rgb)


def oklch_hex(L, C, h):
    """Largest chroma <= C (by bisection) that fits sRGB, as #rrggbb."""
    rad = math.radians(h)

    def rgb(c):
        return oklab_to_linear_srgb(L, c * math.cos(rad), c * math.sin(rad))

    lo, hi = 0.0, C
    if not in_gamut(rgb(hi)):
        for _ in range(60):
            mid = (lo + hi) / 2
            if in_gamut(rgb(mid)):
                lo = mid
            else:
                hi = mid
        hi = lo
    return "#" + "".join(f"{round(encode(c) * 255):02x}" for c in rgb(hi))


def chroma_at(L, peak):
    # Taper the chroma towards the light and dark ends of the ramp.
    t = 1.0 - abs(L - 0.6) / 0.45
    return peak * max(0.25, min(1.0, t * 1.4))


def derive_ramps():
    ramps = {}
    for hue, (h, peak) in HUES.items():
        ramps[hue] = {step: oklch_hex(L, chroma_at(L, peak), h) for step, L in STEPS}
    ramps["neutral"]["0"] = "#ffffff"
    ramps["neutral"]["1000"] = "#000000"
    return ramps


# Frozen output of derive_ramps() (run `make_tokens.py --derive` to print it again). The hex
# values are the source of truth, so a last-bit difference in a platform's pow() cannot change
# the shipped files.
RAMPS = {
    "neutral": {
        "50": "#f5f7f9", "100": "#ebedf0", "200": "#d8dbdf", "300": "#bcc1c8", "400": "#9fa5ad",
        "500": "#848a92", "600": "#6b7178", "700": "#545a61", "800": "#404449", "900": "#2d2f32",
        "950": "#1c1d1f", "0": "#ffffff", "1000": "#000000",
    },
    "blue": {
        "50": "#f2f7ff", "100": "#e3eeff", "200": "#c6dcff", "300": "#9cc3ff", "400": "#6aa4ff",
        "500": "#4687eb", "600": "#2c6cce", "700": "#1454b2", "800": "#164183", "900": "#142e56",
        "950": "#111d30",
    },
    "teal": {
        "50": "#e4fdfb", "100": "#d4f5f3", "200": "#b4e6e4", "300": "#82d1ce", "400": "#41b9b5",
        "500": "#149d99", "600": "#00817e", "700": "#006765", "800": "#004f4d", "900": "#033736",
        "950": "#0d2120",
    },
    "green": {
        "50": "#e7fee9", "100": "#d8f7db", "200": "#bae9bf", "300": "#8ed598", "400": "#59be6c",
        "500": "#3aa151", "600": "#158638", "700": "#006d28", "800": "#03531e", "900": "#0f3919",
        "950": "#102213",
    },
    "amber": {
        "50": "#fff5e7", "100": "#ffeaca", "200": "#f6d5a3", "300": "#e9b869", "400": "#d79700",
        "500": "#b47d00", "600": "#946600", "700": "#775100", "800": "#5b3d00", "900": "#402a00",
        "950": "#271b07",
    },
    "orange": {
        "50": "#fff4ee", "100": "#ffe8da", "200": "#ffcfb3", "300": "#ffaa75", "400": "#f58028",
        "500": "#d26600", "600": "#ad5300", "700": "#8c4200", "800": "#6b3100", "900": "#4d2100",
        "950": "#2c1709",
    },
    "red": {
        "50": "#fff4f2", "100": "#ffe6e3", "200": "#ffcdc7", "300": "#ffa69c", "400": "#ff7269",
        "500": "#e54c48", "600": "#c72d2e", "700": "#a90617", "800": "#7d1517", "900": "#541715",
        "950": "#301311",
    },
    "violet": {
        "50": "#f7f5ff", "100": "#eee9ff", "200": "#ded3ff", "300": "#c7b3ff", "400": "#af8cff",
        "500": "#946fe3", "600": "#7b54c6", "700": "#643daa", "800": "#4b317d", "900": "#332453",
        "950": "#1f182f",
    },
    "magenta": {
        "50": "#fff3fb", "100": "#ffe4f6", "200": "#ffc7ed", "300": "#f6a0dd", "400": "#e676c9",
        "500": "#c85aac", "600": "#ab3f92", "700": "#902679", "800": "#6b225a", "900": "#481c3d",
        "950": "#2a1524",
    },
    "azure": {
        "50": "#ecf9ff", "100": "#d5f3ff", "200": "#a7e6ff", "300": "#66d0f7", "400": "#00b5e4",
        "500": "#0097bf", "600": "#007c9d", "700": "#00637e", "800": "#004b61", "900": "#003545",
        "950": "#05202a",
    },
    "yellow": {
        "50": "#fef7db", "100": "#f7edc8", "200": "#eadba3", "300": "#d7c068", "400": "#c2a200",
        "500": "#a28700", "600": "#856e00", "700": "#6a5800", "800": "#514300", "900": "#392e00",
        "950": "#221d06",
    },
    "olive": {
        "50": "#f6fae1", "100": "#ebf1d1", "200": "#d9e0b0", "300": "#bfc97f", "400": "#a2ae44",
        "500": "#879223", "600": "#6e7800", "700": "#586000", "800": "#434900", "900": "#2e3206",
        "950": "#1c1f0c",
    },
}


def primitives():
    return RAMPS


# ------------------------------------------------------------------ semantic roles

P = "prim.color."

# name -> (description, {theme: primitive "<hue>.<step>"}).
NOCOLOR = " Colour is never the only cue (principle P-NOCOLOR): always shown with a word and a symbol."
STATUS_NOTE = (
    " Locked value of the status palette tokens/status.json (spec node UX.TOKENS.STATUS); shown"
    " with glyph state.%s and the text of message icon.state.%s."
)
CLAIMED = (
    " A claimed value never looks measured: in every theme this role uses a hue ramp that no"
    " measured-value or brand-tier role uses, stays at least 0.10 in OKLab from every measured,"
    " brand-tier and status role, and is always shown with the word 'claimed' (UNC.UEC.CLASS)."
)

SEM = {
    # text
    "sem.text.primary": ("Body text and default numbers.", dict(light="neutral.900", dark="neutral.50", hc="neutral.1000", floor="neutral.950")),
    "sem.text.secondary": ("Labels and secondary text.", dict(light="neutral.800", dark="neutral.200", hc="neutral.1000", floor="neutral.800")),
    "sem.text.muted": ("Hints and placeholders (still meets text contrast).", dict(light="neutral.700", dark="neutral.300", hc="neutral.950", floor="neutral.700")),
    "sem.text.inverse": ("Text on the inverse surface.", dict(light="neutral.50", dark="neutral.950", hc="neutral.0", floor="neutral.0")),
    "sem.text.link": ("Link text; links are also underlined.", dict(light="blue.700", dark="blue.300", hc="blue.800", floor="blue.800")),
    "sem.text.value.measured": ("A measured value.", dict(light="neutral.900", dark="neutral.50", hc="neutral.1000", floor="neutral.950")),
    "sem.text.value.claimed": ("A claimed (declared or specification-derived) value." + CLAIMED, dict(light="magenta.700", dark="magenta.200", hc="magenta.800", floor="magenta.700")),
    # surfaces
    "sem.surface.base": ("Application background.", dict(light="neutral.0", dark="neutral.950", hc="neutral.0", floor="neutral.0")),
    "sem.surface.raised": ("Panels and cards above the base.", dict(light="neutral.50", dark="neutral.900", hc="neutral.0", floor="neutral.50")),
    "sem.surface.sunken": ("Wells, table headers.", dict(light="neutral.100", dark="neutral.1000", hc="neutral.50", floor="neutral.100")),
    "sem.surface.inverse": ("Inverse surface (tooltips, toasts).", dict(light="neutral.900", dark="neutral.100", hc="neutral.1000", floor="neutral.900")),
    "sem.surface.value.claimed": ("Background tint behind a claimed value." + CLAIMED, dict(light="magenta.200", dark="magenta.900", hc="magenta.200", floor="magenta.200")),
    # borders
    "sem.border.subtle": ("Decorative dividers (no contrast obligation).", dict(light="neutral.200", dark="neutral.800", hc="neutral.700", floor="neutral.300")),
    "sem.border.strong": ("Boundaries of inputs and controls.", dict(light="neutral.600", dark="neutral.400", hc="neutral.1000", floor="neutral.600")),
    "sem.border.value.claimed": ("Dashed outline around a claimed value." + CLAIMED, dict(light="magenta.600", dark="magenta.400", hc="magenta.800", floor="magenta.700")),
    # focus
    "sem.focus.ring": ("Keyboard focus indicator.", dict(light="blue.600", dark="blue.300", hc="blue.800", floor="blue.700")),
    # status: the locked status palette of UX.TOKENS.STATUS (also written to tokens/status.json)
    "sem.status.conforms": ("Display state CONFORMS." + NOCOLOR + STATUS_NOTE % ("conforms", "conforms"), dict(light="azure.700", dark="azure.400", hc="azure.800", floor="azure.700")),
    "sem.status.nonconforms": ("Display state NONCONFORMS." + NOCOLOR + STATUS_NOTE % ("nonconforms", "nonconforms"), dict(light="red.600", dark="red.400", hc="red.700", floor="red.600")),
    "sem.status.indeterminate": ("Display state INDETERMINATE." + NOCOLOR + STATUS_NOTE % ("indeterminate", "indeterminate"), dict(light="yellow.900", dark="yellow.200", hc="yellow.950", floor="yellow.900")),
    "sem.status.not-decidable": ("Display state NOT_DECIDABLE." + NOCOLOR + STATUS_NOTE % ("not-decidable", "not-decidable"), dict(light="olive.950", dark="olive.50", hc="olive.950", floor="olive.950")),
    "sem.status.not-evaluated": ("Display state not evaluated." + NOCOLOR + STATUS_NOTE % ("not-evaluated", "not-evaluated"), dict(light="neutral.950", dark="neutral.50", hc="neutral.1000", floor="neutral.950")),
    "sem.status.stale": ("Display state stale." + NOCOLOR + STATUS_NOTE % ("stale", "stale"), dict(light="orange.700", dark="orange.400", hc="orange.800", floor="orange.700")),
    # brand (the brand tier; brand-neutral defaults until OUTR.BRAND.GUIDE.VISUAL)
    "sem.brand.accent": ("Brand accent: primary buttons and selection. Brand tier.", dict(light="blue.600", dark="blue.300", hc="blue.800", floor="blue.700")),
    "sem.brand.accent-hover": ("Brand accent, hovered. Brand tier.", dict(light="blue.700", dark="blue.400", hc="blue.900", floor="blue.800")),
    "sem.brand.on-accent": ("Text and icons on the brand accent. Brand tier.", dict(light="neutral.0", dark="neutral.950", hc="neutral.0", floor="neutral.0")),
    "sem.brand.subtle": ("Brand tint for selected rows. Brand tier.", dict(light="blue.50", dark="blue.950", hc="blue.50", floor="blue.50")),
    # agent provenance
    "sem.agent.accent": ("Accent of agent-authored content; always with the agent icon and label (UX.PATTERN).", dict(light="teal.600", dark="teal.400", hc="teal.800", floor="teal.700")),
    "sem.agent.surface": ("Background of agent-authored proposals.", dict(light="teal.50", dark="teal.950", hc="neutral.0", floor="teal.50")),
    "sem.agent.text": ("Text that labels agent-authored content.", dict(light="teal.800", dark="teal.200", hc="teal.900", floor="teal.900")),
}

BRAND_TIER = ["sem.brand.accent", "sem.brand.accent-hover", "sem.brand.on-accent", "sem.brand.subtle"]

# Component aliases: file -> {name: (semantic target, description)}.
COMP = {
    "button": {
        "comp.button.bg.primary": ("sem.brand.accent", "Primary button fill."),
        "comp.button.bg.primary-hover": ("sem.brand.accent-hover", "Primary button fill, hovered."),
        "comp.button.fg.primary": ("sem.brand.on-accent", "Primary button label."),
        "comp.button.fg.secondary": ("sem.text.primary", "Secondary button label."),
        "comp.button.border.secondary": ("sem.border.strong", "Secondary button outline."),
    },
    "input": {
        "comp.input.bg.default": ("sem.surface.base", "Input field fill."),
        "comp.input.fg.default": ("sem.text.primary", "Input text."),
        "comp.input.fg.placeholder": ("sem.text.muted", "Placeholder text."),
        "comp.input.border.default": ("sem.border.strong", "Input boundary."),
        "comp.input.border.focus": ("sem.focus.ring", "Input boundary with keyboard focus."),
    },
    "table": {
        "comp.table.bg.header": ("sem.surface.sunken", "Header row fill."),
        "comp.table.fg.header": ("sem.text.secondary", "Header row text."),
        "comp.table.bg.selected": ("sem.brand.subtle", "Selected row fill."),
        "comp.table.border.row": ("sem.border.subtle", "Row divider."),
    },
    "value": {
        "comp.value.fg.measured": ("sem.text.value.measured", "A measured number."),
        "comp.value.fg.claimed": ("sem.text.value.claimed", "A claimed number; the cell also shows the word 'claimed'."),
        "comp.value.bg.claimed": ("sem.surface.value.claimed", "Cell tint behind a claimed number."),
        "comp.value.border.claimed": ("sem.border.value.claimed", "Dashed outline of a claimed number."),
    },
}

TEXT_FG = ["sem.text.primary", "sem.text.secondary", "sem.text.muted", "sem.text.link", "sem.text.value.measured", "sem.text.value.claimed"]
PLAIN_BG = ["sem.surface.base", "sem.surface.raised", "sem.surface.sunken"]
STATUS = [n for n in SEM if n.startswith("sem.status.")]


def pairs():
    out = []
    for fg in TEXT_FG:
        for bg in PLAIN_BG:
            out.append((fg, bg, "text"))
    for fg in ["sem.text.primary", "sem.text.secondary", "sem.text.value.claimed"]:
        out.append((fg, "sem.surface.value.claimed", "text"))
    out.append(("sem.text.inverse", "sem.surface.inverse", "text"))
    out.append(("sem.brand.on-accent", "sem.brand.accent", "text"))
    out.append(("sem.brand.on-accent", "sem.brand.accent-hover", "text"))
    for fg in ["sem.text.primary", "sem.text.secondary", "sem.text.link"]:
        out.append((fg, "sem.brand.subtle", "text"))
    for fg in ["sem.agent.text", "sem.text.primary"]:
        out.append((fg, "sem.agent.surface", "text"))
    for s in STATUS:
        for bg in ["sem.surface.base", "sem.surface.raised"]:
            out.append((s, bg, "text"))
    for fg in ["sem.border.strong", "sem.focus.ring", "sem.brand.accent", "sem.agent.accent"]:
        for bg in PLAIN_BG:
            out.append((fg, bg, "non-text"))
    for bg in ["sem.surface.base", "sem.surface.raised", "sem.surface.value.claimed"]:
        out.append(("sem.border.value.claimed", bg, "non-text"))
    out.append(("sem.focus.ring", "sem.brand.subtle", "non-text"))
    out.append(("sem.text.value.claimed", "sem.brand.subtle", "text"))
    for s in STATUS:
        out.append((s, "sem.surface.sunken", "text"))
    for bg in ["sem.surface.value.claimed", "sem.agent.surface"]:
        out.append(("sem.focus.ring", bg, "non-text"))
    return sorted(set(out))


# ------------------------------------------------------------------ status palette (UX.TOKENS.STATUS)

# The six display states, named as in `sem.status.<state>` and the glyph IDs `state.<state>` of
# UX.ICONS.CLASS. `out-of-tolerance` names the out-of-tolerance region colours of the deviation
# palettes (UX.TOKENS.DATAPAL); it is a nonconforming-set member without a colour of its own here.
STATUS_STATES = ["conforms", "nonconforms", "indeterminate", "not-decidable", "not-evaluated", "stale"]
STATUS_SETS = {"conforming": ["conforms"], "nonconforming": ["nonconforms", "out-of-tolerance", "stale"]}
STATUS_REFS = [
    "The UX.TOKENS.STATUS, UX.ICONS.CLASS and VIZ.OVERLAY.STATUS spec nodes in this repository",
    "The CIEDE2000 colour-difference formula (Sharma, Wu and Dalal, 2005), used only to verify the values",
    "The colour-vision-deficiency simulation matrices of Machado, Oliveira and Fernandes (2009) at severity 1.0, used only to verify the values",
]
AUTHOR = "Claude Code (Claude Opus 5.5), agent author of UX.TOKENS.COLOR; values generated by fixtures/ux/tokens/color/make_tokens.py from hue angles chosen for this project"
AUTHOR_STATUS = "Claude Code (Claude Opus 5.5), agent author of UX.TOKENS.COLOR and UX.TOKENS.STATUS; values generated by fixtures/ux/tokens/color/make_tokens.py from hue angles chosen for this project, status steps selected for this project"
# Files whose content UX.TOKENS.STATUS changed.
STATUS_FILES = {
    "tokens/status.json",
    "tokens/primitive/color.tokens.json",
    "tokens/semantic/color.tokens.json",
    "tokens/themes/light.json",
    "tokens/themes/dark.json",
    "tokens/themes/hc.json",
    "tokens/themes/floor.json",
}


def status_cues():
    """Glyph (or pattern) and accessible-name IDs of UX.ICONS.CLASS for every state, and for the
    out-of-tolerance region, whose colour comes from UX.TOKENS.DATAPAL but whose stripe pattern
    must still go with it."""
    cues = {s: {"glyph": "state." + s, "text": "icon.state." + s} for s in STATUS_STATES}
    cues["out-of-tolerance"] = {"glyph": "pattern.out-of-tolerance", "text": "icon.pattern.out-of-tolerance"}
    return cues


def status_palette():
    """`tokens/status.json` (`status-palette@1`): the hex value of every `sem.status.<state>`
    binding in every theme, so the semantic tokens alias exactly these values."""
    themes = {}
    for theme in THEMES:
        row = {}
        for state in STATUS_STATES:
            hue, step = SEM["sem.status." + state][1][theme].split(".")
            row[state] = RAMPS[hue][step]
        themes[theme] = row
    return {
        "format": "status-palette@1",
        "locked": True,
        "formula": "ciede2000",
        "min_delta_e2000": 20,
        "sets": STATUS_SETS,
        "cues": status_cues(),
        "themes": themes,
    }


# ------------------------------------------------------------------ writers


def canon(obj):
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def nest(flat, root_type=None):
    """{"a.b.c": token} -> nested DTCG groups."""
    out = {}
    for name, tok in flat.items():
        cur = out
        segs = name.split(".")
        for s in segs[:-1]:
            cur = cur.setdefault(s, {})
        cur[segs[-1]] = tok
    if root_type:
        for top in out.values():
            top["$type"] = root_type
    return out


def files():
    ramps = primitives()
    out = {}
    prim = {}
    for hue, steps in ramps.items():
        for step, hexv in steps.items():
            prim[f"{P}{hue}.{step}"] = {"$value": hexv}
    out["tokens/primitive/color.tokens.json"] = nest(prim, "color")

    sem = {}
    for name, (desc, binding) in SEM.items():
        assert set(binding) == set(THEMES), name
        sem[name] = {"$value": "{" + P + binding["light"] + "}", "$description": desc + " The value here is the light binding; every theme binds it in tokens/themes/."}
    out["tokens/semantic/color.tokens.json"] = nest(sem, "color")

    for comp, toks in COMP.items():
        flat = {n: {"$value": "{" + t + "}", "$description": d} for n, (t, d) in toks.items()}
        out[f"tokens/component/{comp}.tokens.json"] = nest(flat, "color")

    for theme in THEMES:
        out[f"tokens/themes/{theme}.json"] = {
            "format": "fid-design-theme@1",
            "theme": theme,
            "bindings": {n: "{" + P + b[theme] + "}" for n, (_, b) in SEM.items()},
        }
    out["tokens/brand-tier.json"] = {"format": "fid-design-brand-tier@1", "tokens": BRAND_TIER}
    out["tokens/pairs.json"] = {
        "format": "fid-design-pairs@1",
        "pairs": [{"fg": f, "bg": b, "tag": t} for f, b, t in pairs()],
    }

    out["tokens/status.json"] = status_palette()

    decl_refs = [
        "The UX.TOKENS, UX.TOKENS.COLOR and UX.SYSTEM.PRINC spec nodes in this repository",
        "WCAG 2.2 success criteria 1.4.3, 1.4.6 and 1.4.11 (contrast thresholds only)",
        "The OKLab colour space conversion formulas published by Bjorn Ottosson (2020)",
    ]
    for rel in list(out):
        out[f"review/origin/{rel}.origin.json"] = {
            "artefact": rel,
            "author": AUTHOR_STATUS if rel in STATUS_FILES else AUTHOR,
            "date": "2026-11-20",
            "references": decl_refs + (STATUS_REFS if rel == "tokens/status.json" else []),
            "statement_version": "originality-statement@1",
        }
    return {k: canon(v) for k, v in out.items()}


def main():
    if "--derive" in sys.argv[1:]:
        print(json.dumps(derive_ramps(), indent=2))
        return 0
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
        print("out of date (run make_tokens.py):\n  " + "\n  ".join(bad), file=sys.stderr)
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
