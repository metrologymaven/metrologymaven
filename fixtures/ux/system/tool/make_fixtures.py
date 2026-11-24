#!/usr/bin/env python3
"""Writes the fixtures of spec node UX.SYSTEM.TOOL and checks their hand labels.

Synthetic data, CC-BY-4.0. Python stdlib only. Run from any directory:

    python fixtures/ux/system/tool/make_fixtures.py          # (re)write the fixtures
    python fixtures/ux/system/tool/make_fixtures.py --check  # fail if files differ or labels disagree

Each case is a directory `<case>/` holding `sources.json` and any committed synthetic export
files (SVG, PDF, PNG, `.penpot`). The `expected.json` file holds hand-labelled expectations
for `fid-design sources-check --root <case> --json`, cross-checked here by an independent
Python implementation of the UX.SYSTEM.TOOL rules.
"""

import hashlib
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent

OPEN_EXTS = {"svg", "pdf", "png"}
UX_SUBSYSTEMS = {"COMP", "ICONS", "PATTERN", "REPORT", "RESEARCH", "SCREENS", "SYSTEM", "TOKENS"}

SVG_ICON = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">\n'
    '  <!-- Synthetic fixture, CC-BY-4.0 -->\n'
    '  <rect x="2" y="2" width="12" height="12" fill="none" stroke="currentColor" stroke-width="1.5"/>\n'
    '</svg>\n'
).encode("utf-8")

SVG_STALE = (
    '<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 16 16">\n'
    '  <!-- Synthetic fixture, modified after export digest was recorded, CC-BY-4.0 -->\n'
    '  <rect x="3" y="3" width="10" height="10" fill="none" stroke="currentColor" stroke-width="1.5"/>\n'
    '</svg>\n'
).encode("utf-8")

# Tiny valid synthetic PDF (1 blank page, CC-BY-4.0).
PDF_MOCKUP = (
    b"%PDF-1.4\n"
    b"1 0 obj<</Type/Catalog/Pages 2 0 R>>endobj\n"
    b"2 0 obj<</Type/Pages/Count 1/Kids[3 0 R]>>endobj\n"
    b"3 0 obj<</Type/Page/MediaBox[0 0 72 72]/Parent 2 0 R>>endobj\n"
    b"xref\n0 4\n0000000000 65535 f \n"
    b"0000000009 00000 n \n0000000054 00000 n \n0000000105 00000 n \n"
    b"trailer<</Size 4/Root 1 0 R>>\nstartxref\n169\n%%EOF\n"
)

# Tiny synthetic tool export file (`.penpot` placeholder text, CC-BY-4.0).
PENPOT_EXPORT = b'{"format":"synthetic-penpot-fixture","licence":"CC-BY-4.0"}\n'


