#!/usr/bin/env python3
"""Writes the design-system package fixtures of spec node UX.SYSTEM.PKG.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/system/pkg/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/system/pkg/make_fixtures.py --check  # fail if the files on disk differ

The expected results in `expected.json` are hand-labelled from the rules in the leaf's
Requirements and Design (they are written below as literals, not computed by any code under
test). Colours, names and values are invented for these tests; no third-party asset is used.
"""

import copy
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent

TOKEN_FORMAT = "dtcg-second-editors-draft-2022-06-14"


def manifest(version):
    return (
        'format = "fid-design-package@1"\n'
        f'version = "{version}"\n'
        f'token_format = "{TOKEN_FORMAT}"\n'
        'product_name = "{{FAMILY_NAME}}"\n'
    )


def dump(obj):
    return json.dumps(obj, indent=2, ensure_ascii=False) + "\n"


# ---------------------------------------------------------------- v1.0.0 content

PRIMITIVE = {
    "prim": {
        "color": {
            "$type": "color",
            "blue": {"500": {"$value": "#1f5fbf"}},
            "green": {"500": {"$value": "#2e8540"}},
            "grey": {"700": {"$value": "#4a4f55"}},
        }
    }
}

SEMANTIC = {
    "sem": {
        "$type": "color",
        "focus": {"ring": {"$value": "{prim.color.blue.500}", "$description": "Focus indicator."}},
        "status": {"ok": {"$value": "{prim.color.green.500}"}},
        "text": {"muted": {"$value": "{prim.color.grey.700}"}},
    }
}

SPACE_MOTION = {
    "motion": {"fast": {"$type": "duration", "$value": "120ms"}},
    "space": {"$type": "dimension", "2": {"$value": "8px"}},
}

OTHER_NAMES = [
    ("comp.button", "component"),
    ("gdt.flatness", "gdt"),
    ("icon.check", "icon"),
    ("pal.deviation.diverging", "palette"),
    ("pattern.confirm", "pattern"),
    ("screen.home", "screen"),
]

ICON_SVG = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24">'
    '<path d="M4 12l5 5L20 6" fill="none" stroke="currentColor" stroke-width="2"/></svg>\n'
)

CHANGELOG_HEAD = "# Changelog\n\nEntry format: `- <class>: <name> - <one line>`.\n\n"
LOG_100 = "## 1.0.0\n\nFirst release of the synthetic test package.\n"
LOG_110 = (
    "## 1.1.0\n\n"
    "- additive: sem.focus.outline - new focus indicator colour\n"
    "- additive: sem.focus.ring - deprecated; use sem.focus.outline\n\n"
)
LOG_110_REMOVED = "## 1.1.0\n\n- breaking: space.2 - removed\n\n"
LOG_120_NOLOG = "## 1.2.0\n\nNo entries (the value change of motion.fast is not logged).\n\n"


def token_names(*files):
    names = []

    def walk(node, path):
        if "$value" in node:
            names.append(".".join(path))
            return
        for k, v in node.items():
            if not k.startswith("$"):
                walk(v, path + [k])

    for f in files:
        walk(f, [])
    return names


def names_json(token_list, others):
    entries = [{"name": n, "kind": "token"} for n in token_list] + [{"name": n, "kind": k} for n, k in others]
    entries.sort(key=lambda e: (e["name"], e["kind"]))
    return dump({"format": "fid-design-names@1", "names": entries})


def package(version, prim, sem, space, deprecations, changelog, others=OTHER_NAMES):
    files = {
        "package.toml": manifest(version),
        "CHANGELOG.md": CHANGELOG_HEAD + changelog,
        "deprecations.json": dump(deprecations),
        "names.json": names_json(token_names(prim, sem, space), others),
        "tokens/primitive/color.tokens.json": dump(prim),
        "tokens/semantic/color.tokens.json": dump(sem),
        "tokens/space.tokens.json": dump(space),
        "icons/icon.check.svg": ICON_SVG,
        "principles.md": "# Principles\n\nPlaceholder for UX.SYSTEM.PRINC.\n",
    }
    return files


