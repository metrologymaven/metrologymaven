#!/usr/bin/env python3
"""Write the RPT.MODEL.BIND.RESOLVE fixtures `bracket_3stage` and `two_projects`.
Synthetic project data, CC-BY-4.0, authored in-project.

Independent reference: every journal entry, digest and expected byte is built here with the
Python standard library from the rules pinned in spec/rpt/model/bind/resolve/NODE.md and
spec/rpt/model/doc/NODE.md, never by running the Rust code. Canonical JSON is RFC 8785
restricted to what these documents hold (objects, arrays, strings, booleans, null; ASCII
keys, so code-point key order equals UTF-16 order).

Run:   python fixtures/rpt/model/bracket_3stage/make_fixtures.py          (write)
       python fixtures/rpt/model/bracket_3stage/make_fixtures.py --check  (compare, exit 1 on drift)

`bracket_3stage`: one part measured in 3 stages (runs), 50 characteristics, one
ResultRecord per (stage, characteristic) in the GDT.CHAR.RECORD wire form
`fid.gdt-result/1`, each with its own stored uncertainty statement (opaque bytes here: the
resolver checks statement digests only), plus one saved view and one statistics series.
Journal order: view, series, the 50 characteristics, then per stage and characteristic the
statement followed by the record. r1 is (stage 1, characteristic 1); r2 is (stage 3,
characteristic 50), the last entry; H0 is the head just before r2 (r2's statement).

`two_projects`: projects A and B in one store, each with a characteristic, a statement and
a record. The cases bind B's record inside A.

Heads are synthetic: head(i) = sha256(head(i-1) text + entry bytes), head(-1) = "".

Expected outputs (cases.json) follow the pinned rules:
- `value_binding@1` = {field: "/value", record, record_digest: sha256(record bytes),
  schema, statement_digest: the record's /uncertainty/statement};
- the `table` test kind writes one row {"value": <binding>} per value binding, in
  reference order, and the command adds `binding` = {refs: [{id, kind}], schema: "binding@1"};
- the AC-1 report comes from the deterministic ID source of the tests (draw k gives
  unix_ms = BASE_MS + k and random bits k + 1, as in fixtures/rpt/model/doc): report = draw 0,
  page = draw 1, block = draw 2.
"""

import hashlib
import json
import struct
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
TWO = HERE.parent / "two_projects"
BASE_MS = 1_763_510_400_000
PROJECT = "019a9a9a-0000-7000-8000-0000000b3a5e"
PROJECT_A = "019a9a9a-0000-7000-8000-00000000000a"
PROJECT_B = "019a9a9a-0000-7000-8000-00000000000b"
TITLE = "Bracket 3-stage FAI"
STAGES = 3
CHARS = 50


