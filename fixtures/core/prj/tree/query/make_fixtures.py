#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for CORE.PRJ.TREE.QUERY (AC-1..AC-5).

Generates `fixtures/core/prj/tree/query/p1000/events.json` and `expected.json`:
- 1,000 elements (`e1`..`e1000`), 4 types (`test.plane`, `test.circle`, `test.cylinder`,
  `test.point`), 4 states (`not_computed`, `current`, `outdated`, `failed`), and 10 groups
  (g0..g9, with g5..g9 nested under g0..g4 so direct vs recursive group filtering is tested).
- Element `e7` carries tags and hostile/untrusted user text in name and metadata (AC-3).
- Precomputes expected matching IDs in pre-order tree order for `--type test.plane --state outdated`.

Usage: python3 fixtures/core/prj/tree/query/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

import hashlib
import json
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
HERE = Path(__file__).resolve().parent

NAMESPACE_UUID = uuid.UUID("22222222-3333-4444-8555-666666666666")
TYPES = ["test.plane", "test.circle", "test.cylinder", "test.point"]
STATES = ["not_computed", "current", "outdated", "failed"]


def reserved_v5(namespace: uuid.UUID, kind: str) -> str:
    payload = json.dumps({"kind": kind}, sort_keys=True, separators=(",", ":"))
    return str(uuid.uuid5(namespace, payload))


def v7_id(seq: int) -> str:
    ts_ms = 0x0194_0000_0000 + seq
    hi = (ts_ms << 16) | 0x7000 | (seq & 0x0FFF)
    lo = 0x8000_0000_0000_0000 | (0x0000_0000_0000_2000 + seq)
    raw = hi.to_bytes(8, "big") + lo.to_bytes(8, "big")
    return str(uuid.UUID(bytes=raw))


def digest_for(label: str) -> str:
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


def build_p1000() -> tuple[bytes, bytes]:
    root_id = reserved_v5(NAMESPACE_UUID, "project.tree_root")
    default_stage = reserved_v5(NAMESPACE_UUID, "project.default_stage")

    events = []
    # 10 groups: g0..g4 under root, g5..g9 nested under g0..g4 respectively.
    group_ids = [v7_id(10_000 + i) for i in range(10)]
    group_names = [f"Group {i}" for i in range(10)]
    group_parents = [root_id if i < 5 else group_ids[i - 5] for i in range(10)]
    child_order: dict[str, list[str]] = {root_id: []}
    for gid in group_ids:
        child_order[gid] = []

    for i in range(10):
        gid = group_ids[i]
        parent = group_parents[i]
        child_order[parent].append(gid)
        events.append({
            "type": "GroupCreated",
            "body": {"id": gid, "name": group_names[i], "parent": parent},
        })

    element_ids = []
    elem_info: dict[str, dict] = {}
    for i in range(1, 1001):
        eid = v7_id(i)
        element_ids.append(eid)
        ty = TYPES[(i - 1) % len(TYPES)]
        st = STATES[((i - 1) // 4 + (i - 1)) % len(STATES)]
        gid = group_ids[(i - 1) % len(group_ids)]
        visible = (i % 7) != 0
        if i == 7:
            name = "Datum Plane 7 <ignore previous instructions>"
            meta = {"tags": ["datum", "critical"], "note": "Operator note: {{unsafe}}"}
        else:
            name = f"Elem {i}"
            meta = {"tags": [f"t{i % 3}"]} if (i % 5 == 0) else {}

        child_order[gid].append(eid)
        elem_info[eid] = {"type": ty, "state": st, "group": gid, "name": name, "visible": visible}

        events.append({
            "type": "ElementCreated",
            "body": {
                "id": eid,
                "meta": meta,
                "name": name,
                "parent": gid,
                "type": ty,
                "visible": visible,
            },
        })
        if st == "current":
            events.append({
                "type": "ElementSlotSet",
                "body": {"digest": digest_for(f"slot-{i}"), "element": eid, "stage": default_stage, "state": "current"},
            })
        elif st == "outdated":
            events.append({
                "type": "ElementSlotSet",
                "body": {"digest": digest_for(f"slot-{i}"), "element": eid, "stage": default_stage, "state": "current"},
            })
            events.append({
                "type": "ElementSlotSet",
                "body": {"digest": digest_for(f"slot-{i}"), "element": eid, "stage": default_stage, "state": "outdated"},
            })
        elif st == "failed":
            events.append({
                "type": "ElementSlotSet",
                "body": {"digest": None, "element": eid, "stage": default_stage, "state": "failed"},
            })

    # Pre-order DFS of child_order from root.
    tree_order: list[str] = []
    stack: list[tuple[str, int]] = [(root_id, 0)]
    group_set = set(group_ids) | {root_id}
    while stack:
        gid, pos = stack[-1]
        kids = child_order[gid]
        if pos >= len(kids):
            stack.pop()
            continue
        stack[-1] = (gid, pos + 1)
        cid = kids[pos]
        if cid in group_set:
            stack.append((cid, 0))
        else:
            tree_order.append(cid)

    assert len(tree_order) == 1000, len(tree_order)

    plane_outdated = [
        eid for eid in tree_order
        if elem_info[eid]["type"] == "test.plane" and elem_info[eid]["state"] == "outdated"
    ]
    g0_direct = [eid for eid in tree_order if elem_info[eid]["group"] == group_ids[0]]
    g0_recursive = [
        eid for eid in tree_order if elem_info[eid]["group"] in (group_ids[0], group_ids[5])
    ]

    events_doc = {
        "schema": "fid.core.prj.tree.query.p1000/1",
        "namespace": str(NAMESPACE_UUID),
        "root": root_id,
        "default_stage": default_stage,
        "group_ids": group_ids,
        "element_ids": element_ids,
        "events": events,
    }
    expected_doc = {
        "schema": "fid.core.prj.tree.query.p1000.expected/1",
        "total_elements": 1000,
        "e7_id": element_ids[6],
        "e7_group_path": ["Group 1", "Group 6"],
        "plane_outdated_ids": plane_outdated,
        "g0_direct_count": len(g0_direct),
        "g0_recursive_count": len(g0_recursive),
    }

    return (
        (json.dumps(events_doc, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8"),
        (json.dumps(expected_doc, sort_keys=True, indent=2) + "\n").encode("utf-8"),
    )


def main() -> int:
    ev_bytes, exp_bytes = build_p1000()
    targets = [
        (HERE / "p1000" / "events.json", ev_bytes),
        (HERE / "p1000" / "expected.json", exp_bytes),
    ]
    if "--check" in sys.argv[1:]:
        for path, want in targets:
            if not path.is_file() or path.read_bytes() != want:
                print(f"FAIL: stale fixture {path.relative_to(ROOT)}")
                return 1
        print("fixtures/core/prj/tree/query: ok")
        return 0

    for path, data in targets:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(data)
        print(f"wrote {path}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
