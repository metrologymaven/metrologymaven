#!/usr/bin/env python3
"""Generate the fixtures of CORE.PRJ.CONT.HIER (container entities and hierarchy commands).

Independent reference written with Python's standard library only. Expected IDs, event
bodies and errors come from the rules in spec/core/prj/cont/hier/NODE.md and the pinned
choices below, never from running the Rust code. CC-BY-4.0 project data.

Files written next to this script:

- empty/state.json: the empty project the tests start from (no entities, zero head, no
  settings). A stand-in for a CORE.PRJ.CONT.LIFE project until that leaf lands.
- cases.json:
  - `principal`: who runs every command (`created_by` of every entity);
  - `ac1`: the four create commands of AC-1 with their observations (Unix ms and 74 random
    bits of each UUIDv7) and the one `ContainerCreated` event each must return;
  - `ac1_observations`: the observations of AC-1 in their journal form, in draw order
    (AC-4 replays them);
  - `ac1_after_undo`: the entity IDs left after undoing the last command of AC-1 (AC-4);
  - `ac2`: setup commands (part, r1, r2, plan of r1) then the mixed run, which must fail
    with validation/hierarchy and draw no observation;
  - `errors`: hand-labelled refusals, each run against the state after AC-1 (AC-3 is
    `plan_names_a_part`): the bus code and reason, and the container error code;
  - `more_ok`: further accepted commands against the state after AC-1 (imported IDs, a
    256-character label), each with its expected event.

Pinned choices (stated in the PR):
- entity kinds are `project.part`, `project.revision`, `project.plan`, `project.run`;
- the event type is `ContainerCreated`; its body is the entity: `created_by` {id, kind},
  `id`, `kind`, `label` (string, or null when a plan or run is created without one),
  `parent` (null for a part, the part of a revision, the revision of a plan, the plan of a
  run) and, for a run only, `revision`;
- `label` is required for part and revision and optional for plan and run (AC-1 creates
  the plan and the run without one); a label is 1..=256 Unicode scalar values with no Cc
  control, no bidi control (U+202A..U+202E, U+2066..U+2069, U+200E, U+200F, U+061C) and in
  NFC (the text rules of #290); anything else is `label`;
- every accepted command's result is `{<part|revision|plan|run>: <new ID>}`;
- references and imported IDs are canonical lowercase hyphenated UUID text; anything else
  is validation/schema, as is the nil or max UUID as an imported ID;
- check order: parameter shape, label, references (plan before revision for a run), the
  plan-revision rule, then the ID. Only a command that passes every check draws an ID;
- an imported ID already in the project is `id_collision`, reported on the bus as
  conflict/id_collision (bus error catalogue v2).

UUIDv7 convention (CORE.PRJ.CONT.ID): one 74-bit value R; rand_a = R >> 62, rand_b = the
low 62 bits of R.

Run:  python fixtures/core/prj/cont/hier/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
import unicodedata
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
ZERO_HEAD = "sha256:" + "0" * 64
PRINCIPAL = {"id": "alice", "kind": "human"}
EVENT = "ContainerCreated"
KIND = {"part": "project.part", "revision": "project.revision", "plan": "project.plan", "run": "project.run"}
COMMAND = {k: f"project.create_{k}@1" for k in KIND}


def v7(ts_ms: int, r74: int) -> str:
    assert 0 <= ts_ms < (1 << 48) and 0 <= r74 < (1 << 74)
    n = (ts_ms << 80) | (0x7 << 76) | ((r74 >> 62) << 64) | (0b10 << 62) | (r74 & ((1 << 62) - 1))
    u = uuid.UUID(int=n)
    assert u.variant == uuid.RFC_4122 and u.version == 7
    return str(u)


# RFC 9562 Appendix A.6, hand-labelled, guards the layout above.
assert v7(0x017F22E279B0, (0xCC3 << 62) | 0x18C4DC0C0C07398F) == "017f22e2-79b0-7cc3-98c4-dc0c0c07398f"


def observation(i: int):
    """The i-th fixed observation pair: a millisecond and 74 bits from SHA-256."""
    ms = 1_763_500_000_000 + 1000 * i
    r = int.from_bytes(hashlib.sha256(f"prj-hier-{i}".encode()).digest()[:10], "big") >> 6
    assert r < (1 << 74)
    return ms, r


def obs_json(ms: int, r: int):
    return [
        {"kind": "now_ms", "value": str(ms)},
        {"kind": "random_bits", "value": f"0x{r:x}", "width": "74"},
    ]


def body(kind, ident, label, parent, revision=None):
    b = {"created_by": dict(PRINCIPAL), "id": ident, "kind": KIND[kind], "label": label, "parent": parent}
    if kind == "run":
        b["revision"] = revision
    return b


def event(b):
    return {"body": b, "type": EVENT}


class Model:
    """A tiny reference of the hierarchy rules, used to label the cases."""

    def __init__(self):
        self.entities = {}  # id -> body
        self.next_obs = 0

    def draw(self):
        ms, r = observation(self.next_obs)
        self.next_obs += 1
        return ms, r


BIDI = {0x200E, 0x200F, 0x061C} | set(range(0x202A, 0x202F)) | set(range(0x2066, 0x206A))


def label_ok(text: str) -> bool:
    """The label rule, from the leaf and #290: length, no Cc, no bidi control, NFC."""
    return (
        1 <= len(text) <= 256
        and not any(unicodedata.category(c) == "Cc" or ord(c) in BIDI for c in text)
        and unicodedata.is_normalized("NFC", text)
    )


