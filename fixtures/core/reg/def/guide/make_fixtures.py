#!/usr/bin/env python3
"""Fixtures of spec node CORE.REG.DEF.GUIDE (AC-1 .. AC-4), with an independent checker.

Writes, next to this script:
- commands.json      fixture command definitions (CORE.REG.DEF.FIELD shape), one per class
                     a guide may or may not name, plus one `removed` major;
- guides_ok.json     guide `demo.align_scan` (three steps) and a boundary guide at every
                     size limit, sorted by guide ID (REQ-005);
- guides_bad.json    error cases: `{"name", "guides", "expect": [codes]}`.

The expected codes are computed by `expect()` below, a stdlib-only reading of the leaf's
rules written without looking at the Rust results:
- REQ-001: ID pattern ^[a-z][a-z0-9]*\\.[a-z][a-z0-9_]*$ (and at most 64 bytes, a bound this
  build adds); step command names at most 128 bytes (also this build's bound); profile pattern ^[a-z][a-z0-9_]{1,31}$ (AUTO.MCP.SRV REQ-001); title <= 80,
  summary <= 500, purpose <= 200 characters (code points), 1..30 steps;
- REQ-002: a step naming an (ID, major) not declared, or declared `removed` (absent from
  the manifest, CORE.REG.VER.LIFE REQ-004), fails `guide_unknown_command`;
- review of PR #157: a step naming a `hidden` major fails `guide_hidden_command`, one whose
  lifecycle record cannot be read (unknown state, impossible date) fails
  `guide_bad_lifecycle`, and a `deprecated` major passes;
- REQ-003: a step naming an M1, S1 or A0 command fails `guide_forbidden_step`.
Reading errors (unknown_field, missing_field, bad_type) come first and stop the read; an
empty text or zero steps is `guide_empty`; two guides with one ID is `guide_duplicate`.
Project-authored text, CC-BY-4.0. Nothing here is standards text.

Run:    python fixtures/core/reg/def/guide/make_fixtures.py          (rewrite)
Check:  python fixtures/core/reg/def/guide/make_fixtures.py --check  (compare bytes)
"""

import json
import re
from datetime import date
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

GUIDE_ID = re.compile(r"[a-z][a-z0-9]*\.[a-z][a-z0-9_]*")
PROFILE = re.compile(r"[a-z][a-z0-9_]{1,31}")
COMMAND = re.compile(r"([a-z0-9](?:[a-z0-9_.]*[a-z0-9])?)@(0|[1-9][0-9]{0,4})")
FORBIDDEN = {"M1", "S1", "A0"}


# ------------------------------------------------------------------ fixture commands

def command(cid, major, side_effect, state="active", since="2026-01-01"):
    return {
        "id": cid,
        "major": major,
        "title": f"Fixture command {cid}",
        "description": f"Fixture definition of class {side_effect} for the task-guide tests.",
        "params_schema": {"type": "object"},
        "result_schema": {"type": "object"},
        "side_effect": side_effect,
        "mcp_projection": "exposed" if side_effect in ("R0", "R1", "W2") else "request_only",
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "head_move" if side_effect == "W2" else "not_undoable",
        "preconditions": [],
        "effects": [],
        "algorithms": [],
        "exposure": ["cli", "mcp"],
        "lifecycle": {"state": state, "since": since},
        "web": "available",
    }


COMMANDS = [
    command("demo.load_scan", 1, "R0"),
    command("demo.fit_alignment", 1, "R1"),
    command("demo.apply_alignment", 1, "W2"),
    command("demo.export_report", 1, "X1"),
    command("demo.move_axis", 1, "M1"),
    command("demo.sign_report", 1, "S1"),
    command("demo.set_safety_limits", 1, "A0"),
    command("demo.old_import", 1, "R0", state="removed"),
    command("demo.legacy_fit", 1, "R1", state="deprecated"),
    command("demo.retired_export", 1, "R0", state="hidden"),
    command("demo.broken_life", 1, "R0", since="2026-02-30"),
    command("demo.odd_state", 1, "R0", state="paused"),
]


# ------------------------------------------------------------------ guides

def step(cmd, purpose):
    return {"command": cmd, "purpose": purpose}


def guide(gid="demo.align_scan", steps=None, **over):
    g = {
        "id": gid,
        "title": "Align a scan to CAD",
        "summary": "Load a point cloud, fit it to the nominal CAD model and apply the alignment "
                   "to the working document.",
        "profile": "core",
        "steps": steps if steps is not None else [
            step("demo.load_scan@1", "Load the point cloud to align."),
            step("demo.fit_alignment@1", "Fit the scan to the CAD model and review the residuals."),
            step("demo.apply_alignment@1", "Apply the accepted alignment to the working document."),
        ],
    }
    g.update(over)
    return g