def build():
    pk = {}
    pk["v1.0.0"] = package("1.0.0", PRIMITIVE, SEMANTIC, SPACE_MOTION, [], LOG_100)

    # v1.1.0: one token added (sem.focus.outline), one deprecated (sem.focus.ring) with it as
    # the replacement; the changelog classifies both as additive.
    sem110 = copy.deepcopy(SEMANTIC)
    sem110["sem"]["focus"]["outline"] = {"$value": "#174a94"}
    dep110 = [{"name": "sem.focus.ring", "replacement": "sem.focus.outline", "since": "1.1.0", "remove_after": "2.0.0"}]
    pk["v1.1.0"] = package("1.1.0", PRIMITIVE, sem110, SPACE_MOTION, dep110, LOG_110 + LOG_100)

    # v1.1.0-removed: token space.2 removed in a minor release.
    space_removed = copy.deepcopy(SPACE_MOTION)
    del space_removed["space"]
    pk["v1.1.0-removed"] = package("1.1.0", PRIMITIVE, SEMANTIC, space_removed, [], LOG_110_REMOVED + LOG_100)

    # v1.2.0-nolog: from v1.1.0, motion.fast changes 120ms -> 100ms with no changelog entry.
    space120 = copy.deepcopy(SPACE_MOTION)
    space120["motion"]["fast"]["$value"] = "100ms"
    pk["v1.2.0-nolog"] = package("1.2.0", PRIMITIVE, sem110, space120, dep110, LOG_120_NOLOG + LOG_110 + LOG_100)

    # v2.0.0: from v1.1.0, the deprecated sem.focus.ring is removed in a major release after one
    # further minor; the changelog classifies it as breaking.
    sem200 = copy.deepcopy(sem110)
    del sem200["sem"]["focus"]["ring"]
    log200 = "## 2.0.0\n\n- breaking: sem.focus.ring - removed; use sem.focus.outline\n\n"
    pk["v2.0.0"] = package("2.0.0", PRIMITIVE, sem200, SPACE_MOTION, dep110, log200 + LOG_110 + LOG_100)

    # v2.0.0-backdated: from v1.0.0 (no deprecations), space.2 is removed in a major release and
    # the new package adds a deprecation entry backdated to 1.0.0. Only the released package's
    # entries count, so the removal is refused (PKG-DEPRECATION-EARLY); the backdated entry still
    # names the replacement space.3, so the diff classifies space.2 as renamed.
    space_bd = copy.deepcopy(SPACE_MOTION)
    space_bd["space"]["3"] = space_bd["space"].pop("2")
    dep_bd = [{"name": "space.2", "replacement": "space.3", "since": "1.0.0", "remove_after": "1.1.0"}]
    log_bd = "## 2.0.0\n\n- additive: space.3 - replaces space.2\n- breaking: space.2 - removed; use space.3\n\n"
    pk["v2.0.0-backdated"] = package("2.0.0", PRIMITIVE, SEMANTIC, space_bd, dep_bd, log_bd + LOG_100)

    # v1.1.0-breaking-log: from v1.0.0, motion.fast 120ms -> 100ms logged as `breaking` in a
    # minor release: PKG-BREAKING-MINOR.
    space_bl = copy.deepcopy(SPACE_MOTION)
    space_bl["motion"]["fast"]["$value"] = "100ms"
    log_bl = "## 1.1.0\n\n- breaking: motion.fast - slower\n\n"
    pk["v1.1.0-breaking-log"] = package("1.1.0", PRIMITIVE, SEMANTIC, space_bl, [], log_bl + LOG_100)

    # v1.1.0-type: from v1.0.0, space.2 changes $type dimension -> number in a minor release,
    # logged as `fix`: PKG-BREAKING-MINOR (type change) and PKG-CHANGELOG-CLASS (fix, not breaking).
    space_ty = copy.deepcopy(SPACE_MOTION)
    space_ty["space"]["2"] = {"$type": "number", "$value": 8}
    log_ty = "## 1.1.0\n\n- fix: space.2 - now a plain number\n\n"
    pk["v1.1.0-type"] = package("1.1.0", PRIMITIVE, SEMANTIC, space_ty, [], log_ty + LOG_100)

    # v1.1.0-kind: from v1.0.0, icon.check is re-listed as a component in a minor release,
    # logged as `breaking`: PKG-BREAKING-MINOR once.
    others_kind = [(n, "component" if n == "icon.check" else k) for n, k in OTHER_NAMES]
    log_kind = "## 1.1.0\n\n- breaking: icon.check - now a component\n\n"
    pk["v1.1.0-kind"] = package("1.1.0", PRIMITIVE, SEMANTIC, SPACE_MOTION, [], log_kind + LOG_100, others=others_kind)

    # dup-name: an icon and a token both named status.ok.
    status = {"status": {"ok": {"$type": "color", "$value": "#2e8540"}}}
    dup = package("1.0.0", {}, {}, status, [], LOG_100, others=[("status.ok", "icon")])
    del dup["tokens/primitive/color.tokens.json"]
    del dup["tokens/semantic/color.tokens.json"]
    del dup["icons/icon.check.svg"]
    dup["icons/status.ok.svg"] = ICON_SVG
    pk["dup-name"] = dup
    return pk


