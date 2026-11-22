#!/usr/bin/env python3
"""Generate and verify synthetic fixtures for UX.SCREENS.INV.

License: CC-BY-4.0 (synthetic fixtures for tests of fid-design screens-check and
fid-design screens-gate).

Usage:
    python3 fixtures/ux/screens/inv/make_fixtures.py
    python3 fixtures/ux/screens/inv/make_fixtures.py --check
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def canon_json(obj: object) -> bytes:
    """RFC 8785 canonical JSON bytes for objects without non-integer floats."""
    return json.dumps(
        obj,
        sort_keys=True,
        separators=(",", ":"),
        ensure_ascii=False,
    ).encode("utf-8")


def jcs_sha256(obj: object) -> str:
    return "sha256:" + hashlib.sha256(canon_json(obj)).hexdigest()


def pretty_json(obj: object) -> str:
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def journey_per01(step1_action: str = "Open AS9102 Form 3 and import ballooned print") -> dict:
    return {
        "format": "journey-map@1",
        "map_id": "per-01",
        "persona": "Quality Engineer",
        "steps": [
            {"n": 1, "screen": "SCR.RPT.FAIFORM", "action": step1_action},
            {"n": 2, "screen": "SCR.RPT.BALLOON", "action": "Assign balloon numbers to characteristics"},
        ],
    }


def journey_per02() -> dict:
    return {
        "format": "journey-map@1",
        "map_id": "per-02",
        "persona": "CMM Inspector",
        "steps": [
            {"n": 1, "screen": "SCR.INSP.ALIGN", "action": "Run datum alignment and review residuals"},
        ],
    }


def approval(pr: int, screen: str, artefact: str, digests: dict[str, str], date: str = "2026-11-17") -> dict:
    return {
        "format": "screen-approval@1",
        "pr": pr,
        "screen": screen,
        "artefact": artefact,
        "journey_digests": digests,
        "approver": {"name": "Design Reviewer", "principal": "human"},
        "date": date,
    }


def build_files() -> dict[str, str]:
    files: dict[str, str] = {}

    j1 = journey_per01()
    j1_changed = journey_per01("Open AS9102 Form 3, import PDF and verify tolerance bands")
    j2 = journey_per02()
    d1 = jcs_sha256(j1)
    d2 = jcs_sha256(j2)

    # 1. ok/ (AC-1 and AC-5)
    ok_inv = {
        "format": "screen-inventory@1",
        "screens": [
            {
                "id": "SCR.RPT.FAIFORM",
                "app": "report",
                "title": "AS9102 FAI Form Editor",
                "role_layout": "two-pane table and drawing preview",
                "phase": "P1a",
                "traces": [{"map": "per-01", "step": 1}],
                "keyboard_path": "Tab to form grid, Enter to edit cell, Ctrl+S to save",
                "wireframe": "WF.RPT.FAIFORM@2",
                "mockup": "MK.RPT.FAIFORM@1",
            },
            {
                "id": "SCR.RPT.BALLOON",
                "app": "report",
                "title": "Drawing Ballooning",
                "role_layout": "canvas plus characteristic rail",
                "phase": "P1a",
                "traces": [{"map": "per-01", "step": 2}],
                "keyboard_path": "B to place balloon, Enter to confirm number",
                "wireframe": "WF.RPT.BALLOON@1",
                "mockup": "none",
            },
            {
                "id": "SCR.INSP.ALIGN",
                "app": "inspect",
                "title": "Datum Alignment Workbench",
                "role_layout": "3D viewport plus inspector rail",
                "phase": "P1a",
                "traces": [{"map": "per-02", "step": 1}],
                "keyboard_path": "Alt+A to run alignment, Tab to inspect residuals",
                "wireframe": "WF.INSP.ALIGN@1",
                "mockup": "MK.INSP.ALIGN@1",
            },
            {
                "id": "SCR.SHELL.START",
                "app": "shell",
                "title": "Workspace Home",
                "role_layout": "centered launcher with recent jobs",
                "phase": "P1a",
                "traces": [
                    {"map": "per-01", "step": 1},
                    {"map": "per-02", "step": 1},
                ],
                "keyboard_path": "Tab through recent projects, Enter to open",
                "wireframe": "WF.SHELL.START@1",
                "mockup": "none",
            },
        ],
    }
    files["ok/design/screens/inventory.json"] = pretty_json(ok_inv)
    files["ok/journeys/per-01.json"] = pretty_json(j1)
    files["ok/journeys/per-02.json"] = pretty_json(j2)
    files["ok/design/screens/approvals/wf-rpt-faiform-2.json"] = pretty_json(
        approval(101, "SCR.RPT.FAIFORM", "WF.RPT.FAIFORM@2", {"per-01": d1})
    )
    files["ok/design/screens/approvals/mk-rpt-faiform-1.json"] = pretty_json(
        approval(102, "SCR.RPT.FAIFORM", "MK.RPT.FAIFORM@1", {"per-01": d1})
    )
    files["ok/design/screens/approvals/wf-rpt-balloon-1.json"] = pretty_json(
        approval(103, "SCR.RPT.BALLOON", "WF.RPT.BALLOON@1", {"per-01": d1})
    )
    files["ok/design/screens/approvals/wf-insp-align-1.json"] = pretty_json(
        approval(104, "SCR.INSP.ALIGN", "WF.INSP.ALIGN@1", {"per-02": d2})
    )
    files["ok/design/screens/approvals/mk-insp-align-1.json"] = pretty_json(
        approval(105, "SCR.INSP.ALIGN", "MK.INSP.ALIGN@1", {"per-02": d2})
    )
    files["ok/design/screens/approvals/wf-shell-start-1.json"] = pretty_json(
        approval(106, "SCR.SHELL.START", "WF.SHELL.START@1", {"per-01": d1, "per-02": d2})
    )

    # 2. untraced/ (AC-2)
    untraced_inv = {
        "format": "screen-inventory@1",
        "screens": [
            {
                "id": "SCR.RPT.FAIFORM",
                "app": "report",
                "title": "AS9102 FAI Form Editor",
                "role_layout": "two-pane table and drawing preview",
                "phase": "P1a",
                "traces": [],
                "keyboard_path": "Tab to form grid, Enter to edit cell",
                "wireframe": "none",
                "mockup": "none",
            }
        ],
    }
    files["untraced/design/screens/inventory.json"] = pretty_json(untraced_inv)
    files["untraced/journeys/per-01.json"] = pretty_json(j1)

    # 3. stale/ (AC-3)
    stale_inv = {
        "format": "screen-inventory@1",
        "screens": [
            {
                "id": "SCR.RPT.FAIFORM",
                "app": "report",
                "title": "AS9102 FAI Form Editor",
                "role_layout": "two-pane table and drawing preview",
                "phase": "P1a",
                "traces": [{"map": "per-01", "step": 1}],
                "keyboard_path": "Tab to form grid, Enter to edit cell",
                "wireframe": "WF.RPT.FAIFORM@1",
                "mockup": "none",
            }
        ],
    }
    files["stale/design/screens/inventory.json"] = pretty_json(stale_inv)
    files["stale/design/screens/approvals/wf-rpt-faiform-1.json"] = pretty_json(
        approval(90, "SCR.RPT.FAIFORM", "WF.RPT.FAIFORM@1", {"per-01": d1})
    )
    files["stale/journeys/per-01.json"] = pretty_json(j1_changed)

    # 4. bad-id/ (AC-6)
    bad_id_inv = {
        "format": "screen-inventory@1",
        "screens": [
            {
                "id": "SCR.rpt.form",
                "app": "report",
                "title": "Malformed ID Screen",
                "role_layout": "single pane",
                "phase": "P1a",
                "traces": [{"map": "per-01", "step": 1}],
                "keyboard_path": "Tab to edit",
                "wireframe": "none",
                "mockup": "none",
            }
        ],
    }
    files["bad-id/design/screens/inventory.json"] = pretty_json(bad_id_inv)
    files["bad-id/journeys/per-01.json"] = pretty_json(j1)

    # 5. tree/ (AC-4: two leaves, one citing unapproved WF.RPT.FAIFORM@3 and one citing approved WF.INSP.ALIGN@1)
    files["tree/rpt/faiform/NODE.md"] = (
        "---\n"
        "id: RPT.FAIFORM\n"
        "title: FAI Form UI\n"
        "status: leaf\n"
        "claimed_by:\n"
        "---\n\n"
        "# RPT.FAIFORM: FAI Form UI\n\n"
        "## Design\n\n"
        "- Screen: `SCR.RPT.FAIFORM`\n"
        "- Wireframes: WF.RPT.FAIFORM@3\n"
    )
    files["tree/insp/align/NODE.md"] = (
        "---\n"
        "id: INSP.ALIGN\n"
        "title: Alignment Workbench UI\n"
        "status: leaf\n"
        "claimed_by: alice\n"
        "wireframes: [WF.INSP.ALIGN@1]\n"
        "---\n\n"
        "# INSP.ALIGN: Alignment Workbench UI\n\n"
        "## Design\n\n"
        "- Screen: `SCR.INSP.ALIGN`\n"
        "- Wireframes: WF.INSP.ALIGN@1\n"
    )

    # 6. expected.json
    expected = {
        "format": "ux-screens-inv-expected@1",
        "license": "CC-BY-4.0",
        "cases": {
            "ac1_ok": {"exit": 0, "codes": []},
            "ac2_untraced": {"exit": 1, "codes": ["SCREEN-UNTRACED"], "subject": "SCR.RPT.FAIFORM"},
            "ac3_stale": {
                "exit": 1,
                "codes": ["SCREEN-APPROVAL-STALE"],
                "subject": "SCR.RPT.FAIFORM WF.RPT.FAIFORM@1",
            },
            "ac4_gate_unapproved": {
                "exit": 1,
                "codes": ["SCREEN-GATE-UNAPPROVED"],
                "subject": "RPT.FAIFORM WF.RPT.FAIFORM@3",
            },
            "ac5_ga_report": {
                "exit": 1,
                "codes": ["SCREEN-GA-MOCKUP-MISSING"],
                "subject": "SCR.RPT.BALLOON",
            },
            "ac6_bad_id": {"exit": 1, "codes": ["SCREEN-ID-SYNTAX"], "subject": "SCR.rpt.form"},
        },
    }
    files["expected.json"] = pretty_json(expected)
    return files


def main() -> int:
    check_mode = "--check" in sys.argv[1:]
    files = build_files()

    if check_mode:
        errors: list[str] = []
        for rel, want in sorted(files.items()):
            path = ROOT / rel
            if not path.is_file():
                errors.append(f"missing fixture file: {rel}")
                continue
            got = path.read_text(encoding="utf-8")
            if got != want:
                errors.append(f"fixture drift: {rel}")
        # Ensure no unexpected files live inside generated subdirs
        for sub in ("ok", "untraced", "stale", "bad-id", "tree"):
            subdir = ROOT / sub
            if subdir.is_dir():
                for p in sorted(subdir.rglob("*")):
                    if p.is_file():
                        rel = p.relative_to(ROOT).as_posix()
                        if rel not in files:
                            errors.append(f"unexpected extra file: {rel}")
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        return 0

    for rel, content in sorted(files.items()):
        path = ROOT / rel
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