def ok_step(model, kind, params, label, parent, revision=None, imported=None):
    """An accepted command: draws an observation unless `imported`, returns the case."""
    case = {"command": COMMAND[kind], "params": params}
    if imported is None:
        ms, r = model.draw()
        ident = v7(ms, r)
        case["observation"] = {"now_ms": str(ms), "random_bits": f"0x{r:x}"}
    else:
        ident = imported
        case["observation"] = None
    b = body(kind, ident, label, parent, revision)
    model.entities[ident] = b
    case["event"] = event(b)
    case["id"] = ident
    case["result"] = {kind: ident}
    return case


def main() -> int:
    out = {}

    # ---------------------------------------------------------------- AC-1
    m = Model()
    p = ok_step(m, "part", {"label": "Bracket"}, "Bracket", None)
    r = ok_step(m, "revision", {"part": p["id"], "label": "B"}, "B", p["id"])
    pl = ok_step(m, "plan", {"revision": r["id"]}, None, r["id"])
    run = ok_step(m, "run", {"plan": pl["id"], "revision": r["id"]}, None, pl["id"], r["id"])
    ac1 = [p, r, pl, run]
    obs = []
    for i in range(4):
        obs += obs_json(*observation(i))
    P, R, PL, RUN = (c["id"] for c in ac1)

    # ---------------------------------------------------------------- AC-2
    m2 = Model()
    a = ok_step(m2, "part", {"label": "Housing"}, "Housing", None)
    r1 = ok_step(m2, "revision", {"part": a["id"], "label": "r1"}, "r1", a["id"])
    r2 = ok_step(m2, "revision", {"part": a["id"], "label": "r2"}, "r2", a["id"])
    plan1 = ok_step(m2, "plan", {"revision": r1["id"], "label": "CMM plan"}, "CMM plan", r1["id"])
    ac2 = {
        "setup": [a, r1, r2, plan1],
        "command": COMMAND["run"],
        "params": {"plan": plan1["id"], "revision": r2["id"]},
        "code": "validation",
        "reason": "hierarchy",
        "error": "hierarchy",
    }

    # ---------------------------------------------------------------- errors (state after AC-1)
    unknown = "5f0c2d8e-3b1a-4c7e-9a2f-6d4b8e1c0a93"  # a hand-picked v4 UUID not in the project
    upper = P.upper()

    def err(name, cmd, params, code, reason, error):
        return {"name": name, "command": COMMAND[cmd], "params": params, "code": code, "reason": reason, "error": error}

    v = "validation"
    errors = [
        # AC-3
        err("plan_names_a_part", "plan", {"revision": P}, v, "bad_reference", "bad_reference"),
        err("revision_of_unknown_part", "revision", {"part": unknown, "label": "C"}, v, "bad_reference", "bad_reference"),
        err("revision_names_a_run", "revision", {"part": RUN, "label": "C"}, v, "bad_reference", "bad_reference"),
        err("plan_of_unknown_revision", "plan", {"revision": unknown}, v, "bad_reference", "bad_reference"),
        err("run_plan_is_a_revision", "run", {"plan": R, "revision": R}, v, "bad_reference", "bad_reference"),
        err("run_revision_is_a_part", "run", {"plan": PL, "revision": P}, v, "bad_reference", "bad_reference"),
        err("run_of_unknown_plan", "run", {"plan": unknown, "revision": R}, v, "bad_reference", "bad_reference"),
        # The plan reference is checked before the revision reference.
        err("run_both_bad_plan_first", "run", {"plan": unknown, "revision": P}, v, "bad_reference", "bad_reference"),
        err("empty_label", "part", {"label": ""}, v, "label", "label"),
        err("label_257_chars", "part", {"label": "x" * 257}, v, "label", "label"),
        err("label_with_bell", "part", {"label": "Bra\u0007cket"}, v, "label", "label"),
        err("label_with_newline", "part", {"label": "line 1\nline 2"}, v, "label", "label"),
        err("label_with_tab", "revision", {"part": P, "label": "B\t2"}, v, "label", "label"),
        err("label_with_del", "part", {"label": "x\u007f"}, v, "label", "label"),
        err("label_with_c1_control", "part", {"label": "x\u0085"}, v, "label", "label"),
        err("label_with_rlo", "part", {"label": "abc\u202edef"}, v, "label", "label"),
        err("label_with_lre", "part", {"label": "\u202aabc"}, v, "label", "label"),
        err("label_with_rli", "plan", {"revision": R, "label": "\u2067x"}, v, "label", "label"),
        err("label_with_pdi", "run", {"plan": PL, "revision": R, "label": "x\u2069"}, v, "label", "label"),
        err("label_with_lrm", "part", {"label": "a\u200eb"}, v, "label", "label"),
        err("label_with_rlm", "part", {"label": "a\u200fb"}, v, "label", "label"),
        err("label_with_alm", "part", {"label": "a\u061cb"}, v, "label", "label"),
        err("label_not_nfc", "part", {"label": "Bracke\u0301t"}, v, "label", "label"),
        err("label_not_nfc_hangul", "part", {"label": "\u1100\u1161"}, v, "label", "label"),
        err("plan_empty_label", "plan", {"revision": R, "label": ""}, v, "label", "label"),
        # The label is checked before the references.
        err("label_before_reference", "revision", {"part": unknown, "label": ""}, v, "label", "label"),
        err("part_without_label", "part", {}, v, "schema", "schema"),
        err("revision_without_part", "revision", {"label": "C"}, v, "schema", "schema"),
        err("run_without_revision", "run", {"plan": PL}, v, "schema", "schema"),
        err("unknown_parameter", "part", {"label": "A", "colour": "red"}, v, "schema", "schema"),
        err("label_not_a_string", "part", {"label": 5}, v, "schema", "schema"),
        err("plan_label_null", "plan", {"revision": R, "label": None}, v, "schema", "schema"),
        err("uppercase_reference", "revision", {"part": upper, "label": "C"}, v, "schema", "schema"),
        err("reference_not_a_uuid", "plan", {"revision": "rev-1"}, v, "schema", "schema"),
        err("imported_nil_id", "part", {"label": "N", "id": "00000000-0000-0000-0000-000000000000"}, v, "schema", "schema"),
        err("imported_max_id", "part", {"label": "N", "id": "ffffffff-ffff-ffff-ffff-ffffffffffff"}, v, "schema", "schema"),
        err("imported_uppercase_id", "part", {"label": "N", "id": unknown.upper()}, v, "schema", "schema"),
        err("imported_id_collides", "part", {"label": "Dup", "id": P}, "conflict", "id_collision", "id_collision"),
        err("imported_id_collides_with_run", "plan", {"revision": R, "id": RUN}, "conflict", "id_collision", "id_collision"),
    ]

    # ---------------------------------------------------------------- more accepted commands
    m3 = Model()
    m3.entities = {c["id"]: c["event"]["body"] for c in ac1}
    m3.next_obs = 4
    imported = "8d3e5f60-1c2b-4a9d-8e7f-0123456789ab"  # hand-picked v4 UUID
    more_ok = []
    for name, kind, params, label, parent, rev, imp in [
        ("imported_part_id", "part", {"label": "Imported", "id": imported}, "Imported", None, None, imported),
        ("label_256_chars", "part", {"label": "é" * 256}, "é" * 256, None, None, None),
        ("label_nfc_composed", "part", {"label": "Br\u00e9cket \uac00"}, "Br\u00e9cket \uac00", None, None, None),
        ("label_markup_and_zwj", "part", {"label": "<b>{{x}}</b> \U0001f469\u200d\U0001f527"},
         "<b>{{x}}</b> \U0001f469\u200d\U0001f527", None, None, None),
        ("second_revision", "revision", {"part": P, "label": "B"}, "B", P, None, None),
        ("labelled_plan", "plan", {"revision": R, "label": "Plan 2"}, "Plan 2", R, None, None),
        ("second_run_same_plan", "run", {"plan": PL, "revision": R, "label": "Run 2"}, "Run 2", PL, R, None),
        ("imported_run_id", "run", {"plan": PL, "revision": R, "id": imported}, None, PL, R, imported),
    ]:
        # Every case runs against the state after AC-1 alone; observations are numbered
        # on from AC-1 so each accepted v7 case has its own.
        m3.entities = {c["id"]: c["event"]["body"] for c in ac1}
        case = ok_step(m3, kind, params, label, parent, rev, imp)
        case["name"] = name
        more_ok.append(case)

    # The hand labels agree with the label rule.
    for e in errors:
        if "label" in e["params"] and isinstance(e["params"]["label"], str):
            assert label_ok(e["params"]["label"]) == (e["error"] != "label"), e["name"]
    for c in ac1 + ac2["setup"] + more_ok:
        if "label" in c["params"]:
            assert label_ok(c["params"]["label"]), c["params"]

    out = {
        "principal": PRINCIPAL,
        "event_type": EVENT,
        "ac1": ac1,
        "ac1_observations": obs,
        "ac1_after_undo": sorted([P, R, PL]),
        "ac2": ac2,
        "errors": errors,
        "more_ok": more_ok,
    }
    files = {
        HERE / "cases.json": out,
        HERE / "empty" / "state.json": {"entities": [], "head": ZERO_HEAD, "settings": {}},
    }
    check = "--check" in sys.argv[1:]
    stale = 0
    for path, value in files.items():
        text = json.dumps(value, ensure_ascii=False, indent=1, sort_keys=True) + "\n"
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"stale: {path.relative_to(HERE)}")
                stale += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    if check:
        print("prj/cont/hier fixtures:", "stale" if stale else "ok")
    return 1 if stale else 0


if __name__ == "__main__":
    sys.exit(main())
