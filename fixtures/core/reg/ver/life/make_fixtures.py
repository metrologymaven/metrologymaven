#!/usr/bin/env python3
"""Generate the fixtures of spec node CORE.REG.VER.LIFE (AC-1 .. AC-3).

Expected outcomes are stated from the leaf's rules, not computed by the Rust code:
- the legal transitions are the four pairs listed in REQ-002, written out by hand below;
- "at least 12 months" is checked with Python's own calendar: the release date must be on
  or after the same month and day one year later (REQ-003). No case uses 29 February as
  a `since` date, so that comparison never needs clamping.
CC-BY-4.0 project data.
Run:  python fixtures/core/reg/ver/life/make_fixtures.py
"""

import json
from datetime import date
from pathlib import Path

HERE = Path(__file__).resolve().parent
STATES = ["active", "deprecated", "hidden", "removed"]
LEGAL = {("active", "deprecated"), ("deprecated", "hidden"), ("hidden", "removed"), ("deprecated", "active")}


def twelve_months_after(since):
    return since.replace(year=since.year + 1)


def expect(old, new, since, release):
    if (old, new) not in LEGAL:
        return "illegal_transition"
    if (old, new) == ("deprecated", "hidden") and release < twelve_months_after(since):
        return "deprecation_too_short"
    return "ok"


cases = []


def case(name, old, since, new, release):
    cases.append({
        "name": name,
        "from": {"state": old, "since": since.isoformat()},
        "to": new,
        "release": release.isoformat(),
        "expect": expect(old, new, since, release),
    })


# A. All 16 state pairs, released two years after `since` so the date rule never bites.
for old in STATES:
    for new in STATES:
        case(f"{old} -> {new}", old, date(2024, 3, 10), new, date(2026, 3, 10))

# B. The 12-month boundary of deprecated -> hidden.
boundary = [
    # 2025-01-15 + 11 months = 2025-12-15, + 30 days = 2026-01-14.
    ("11 months 30 days (January)", date(2025, 1, 15), date(2026, 1, 14)),
    ("12 months exactly (January)", date(2025, 1, 15), date(2026, 1, 15)),
    # 2024-11-01 + 11 months = 2025-10-01, + 30 days = 2025-10-31.
    ("11 months 30 days (November)", date(2024, 11, 1), date(2025, 10, 31)),
    ("12 months exactly (November)", date(2024, 11, 1), date(2025, 11, 1)),
    ("12 months 1 day", date(2024, 11, 1), date(2025, 11, 2)),
    ("13 months, earlier day of month", date(2024, 6, 20), date(2025, 7, 5)),
    ("11 months, later day of month", date(2024, 6, 20), date(2025, 5, 28)),
    ("same day", date(2025, 6, 1), date(2025, 6, 1)),
    ("release before since", date(2025, 6, 1), date(2024, 12, 31)),
    ("month 31 to month 30, one year later", date(2024, 12, 31), date(2025, 12, 30)),
]
for name, since, release in boundary:
    case(f"deprecated -> hidden, {name}", "deprecated", since, "hidden", release)

ok = sum(1 for c in cases[:16] if c["expect"] == "ok")
assert ok == 4, ok
(HERE / "transitions.json").write_text(json.dumps({"cases": cases}, indent=1) + "\n", encoding="utf-8")


# AC-2: a registry of three definitions; the retained list is stated by hand.
def definition(cid, major, lifecycle):
    return {
        "id": cid,
        "major": major,
        "title": f"Demo command {cid}@{major}",
        "description": "Fixture definition for CORE.REG.VER.LIFE.",
        "params_schema": {"type": "object"},
        "result_schema": {"type": "object"},
        "side_effect": "R0",
        "mcp_projection": "exposed",
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "head_move",
        "preconditions": [],
        "effects": [],
        "algorithms": [],
        "exposure": ["cli"],
        "lifecycle": lifecycle,
        "web": "available",
    }


registry = [
    definition("demo.cmd", 1, {"state": "hidden", "since": "2026-06-01", "replaced_by": "demo.cmd@2",
                               "remove_after": "2027-06-01"}),
    definition("demo.cmd", 2, {"state": "active", "since": "2025-01-01"}),
    definition("demo.old", 1, {"state": "removed", "since": "2026-01-01"}),
]
(HERE / "registry.json").write_text(
    json.dumps({"definitions": registry, "expect_retained": ["demo.cmd@1", "demo.cmd@2"]}, indent=1) + "\n",
    encoding="utf-8")

# AC-3: migrator sets checked against the registry above, each labelled by hand.
ALG = "demo.cmd.migrate@1.0.0"
migrators = [
    {"name": "valid 1 -> 2", "migrators": [
        {"algorithm": ALG, "command": "demo.cmd", "from_major": 1, "to_major": 2}], "expect": "ok"},
    {"name": "target below source (3 -> 2)", "migrators": [
        {"algorithm": ALG, "command": "demo.cmd", "from_major": 3, "to_major": 2}], "expect": "migrator_invalid"},
    {"name": "target equals source", "migrators": [
        {"algorithm": ALG, "command": "demo.cmd", "from_major": 2, "to_major": 2}], "expect": "migrator_invalid"},
    {"name": "source removed", "migrators": [
        {"algorithm": ALG, "command": "demo.old", "from_major": 1, "to_major": 2}], "expect": "migrator_invalid"},
    {"name": "source not declared", "migrators": [
        {"algorithm": ALG, "command": "demo.none", "from_major": 1, "to_major": 2}], "expect": "migrator_invalid"},
    {"name": "duplicate source and target", "migrators": [
        {"algorithm": ALG, "command": "demo.cmd", "from_major": 1, "to_major": 2},
        {"algorithm": "demo.cmd.migrate@1.1.0", "command": "demo.cmd", "from_major": 1, "to_major": 2}],
     "expect": "migrator_invalid"},
]
(HERE / "migrators.json").write_text(json.dumps({"cases": migrators}, indent=1) + "\n", encoding="utf-8")
print(f"{len(cases)} transition cases, {len(migrators)} migrator cases")