def exactly(n, seed):
    """A text of exactly n code points, with a non-ASCII letter so bytes != characters."""
    base = (seed + " étape ") * (n // 4 + 2)
    return base[:n]


ALLOWED = ["demo.load_scan@1", "demo.fit_alignment@1", "demo.apply_alignment@1", "demo.export_report@1"]

BOUNDS = guide(
    "demo.bounds_check",
    title=exactly(80, "Title"),
    summary=exactly(500, "Summary"),
    profile="p" + "x" * 31,
    steps=[step(ALLOWED[i % 4], exactly(200, f"Step {i}")) for i in range(30)],
)

DEPRECATED_OK = guide("demo.legacy_path", steps=[
    step("demo.load_scan@1", "Load the point cloud."),
    step("demo.legacy_fit@1", "Fit with the deprecated major; still allowed."),
])

GUIDES_OK = [guide(), BOUNDS, DEPRECATED_OK]

BAD = []


def bad(name, guides):
    BAD.append({"name": name, "guides": guides})


# AC-2, REQ-002
bad("ghost command", [guide(steps=[step("demo.load_scan@1", "Load."), step("demo.ghost_cmd@1", "Nothing.")])])
bad("undeclared major", [guide(steps=[step("demo.load_scan@2", "Load with a major that does not exist.")])])
bad("removed major", [guide(steps=[step("demo.old_import@1", "A major the manifest no longer lists.")])])
bad("hidden major", [guide(steps=[step("demo.retired_export@1", "A hidden major.")])])
bad("impossible lifecycle date", [guide(steps=[step("demo.broken_life@1", "Lifecycle since 2026-02-30.")])])
bad("unknown lifecycle state", [guide(steps=[step("demo.odd_state@1", "Lifecycle state paused.")])])
bad("128-byte command name, undeclared", [guide(steps=[step("d." + "a" * 126 + "@1", "Long name.")])])
bad("129-byte command name", [guide(steps=[step("d." + "a" * 127 + "@1", "Too long a name.")])])
# AC-3, REQ-003
bad("M1 step", [guide(steps=[step("demo.load_scan@1", "Load."), step("demo.move_axis@1", "Move an axis.")])])
bad("S1 step", [guide(steps=[step("demo.sign_report@1", "Sign the report.")])])
bad("A0 step", [guide(steps=[step("demo.set_safety_limits@1", "Change the safety limits.")])])
bad("unknown then forbidden, in step order",
    [guide(steps=[step("demo.move_axis@1", "Move."), step("demo.ghost_cmd@1", "Ghost."),
                  step("demo.load_scan@1", "Load.")])])
# AC-4, REQ-001 sizes
bad("31 steps", [guide(steps=[step("demo.load_scan@1", "Load again.")] * 31)])
bad("201-character purpose", [guide(steps=[step("demo.load_scan@1", exactly(201, "Long"))])])
bad("81-character title", [guide(title=exactly(81, "Title"))])
bad("501-character summary", [guide(summary=exactly(501, "Summary"))])
# guide_bad_id
for i, gid in enumerate(["Demo.align_scan", "demo", "demo.align-scan", "1demo.align", "demo.align.scan",
                         "demo._align", "de_mo.align", "demo." + "a" * 60]):
    bad(f"bad guide id {i}: {gid[:20]}", [guide(gid)])
for p in ["Core", "c", "core-app", "p" + "x" * 32, "_core"]:
    bad(f"bad profile {p[:12]}", [guide(profile=p)])
# guide_empty
bad("empty title", [guide(title="")])
bad("empty summary", [guide(summary="")])
bad("no steps", [guide(steps=[])])
bad("empty purpose", [guide(steps=[step("demo.load_scan@1", "")])])
# reading errors
bad("unknown top-level field (a value)", [dict(guide(), values=[1.0])])
bad("unknown step field", [guide(steps=[dict(step("demo.load_scan@1", "Load."), params={})])])
missing = guide()
del missing["summary"]
bad("missing summary", [missing])
bad("missing purpose", [guide(steps=[{"command": "demo.load_scan@1"}])])
bad("major with leading zero", [guide(steps=[step("demo.load_scan@01", "Load.")])])
bad("command without major", [guide(steps=[step("demo.load_scan", "Load.")])])
bad("title not a string", [guide(title=7)])
bad("steps not an array", [guide(steps={"command": "demo.load_scan@1"})])
bad("step not an object", [guide(steps=["demo.load_scan@1"])])
# lists
bad("duplicate guide id", [guide(), guide()])
bad("errors in two guides", [guide("demo.first", steps=[step("demo.ghost_cmd@1", "Ghost.")]),
                             guide("demo.second", steps=[step("demo.sign_report@1", "Sign.")])])


# ------------------------------------------------------------------ independent checker

class ReadError(Exception):
    pass


def read_guide(g):
    """Reading and shape rules; raises ReadError(code) on the first failure."""
    if not isinstance(g, dict):
        raise ReadError("bad_type")
    if any(k not in ("id", "title", "summary", "profile", "steps") for k in g):
        raise ReadError("unknown_field")
    for k in ("id", "title", "summary", "profile"):
        if k not in g:
            raise ReadError("missing_field")
        if not isinstance(g[k], str):
            raise ReadError("bad_type")
    if "steps" not in g:
        raise ReadError("missing_field")
    if not isinstance(g["steps"], list):
        raise ReadError("bad_type")
    for s in g["steps"]:
        if not isinstance(s, dict):
            raise ReadError("bad_type")
        if any(k not in ("command", "purpose") for k in s):
            raise ReadError("unknown_field")
        for k in ("command", "purpose"):
            if k not in s:
                raise ReadError("missing_field")
            if not isinstance(s[k], str):
                raise ReadError("bad_type")
            if k == "command" and (not COMMAND.fullmatch(s[k]) or len(s[k].split("@")[0]) > 128):
                raise ReadError("bad_type")
    if not GUIDE_ID.fullmatch(g["id"]) or len(g["id"].encode()) > 64:
        raise ReadError("guide_bad_id")
    if not PROFILE.fullmatch(g["profile"]):
        raise ReadError("guide_bad_id")
    for k, limit in (("title", 80), ("summary", 500)):
        if len(g[k]) == 0:
            raise ReadError("guide_empty")
        if len(g[k]) > limit:
            raise ReadError("guide_too_large")
    if len(g["steps"]) == 0:
        raise ReadError("guide_empty")
    if len(g["steps"]) > 30:
        raise ReadError("guide_too_large")
    for s in g["steps"]:
        if len(s["purpose"]) == 0:
            raise ReadError("guide_empty")
        if len(s["purpose"]) > 200:
            raise ReadError("guide_too_large")


def lifecycle_state(life):
    """The state of a fixture lifecycle record, or "bad" when it cannot be read."""
    if set(life) - {"state", "since", "replaced_by", "remove_after"}:
        return "bad"
    if life.get("state") not in ("active", "deprecated", "hidden", "removed"):
        return "bad"
    try:
        date.fromisoformat(life.get("since", ""))
    except ValueError:
        return "bad"
    return life["state"]


def expect(guides):
    """Codes the registry build reports for this list of guides."""
    try:
        for g in guides:
            read_guide(g)
    except ReadError as e:
        return [str(e)]
    declared = {(c["id"], c["major"]): c for c in COMMANDS}
    out = []
    for g in guides:
        for s in g["steps"]:
            name, major = COMMAND.fullmatch(s["command"]).groups()
            c = declared.get((name, int(major)))
            state = lifecycle_state(c["lifecycle"]) if c else None
            if c is None or state == "removed":
                out.append("guide_unknown_command")
            elif state == "bad":
                out.append("guide_bad_lifecycle")
            elif state == "hidden":
                out.append("guide_hidden_command")
            elif c["side_effect"] in FORBIDDEN:
                out.append("guide_forbidden_step")
    ids = [g["id"] for g in guides]
    out += ["guide_duplicate" for i in sorted(set(ids)) if ids.count(i) > 1]
    return out


def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False) + "\n").encode("utf-8")


def outputs():
    assert expect(GUIDES_OK) == [], "guides_ok must pass"
    assert [g["id"] for g in GUIDES_OK] == sorted(g["id"] for g in GUIDES_OK), "guides_ok must be sorted"
    cases = [dict(c, expect=expect(c["guides"])) for c in BAD]
    for c in cases:
        assert c["expect"], f"case {c['name']} must fail"
    return {"commands.json": dump(COMMANDS), "guides_ok.json": dump(GUIDES_OK), "guides_bad.json": dump(cases)}


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in outputs().items():
        p = HERE / name
        if check:
            if not p.exists() or p.read_bytes() != data:
                stale.append(name)
        else:
            p.write_bytes(data)
    if stale:
        print("stale fixtures (rerun make_fixtures.py):", ", ".join(stale))
        return 1
    print("CORE.REG.DEF.GUIDE fixtures:", "ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
