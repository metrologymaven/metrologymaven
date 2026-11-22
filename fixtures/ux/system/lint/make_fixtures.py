#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.SYSTEM.LINT and checks their hand labels.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/system/lint/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/system/lint/make_fixtures.py --check  # fail if files differ or labels disagree

Produces:
- `clean/theme.rs`, `clean/style.toml` (AC-1: token names only)
- `dirty/theme.rs` (AC-2: `Color32::from_rgb(0x1f, 0x6f, 0xeb)` on line 12)
- `pkg-accent/` (minimal valid package with `sem.brand.accent` = `#1f6feb` for AC-2)
- `dirty-multi/` (CSS, SVG, TOML, JSON, Rust literals exercising all default rules & multi-token suggestions against `fixtures/ux/system/pkg/v1.0.0/`)
- `allowed/legacy.css` (AC-3: `/* design-allow: third-party widget */`)
- `allowed/empty-reason.css` (AC-4: `/* design-allow: */` -> `LIT-ALLOW-NOREASON`)
- `allowlist.txt` (path allowlist fixture)
- `expected.json` (hand-labelled expectations, cross-checked by an independent Python oracle)
"""

import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
REPO = HERE.parents[3]

CLEAN_RS = """\
// Synthetic fixture (CC-BY-4.0): resolves colours and spacing by token name only.
pub struct ThemeTokens;

impl ThemeTokens {
    pub fn focus_ring(&self) -> &'static str {
        "sem.focus.ring"
    }

    pub fn gap(&self) -> &'static str {
        "space.2"
    }

    pub fn transition(&self) -> &'static str {
        "motion.fast"
    }
}
"""

CLEAN_TOML = """\
# Synthetic fixture (CC-BY-4.0): token names only, no literals.
[panel]
accent = "sem.focus.ring"
ok = "sem.status.ok"
gap = "space.2"
duration = "motion.fast"
"""

# Exact line 12 holds `Color32::from_rgb(0x1f, 0x6f, 0xeb)` (AC-2).
DIRTY_RS = """\
// Synthetic fixture (CC-BY-4.0) for UX.SYSTEM.LINT AC-2.
pub struct Color32(pub u8, pub u8, pub u8);

impl Color32 {
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        Self(r, g, b)
    }
}

pub fn accent_color() -> Color32 {
    // Hard-coded colour literal on line 12:
    Color32::from_rgb(0x1f, 0x6f, 0xeb)
}
"""

ALLOWED_LEGACY_CSS = """\
/* Synthetic fixture (CC-BY-4.0) for UX.SYSTEM.LINT AC-3. */
.legacy-widget {
  color: #1f6feb; /* design-allow: third-party widget */
}
"""

ALLOWED_EMPTY_CSS = """\
/* Synthetic fixture (CC-BY-4.0) for UX.SYSTEM.LINT AC-4. */
.broken-allow {
  color: #1f6feb; /* design-allow: */
}
"""

DIRTY_MULTI_CSS = """\
/* Synthetic fixture (CC-BY-4.0): multi-rule coverage against pkg v1.0.0. */
.panel {
  color: #1f5fbf;
  background: rgb(46, 133, 64);
  font-size: 12pt;
  padding: 8px;
  transition-duration: 0.12s;
}
"""

DIRTY_MULTI_RS = """\
// Synthetic fixture (CC-BY-4.0): Rust duration literal matching motion.fast.
use std::time::Duration;

pub fn anim() -> Duration {
    Duration::from_millis(120)
}
"""

ALLOWLIST_TXT = """\
# Synthetic allowlist (CC-BY-4.0).
theme.rs: legacy bridge scheduled for removal
"""

PKG_ACCENT_TOML = """\
format = "fid-design-package@1"
version = "1.0.0"
token_format = "dtcg-second-editors-draft-2022-06-14"
product_name = "{{FAMILY_NAME}}"
"""

PKG_ACCENT_CHANGELOG = """\
# Changelog

## 1.0.0

- additive: sem.brand.accent - initial accent token
"""

PKG_ACCENT_PRINCIPLES = """\
# Principles

