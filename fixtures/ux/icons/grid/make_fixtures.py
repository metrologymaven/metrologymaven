#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.ICONS.GRID and checks their hand labels.

Original synthetic SVG drawings, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/icons/grid/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/icons/grid/make_fixtures.py --check  # fail if files differ or labels disagree

Produces:
- `ok/svg/target.16.svg` and `ok/svg/target.24.svg` (AC-1)
- `has-text.svg` (AC-2: ICON-ELEMENT text)
- `literal-fill.svg` (AC-3: ICON-COLOR-LITERAL)
- `off-grid.svg` (AC-4: ICON-OFF-GRID, point at 3.3)
- `missing-16/svg/target.24.svg` (AC-5: ICON-MASTER-MISSING 16)
- `bad-viewbox.svg` (ICON-VIEWBOX)
- `bad-attr.svg` (ICON-ATTR)
- `external-ref.svg` (ICON-EXTERNAL-REF)
- `expected.json` (hand-labelled expectations, cross-checked by an independent Python oracle)
"""

import json
import pathlib
import re
import sys
import xml.etree.ElementTree as ET

HERE = pathlib.Path(__file__).resolve().parent

SVG_NS = "http://www.w3.org/2000/svg"
ALLOWED_ELEMENTS = {"svg", "g", "path", "circle", "rect", "line", "polyline", "polygon"}
GRID_EPS = 1e-6

TARGET_16 = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1" stroke-linecap="round" stroke-linejoin="round">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <circle cx="8" cy="8" r="5" data-role="sem.icon.primary"/>
  <circle cx="8" cy="8" r="1" fill="currentColor" stroke="none" data-role="sem.icon.accent"/>
  <path d="M8 1.5v2.5M8 12v2.5M1.5 8h2.5M12 8h2.5" data-role="sem.icon.primary"/>
</svg>
"""

TARGET_24 = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <circle cx="12" cy="12" r="7.5" data-role="sem.icon.primary"/>
  <circle cx="12" cy="12" r="2" data-role="sem.icon.accent"/>
  <line x1="12" y1="2.5" x2="12" y2="6.5"/>
  <line x1="12" y1="17.5" x2="12" y2="21.5"/>
  <line x1="2.5" y1="12" x2="6.5" y2="12"/>
  <line x1="17.5" y1="12" x2="21.5" y2="12"/>
</svg>
"""

HAS_TEXT = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <text x="2" y="10">A</text>
</svg>
"""

LITERAL_FILL = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <rect x="2" y="2" width="12" height="12" fill="#ff0000" stroke="none"/>
</svg>
"""

OFF_GRID = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <path d="M2 2L3.3 12"/>
</svg>
"""

BAD_VIEWBOX = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 32 32" fill="none" stroke="currentColor" stroke-width="1">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <circle cx="8" cy="8" r="4"/>
</svg>
"""

BAD_ATTR = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <rect x="2" y="2" width="12" height="12" style="opacity: 0.5"/>
</svg>
"""

EXTERNAL_REF = """\
<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16" fill="none" stroke="currentColor" stroke-width="1">
  <!-- Synthetic original fixture, CC-BY-4.0 -->
  <g href="https://example.invalid/icon.svg">
    <circle cx="8" cy="8" r="4"/>
  </g>
</svg>
"""


# Hand-labelled expectations derived directly from UX.ICONS.GRID Requirements, Design and AC-1..5.
LABELS = {
    "ok": {
        "root": "ok",
        "exit": 0,
        "files": ["svg/target.16.svg", "svg/target.24.svg"],
        "findings": [],
    },
    "has-text": {
        "root": "has-text.svg",
        "exit": 1,
        "files": ["has-text.svg"],
        "findings": ["ICON-ELEMENT text"],
    },
    "literal-fill": {
        "root": "literal-fill.svg",
        "exit": 1,
        "files": ["literal-fill.svg"],
        "findings": ["ICON-COLOR-LITERAL literal-fill.svg"],
    },
    "off-grid": {
        "root": "off-grid.svg",
        "exit": 1,
        "files": ["off-grid.svg"],
        "findings": ["ICON-OFF-GRID off-grid.svg"],
    },
    "missing-16": {
        "root": "missing-16",
        "exit": 1,
        "files": ["svg/target.24.svg"],
        "findings": ["ICON-MASTER-MISSING 16"],
    },
    "bad-viewbox": {
        "root": "bad-viewbox.svg",
        "exit": 1,
        "files": ["bad-viewbox.svg"],
        "findings": ["ICON-VIEWBOX bad-viewbox.svg"],
    },
    "bad-attr": {
        "root": "bad-attr.svg",
        "exit": 1,
        "files": ["bad-attr.svg"],
        "findings": ["ICON-ATTR bad-attr.svg"],
    },
    "external-ref": {
        "root": "external-ref.svg",
        "exit": 1,
        "files": ["external-ref.svg"],
        "findings": ["ICON-EXTERNAL-REF external-ref.svg"],
    },
}


def js(v) -> bytes:
    return (json.dumps(v, indent=2, ensure_ascii=True) + "\n").encode("utf-8")


def on_half_grid(v: float) -> bool:
    return abs(v - round(v * 2.0) * 0.5) <= GRID_EPS


def strip_ns(tag: str) -> str:
    if tag.startswith("{"):
        return tag.split("}", 1)[1]
    return tag


