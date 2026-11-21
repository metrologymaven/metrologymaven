#!/usr/bin/env python3
"""Generate the fixtures of CORE.PRJ.STAGE.SWITCH (active stage, per-stage value cache).

Independent reference written with Python's standard library only. Expected events, results,
errors, values and digests come from the rules in spec/core/prj/stage/switch/NODE.md, the
stage model of CORE.PRJ.STAGE.MODEL (#347) and the pinned choices below, never from running
the Rust code. CC-BY-4.0 project data.

Files written next to this script:

- p3s/setup.json: namespace, reserved IDs, container, stage and element IDs, principal, type.
- p3s/events.jsonl: project p3s: a Part, Revision, Plan and Run, stages S1 (adopts the empty
  implicit default stage), S2 and S3, 50 elements, and every slot of every element `current`
  with a distinct value (150 values).
- p3s/values.json: {slot digest: value text}; the value of a slot is the UTF-8 bytes of the
  canonical JSON {"element": <id>, "stage": <label>, "value": <integer>}; its slot digest is
  the SHA-256 of those bytes (the content address).
- p3s/expected_digest.txt: the canonical state digest of p3s.
- cases.json: AC-1 (switch to S2, e1's value), AC-3 refusals, AC-5 journal digest.

Pinned choices (stated in the PR):
- Event ActiveStageSet {stage}; applying it sets the state's `active_stage` only: elements,
  slots, stage records and every per-stage digest are unchanged.
- project.set_active_stage@1 params {stage}; result {stage}. Check order: parameter shape
  (validation/schema: an object with exactly the key `stage`, a canonical lowercase UUID),
  then the stage: anything but a live stage of the project (unknown, removed, an element or
  container ID) is validation/bad_stage. The active stage itself is accepted.
- State and per-stage digest: the forms of CORE.PRJ.STAGE.MODEL (#347): the per-stage digest
  is SHA-256 of canonical {"elements": {id: {"digest", "state"}}, "stage": id} over the
  elements whose slot is not empty.
- Canonical JSON: sorted keys, compact, ensure_ascii=False (ASCII keys, strings, booleans,
  null, small integers), which is RFC 8785 for these values.

Run:  python fixtures/core/prj/stage/switch/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
NAMESPACE = uuid.UUID("7a1e9c44-3b2d-4f6a-9e8d-1c2b3a4d5e6f")
PRINCIPAL = {"id": "alice", "kind": "human"}
TYPE = {"type_id": "test.point", "display_kind": "point"}
EMPTY = {"digest": None, "state": "not_computed"}


def canon(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha_bytes(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def sha(v) -> str:
    return sha_bytes(canon(v))


def reserved(kind: str) -> str:
    return str(uuid.uuid5(NAMESPACE, canon({"kind": kind}).decode("utf-8")))


def v7_like(tag: str) -> str:
    n = int.from_bytes(hashlib.sha256(tag.encode()).digest()[:16], "big")
    n = (n & ~(0xF << 76)) | (0x7 << 76)
    n = (n & ~(0b11 << 62)) | (0b10 << 62)
    u = uuid.UUID(int=n)
    assert u.version == 7 and u.variant == uuid.RFC_4122
    return str(u)


ROOT = reserved("project.tree_root")
DEFAULT = reserved("project.default_stage")
STAGE_LIST = reserved("project.stage_list")
PART, REV, PLAN, RUN = (v7_like(f"stage-switch-{t}") for t in ("part", "revision", "plan", "run"))
S = {n: v7_like(f"stage-switch-s{n}") for n in (1, 2, 3, 4)}
ELEMENTS = [v7_like(f"stage-switch-element-{i}") for i in range(50)]


def ev(ty, body):
    return {"body": body, "type": ty}


def container(kind, ident, label, parent, revision=None):
    b = {"created_by": dict(PRINCIPAL), "id": ident, "kind": "project." + kind, "label": label, "parent": parent}
    if kind == "run":
        b["revision"] = revision
    return ev("ContainerCreated", b)


def value_bytes(element, label, i):
    return canon({"element": element, "stage": label, "value": i})


class Project:
    def __init__(self):
        self.containers = {}
        self.elements = {}
        self.stages = [DEFAULT]
        self.records = {}
        self.active = DEFAULT

    def live(self):
        if not self.records:
            return [DEFAULT]
        return [s for s in self.stages if not self.records[s]["removed"]]

    def apply(self, e):
        t, b = e["type"], e["body"]
        if t == "ContainerCreated":
            self.containers[b["id"]] = b["kind"]
        elif t == "ElementCreated":
            body = dict(b)
            body["slots"] = {s: dict(EMPTY) for s in self.live()}
            self.elements[b["id"]] = body
        elif t == "ElementSlotSet":
            assert b["stage"] in self.live()
            self.elements[b["element"]]["slots"][b["stage"]] = {"digest": b["digest"], "state": b["state"]}
        elif t == "StageAdded":
            ident = b["id"]
            if b["adopts"] is not None:
                assert not self.records and all(e["slots"][DEFAULT] == EMPTY for e in self.elements.values())
                for el in self.elements.values():
                    el["slots"][ident] = el["slots"].pop(DEFAULT)
                self.stages = [ident]
                self.active = ident
            else:
                assert self.records
                for el in self.elements.values():
                    el["slots"][ident] = dict(EMPTY)
                self.stages.append(ident)
            self.records[ident] = {k: v for k, v in b.items() if k != "adopts"} | {"removed": False}
        elif t == "StageRemoved":
            assert b["stage"] in self.live() and b["stage"] != self.active
            self.records[b["stage"]]["removed"] = True
        elif t == "ActiveStageSet":
            if b["stage"] not in self.live():
                raise Refused("validation", "bad_stage")
            self.active = b["stage"]
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
        els = {i: e["slots"][s] for i, e in self.elements.items() if s in e["slots"] and e["slots"][s] != EMPTY}
        return sha({"elements": els, "stage": s})

    def set_active(self, params):
        if not isinstance(params, dict) or set(params) != {"stage"} or not isinstance(params["stage"], str):
            raise Refused("validation", "schema")
        try:
            u = uuid.UUID(params["stage"])
        except ValueError:
            raise Refused("validation", "schema")
        if str(u) != params["stage"]:
            raise Refused("validation", "schema")
        if params["stage"] not in self.live():
            raise Refused("validation", "bad_stage")
        return ev("ActiveStageSet", {"stage": params["stage"]})


class Refused(Exception):
    def __init__(self, code, reason):
        super().__init__(f"{code}/{reason}")
        self.code, self.reason = code, reason


def stage_added(ident, label, adopts):
    return ev("StageAdded", {"adopts": adopts, "id": ident, "label": label, "metadata": {}, "run": RUN, "serial": None})


def p3s_events():
    evs = [
        container("part", PART, "Bracket", None),
        container("revision", REV, "A", PART),
        container("plan", PLAN, None, REV),
        container("run", RUN, None, PLAN, REV),
        stage_added(S[1], "S1", DEFAULT),
        stage_added(S[2], "S2", None),
        stage_added(S[3], "S3", None),
    ]
    for i, e in enumerate(ELEMENTS):
        evs.append(ev("ElementCreated", {"id": e, "meta": {}, "name": f"E{i + 1}", "parent": ROOT,
                                         "type": TYPE["type_id"], "visible": True}))
    values = {}
    for n in (1, 2, 3):
        for i, e in enumerate(ELEMENTS):
            b = value_bytes(e, f"S{n}", 1000 * n + i)
            d = sha_bytes(b)
            assert d not in values
            values[d] = b.decode("utf-8")
            evs.append(ev("ElementSlotSet", {"digest": d, "element": e, "stage": S[n], "state": "current"}))
    return evs, values


def main_cases(p, values):
    cases = {}
    before = {S[n]: p.stage_digest(S[n]) for n in (1, 2, 3)}
    q = copy.deepcopy(p)
    params = {"stage": S[2]}
    e = q.set_active(params)
    q.apply(e)
    e1 = ELEMENTS[0]
    d1 = q.elements[e1]["slots"][S[2]]["digest"]
    cases["ac1"] = {
        "command": "project.set_active_stage@1",
        "params": params,
        "event": e,
        "result": {"stage": S[2]},
        "digest_after": q.digest(),
        "stage_digests": {S[n]: q.stage_digest(S[n]) for n in (1, 2, 3)},
        "list_entity": {"active_stage": S[2], "adoptable": None, "stages": [S[1], S[2], S[3]]},
        "e1": e1,
        "e1_digest": d1,
        "e1_value": values[d1],
        "e1_value_s1": values[p.elements[e1]["slots"][S[1]]["digest"]],
    }
    assert cases["ac1"]["stage_digests"] == before
    # Back to S1: the state digest returns to p3s's (nothing else moved).
    r = copy.deepcopy(q)
    r.apply(r.set_active({"stage": S[1]}))
    assert r.digest() == p.digest()
    cases["ac1"]["back_to_s1_digest"] = r.digest()

    # AC-3: S3 removed (S1 active), then refusals; nothing changes.
    q = copy.deepcopy(p)
    rm = ev("StageRemoved", {"stage": S[3]})
    q.apply(rm)
    errors = []

    def refuse(name, params):
        before = q.digest()
        try:
            q.set_active(params)
        except Refused as x:
            assert q.digest() == before
            errors.append({"name": name, "params": params, "code": x.code, "reason": x.reason})
            return
        raise AssertionError(name)

    refuse("removed_stage", {"stage": S[3]})
    refuse("unknown_stage", {"stage": S[4]})
    refuse("element_id", {"stage": ELEMENTS[3]})
    refuse("run_id", {"stage": RUN})
    refuse("default_after_adoption", {"stage": DEFAULT})
    refuse("upper_case", {"stage": S[2].upper()})
    refuse("not_a_string", {"stage": 7})
    refuse("no_stage", {})
    refuse("extra_key", {"stage": S[2], "force": True})
    cases["ac3"] = {"setup": [rm], "digest_before": q.digest(), "errors": errors}
    # The active stage itself is accepted (a no-op switch is still one W2 entry).
    same = q.set_active({"stage": S[1]})
    q.apply(same)
    cases["ac3"]["same_stage"] = {"params": {"stage": S[1]}, "event": same, "digest_after": q.digest()}

    # AC-5: the journal of AC-1 applied from the empty project.
    cases["ac5"] = {"digest": cases["ac1"]["digest_after"]}
    return cases


def outputs():
    evs, values = p3s_events()
    p = Project()
    for e in evs:
        p.apply(e)
    assert p.live() == [S[1], S[2], S[3]] and p.active == S[1] and len(p.elements) == 50
    setup = {
        "namespace": str(NAMESPACE),
        "root": ROOT,
        "default_stage": DEFAULT,
        "stage_list": STAGE_LIST,
        "run": RUN,
        "stages": [S[1], S[2], S[3]],
        "unknown_stage": S[4],
        "elements": ELEMENTS,
        "type": TYPE,
    }
    return {
        "p3s/setup.json": json.dumps(setup, indent=1, sort_keys=True) + "\n",
        "p3s/events.jsonl": "".join(canon(e).decode("utf-8") + "\n" for e in evs),
        "p3s/values.json": json.dumps(values, indent=0, sort_keys=True, ensure_ascii=False) + "\n",
        "p3s/expected_digest.txt": p.digest() + "\n",
        "cases.json": canon(main_cases(p, values)).decode("utf-8") + "\n",
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
