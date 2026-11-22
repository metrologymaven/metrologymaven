#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for CORE.PRJ.TREE.GROUP (AC-1..AC-5).

Generates `fixtures/core/prj/tree/group/p30/events.json`: a deterministic project namespace
and 30 `ElementCreated` events (`e1`..`e30`) under the reserved root group, so tests can
exercise group creation, order-preserving batch moves, non-unique renames, visibility toggles,
undo by head move, and exact-mode replay.

Usage: python3 fixtures/core/prj/tree/group/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

import json
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
HERE = Path(__file__).resolve().parent

NAMESPACE_UUID = uuid.UUID("11111111-2222-4333-8444-555555555555")


def reserved_v5(namespace: uuid.UUID, kind: str) -> str:
    payload = json.dumps({"kind": kind}, sort_keys=True, separators=(",", ":"))
    return str(uuid.uuid5(namespace, payload))


def v7_id(seq: int) -> str:
    # Synthetic deterministic RFC 9562 UUIDv7 (48-bit ms timestamp, version 7, variant 10xx).
    ts_ms = 0x0193_0000_0000 + seq
    hi = (ts_ms << 16) | 0x7000 | (seq & 0x0FFF)
    lo = 0x8000_0000_0000_0000 | (0x0000_0000_0000_1000 + seq)
    raw = hi.to_bytes(8, "big") + lo.to_bytes(8, "big")
    return str(uuid.UUID(bytes=raw))


def build_p30() -> bytes:
    root_id = reserved_v5(NAMESPACE_UUID, "project.tree_root")
    events = []
    element_ids = []
    for i in range(1, 31):
        eid = v7_id(i)
        element_ids.append(eid)
        events.append(
            {
                "type": "ElementCreated",
                "body": {
                    "id": eid,
                    "meta": {},
                    "name": f"Element {i}",
                    "parent": root_id,
                    "type": "test.plane",
                    "visible": True,
                },
            }
        )
    doc = {
        "schema": "fid.core.prj.tree.group.p30/1",
        "namespace": str(NAMESPACE_UUID),
        "root": root_id,
        "element_ids": element_ids,
        "events": events,
    }
    return (json.dumps(doc, sort_keys=True, indent=2) + "\n").encode("utf-8")


def main() -> int:
    target = HERE / "p30" / "events.json"
    want = build_p30()
    if "--check" in sys.argv[1:]:
        if not target.is_file() or target.read_bytes() != want:
            print(f"FAIL: stale fixture {target.relative_to(ROOT)}")
            return 1
        print("fixtures/core/prj/tree/group: ok")
        return 0
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(want)
    print(f"wrote {target}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