def sha(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def js(v) -> bytes:
    return (json.dumps(v, indent=2, ensure_ascii=True) + "\n").encode("utf-8")


BASE_TOOLS = [
    {
        "id": "penpot",
        "name": "Penpot",
        "open_source": True,
        "requires_paid_account": False,
        "export_extensions": ["penpot"],
    },
    {
        "id": "inkscape",
        "name": "Inkscape",
        "open_source": True,
        "requires_paid_account": False,
        "export_extensions": ["svg"],
    },
    {
        "id": "paid-suite",
        "name": "Proprietary Paid Suite (synthetic)",
        "open_source": False,
        "requires_paid_account": True,
    },
]


def build_cases():
    cases = {}

    # AC-1: ok/ — two sources with committed SVG, PDF and tool (.penpot) exports and matching digests.
    cases["ok"] = {
        "exports/icon-probe.svg": SVG_ICON,
        "exports/report-summary.pdf": PDF_MOCKUP,
        "exports/report-summary.penpot": PENPOT_EXPORT,
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "icon-probe-v1",
                        "node": "UX.ICONS.CORESET",
                        "tool": "inkscape",
                        "location": "exports/icon-probe.svg",
                        "exports": [
                            {"path": "exports/icon-probe.svg", "sha256": sha(SVG_ICON)},
                        ],
                    },
                    {
                        "id": "report-summary-v1",
                        "node": "UX.REPORT.MOCK",
                        "tool": "penpot",
                        "location": "https://design.penpot.app/#/view/synthetic-report-summary",
                        "exports": [
                            {"path": "exports/report-summary.pdf", "sha256": sha(PDF_MOCKUP)},
                            {"path": "exports/report-summary.penpot", "sha256": sha(PENPOT_EXPORT)},
                        ],
                    },
                ],
            }
        ),
    }

    # AC-2: no-export/ — listed source has empty exports list.
    cases["no-export"] = {
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "screen-inspect-v1",
                        "node": "UX.SCREENS.INSPECT",
                        "tool": "penpot",
                        "location": "https://design.penpot.app/#/view/synthetic-screen-inspect",
                        "exports": [],
                    },
                ],
            }
        ),
    }

    # Missing export file on disk (second path of SOURCE-EXPORT-MISSING).
    cases["missing-file"] = {
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "icon-missing-v1",
                        "node": "UX.ICONS.CORESET",
                        "tool": "inkscape",
                        "location": "exports/missing.svg",
                        "exports": [
                            {"path": "exports/missing.svg", "sha256": sha(SVG_ICON)},
                        ],
                    },
                ],
            }
        ),
    }

    # AC-3: stale-export/ — committed export exists, but SHA-256 differs from recorded digest.
    cases["stale-export"] = {
        "exports/icon-probe.svg": SVG_STALE,
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "icon-stale-v1",
                        "node": "UX.ICONS.CORESET",
                        "tool": "inkscape",
                        "location": "exports/icon-probe.svg",
                        "exports": [
                            {"path": "exports/icon-probe.svg", "sha256": sha(SVG_ICON)},
                        ],
                    },
                ],
            }
        ),
    }

    # AC-4: paid-tool/ — source names a tool row with requires_paid_account = true.
    cases["paid-tool"] = {
        "exports/mockup.pdf": PDF_MOCKUP,
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "mockup-paid-v1",
                        "node": "UX.REPORT.MOCK",
                        "tool": "paid-suite",
                        "location": "https://example.invalid/paid-suite/mockup",
                        "exports": [
                            {"path": "exports/mockup.pdf", "sha256": sha(PDF_MOCKUP)},
                        ],
                    },
                ],
            }
        ),
    }

    # Unknown UX spec node ID.
    cases["unknown-node"] = {
        "exports/icon-probe.svg": SVG_ICON,
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "orphan-source-v1",
                        "node": "UX.BOGUS.LEAF",
                        "tool": "inkscape",
                        "location": "exports/icon-probe.svg",
                        "exports": [
                            {"path": "exports/icon-probe.svg", "sha256": sha(SVG_ICON)},
                        ],
                    },
                ],
            }
        ),
    }

    # Unsupported export format (.fig) fails with SOURCES-SCHEMA.
    cases["bad-format"] = {
        "exports/mockup.fig": b"synthetic proprietary file\n",
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "mockup-fig-v1",
                        "node": "UX.SCREENS.INSPECT",
                        "tool": "penpot",
                        "location": "exports/mockup.fig",
                        "exports": [
                            {"path": "exports/mockup.fig", "sha256": sha(b"synthetic proprietary file\n")},
                        ],
                    },
                ],
            }
        ),
    }

    # REQ-002: tool-only export (.penpot without svg/pdf/png) fails with SOURCE-EXPORT-MISSING.
    cases["penpot-only"] = {
        "exports/report-summary.penpot": PENPOT_EXPORT,
        "sources.json": js(
            {
                "format": "design-sources@1",
                "tools": BASE_TOOLS,
                "sources": [
                    {
                        "id": "report-penpot-only-v1",
                        "node": "UX.REPORT.MOCK",
                        "tool": "penpot",
                        "location": "https://design.penpot.app/#/view/synthetic-report-summary",
                        "exports": [
                            {"path": "exports/report-summary.penpot", "sha256": sha(PENPOT_EXPORT)},
                        ],
                    },
                ],
            }
        ),
    }

    return cases


# Hand-labelled expectations derived directly from UX.SYSTEM.TOOL Requirements, Design and AC-1..4.
LABELS = {
    "ok": {
        "root": "ok",
        "exit": 0,
        "tools": 3,
        "sources": 2,
        "exports_checked": 3,
        "findings": [],
    },
    "no-export": {
        "root": "no-export",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 0,
        "findings": ["SOURCE-EXPORT-MISSING screen-inspect-v1"],
    },
    "missing-file": {
        "root": "missing-file",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 0,
        "findings": ["SOURCE-EXPORT-MISSING icon-missing-v1"],
    },
    "stale-export": {
        "root": "stale-export",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 1,
        "findings": ["SOURCE-EXPORT-DIGEST icon-stale-v1"],
    },
    "paid-tool": {
        "root": "paid-tool",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 1,
        "findings": ["SOURCE-PAID-TOOL mockup-paid-v1"],
    },
    "unknown-node": {
        "root": "unknown-node",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 1,
        "findings": ["SOURCE-NODE-UNKNOWN orphan-source-v1"],
    },
    "bad-format": {
        "root": "bad-format",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 1,
        "findings": ["SOURCES-SCHEMA mockup-fig-v1"],
    },
    "penpot-only": {
        "root": "penpot-only",
        "exit": 1,
        "tools": 3,
        "sources": 1,
        "exports_checked": 1,
        "findings": ["SOURCE-EXPORT-MISSING report-penpot-only-v1"],
    },
}


