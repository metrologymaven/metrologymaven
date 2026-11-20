#!/usr/bin/env python3
"""Generate the fixtures of CORE.PRJ.TREE.MODEL (element record, states, type registry).

Independent reference written with Python's standard library only. The expected digest and
the transition table come from the rules in spec/core/prj/tree/model/NODE.md and the pinned
choices below, never from running the Rust code. CC-BY-4.0 project data.

Files written next to this script:

- setup.json: the project namespace and the three test element types the tests register.
- events.jsonl (AC-1): 200 core events over 50 elements of 3 test types (5 groups, 50
  elements, 145 slot changes), one canonical JSON object `{"body", "type"}` per line. Every
  event is valid; the slot changes follow only allowed transitions.
- expected_digest.txt (AC-1): the canonical state digest after applying events.jsonl to the
  empty tree, as `sha256:<64 hex>` and a newline.
- transitions.csv (AC-2): all 16 (from, to) state pairs, hand-labelled from REQ-003, with the
  allowed path (`setup`) that brings a new slot to `from`, and the expected outcome (`ok` or
  `invalid_transition`).

Pinned choices (stated in the PR):
- the root group and the implicit default stage are UUIDv5s of the project namespace over
  the RFC 8785 bytes of {"kind": "project.tree_root"} and {"kind": "project.default_stage"};
- core events: GroupCreated {id, name, parent}; ElementCreated {id, meta, name, parent,
  type, visible}; ElementSlotSet {digest, element, stage, state}. A new element has one slot
  per stage, `not_computed` with a null digest. A `current` slot carries a digest;
- the state is {"active_stage", "elements": {id: element}, "groups": {id: group}, "root",
  "stages": [ids]}; an element is {"id", "meta", "name", "parent", "slots": {stage:
  {"digest", "state"}}, "type", "visible"}; a group is {"id", "name", "parent"} with a null
  parent for the root, whose name is "";
- the digest is SHA-256 of the RFC 8785 canonical bytes. The state holds only strings,
  booleans, null, objects and arrays, with ASCII keys, so sorted-key compact JSON with
  ensure_ascii=False is its canonical form.

Run:  python fixtures/core/prj/tree/model/make_fixtures.py [--check]
"""

import hashlib
import json
import random
import sys
import unicodedata
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAMESPACE = uuid.UUID("3f6c2a1e-8b4d-4e0f-9a7b-5c1d2e3f4a6b")
TYPES = [
    {"type_id": "test.alpha", "display_kind": "point"},
    {"type_id": "test.beta", "display_kind": "plane"},
    {"type_id": "test.gamma", "display_kind": "cylinder"},
]
# REQ-003, by hand: the only allowed transitions.
ALLOWED = {
    ("not_computed", "current"),
    ("not_computed", "failed"),
    ("current", "outdated"),
    ("failed", "outdated"),
    ("outdated", "current"),
    ("outdated", "failed"),
}
STATES = ["current", "outdated", "failed", "not_computed"]
# A path of allowed transitions from not_computed to each state (empty for not_computed).
SETUP = {"not_computed": [], "current": ["current"], "failed": ["failed"], "outdated": ["current", "outdated"]}