# Hand-labelled expectations (from the leaf's rules; see the comments in build()).
EXPECTED = {
    "check": {
        "v1.0.0": {
            "exit": 0,
            "counts": {"component": 1, "gdt": 1, "icon": 1, "palette": 1, "pattern": 1, "screen": 1, "token": 8},
            "findings": [],
        },
        "v1.1.0": {"exit": 0, "findings": []},
        "v2.0.0": {"exit": 0, "findings": []},
        "v2.0.0-backdated": {"exit": 0, "findings": []},
        "dup-name": {"exit": 1, "findings": ["PKG-NAME-DUPLICATE status.ok"]},
    },
    "diff": {
        "v1.0.0..v1.1.0": {
            "exit": 0,
            "changed": {"sem.focus.outline": "added", "sem.focus.ring": "deprecated"},
            "findings": [],
        },
        "v1.0.0..v1.1.0-removed": {
            "exit": 1,
            "changed": {"space.2": "removed"},
            "findings": ["PKG-BREAKING-MINOR space.2", "PKG-DEPRECATION-EARLY space.2"],
        },
        "v1.1.0..v1.2.0-nolog": {
            "exit": 1,
            "changed": {"motion.fast": "value-changed"},
            "findings": ["PKG-CHANGELOG-MISSING motion.fast"],
        },
        "v1.1.0..v2.0.0": {"exit": 0, "changed": {"sem.focus.ring": "renamed"}, "findings": []},
        "v1.0.0..v2.0.0-backdated": {
            "exit": 1,
            "changed": {"space.2": "renamed", "space.3": "added"},
            "findings": ["PKG-DEPRECATION-EARLY space.2"],
        },
        "v1.0.0..v1.1.0-breaking-log": {
            "exit": 1,
            "changed": {"motion.fast": "value-changed"},
            "findings": ["PKG-BREAKING-MINOR motion.fast"],
        },
        "v1.0.0..v1.1.0-type": {
            "exit": 1,
            "changed": {"space.2": "type-changed"},
            "findings": ["PKG-BREAKING-MINOR space.2", "PKG-CHANGELOG-CLASS space.2"],
        },
        "v1.0.0..v1.1.0-kind": {
            "exit": 1,
            "changed": {"icon.check": "kind-changed"},
            "findings": ["PKG-BREAKING-MINOR icon.check"],
        },
    },
    "resolve": {
        "v1.1.0": {
            "sem.focus.ring": {
                "value": "#174a94",
                "warnings": [["deprecated_name", "sem.focus.ring", "sem.focus.outline"]],
            },
            "sem.text.muted": {"value": "#4a4f55", "warnings": []},
        }
    },
}


def main():
    check = "--check" in sys.argv[1:]
    want = {}
    for d, files in build().items():
        for rel, text in files.items():
            want[HERE / d / rel] = text
    want[HERE / "expected.json"] = dump(EXPECTED)
    bad = []
    for path, text in sorted(want.items()):
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                bad.append(str(path.relative_to(HERE)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(text.encode("utf-8"))
    if bad:
        print("stale fixtures: " + ", ".join(bad))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