def oracle_path_anchors(d: str):
    tokens = re.findall(r"[A-Za-z]|[-+]?(?:\d+\.?\d*|\.\d+)(?:[eE][-+]?\d+)?", d)
    anchors = []
    cx = cy = sx = sy = 0.0
    i = 0
    cmd = None
    while i < len(tokens):
        t = tokens[i]
        if t.isalpha():
            cmd = t
            i += 1
        elif cmd == "M":
            cmd = "L"
        elif cmd == "m":
            cmd = "l"
        if cmd in ("M", "m"):
            x, y = float(tokens[i]), float(tokens[i + 1])
            i += 2
            cx, cy = (x, y) if cmd == "M" else (cx + x, cy + y)
            sx, sy = cx, cy
            anchors.append((cx, cy))
        elif cmd in ("L", "l", "T", "t"):
            x, y = float(tokens[i]), float(tokens[i + 1])
            i += 2
            cx, cy = (x, y) if cmd.isupper() else (cx + x, cy + y)
            anchors.append((cx, cy))
        elif cmd in ("H", "h"):
            x = float(tokens[i])
            i += 1
            cx = x if cmd == "H" else cx + x
            anchors.append((cx, cy))
        elif cmd in ("V", "v"):
            y = float(tokens[i])
            i += 1
            cy = y if cmd == "V" else cy + y
            anchors.append((cx, cy))
        elif cmd in ("Z", "z"):
            cx, cy = sx, sy
        else:
            raise ValueError(f"unsupported command in fixture oracle: {cmd}")
    return anchors


def oracle_lint_one(rel: str, text: str):
    findings = []
    root = ET.fromstring(text)
    vb = root.attrib.get("viewBox")
    if vb not in ("0 0 16 16", "0 0 24 24"):
        findings.append(f"ICON-VIEWBOX {rel}")
    for el in root.iter():
        tag = strip_ns(el.tag)
        if tag not in ALLOWED_ELEMENTS:
            findings.append(f"ICON-ELEMENT {tag}")
            continue
        for k, v in el.attrib.items():
            kl = k.lower()
            if kl in ("href", "xlink:href", "src") or any(
                s in v.lower() for s in ("url(", "http://", "https://", "data:")
            ):
                findings.append(f"ICON-EXTERNAL-REF {rel}")
            elif kl in ("style", "class", "transform") or kl.startswith("on"):
                findings.append(f"ICON-ATTR {rel}")
            elif k in ("fill", "stroke") and v not in ("none", "currentColor"):
                findings.append(f"ICON-COLOR-LITERAL {rel}")
        if tag == "path" and "d" in el.attrib:
            for x, y in oracle_path_anchors(el.attrib["d"]):
                if not on_half_grid(x) or not on_half_grid(y):
                    findings.append(f"ICON-OFF-GRID {rel}")
        elif tag == "circle":
            for k in ("cx", "cy", "r"):
                if not on_half_grid(float(el.attrib.get(k, "0"))):
                    findings.append(f"ICON-OFF-GRID {rel}")
        elif tag == "line":
            for k in ("x1", "y1", "x2", "y2"):
                if not on_half_grid(float(el.attrib.get(k, "0"))):
                    findings.append(f"ICON-OFF-GRID {rel}")
        elif tag == "rect":
            for k in ("x", "y", "width", "height"):
                if not on_half_grid(float(el.attrib.get(k, "0"))):
                    findings.append(f"ICON-OFF-GRID {rel}")
    return findings


def oracle_check(target: pathlib.Path):
    findings = []
    files = []
    if target.is_file():
        files = [target.name]
        findings.extend(oracle_lint_one(target.name, target.read_text(encoding="utf-8")))
    else:
        masters = {}
        for p in sorted(target.rglob("*.svg")):
            rel = p.relative_to(target).as_posix()
            files.append(rel)
            findings.extend(oracle_lint_one(rel, p.read_text(encoding="utf-8")))
            if rel.endswith(".16.svg"):
                masters.setdefault(rel[:-7], set()).add(16)
            elif rel.endswith(".24.svg"):
                masters.setdefault(rel[:-7], set()).add(24)
        for stem, sizes in sorted(masters.items()):
            if 16 not in sizes:
                findings.append("ICON-MASTER-MISSING 16")
            if 24 not in sizes:
                findings.append("ICON-MASTER-MISSING 24")
    findings = sorted(set(findings))
    return {"exit": 1 if findings else 0, "files": sorted(files), "findings": findings}


def expected_files():
    return {
        "ok/svg/target.16.svg": TARGET_16.encode("utf-8"),
        "ok/svg/target.24.svg": TARGET_24.encode("utf-8"),
        "has-text.svg": HAS_TEXT.encode("utf-8"),
        "literal-fill.svg": LITERAL_FILL.encode("utf-8"),
        "off-grid.svg": OFF_GRID.encode("utf-8"),
        "missing-16/svg/target.24.svg": TARGET_24.encode("utf-8"),
        "bad-viewbox.svg": BAD_VIEWBOX.encode("utf-8"),
        "bad-attr.svg": BAD_ATTR.encode("utf-8"),
        "external-ref.svg": EXTERNAL_REF.encode("utf-8"),
        "expected.json": js(LABELS),
    }


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
        for key, label in sorted(LABELS.items()):
            got = oracle_check(HERE / label["root"])
            for field in ("exit", "files", "findings"):
                if got[field] != label[field]:
                    errors.append(f"oracle disagrees on {key}.{field}: got {got[field]!r}, label {label[field]!r}")
        if errors:
            for e in errors:
                print(f"error: {e}", file=sys.stderr)
            sys.exit(1)
        print(f"fixtures/ux/icons/grid: {len(LABELS)} cases, {len(files)} files ok")
        return

    for rel, data in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(files)} files across {len(LABELS)} cases")


if __name__ == "__main__":
    main(sys.argv[1:])
