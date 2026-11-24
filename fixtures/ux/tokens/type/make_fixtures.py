#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.TOKENS.TYPE and checks their hand labels.

Synthetic data, CC-BY-4.0. No font binaries are committed. Python stdlib only. Run from any
directory:

    python fixtures/ux/tokens/type/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/tokens/type/make_fixtures.py --check  # fail if files differ or labels disagree

Produces:
- `allow.txt` (`OFL-1.1`, `Apache-2.0`, `MIT`, AC-1)
- `proprietary/` (AC-2: FONT-LICENCE)
- `no-tnum/` (AC-3: FONT-TNUM type.numeric, family without tabular_numerals)
- `no-tnum-feature/` (FONT-TNUM type.numeric, missing `tnum` feature)
- `small-floor/` (AC-4: TYPE-SIZE floor type.body at 14px)
- `small-standard/` (TYPE-SIZE light type.label at 11px)
- `unknown-font/` (FONT-UNKNOWN type.code)
- `missing-role/` (TYPE-ROLE-MISSING type.heading.3)
- `expected.json` (hand-labelled expectations, cross-checked by an independent Python oracle)
"""

import copy
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[3]
DESIGN = REPO / "design"

THEMES = ("light", "dark", "hc", "floor")
REQUIRED_ROLES = (
    "type.body",
    "type.body.small",
    "type.code",
    "type.family.display",
    "type.heading.1",
    "type.heading.2",
    "type.heading.3",
    "type.label",
    "type.numeric",
    "type.numeric.large",
)

ALLOW_TXT = """\
# Synthetic SPDX font licence allowlist for UX.TOKENS.TYPE fixtures (CC-BY-4.0).
OFL-1.1
Apache-2.0
MIT
"""


def js(v) -> bytes:
    return (json.dumps(v, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")


def base_fonts():
    return {
        "format": "font-list@1",
        "families": [
            {
                "family": "Synth Sans",
                "files": ["SynthSans-Regular.woff2", "SynthSans-Medium.woff2", "SynthSans-SemiBold.woff2"],
                "licence": "OFL-1.1",
                "source": "https://example.invalid/synth-sans",
                "tabular_numerals": True,
                "scripts": ["Latin"],
            },
            {
                "family": "Synth Mono",
                "files": ["SynthMono-Regular.woff2"],
                "licence": "Apache-2.0",
                "source": "https://example.invalid/synth-mono",
                "tabular_numerals": True,
                "scripts": ["Latin"],
            },
            {
                "family": "Synth Display",
                "files": ["SynthDisplay-Bold.woff2"],
                "licence": "MIT",
                "source": "https://example.invalid/synth-display",
                "tabular_numerals": False,
                "scripts": ["Latin"],
            },
        ],
    }


def make_role(family, size_std, size_floor, weight=400, lh=1.4, tnum=False):
    out = {}
    for t in THEMES:
        v = {
            "fontFamily": family,
            "fontSize": f"{size_floor if t == 'floor' else size_std}px",
            "fontWeight": weight,
            "lineHeight": lh,
        }
        if tnum:
            v["fontFeatureSettings"] = ["tnum"]
        out[t] = {"$value": v}
    return out


def base_type_tokens():
    body = make_role("Synth Sans", 14, 18, 400, 1.43)
    body["small"] = make_role("Synth Sans", 12, 16, 400, 1.33)
    numeric = make_role("Synth Sans", 14, 18, 400, 1.43, tnum=True)
    numeric["large"] = make_role("Synth Sans", 24, 32, 600, 1.2, tnum=True)
    return {
        "type": {
            "$type": "typography",
            "body": body,
            "label": make_role("Synth Sans", 12, 16, 500, 1.33),
            "heading": {
                "1": make_role("Synth Sans", 24, 30, 600, 1.25),
                "2": make_role("Synth Sans", 18, 24, 600, 1.33),
                "3": make_role("Synth Sans", 15, 20, 600, 1.33),
            },
            "numeric": numeric,
            "code": make_role("Synth Mono", 13, 16, 400, 1.46, tnum=True),
            "family": {
                "display": make_role("Synth Display", 28, 36, 600, 1.2),
            },
        }
    }


def build_cases():
    cases = {}

    # AC-2: proprietary/ — family with a non-allowlisted licence.
    f_prop = base_fonts()
    f_prop["families"][0]["licence"] = "LicenseRef-Proprietary-Corp"
    cases["proprietary"] = {
        "tokens/fonts.json": js(f_prop),
        "tokens/type.tokens.json": js(base_type_tokens()),
    }

    # AC-3: no-tnum/ — type.numeric bound to Synth Display (tabular_numerals = false).
    t_notnum = base_type_tokens()
    for t in THEMES:
        t_notnum["type"]["numeric"][t]["$value"]["fontFamily"] = "Synth Display"
    cases["no-tnum"] = {
        "tokens/fonts.json": js(base_fonts()),
        "tokens/type.tokens.json": js(t_notnum),
    }

    # Missing tnum feature setting on type.numeric (family supports it, but feature omitted).
    t_nofeat = base_type_tokens()
    for t in THEMES:
        del t_nofeat["type"]["numeric"][t]["$value"]["fontFeatureSettings"]
    cases["no-tnum-feature"] = {
        "tokens/fonts.json": js(base_fonts()),
        "tokens/type.tokens.json": js(t_nofeat),
    }

    # AC-4: small-floor/ — floor type.body at 14px (below 16px floor minimum).
    t_small_floor = base_type_tokens()
    t_small_floor["type"]["body"]["floor"]["$value"]["fontSize"] = "14px"
    cases["small-floor"] = {
        "tokens/fonts.json": js(base_fonts()),
        "tokens/type.tokens.json": js(t_small_floor),
    }

    # Light label at 11px (below 12px standard minimum).
    t_small_std = base_type_tokens()
    t_small_std["type"]["label"]["light"]["$value"]["fontSize"] = "11px"
    cases["small-standard"] = {
        "tokens/fonts.json": js(base_fonts()),
        "tokens/type.tokens.json": js(t_small_std),
    }

    # Unknown font family on type.code.
    t_unk = base_type_tokens()
    for t in THEMES:
        t_unk["type"]["code"][t]["$value"]["fontFamily"] = "Unlisted Mono"
    cases["unknown-font"] = {
        "tokens/fonts.json": js(base_fonts()),
        "tokens/type.tokens.json": js(t_unk),
    }

    # Missing required role type.heading.3.
    t_miss = base_type_tokens()
    del t_miss["type"]["heading"]["3"]
    cases["missing-role"] = {
        "tokens/fonts.json": js(base_fonts()),
        "tokens/type.tokens.json": js(t_miss),
    }

    return cases


# Hand-labelled expectations derived directly from UX.TOKENS.TYPE Requirements, Design and AC-1..4.
LABELS = {
    "proprietary": {
        "root": "proprietary",
        "exit": 1,
        "findings": ["FONT-LICENCE Synth Sans"],
    },
    "no-tnum": {
        "root": "no-tnum",
        "exit": 1,
        "findings": ["FONT-TNUM type.numeric"],
    },
    "no-tnum-feature": {
        "root": "no-tnum-feature",
        "exit": 1,
        "findings": ["FONT-TNUM type.numeric"],
    },
    "small-floor": {
        "root": "small-floor",
        "exit": 1,
        "findings": ["TYPE-SIZE floor type.body"],
    },
    "small-standard": {
        "root": "small-standard",
        "exit": 1,
        "findings": ["TYPE-SIZE light type.label"],
    },
    "unknown-font": {
        "root": "unknown-font",
        "exit": 1,
        "findings": ["FONT-UNKNOWN type.code"],
    },
    "missing-role": {
        "root": "missing-role",
        "exit": 1,
        "findings": ["TYPE-ROLE-MISSING type.heading.3"],
    },
}


def collect_tokens(obj, prefix=()):
    if "$value" in obj:
        yield ".".join(prefix), obj["$value"]
        return
    for k, v in obj.items():
        if k.startswith("$"):
            continue
        yield from collect_tokens(v, prefix + (k,))


def oracle_check(root_dir: pathlib.Path, allow_text: str):
    allow = {ln.strip() for ln in allow_text.splitlines() if ln.strip() and not ln.strip().startswith("#")}
    fonts_doc = json.loads((root_dir / "tokens/fonts.json").read_text(encoding="utf-8"))
    type_doc = json.loads((root_dir / "tokens/type.tokens.json").read_text(encoding="utf-8"))
    fams = {e["family"]: e for e in fonts_doc["families"]}
    findings = []
    for name, fam in fams.items():
        if fam["licence"] not in allow:
            findings.append(f"FONT-LICENCE {name}")
    roles = {}
    for full, val in collect_tokens(type_doc):
        role, theme = full.rsplit(".", 1)
        roles.setdefault(role, {})[theme] = val
    for r in REQUIRED_ROLES:
        if r not in roles or any(t not in roles[r] for t in THEMES):
            findings.append(f"TYPE-ROLE-MISSING {r}")
    for role, by_theme in roles.items():
        for theme, val in by_theme.items():
            fam_name = val["fontFamily"]
            if fam_name not in fams:
                findings.append(f"FONT-UNKNOWN {role}")
            elif role == "type.numeric" or role.startswith("type.numeric."):
                if not fams[fam_name]["tabular_numerals"]:
                    findings.append(f"FONT-TNUM {role}")
            if role == "type.numeric" or role.startswith("type.numeric."):
                if "tnum" not in val.get("fontFeatureSettings", []):
                    findings.append(f"FONT-TNUM {role}")
            if role in ("type.body", "type.body.small", "type.label") or role.startswith(("type.body.", "type.label.")):
                px = float(val["fontSize"][:-2])
                if px < (16.0 if theme == "floor" else 12.0):
                    findings.append(f"TYPE-SIZE {theme} {role}")
    findings = sorted(set(findings))
    return {"exit": 1 if findings else 0, "findings": findings}


def expected_files():
    out = {
        "allow.txt": ALLOW_TXT.encode("utf-8"),
        "expected.json": js(LABELS),
    }
    for name, files in build_cases().items():
        for rel, data in files.items():
            out[f"{name}/{rel}"] = data
    return out


def main(argv):
    check = "--check" in argv
    if any(a not in ("--check",) for a in argv):
        sys.exit("usage: make_fixtures.py [--check]")

    files = expected_files()
    if check:
        errors = []
        for rel, want in sorted(files.items()):
            p = HERE / rel
            if not p.is_file():
                errors.append(f"missing {rel}")
            elif p.read_bytes() != want:
                errors.append(f"stale {rel}")
        for p in sorted(HERE.rglob("*")):
            if p.is_dir():
                continue
            rel = p.relative_to(HERE).as_posix()
            if rel != "make_fixtures.py" and rel not in files:
                errors.append(f"unexpected {rel}")
        # Cross-check shipped design/ tokens against oracle (AC-1).
        shipped_got = oracle_check(DESIGN, ALLOW_TXT)
        if shipped_got != {"exit": 0, "findings": []}:
            errors.append(f"oracle failed on shipped design/: {shipped_got!r}")
        for key, label in sorted(LABELS.items()):
            got = oracle_check(HERE / label["root"], ALLOW_TXT)
            for field in ("exit", "findings"):
                if got[field] != label[field]:
                    errors.append(f"oracle disagrees on {key}.{field}: got {got[field]!r}, label {label[field]!r}")
        if errors:
            for e in errors:
                print(f"error: {e}", file=sys.stderr)
            sys.exit(1)
        print(f"fixtures/ux/tokens/type: {len(LABELS)} cases + shipped design/, {len(files)} files ok")
        return

    for rel, data in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(files)} files across {len(LABELS)} cases")


if __name__ == "__main__":
    main(sys.argv[1:])
