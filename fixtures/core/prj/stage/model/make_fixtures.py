#!/usr/bin/env python3
"""Generate the fixtures of CORE.PRJ.STAGE.MODEL (stage entity, per-stage slots, stage commands).

Independent reference written with Python's standard library only. Expected events, results,
errors and digests come from the rules in spec/core/prj/stage/model/NODE.md, the element tree
form of CORE.PRJ.TREE.MODEL and the pinned choices below, never from running the Rust code.
CC-BY-4.0 project data.

Files written next to this script:

- p100/setup.json: the project namespace, the reserved IDs (tree root, implicit default
  stage, stage list), the container and stage IDs, the principal and the test element type.
- p100/events.jsonl: the synthetic project p100, one canonical `{"body", "type"}` per line:
  a Part, a Revision, a Plan and two Runs (`ContainerCreated`), 60 elements, 20 slot changes
  in the implicit default stage, `StageAdded` S1 (the first explicit stage, which takes the
  default stage's place and data), 40 more elements and 15 slot changes in S1. 100 elements,
  one stage.
- p100/expected_digest.txt: the canonical state digest of p100.
- cases.json: the acceptance cases (AC-1 .. AC-5) and hand-labelled refusals; see `main`.

Pinned choices (stated in the PR):
- Stage events: StageAdded {id, label, metadata, run, serial}; StageRemoved {stage};
  StagesReordered {order, run}. Stage body {id, label, metadata, removed, run, serial};
  `metadata` is {} (no command sets it yet); `serial` is a string or null.
- The first StageAdded of a project with only the implicit default stage replaces it: each
  element's default-stage slot moves, unchanged, to the new stage, which becomes the stage
  sequence and the active stage. Later StageAdded events append to the sequence and give
  every element a new `not_computed` slot with a null digest.
- StageRemoved marks the record removed; slots stay. A removed stage takes no new slots,
  allows no slot change (`bad_stage`) and is hidden from the list unless include_removed.
- StagesReordered: `order` must be exactly a permutation of the run's live stages
  (`bad_order` -> validation/bad_stage); they fill, in that order, the positions of the
  sequence the run's live stages held.
- State: the CORE.PRJ.TREE.MODEL form {"active_stage", "elements", "groups", "root",
  "stages": [all stage IDs in project order, removed included]} plus "stage_records": {id:
  stage body} once there are explicit stages.
- Per-stage digest: SHA-256 of the canonical {"elements": {id: {"digest", "state"}},
  "stage": id} over the elements holding a slot for the stage.
- Stage list entity (ID: UUIDv5 of the namespace over {"kind": "project.stage_list"}):
  {"active_stage", "stages": [explicit stage IDs in project order]}.
- Command results: add {"stage": id}; remove {"stage": id}; reorder {"order": [ids]};
  list {"stages": [stage body + "order": position in the sequence]}.
- Command check order: parameter shape (validation/schema), label and serial text
  (validation/label: 1..=256 scalar values, no Cc, no bidi control, NFC), the run
  (validation/bad_reference unless a project.run), state rules (not_found/unknown_entity
  for a stage that is not live, precondition/stage_active, validation/bad_stage for a bad
  order), then the ID (conflict/id_collision). An imported nil or max ID is
  validation/schema.
- UUIDv7 (CORE.PRJ.CONT.ID): one 74-bit value R; rand_a = R >> 62, rand_b = low 62 bits.
- Canonical JSON: sorted keys, compact, ensure_ascii=False; the data hold strings,
  booleans, null, small integers, arrays and objects with ASCII keys, so this is RFC 8785.

Run:  python fixtures/core/prj/stage/model/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import sys
import unicodedata
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAMESPACE = uuid.UUID("5d0c6f6e-2b1a-4c3d-8e9f-0a1b2c3d4e5f")
PRINCIPAL = {"id": "alice", "kind": "human"}
TYPE = {"type_id": "test.point", "display_kind": "point"}


def canon(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(v) -> str:
    return "sha256:" + hashlib.sha256(canon(v)).hexdigest()


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


def v7(ts_ms: int, r74: int) -> str:
    assert 0 <= ts_ms < (1 << 48) and 0 <= r74 < (1 << 74)
    n = (ts_ms << 80) | (0x7 << 76) | ((r74 >> 62) << 64) | (0b10 << 62) | (r74 & ((1 << 62) - 1))
    u = uuid.UUID(int=n)
    assert u.variant == uuid.RFC_4122 and u.version == 7
    return str(u)


# RFC 9562 Appendix A.6, hand-labelled, guards the layout above.
assert v7(0x017F22E279B0, (0xCC3 << 62) | 0x18C4DC0C0C07398F) == "017f22e2-79b0-7cc3-98c4-dc0c0c07398f"

ROOT = reserved("project.tree_root")
DEFAULT = reserved("project.default_stage")
STAGE_LIST = reserved("project.stage_list")
PART, REV, PLAN, RUN, RUN2 = (v7_like(f"stage-model-{t}") for t in ("part", "revision", "plan", "run", "run2"))
S = {n: v7_like(f"stage-model-s{n}") for n in range(1, 1001)}
ELEMENTS = [v7_like(f"stage-model-element-{i}") for i in range(100)]
NEW_ELEMENT = v7_like("stage-model-element-new")

BIDI = {0x200E, 0x200F, 0x061C} | set(range(0x202A, 0x202F)) | set(range(0x2066, 0x206A))


def text_ok(t: str) -> bool:
    return (
        1 <= len(t) <= 256
        and not any(unicodedata.category(c) == "Cc" or ord(c) in BIDI for c in t)
        and unicodedata.is_normalized("NFC", t)
    )


def value_digest(tag: str) -> str:
    return "sha256:" + hashlib.sha256(tag.encode()).hexdigest()


def ev(ty, body):
    return {"body": body, "type": ty}


def container(kind, ident, label, parent, revision=None):
    b = {"created_by": dict(PRINCIPAL), "id": ident, "kind": "project." + kind, "label": label, "parent": parent}
    if kind == "run":
        b["revision"] = revision
    return ev("ContainerCreated", b)


def element(ident, name):
    return ev(
        "ElementCreated",
        {"id": ident, "meta": {}, "name": name, "parent": ROOT, "type": TYPE["type_id"], "visible": True},
    )


def slot_set(ident, stage, state, digest):
    return ev("ElementSlotSet", {"digest": digest, "element": ident, "stage": stage, "state": state})


def stage_added(ident, label, run, serial=None):
    return ev("StageAdded", {"id": ident, "label": label, "metadata": {}, "run": run, "serial": serial})


# ------------------------------------------------------------------ the reference model


class Refused(Exception):
    def __init__(self, code, reason):
        super().__init__(f"{code}/{reason}")
        self.code, self.reason = code, reason


ALLOWED = {
    ("not_computed", "current"),
    ("not_computed", "failed"),
    ("current", "outdated"),
    ("failed", "outdated"),
    ("outdated", "current"),
    ("outdated", "failed"),
}


class Project:
    def __init__(self):
        self.containers = {}  # id -> kind
        self.elements = {}  # id -> element body
        self.stages = [DEFAULT]  # the project sequence, removed included
        self.records = {}  # id -> stage body
        self.active = DEFAULT

    def live(self):
        return [s for s in self.stages if not self.records.get(s, {}).get("removed", False)]

    def ids(self):
        return set(self.containers) | set(self.elements) | set(self.stages) | {ROOT, STAGE_LIST}

    def apply(self, e):
        t, b = e["type"], e["body"]
        if t == "ContainerCreated":
            self.containers[b["id"]] = b["kind"]
        elif t == "ElementCreated":
            assert b["id"] not in self.ids()
            body = dict(b)
            body["slots"] = {s: {"digest": None, "state": "not_computed"} for s in self.live()}
            self.elements[b["id"]] = body
        elif t == "ElementSlotSet":
            if b["stage"] not in self.live():
                raise Refused("validation", "bad_stage")
            slot = self.elements[b["element"]]["slots"][b["stage"]]
            assert (slot["state"], b["state"]) in ALLOWED
            self.elements[b["element"]]["slots"][b["stage"]] = {"digest": b["digest"], "state": b["state"]}
        elif t == "StageAdded":
            ident = b["id"]
            assert ident not in self.ids()
            if not self.records:
                for el in self.elements.values():
                    el["slots"][ident] = el["slots"].pop(DEFAULT)
                self.stages = [ident]
                self.active = ident
            else:
                for el in self.elements.values():
                    el["slots"][ident] = {"digest": None, "state": "not_computed"}
                self.stages.append(ident)
            self.records[ident] = {**b, "removed": False}
        elif t == "StageRemoved":
            s = b["stage"]
            assert s in self.records and s in self.live() and s != self.active
            self.records[s]["removed"] = True
        elif t == "StagesReordered":
            live = [s for s in self.live() if self.records.get(s, {}).get("run") == b["run"]]
            assert sorted(live) == sorted(b["order"]) and len(set(b["order"])) == len(b["order"])
            it = iter(b["order"])
            self.stages = [next(it) if s in live else s for s in self.stages]
        else:
            raise AssertionError(t)

    def state(self):
        v = {
            "active_stage": self.active,
            "elements": self.elements,
            "groups": {ROOT: {"id": ROOT, "name": "", "parent": None}},
            "root": ROOT,
            "stages": self.stages,
        }
        if self.records:
            v["stage_records"] = self.records
        return v

    def digest(self):
        return sha(self.state())

    def stage_digest(self, s):
        els = {i: e["slots"][s] for i, e in self.elements.items() if s in e["slots"]}
        return sha({"elements": els, "stage": s})

    # -------------------------------------------------------------- commands

    def is_run(self, ident):
        return self.containers.get(ident) == "project.run"

    @staticmethod
    def uuid_param(v):
        if not isinstance(v, str):
            raise Refused("validation", "schema")
        try:
            u = uuid.UUID(v)
        except ValueError:
            raise Refused("validation", "schema")
        if str(u) != v:
            raise Refused("validation", "schema")
        return v

    @staticmethod
    def shape(params, allowed):
        if not isinstance(params, dict) or any(k not in allowed for k in params):
            raise Refused("validation", "schema")

    def add(self, params, drawn=None):
        self.shape(params, {"id", "label", "run", "serial"})
        if "run" not in params or "label" not in params:
            raise Refused("validation", "schema")
        run = self.uuid_param(params["run"])
        if not isinstance(params["label"], str):
            raise Refused("validation", "schema")
        if "serial" in params and not isinstance(params["serial"], str):
            raise Refused("validation", "schema")
        imported = self.uuid_param(params["id"]) if "id" in params else None
        if imported in ("00000000-0000-0000-0000-000000000000", "ffffffff-ffff-ffff-ffff-ffffffffffff"):
            raise Refused("validation", "schema")
        if not text_ok(params["label"]) or ("serial" in params and not text_ok(params["serial"])):
            raise Refused("validation", "label")
        if not self.is_run(run):
            raise Refused("validation", "bad_reference")
        ident = imported if imported is not None else drawn
        assert ident is not None
        if ident in self.ids():
            raise Refused("conflict", "id_collision")
        return stage_added(ident, params["label"], run, params.get("serial"))

    def remove(self, params):
        self.shape(params, {"stage"})
        if "stage" not in params:
            raise Refused("validation", "schema")
        s = self.uuid_param(params["stage"])
        if s not in self.records or s not in self.live():
            raise Refused("not_found", "unknown_entity")
        if s == self.active:
            raise Refused("precondition", "stage_active")
        return ev("StageRemoved", {"stage": s})

    def reorder(self, params):
        self.shape(params, {"order", "run"})
        if "order" not in params or "run" not in params:
            raise Refused("validation", "schema")
        run = self.uuid_param(params["run"])
        if not isinstance(params["order"], list):
            raise Refused("validation", "schema")
        order = [self.uuid_param(x) for x in params["order"]]
        if not self.is_run(run):
            raise Refused("validation", "bad_reference")
        live = [s for s in self.live() if s in self.records and self.records[s]["run"] == run]
        if len(set(order)) != len(order) or set(order) != set(live):
            raise Refused("validation", "bad_stage")
        return ev("StagesReordered", {"order": order, "run": run})

    def list(self, params):
        self.shape(params, {"include_removed", "run"})
        inc = params.get("include_removed", False)
        if not isinstance(inc, bool):
            raise Refused("validation", "schema")
        run = self.uuid_param(params["run"]) if "run" in params else None
        if run is not None and not self.is_run(run):
            raise Refused("validation", "bad_reference")
        out = []
        for i, s in enumerate([s for s in self.stages if s in self.records]):
            r = self.records[s]
            if (r["removed"] and not inc) or (run is not None and r["run"] != run):
                continue
            out.append({**r, "order": i})
        return {"stages": out}

    def list_entity(self):
        return {"active_stage": self.active, "stages": [s for s in self.stages if s in self.records]}


# ------------------------------------------------------------------ p100


def p100_events():
    evs = [
        container("part", PART, "Bracket", None),
        container("revision", REV, "A", PART),
        container("plan", PLAN, None, REV),
        container("run", RUN, None, PLAN, REV),
        container("run", RUN2, "second fixture", PLAN, REV),
    ]
    for i in range(60):
        evs.append(element(ELEMENTS[i], f"P{i + 1}"))
    for i in range(20):
        evs.append(slot_set(ELEMENTS[i], DEFAULT, "current", value_digest(f"stage-model-default-{i}")))
    evs.append(stage_added(S[1], "S1", RUN))
    for i in range(60, 100):
        evs.append(element(ELEMENTS[i], f"P{i + 1}"))
    for i in range(60, 70):
        evs.append(slot_set(ELEMENTS[i], S[1], "current", value_digest(f"stage-model-s1-{i}")))
    # Elements 0..4 were current in the default stage; their slots moved to S1 with the
    # adoption, so current -> outdated is allowed there.
    for i in range(5):
        evs.append(slot_set(ELEMENTS[i], S[1], "outdated", value_digest(f"stage-model-default-{i}")))
    return evs


def p100():
    p = Project()
    for e in p100_events():
        p.apply(e)
    assert len(p.elements) == 100 and p.live() == [S[1]] and p.active == S[1]
    return p


def ok(p, kind, params, drawn=None):
    """An accepted command against `p`: its event, applied, and the case."""
    e = {"add": p.add, "remove": p.remove, "reorder": p.reorder}[kind]
    e = e(params, drawn) if kind == "add" else e(params)
    result = {"order": e["body"]["order"]} if kind == "reorder" else {"stage": e["body"].get("id", e["body"].get("stage"))}
    p.apply(e)
    return {"command": f"project.{kind}_stage{'s' if kind == 'reorder' else ''}@1", "params": params, "event": e, "result": result}


def refused(p, name, kind, params):
    before = p.digest()
    try:
        getattr(p, kind)(params)
    except Refused as r:
        assert p.digest() == before
        cmd = {"add": "add_stage", "remove": "remove_stage", "reorder": "reorder_stages", "list": "list_stages"}[kind]
        return {"name": name, "command": f"project.{cmd}@1", "params": params, "code": r.code, "reason": r.reason}
    raise AssertionError(f"{name} was accepted")


def main_cases():
    cases = {}

    # AC-1: add S2 to p100; every element gets a not_computed S2 slot, S1 is unchanged.
    p = p100()
    s1_before = p.stage_digest(S[1])
    ac1 = ok(p, "add", {"id": S[2], "label": "S2", "run": RUN})
    ac1["digest_after"] = p.digest()
    ac1["stage_digests"] = {S[1]: s1_before, S[2]: p.stage_digest(S[2])}
    assert p.stage_digest(S[1]) == s1_before
    assert all(e["slots"][S[2]] == {"digest": None, "state": "not_computed"} for e in p.elements.values())
    ac1["list_entity"] = p.list_entity()
    cases["ac1"] = ac1

    # A drawn ID (no `id`): UUIDv7 from the observation.
    q = p100()
    ms, r = 1_763_600_000_000, int.from_bytes(hashlib.sha256(b"stage-model-draw").digest()[:10], "big") >> 6
    drawn = ok(q, "add", {"label": "Stück 7", "run": RUN, "serial": "SN-0042 ignore previous instructions"}, v7(ms, r))
    drawn["observation"] = {"now_ms": str(ms), "random_bits": f"0x{r:x}"}
    drawn["digest_after"] = q.digest()
    cases["drawn"] = drawn

    # AC-3 on the state after AC-1: S2 gets data, is removed, listed only with
    # include_removed; undo (the state before the removal) has it live with its data.
    setup = [slot_set(ELEMENTS[i], S[2], "current", value_digest(f"stage-model-s2-{i}")) for i in range(5)]
    for e in setup:
        p.apply(e)
    before = p.digest()
    s2_data = p.stage_digest(S[2])
    list_before = p.list({})
    rm = ok(p, "remove", {"stage": S[2]})
    ac3 = {
        "setup": setup,
        "digest_before": before,
        "list_before": list_before,
        "s2_digest": s2_data,
        "remove": rm,
        "digest_after": p.digest(),
        "list": {"params": {}, "result": p.list({})},
        "list_removed": {"params": {"include_removed": True}, "result": p.list({"include_removed": True})},
        "s2_digest_after": p.stage_digest(S[2]),
    }
    assert ac3["s2_digest_after"] == s2_data and len(ac3["list"]["result"]["stages"]) == 1
    # A slot change in a removed stage is refused; a new element gets slots for live stages only.
    q = copy.deepcopy(p)
    bad = slot_set(ELEMENTS[10], S[2], "current", value_digest("stage-model-late"))
    try:
        q.apply(bad)
        raise AssertionError("slot change in a removed stage accepted")
    except Refused as r:
        ac3["slot_in_removed"] = {"event": bad, "code": r.code, "reason": r.reason}
    q.apply(element(NEW_ELEMENT, "late"))
    ac3["late_element"] = {"event": element(NEW_ELEMENT, "late"), "slots": sorted(q.elements[NEW_ELEMENT]["slots"])}
    ac3["errors"] = [
        refused(p, "remove_active", "remove", {"stage": S[1]}),
        refused(p, "remove_removed", "remove", {"stage": S[2]}),
        refused(p, "remove_unknown", "remove", {"stage": S[999]}),
        refused(p, "remove_element", "remove", {"stage": ELEMENTS[0]}),
        refused(p, "remove_upper", "remove", {"stage": S[1].upper()}),
        refused(p, "remove_extra", "remove", {"stage": S[3], "force": True}),
    ]
    cases["ac3"] = ac3

    # AC-4 on the state after AC-3: S3 (run), S5 (run2), S4 (run); reorder the run.
    steps = [
        ok(p, "add", {"id": S[3], "label": "S3", "run": RUN}),
        ok(p, "add", {"id": S[5], "label": "S5", "run": RUN2, "serial": "B-17"}),
        ok(p, "add", {"id": S[4], "label": "S4", "run": RUN}),
    ]
    before = p.digest()
    ac4 = {"setup": steps, "digest_before": before}
    ac4["errors"] = [
        refused(p, "reorder_missing", "reorder", {"order": [S[4], S[1]], "run": RUN}),
        refused(p, "reorder_duplicate", "reorder", {"order": [S[4], S[1], S[1]], "run": RUN}),
        refused(p, "reorder_duplicate_full", "reorder", {"order": [S[4], S[1], S[3], S[3]], "run": RUN}),
        refused(p, "reorder_removed", "reorder", {"order": [S[4], S[1], S[3], S[2]], "run": RUN}),
        refused(p, "reorder_other_run", "reorder", {"order": [S[4], S[1], S[3], S[5]], "run": RUN}),
        refused(p, "reorder_not_a_stage", "reorder", {"order": [S[4], S[1], ELEMENTS[3]], "run": RUN}),
        refused(p, "reorder_run_is_plan", "reorder", {"order": [S[4], S[1], S[3]], "run": PLAN}),
        refused(p, "reorder_not_array", "reorder", {"order": S[4], "run": RUN}),
        refused(p, "reorder_bad_uuid", "reorder", {"order": ["S4"], "run": RUN}),
        refused(p, "reorder_no_run", "reorder", {"order": []}),
    ]
    assert p.digest() == before
    ac4["ok"] = ok(p, "reorder", {"order": [S[4], S[1], S[3]], "run": RUN})
    ac4["digest_after"] = p.digest()
    ac4["sequence_after"] = list(p.stages)
    ac4["list"] = {"params": {}, "result": p.list({})}
    ac4["list_run2"] = {"params": {"run": RUN2}, "result": p.list({"run": RUN2})}
    ac4["list_all"] = {"params": {"include_removed": True}, "result": p.list({"include_removed": True})}
    assert p.stages == [S[4], S[2], S[1], S[5], S[3]]
    cases["ac4"] = ac4

    # Refused adds, against the state after AC-1 plus AC-3's setup (S1 live, S2 live).
    p = p100()
    p.apply(ac1["event"])
    cases["add_errors"] = [
        refused(p, "add_no_label", "add", {"run": RUN}),
        refused(p, "add_no_run", "add", {"label": "S9"}),
        refused(p, "add_unknown_param", "add", {"color": "red", "label": "S9", "run": RUN}),
        refused(p, "add_label_number", "add", {"label": 9, "run": RUN}),
        refused(p, "add_serial_null", "add", {"label": "S9", "run": RUN, "serial": None}),
        refused(p, "add_label_empty", "add", {"label": "", "run": RUN}),
        refused(p, "add_label_257", "add", {"label": "x" * 257, "run": RUN}),
        refused(p, "add_label_bidi", "add", {"label": "S‮9", "run": RUN}),
        refused(p, "add_label_nfd", "add", {"label": "Stück", "run": RUN}),
        refused(p, "add_serial_control", "add", {"label": "S9", "run": RUN, "serial": "a\u0007"}),
        refused(p, "add_run_is_plan", "add", {"label": "S9", "run": PLAN}),
        refused(p, "add_run_is_stage", "add", {"label": "S9", "run": S[1]}),
        refused(p, "add_run_unknown", "add", {"label": "S9", "run": S[998]}),
        refused(p, "add_run_upper", "add", {"label": "S9", "run": RUN.upper()}),
        refused(p, "add_id_nil", "add", {"id": "00000000-0000-0000-0000-000000000000", "label": "S9", "run": RUN}),
        refused(p, "add_id_collides_stage", "add", {"id": S[1], "label": "S9", "run": RUN}),
        refused(p, "add_id_collides_element", "add", {"id": ELEMENTS[7], "label": "S9", "run": RUN}),
        refused(p, "add_id_collides_run", "add", {"id": RUN, "label": "S9", "run": RUN}),
        refused(p, "list_bad_flag", "list", {"include_removed": "yes"}),
        refused(p, "list_run_is_part", "list", {"run": PART}),
        refused(p, "list_unknown_param", "list", {"stage": S[1]}),
    ]
    # The untrusted label is stored verbatim; a 256-character label is accepted.
    long_ok = ok(p, "add", {"id": S[6], "label": "y" * 256, "run": RUN2})
    cases["more_ok"] = [long_ok]

    # AC-2: S2 .. S1000 added to p100 by a script: 1,000 stages, 1,000 slots per element.
    p = p100()
    adds = []
    for n in range(2, 1001):
        params = {"id": S[n], "label": f"S{n}", "run": RUN}
        p.apply(p.add(params))
        adds.append(params)
    lst = p.list({})
    assert len(lst["stages"]) == 1000 and all(len(e["slots"]) == 1000 for e in p.elements.values())
    assert [s["order"] for s in lst["stages"]] == list(range(1000))
    cases["ac2"] = {
        "adds": adds,
        "list_count": 1000,
        "list_digest": sha(lst),
        "digest_after": p.digest(),
        "s1_digest": p.stage_digest(S[1]),
        "s1000_digest": p.stage_digest(S[1000]),
    }
    return cases


def outputs():
    p = p100()
    setup = {
        "namespace": str(NAMESPACE),
        "root": ROOT,
        "default_stage": DEFAULT,
        "stage_list": STAGE_LIST,
        "part": PART,
        "revision": REV,
        "plan": PLAN,
        "run": RUN,
        "run2": RUN2,
        "s1": S[1],
        "elements": ELEMENTS,
        "principal": PRINCIPAL,
        "type": TYPE,
        "stage_digest_s1": p.stage_digest(S[1]),
        "list_entity": p.list_entity(),
    }
    lines = "".join(canon(e).decode("utf-8") + "\n" for e in p100_events())
    return {
        "p100/setup.json": json.dumps(setup, indent=1, sort_keys=True, ensure_ascii=False) + "\n",
        "p100/events.jsonl": lines,
        "p100/expected_digest.txt": p.digest() + "\n",
        "cases.json": canon(main_cases()).decode("utf-8") + "\n",
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
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale) + "; run make_fixtures.py", file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
