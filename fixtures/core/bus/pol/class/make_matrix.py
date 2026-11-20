#!/usr/bin/env python3
"""Write the class / principal / human_only matrix of CORE.BUS.POL.CLASS.

An independent reference written with Python's standard library only. The Rust module
`fid_bus::policy::class` is checked against the files written here; nothing in them is
produced by running the Rust code.

Files written:
- fixtures/core/bus/pol/class/matrix.csv (AC-1): one row per combination of principal
  kind, side-effect class and `human_only` value (6 x 11 x 2 = 132 rows), with the
  expected outcome `allow` or `policy/<reason>`;
- schema/policy-matrix.json (REQ-005): the same table as RFC 8785 canonical JSON plus a
  trailing newline, for documentation generation.

Rules, from CONSTITUTION §3 (side-effect classes, human-only commands D-031, principals
D-012) and spec/core/bus/pol/class/NODE.md, applied in this order:
1. a `human` principal is allowed (motion release is CMM's check, the S1 ceremony is
   TRUST's, both later);
2. any other principal invoking an M1, S1, W3 or A0 command -> policy/class_forbidden
   (REQ-001, REQ-003); this is checked before `human_only`, so an agent calling a
   human-only W3 command gets class_forbidden;
3. any other principal invoking a `human_only: true` command -> policy/human_only.
   REQ-002 names agent and script; service, device and system are refused too, fail
   closed (a human-only command is one only a human may run; see the PR and the node's
   Open questions);
4. otherwise allow.

Every value is ASCII, so json.dumps(sort_keys=True, separators=(",", ":")) equals RFC 8785.
CC-BY-4.0 project data.
Run:  python fixtures/core/bus/pol/class/make_matrix.py [--check]
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[4]
CSV_PATH = HERE / "matrix.csv"
JSON_PATH = ROOT / "schema" / "policy-matrix.json"

# Declaration order of the journal's principal kinds (CONSTITUTION §3, D-012) and of the
# side-effect class table in CONSTITUTION §3.
PRINCIPALS = ["human", "agent", "script", "service", "device", "system"]
CLASSES = ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]
HUMAN_ONLY = [False, True]

# "Agents never perform M1, S1, W3 or A0" (CONSTITUTION §3); extended to every non-human
# kind by REQ-CORE.BUS.POL.CLASS-003.
HUMAN_CLASSES = {"M1", "S1", "W3", "A0"}

SCHEMA_VERSION = 1
NODE = "CORE.BUS.POL.CLASS"


def decide(principal, cls, human_only):
    """None for allow, else the `policy` reason."""
    if principal == "human":
        return None
    if cls in HUMAN_CLASSES:
        return "class_forbidden"
    if human_only:
        return "human_only"
    return None


def rows():
    out = []
    for p in PRINCIPALS:
        for c in CLASSES:
            for h in HUMAN_ONLY:
                out.append((p, c, h, decide(p, c, h)))
    return out


def csv_bytes(table):
    lines = ["principal,class,human_only,expected"]
    for p, c, h, reason in table:
        expected = "allow" if reason is None else "policy/" + reason
        lines.append(",".join([p, c, "true" if h else "false", expected]))
    return ("\n".join(lines) + "\n").encode("ascii")


def json_bytes(table):
    entries = []
    for p, c, h, reason in table:
        e = {"principal": p, "class": c, "human_only": h, "allow": reason is None}
        if reason is not None:
            e["code"] = "policy"
            e["reason"] = reason
        entries.append(e)
    doc = {
        "node": NODE,
        "principal_kinds": PRINCIPALS,
        "classes": CLASSES,
        "rows": entries,
        "schema_version": SCHEMA_VERSION,
    }
    return (json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=True) + "\n").encode("ascii")


def self_check(table):
    assert len(table) == len(PRINCIPALS) * len(CLASSES) * len(HUMAN_ONLY) == 132
    assert len({(p, c, h) for p, c, h, _ in table}) == 132
    # Hand-labelled spot checks from the node's acceptance criteria.
    lookup = {(p, c, h): r for p, c, h, r in table}
    assert lookup[("agent", "W3", False)] == "class_forbidden"  # AC-2
    assert lookup[("script", "W2", True)] == "human_only"  # AC-3
    assert lookup[("human", "W2", True)] is None  # AC-3, upgraded session
    assert lookup[("agent", "W2", False)] is None
    assert all(r is None for (p, _, _), r in lookup.items() if p == "human")


def main():
    table = rows()
    self_check(table)
    outputs = [(CSV_PATH, csv_bytes(table)), (JSON_PATH, json_bytes(table))]
    if "--check" in sys.argv[1:]:
        stale = [p for p, data in outputs if not p.exists() or p.read_bytes() != data]
        if stale:
            for p in stale:
                print("stale fixture (run make_matrix.py): " + p.relative_to(ROOT).as_posix())
            sys.exit(1)
        print("fixtures up to date")
        return
    for p, data in outputs:
        p.write_bytes(data)
        print("wrote " + p.relative_to(ROOT).as_posix())


if __name__ == "__main__":
    main()