def oracle_check(case_dir: pathlib.Path):
    """Independent Python check of UX.SYSTEM.TOOL rules against a fixture directory."""
    raw = (case_dir / "sources.json").read_text(encoding="utf-8")
    doc = json.loads(raw)
    assert doc["format"] == "design-sources@1"
    tools = {t["id"]: t for t in doc["tools"]}
    findings = []
    exports_checked = 0
    for src in doc["sources"]:
        sid = src["id"]
        segs = src["node"].split(".")
        if not (segs[0] == "UX" and (len(segs) == 1 or segs[1] in UX_SUBSYSTEMS)):
            findings.append(f"SOURCE-NODE-UNKNOWN {sid}")
        tool = tools.get(src["tool"])
        if tool is None:
            findings.append(f"SOURCES-SCHEMA {sid}")
        elif tool["requires_paid_account"]:
            findings.append(f"SOURCE-PAID-TOOL {sid}")
        exps = src["exports"]
        if not exps:
            findings.append(f"SOURCE-EXPORT-MISSING {sid}")
        allowed = set(OPEN_EXTS) | set(tool.get("export_extensions", []) if tool else [])
        has_open = False
        all_ext_ok = True
        for exp in exps:
            rel = exp["path"]
            ext = rel.rsplit("/", 1)[-1].rsplit(".", 1)[-1].lower() if "." in rel.rsplit("/", 1)[-1] else ""
            if ext in OPEN_EXTS:
                has_open = True
            if ext not in allowed:
                all_ext_ok = False
                findings.append(f"SOURCES-SCHEMA {sid}")
            p = case_dir / rel
            if not p.is_file():
                findings.append(f"SOURCE-EXPORT-MISSING {sid}")
            else:
                exports_checked += 1
                if sha(p.read_bytes()) != exp["sha256"]:
                    findings.append(f"SOURCE-EXPORT-DIGEST {sid}")
        if exps and all_ext_ok and not has_open:
            findings.append(f"SOURCE-EXPORT-MISSING {sid}")
    findings = sorted(set(findings))
    return {
        "exit": 1 if findings else 0,
        "tools": len(tools),
        "sources": len(doc["sources"]),
        "exports_checked": exports_checked,
        "findings": findings,
    }


def main(argv):
    check = "--check" in argv
    if any(a not in ("--check",) for a in argv):
        sys.exit("usage: make_fixtures.py [--check]")

    cases = build_cases()
    expected_files = {"expected.json": js(LABELS)}
    for name, files in cases.items():
        for rel, data in files.items():
            expected_files[f"{name}/{rel}"] = data

    if check:
        errors = []
        # Check every expected file matches byte for byte.
        for rel, want in sorted(expected_files.items()):
            p = HERE / rel
            if not p.is_file():
                errors.append(f"missing {rel}")
            elif p.read_bytes() != want:
                errors.append(f"stale {rel}")
        # Check no stray generated files exist in fixture subdirs.
        for p in sorted(HERE.rglob("*")):
            if p.is_dir():
                continue
            rel = p.relative_to(HERE).as_posix()
            if rel == "make_fixtures.py":
                continue
            if rel not in expected_files:
                errors.append(f"unexpected {rel}")
        # Cross-check hand labels against the independent Python oracle.
        for key, label in sorted(LABELS.items()):
            got = oracle_check(HERE / label["root"])
            for field in ("exit", "tools", "sources", "exports_checked", "findings"):
                if got[field] != label[field]:
                    errors.append(f"oracle disagrees on {key}.{field}: got {got[field]!r}, label {label[field]!r}")
        if errors:
            for e in errors:
                print(f"error: {e}", file=sys.stderr)
            sys.exit(1)
        print(f"fixtures/ux/system/tool: {len(cases)} cases, {len(expected_files)} files ok")
        return

    for rel, data in sorted(expected_files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(expected_files)} files across {len(cases)} cases")


if __name__ == "__main__":
    main(sys.argv[1:])