def jcs(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def bits(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def v7(k: int) -> str:
    ts, r = BASE_MS + k, k + 1
    n = (ts << 80) | (0x7 << 76) | ((r >> 62) << 64) | (0b10 << 62) | (r & ((1 << 62) - 1))
    u = uuid.UUID(int=n)
    assert u.variant == uuid.RFC_4122 and u.version == 7
    return str(u)


def eid(group: int, a: int, b: int) -> str:
    """A fixed UUIDv7-shaped entity ID: group, then two counters."""
    u = uuid.UUID(f"019a9a9b-{group:04x}-7{a:03x}-8000-{b:012x}")
    assert u.version == 7
    return str(u)


def characteristic(cid: str, c: int) -> dict:
    return {
        "id": cid,
        "name": f"C{c:02d}",
        "nominal": {"unit": "si:m", "value": bits(0.01 + c * 0.001)},
        "schema": "fixture.characteristic/1",
    }


def statement(cid: str, stage: int, c: int) -> dict:
    return {
        "characteristic": cid,
        "class": "UEC-1",
        "expanded": {"U": bits(0.000002 + c * 0.0000001), "k": bits(2.0)},
        "method": "DECLARED",
        "schema": "fixture.unc-statement/1",
        "stage": str(stage),
        "unit": "si:m",
    }


def record(run: str, cid: str, c: int, value: float, stmt_digest: str) -> dict:
    z = "sha256:" + "0" * 63 + "1"
    return {
        "algorithm": "gdt.size.two_point@1.0.0",
        "characteristic": cid,
        "decision": {"basis": None, "outcome": "NOT_DECIDABLE", "reason": "no_rule_bound", "rule": None,
                     "source": "gdt_not_decidable"},
        "dmt_digest": z,
        "dmt_row": "dmt:asme/size/none@1",
        "evaluation_basis": "POINTS",
        "input_digests": [],
        "journal_entry": z,
        "kernel_version": "0.1.0",
        "measurand": f"bracket C{c:02d} two-point size",
        "operator_chain_digest": z,
        "run": run,
        "schema": "fid.gdt-result/1",
        "spec": {"defaults_profile": "dp:fixture@1", "dialect": "ASME", "inherited_from": "explicit",
                 "standards": []},
        "uncertainty": {"claimed": True, "class": "UEC-1", "declared_input": False,
                        "expanded": {"U": bits(0.000004), "k": bits(2.0)}, "interval": None,
                        "method": "DECLARED", "statement": stmt_digest, "u_std": None,
                        "uec0_reason": None, "uncharacterized_input": False, "unit": "si:m"},
        "value": {"unit": "si:m", "value": bits(value)},
    }


class Journal:
    def __init__(self):
        self.entries = []
        self.head = ""

    def add(self, project, kind, id_, doc) -> str:
        text = jcs(doc).decode("utf-8")
        self.head = sha(self.head.encode("utf-8") + text.encode("utf-8"))
        self.entries.append({"head": self.head, "id": id_, "kind": kind, "project": project, "text": text})
        return self.head


def binding_of(rec_id, rec_text, stmt_digest) -> dict:
    return {"field": "/value", "record": rec_id, "record_digest": sha(rec_text.encode("utf-8")),
            "schema": "value_binding@1", "statement_digest": stmt_digest}


def bracket():
    j = Journal()
    view, series = eid(1, 0, 1), eid(1, 0, 2)
    j.add(PROJECT, "named_view", view, {"name": "top view", "schema": "fixture.view/1"})
    j.add(PROJECT, "series", series, {"name": "C01 trend", "schema": "fixture.series/1"})
    chars = []
    for c in range(1, CHARS + 1):
        cid = eid(2, 0, c)
        chars.append(cid)
        j.add(PROJECT, "characteristic", cid, characteristic(cid, c))
    records = {}
    for s in range(1, STAGES + 1):
        run = eid(3, s, 0)
        for c in range(1, CHARS + 1):
            cid = chars[c - 1]
            stmt = jcs(statement(cid, s, c))
            sd = sha(stmt)
            j.add(PROJECT, "statement", None, json.loads(stmt))
            value = 0.01 + c * 0.001 + s * 0.000001
            rid = eid(4, s, c)
            doc = record(run, cid, c, value, sd)
            j.add(PROJECT, "result", rid, doc)
            records[(s, c)] = (rid, jcs(doc).decode("utf-8"), sd, bits(value))
    r1, r2 = records[(1, 1)], records[(STAGES, CHARS)]
    assert j.entries[-1]["id"] == r2[0]
    h0 = j.entries[-2]["head"]
    # AC-1: bind r1, r2 to a table block holding {"rows": []}.
    report, page, block = v7(0), v7(1), v7(2)
    refs = [{"id": r1[0], "kind": "result"}, {"id": r2[0], "kind": "result"}]
    payload = {
        "binding": {"refs": refs, "schema": "binding@1"},
        "rows": [{"value": binding_of(r[0], r[1], r[2])} for r in (r1, r2)],
    }
    stored = {
        "id": report,
        "pages": [{"blocks": [{"id": block, "kind": "table", "kind_version": "1",
                               "payload": {"untrusted": True, "value": payload}}], "id": page}],
        "retired_ids": [],
        "schema_version": "1",
        "template_ref": None,
        "title": {"text": TITLE, "untrusted": True},
    }
    digest = sha(jcs(stored))
    event = {"block": block, "payload": {"untrusted": True, "value": payload}, "report": report,
             "report_digest": digest, "type": "report.bind_block@1"}
    result = {"block_id": block, "refs": refs, "report_digest": digest}
    cases = {
        "project": PROJECT,
        "title": TITLE,
        "r1": r1[0],
        "r2": r2[0],
        "characteristic_1": chars[0],
        "view": view,
        "series": series,
        "h0": h0,
        "current_head": j.head,
        "ac1": {
            "report": report, "page": page, "block": block,
            "initial_payload_text": jcs({"rows": []}).decode("utf-8"),
            "payload_text": jcs(payload).decode("utf-8"),
            "event_text": jcs(event).decode("utf-8"),
            "result_text": jcs(result).decode("utf-8"),
        },
        # The resolved values of AC-3 at the current head, in value-field order.
        "ac3_values": [
            {"pointer": f"/rows/{i}/value", "record": r[0], "unit": "si:m", "bits": r[3], "statement_digest": r[2]}
            for i, r in enumerate((r1, r2))
        ],
        # AC-2: a reference carrying a value, hand-labelled.
        "ac2": {"ref_text": '{"id":"' + r1[0] + '","kind":"result","value":12.003}',
                "code": "report.binding_has_value", "field": "/refs/0/value"},
        # AC-7: another valid sha256 digest in place of r1's record or statement digest.
        "ac7_other_digest": "sha256:" + "5" * 64,
        # AC-8: pointers into r1 that are not its value field (hand-labelled). GDT records
        # carry no nominal or limits (those belong to the characteristic): the absent
        # pointers stand for them; the others are real non-value fields.
        "ac8_fields": ["/nominal", "/limits/upper", "/uncertainty/expanded/U", "/value/value", "/value/unit",
                       "/decision", ""],
    }
    return {"project": PROJECT, "entries": j.entries}, cases


def two_projects():
    j = Journal()
    out = {}
    for name, project, g in (("a", PROJECT_A, 5), ("b", PROJECT_B, 6)):
        cid = eid(g, 0, 1)
        j.add(project, "characteristic", cid, characteristic(cid, 1))
        stmt = jcs(statement(cid, 1, 1))
        sd = sha(stmt)
        j.add(project, "statement", None, json.loads(stmt))
        rid = eid(g, 1, 1)
        doc = record(eid(g, 2, 0), cid, 1, 0.011, sd)
        j.add(project, "result", rid, doc)
        out[name] = {"characteristic": cid, "record": rid,
                     "binding": binding_of(rid, jcs(doc).decode("utf-8"), sd)}
    cases = {
        "project_a": PROJECT_A,
        "project_b": PROJECT_B,
        "a": out["a"],
        "b": out["b"],
        "current_head": j.head,
        # Hand-labelled: binding B's record (or characteristic) in a block of A.
        "expected": {"code": "report.cross_project_binding", "project": PROJECT_B,
                     "error_keys": ["block", "code", "project"]},
    }
    return {"entries": j.entries}, cases


def pretty(v) -> bytes:
    return (json.dumps(v, indent=1, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def outputs() -> dict:
    bj, bc = bracket()
    tj, tc = two_projects()
    return {
        HERE / "journal.json": pretty(bj),
        HERE / "cases.json": pretty(bc),
        TWO / "journal.json": pretty(tj),
        TWO / "cases.json": pretty(tc),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad = 0
    for path, data in outputs().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"drift: {path}")
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