Synthetic fixture, CC-BY-4.0.
"""


def js(v) -> bytes:
    return (json.dumps(v, indent=2, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")


PKG_ACCENT_TOKENS = {
    "sem": {
        "brand": {
            "accent": {
                "$type": "color",
                "$value": "#1f6feb",
            }
        }
    }
}

PKG_ACCENT_NAMES = {
    "format": "fid-design-names@1",
    "names": [
        {"kind": "token", "name": "sem.brand.accent"},
    ],
}

# Hand-labelled expectations derived directly from UX.SYSTEM.LINT Requirements, Design and AC-1..5.
LABELS = {
    "clean": {
        "root": "clean",
        "package": "../pkg/v1.0.0",
        "exit": 0,
        "findings": [],
    },
    "dirty_ac2": {
        "root": "dirty/theme.rs",
        "package": "pkg-accent",
        "exit": 1,
        "findings": [
            {
                "code": "LIT-COLOR-RUST",
                "file": "theme.rs",
                "line": 12,
                "literal": "from_rgb(0x1f, 0x6f, 0xeb)",
                "suggestions": ["sem.brand.accent"],
            }
        ],
    },
    "allowed_legacy": {
        "root": "allowed/legacy.css",
        "package": "pkg-accent",
        "exit": 0,
        "findings": [],
    },
    "allowed_empty": {
        "root": "allowed/empty-reason.css",
        "package": "pkg-accent",
        "exit": 1,
        "findings": [
            {
                "code": "LIT-COLOR-HEX",
                "file": "empty-reason.css",
                "line": 3,
                "literal": "#1f6feb",
                "suggestions": ["sem.brand.accent"],
            },
            {
                "code": "LIT-ALLOW-NOREASON",
                "file": "empty-reason.css",
                "line": 3,
                "literal": None,
                "suggestions": [],
            },
        ],
    },
    "dirty_multi": {
        "root": "dirty-multi",
        "package": "../pkg/v1.0.0",
        "exit": 1,
        "findings": [
            {
                "code": "LIT-DURATION-RUST",
                "file": "anim.rs",
                "line": 5,
                "literal": "from_millis(120)",
                "suggestions": ["motion.fast"],
            },
            {
                "code": "LIT-COLOR-HEX",
                "file": "panel.css",
                "line": 3,
                "literal": "#1f5fbf",
                "suggestions": ["prim.color.blue.500", "sem.focus.ring"],
            },
            {
                "code": "LIT-COLOR-FUNC",
                "file": "panel.css",
                "line": 4,
                "literal": "rgb(46, 133, 64)",
                "suggestions": ["prim.color.green.500", "sem.status.ok"],
            },
            {
                "code": "LIT-FONT-SIZE",
                "file": "panel.css",
                "line": 5,
                "literal": "12pt",
                "suggestions": [],
            },
            {
                "code": "LIT-SPACE-STYLE",
                "file": "panel.css",
                "line": 6,
                "literal": "8px",
                "suggestions": ["space.2"],
            },
            {
                "code": "LIT-DURATION-STYLE",
                "file": "panel.css",
                "line": 7,
                "literal": "0.12s",
                "suggestions": ["motion.fast"],
            },
        ],
    },
}


def expected_files():
    return {
        "clean/theme.rs": CLEAN_RS.encode("utf-8"),
        "clean/style.toml": CLEAN_TOML.encode("utf-8"),
        "dirty/theme.rs": DIRTY_RS.encode("utf-8"),
        "allowed/legacy.css": ALLOWED_LEGACY_CSS.encode("utf-8"),
        "allowed/empty-reason.css": ALLOWED_EMPTY_CSS.encode("utf-8"),
        "dirty-multi/panel.css": DIRTY_MULTI_CSS.encode("utf-8"),
        "dirty-multi/anim.rs": DIRTY_MULTI_RS.encode("utf-8"),
        "allowlist.txt": ALLOWLIST_TXT.encode("utf-8"),
        "pkg-accent/package.toml": PKG_ACCENT_TOML.encode("utf-8"),
        "pkg-accent/CHANGELOG.md": PKG_ACCENT_CHANGELOG.encode("utf-8"),
        "pkg-accent/deprecations.json": js([]),
        "pkg-accent/names.json": js(PKG_ACCENT_NAMES),
        "pkg-accent/principles.md": PKG_ACCENT_PRINCIPLES.encode("utf-8"),
        "pkg-accent/tokens/accent.tokens.json": js(PKG_ACCENT_TOKENS),
        "expected.json": js(LABELS),
    }


def verify_oracle():
    # Confirm dirty/theme.rs has Color32::from_rgb(0x1f, 0x6f, 0xeb) on exact line 12 (AC-2).
    lines = (HERE / "dirty/theme.rs").read_text(encoding="utf-8").splitlines()
    if len(lines) < 12 or "Color32::from_rgb(0x1f, 0x6f, 0xeb)" not in lines[11]:
        return ["dirty/theme.rs line 12 does not contain Color32::from_rgb(0x1f, 0x6f, 0xeb)"]
    # Confirm clean/ files contain no hex colours, rgb/hsl, px/pt, or ms/s literals.
    for rel in ("clean/theme.rs", "clean/style.toml"):
        txt = (HERE / rel).read_text(encoding="utf-8")
        if re.search(r"#[0-9a-fA-F]{3,8}\b|from_rgb|rgb\(|px\b|pt\b|\d+ms\b", txt):
            return [f"{rel} unexpectedly contains a literal"]
    return []


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
        errors.extend(verify_oracle())
        if errors:
            for e in errors:
                print(f"error: {e}", file=sys.stderr)
            sys.exit(1)
        print(f"fixtures/ux/system/lint: {len(LABELS)} cases, {len(files)} files ok")
        return

    for rel, data in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(files)} files across {len(LABELS)} cases")


if __name__ == "__main__":
    main(sys.argv[1:])