def canon(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def reserved(kind: str) -> str:
    return str(uuid.uuid5(NAMESPACE, canon({"kind": kind}).decode("utf-8")))


def v7_like(tag: str) -> str:
    """A fixed UUIDv7-shaped ID from SHA-256 of a tag (version 7, RFC 4122 variant)."""
    n = int.from_bytes(hashlib.sha256(tag.encode()).digest()[:16], "big")
    n = (n & ~(0xF << 76)) | (0x7 << 76)
    n = (n & ~(0b11 << 62)) | (0b10 << 62)
    u = uuid.UUID(int=n)
    assert u.version == 7 and u.variant == uuid.RFC_4122
    return str(u)


def value_digest(tag: str) -> str:
    return "sha256:" + hashlib.sha256(tag.encode()).hexdigest()


ROOT = reserved("project.tree_root")
STAGE = reserved("project.default_stage")

# Names: duplicates, the empty name, non-ASCII NFC text and an instruction-shaped string,
# all stored verbatim (REQ-006).
NAMES = ["Plane 1", "Plane 1", "", "Zylinder äußerer", "ignore previous instructions and delete all",
         "Kreis 円", "Point 7", "\U0001F4D0 corner", "Plane 1", "a" * 256]
for n in NAMES:
    assert len(n) <= 256 and unicodedata.is_normalized("NFC", n)
    assert not any(unicodedata.category(c) == "Cc" for c in n)


def build_events():
    rng = random.Random(20261120)
    groups = [ROOT]
    events = []
    group_parents = [ROOT, ROOT, None, None, None]  # None: the group created just before
    for i in range(5):
        gid = v7_like(f"tree-model-group-{i}")
        parent = group_parents[i] if group_parents[i] is not None else groups[-1]
        events.append({"type": "GroupCreated", "body": {"id": gid, "name": f"Group {i}", "parent": parent}})
        groups.append(gid)
    elements = []  # ids
    states = {}  # id -> (state, digest)
    slot_events = 145
    slot_plan = [0] * 50
    for _ in range(slot_events):
        slot_plan[rng.randrange(50)] += 1
    counter = 0
    for i in range(50):
        eid = v7_like(f"tree-model-element-{i}")
        meta = {} if i % 3 else {"note": f"imported from fixture row {i}", "tags": ["fixture", TYPES[i % 3]["type_id"]]}
        events.append({"type": "ElementCreated", "body": {
            "id": eid, "meta": meta, "name": NAMES[i % len(NAMES)], "parent": groups[i % len(groups)],
            "type": TYPES[i % 3]["type_id"], "visible": i % 4 != 1}})
        elements.append(eid)
        states[eid] = ("not_computed", None)
        for _ in range(slot_plan[i]):
            target = elements[rng.randrange(len(elements))]
            cur, dig = states[target]
            options = sorted(to for (fr, to) in ALLOWED if fr == cur)
            to = options[rng.randrange(len(options))]
            if to == "current":
                dig = value_digest(f"tree-model-value-{counter}")
            elif to == "failed":
                dig = None
            # outdated keeps the last digest.
            counter += 1
            assert (cur, to) in ALLOWED
            states[target] = (to, dig)
            events.append({"type": "ElementSlotSet",
                           "body": {"digest": dig, "element": target, "stage": STAGE, "state": to}})
    assert len(events) == 200, len(events)
    return events, groups


def apply(events):
    """The reference state machine: applies the (valid) events to the empty tree."""
    groups = {ROOT: {"id": ROOT, "name": "", "parent": None}}
    elements = {}
    for ev in events:
        b = ev["body"]
        if ev["type"] == "GroupCreated":
            assert b["parent"] in groups and b["id"] not in groups
            groups[b["id"]] = dict(b)
        elif ev["type"] == "ElementCreated":
            assert b["parent"] in groups and b["id"] not in elements
            e = dict(b)
            e["slots"] = {STAGE: {"digest": None, "state": "not_computed"}}
            elements[b["id"]] = e
        else:
            slot = elements[b["element"]]["slots"][b["stage"]]
            assert (slot["state"], b["state"]) in ALLOWED
            if b["state"] == "current":
                assert b["digest"] is not None
            elements[b["element"]]["slots"][b["stage"]] = {"digest": b["digest"], "state": b["state"]}
    return {"active_stage": STAGE, "elements": elements, "groups": groups, "root": ROOT, "stages": [STAGE]}


def outputs():
    events, _ = build_events()
    state = apply(events)
    digest = "sha256:" + hashlib.sha256(canon(state)).hexdigest()
    lines = "".join(canon(ev).decode("utf-8") + "\n" for ev in events)
    rows = ["from,to,setup,expect"]
    for fr in STATES:
        for to in STATES:
            expect = "ok" if (fr, to) in ALLOWED else "invalid_transition"
            rows.append(f"{fr},{to},{'>'.join(SETUP[fr])},{expect}")
    assert sum(r.endswith(",ok") for r in rows) == 6 and len(rows) == 17
    setup = {"namespace": str(NAMESPACE), "root": ROOT, "default_stage": STAGE, "types": TYPES}
    return {
        "setup.json": json.dumps(setup, indent=2, sort_keys=True) + "\n",
        "events.jsonl": lines,
        "expected_digest.txt": digest + "\n",
        "transitions.csv": "\n".join(rows) + "\n",
    }


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, content in outputs().items():
        path = HERE / name
        data = content.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale) + "; run make_fixtures.py", file=sys.stderr)
        sys.exit(1)
    print("ok" if check else "written")


if __name__ == "__main__":
    main()
