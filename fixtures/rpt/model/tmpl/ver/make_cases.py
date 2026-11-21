#!/usr/bin/env python3
"""Write the RPT.MODEL.TMPL.VER fixtures (AC-1..AC-6). CC-BY-4.0, synthetic.

Independent reference: every template document, report document, journal event and digest
here is built with the Python standard library from the rules pinned in
spec/rpt/model/tmpl/ver/NODE.md, spec/rpt/model/tmpl/format/NODE.md and
spec/rpt/model/doc/NODE.md (Design), never by running the Rust code. Canonical JSON is
RFC 8785 restricted to what these documents hold (objects, arrays, strings, booleans and
null; ASCII keys, so code-point key order equals UTF-16 order).

Pinned rules this script encodes (stated in crates/fid-report/src/tmpl/ver/mod.rs):
- a template ID is the document's `template_id` (UUIDv7); a version reference is
  {"builtin", "digest", "id", "version"} with the version a decimal string; built-in
  versions (`builtin:<id>@<v>`, numbered by the shipping crate) and project versions
  (`<id>@<v>`) are separate number spaces;
- save from a report: same page and block IDs, kinds, kind versions, payloads and title;
  `locked` false, `clearance` null; a block whose ID, kind and kind version match a block of
  the template the report was applied from keeps that block's slot and repeat_header_rows
  (and its continuation_block when the target is on the same page); others get null, "0",
  null;
- project version n+1 where n is the highest project version of the ID (built-ins never
  count); identical bytes return the latest project version and journal nothing;
- apply: a report with the drawn ID, the template's pages and blocks (same IDs), no retired
  IDs, schema_version "1", template_ref {digest, id, version} and the given title;
- events: save {type, source, template}, source {kind: "report", report,
  report_digest} or {kind: "file", handle: untrusted text}; apply {type, report,
  report_digest, template, title}; report events as in fixtures/rpt/model/doc.

The report R is the AC-1 report of fixtures/rpt/model/bracket_3stage (same project and
title, same deterministic ID source: draw k gives unix_ms BASE_MS + k and random bits k + 1;
report = draw 0, page = draw 1, table block = draw 2 with payload {"rows": []}).

Run:   python fixtures/rpt/model/tmpl/ver/make_cases.py          (write)
       python fixtures/rpt/model/tmpl/ver/make_cases.py --check  (compare, exit 1 on drift)
"""

import hashlib
import json
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
BASE_MS = 1_763_510_400_000
PROJECT = "019a9a9a-0000-7000-8000-0000000b3a5e"
TITLE = "Bracket 3-stage FAI"
MEDIA_TYPE = "application/vnd.fid.report-template+json"

STARTER = "019a9ec0-5000-7000-8000-000000000501"
ACME = "019a9ec0-7000-7000-8000-00000000ac3e"
DERIVED = "019a9ec0-7100-7000-8000-0000000000de"
FILE_ID = "019a9ec0-7200-7000-8000-0000000000f1"
FAI = "019a9ec0-f003-7000-8000-000000000f03"
FAI_PAGE = "019a9ec0-f003-7000-8000-000000000f10"
FAI_TABLE = "019a9ec0-f003-7000-8000-000000000f11"
STARTER_PAGE = "019a9ec0-5000-7000-8000-000000000510"
STARTER_TEXT = "019a9ec0-5000-7000-8000-000000000511"
FILE_PAGE = "019a9ec0-7200-7000-8000-0000000000f2"
FILE_TEXT = "019a9ec0-7200-7000-8000-0000000000f3"
HANDLE = "root://imports/file_ok.json"
EDIT_TEXT = "Edit note"


def jcs(v) -> bytes:
    def no_numbers(x):
        assert not isinstance(x, (int, float)) or isinstance(x, bool), x
        if isinstance(x, dict):
            assert all(k.isascii() for k in x)
            for c in x.values():
                no_numbers(c)
        elif isinstance(x, list):
            for c in x:
                no_numbers(c)

    no_numbers(v)
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def v7(k: int) -> str:
    ts, r = BASE_MS + k, k + 1
    n = (ts << 80) | (0x7 << 76) | ((r >> 62) << 64) | (0b10 << 62) | (r & ((1 << 62) - 1))
    u = uuid.UUID(int=n)
    assert u.variant == uuid.RFC_4122 and u.version == 7
    return str(u)


def untrusted(s: str) -> dict:
    return {"text": s, "untrusted": True}


def wrap(v) -> dict:
    return {"untrusted": True, "value": v}


# ------------------------------------------------------------------ documents


def tblock(bid, kind, payload, slot=None, rows="0", cont=None):
    return {"continuation_block": cont, "id": bid, "kind": kind, "kind_version": "1", "payload": wrap(payload),
            "repeat_header_rows": rows, "slot": slot}


def template(tid, title, pages, locked=False, clearance=None):
    return {"clearance": clearance, "locked": locked, "media_type": MEDIA_TYPE,
            "pages": [{"blocks": bl, "id": pid} for pid, bl in pages], "schema_version": "1",
            "template_id": tid, "title": untrusted(title)}


def rblock(bid, kind, payload):
    return {"id": bid, "kind": kind, "kind_version": "1", "payload": wrap(payload)}


def report(rid, title, pages, tref=None):
    return {"id": rid, "pages": [{"blocks": bl, "id": pid} for pid, bl in pages], "retired_ids": [],
            "schema_version": "1", "template_ref": tref, "title": untrusted(title)}


def tref(tid, version, doc, builtin=False):
    return {"builtin": builtin, "digest": sha(jcs(doc)), "id": tid, "version": str(version)}


ROW_SLOT = {"kind": "characteristics",
            "spec": wrap({"repeat": {"kind": "row"}, "rule": {"characteristics": "all"}, "sort": None})}

FAI_DOC = template(FAI, "FAI form 3", [(FAI_PAGE, [tblock(FAI_TABLE, "table", {"rows": []}, ROW_SLOT, "1")])],
                   locked=True, clearance={"state": "pending"})
STARTER_DOC = template(STARTER, "Starter", [(STARTER_PAGE, [tblock(STARTER_TEXT, "text", {"text": "Starter"})])])
FILE_DOC = template(FILE_ID, "From file", [(FILE_PAGE, [tblock(FILE_TEXT, "text", {"text": "Imported"})])])
# Unknown top-level key: the format check refuses it (report.template_invalid).
FILE_INVALID = dict(FILE_DOC, extra=True)
# D-045: a label switch in a payload, and the provisional label's text as template text.
LABEL_SWITCH = template(FILE_ID, "Label switch",
                        [(FILE_PAGE, [tblock(FILE_TEXT, "table", {"note": {"labels": "none"}, "rows": []})])])
LABEL_TEXT = template(FILE_ID, "Label text",
                      [(FILE_PAGE, [tblock(FILE_TEXT, "text", {"text": "interpretation: provisional"})])])


def scenario():
    events = []
    rid, page, table = v7(0), v7(1), v7(2)
    r_pages = [(page, [])]

    def r_digest():
        return sha(jcs(report(rid, TITLE, r_pages)))

    events.append({"page": page, "project": PROJECT, "report": rid, "report_digest": r_digest(),
                   "title": untrusted(TITLE), "type": "report.create_report@1"})
    r_pages[0][1].append(rblock(table, "table", {"rows": []}))
    events.append({"after": None, "block": table, "kind": "table", "kind_version": "1", "page": page,
                   "payload": wrap({"rows": []}), "report": rid, "report_digest": r_digest(),
                   "type": "report.add_block@1"})

    def from_report(tid, title, pages, source=None):
        src = {}
        if source is not None:
            for p in source["pages"]:
                for b in p["blocks"]:
                    src[b["id"]] = b
        out = []
        for pid, bl in pages:
            ids = {b["id"] for b in bl}
            blocks = []
            for b in bl:
                s = src.get(b["id"])
                if s and s["kind"] == b["kind"] and s["kind_version"] == b["kind_version"]:
                    cont = s["continuation_block"] if s["continuation_block"] in ids else None
                    blocks.append(tblock(b["id"], b["kind"], b["payload"]["value"], s["slot"],
                                         s["repeat_header_rows"], cont))
                else:
                    blocks.append(tblock(b["id"], b["kind"], b["payload"]["value"]))
            out.append((pid, blocks))
        return template(tid, title, out)

    v1 = from_report(ACME, TITLE, r_pages)
    v1_ref = tref(ACME, 1, v1)
    events.append({"source": {"kind": "report", "report": rid, "report_digest": r_digest()},
                   "template": v1_ref, "type": "report.save_template@1"})
    text = v7(3)
    r_pages[0][1].append(rblock(text, "text", {"text": EDIT_TEXT}))
    events.append({"after": None, "block": text, "kind": "text", "kind_version": "1", "page": page,
                   "payload": wrap({"text": EDIT_TEXT}), "report": rid, "report_digest": r_digest(),
                   "type": "report.add_block@1"})
    v2 = from_report(ACME, TITLE, r_pages)
    v2_ref = tref(ACME, 2, v2)
    events.append({"source": {"kind": "report", "report": rid, "report_digest": r_digest()},
                   "template": v2_ref, "type": "report.save_template@1"})
    # Third save without edits: version 2 again, nothing journaled.

    a_id, a_title = v7(4), "From acme v1"
    a_pages = [(pid, [rblock(b["id"], b["kind"], b["payload"]["value"]) for b in p["blocks"]])
               for p in v1["pages"] for pid in [p["id"]]]
    a_doc = report(a_id, a_title, a_pages, v1_ref)
    events.append({"report": a_id, "report_digest": sha(jcs(a_doc)), "template": v1_ref, "title": untrusted(a_title),
                   "type": "report.apply_template@1"})

    fai_ref = tref(FAI, 1, FAI_DOC, builtin=True)
    f_id, f_title = v7(5), "FAI from form3"
    f_pages = [(FAI_PAGE, [rblock(FAI_TABLE, "table", {"rows": []})])]
    f_doc = report(f_id, f_title, f_pages, fai_ref)
    events.append({"report": f_id, "report_digest": sha(jcs(f_doc)), "template": fai_ref, "title": untrusted(f_title),
                   "type": "report.apply_template@1"})
    f_text = v7(6)
    f_pages[0][1].append(rblock(f_text, "text", {"text": EDIT_TEXT}))
    f_doc = report(f_id, f_title, f_pages, fai_ref)
    events.append({"after": None, "block": f_text, "kind": "text", "kind_version": "1", "page": FAI_PAGE,
                   "payload": wrap({"text": EDIT_TEXT}), "report": f_id, "report_digest": sha(jcs(f_doc)),
                   "type": "report.add_block@1"})
    derived = from_report(DERIVED, f_title, f_pages, FAI_DOC)
    derived_ref = tref(DERIVED, 1, derived)
    events.append({"source": {"kind": "report", "report": f_id, "report_digest": sha(jcs(f_doc))},
                   "template": derived_ref, "type": "report.save_template@1"})

    file_ref = tref(FILE_ID, 1, FILE_DOC)
    events.append({"source": {"handle": untrusted(HANDLE), "kind": "file"}, "template": file_ref,
                   "type": "report.save_template@1"})

    # Saving over the unlocked built-in starter gives project version 1: built-in versions
    # are a number space of their own.
    starter_v1 = tref(STARTER, 1, STARTER_DOC, builtin=True)
    starter2 = from_report(STARTER, a_title, a_pages, v1)
    starter_p1 = tref(STARTER, 1, starter2)
    events.append({"source": {"kind": "report", "report": a_id, "report_digest": sha(jcs(a_doc))},
                   "template": starter_p1, "type": "report.save_template@1"})

    rows = [
        dict(starter_v1, locked=False),
        dict(starter_p1, locked=False),
        dict(v1_ref, locked=False),
        dict(v2_ref, locked=False),
        dict(derived_ref, locked=False),
        dict(file_ref, locked=False),
        dict(fai_ref, locked=True),
    ]
    # Sorted by ID, then built-in before project, then version.
    key = [(r["id"], not r["builtin"], int(r["version"])) for r in rows]
    assert key == sorted(key)
    assert v1_ref["digest"] != v2_ref["digest"]
    return {
        "project": PROJECT,
        "title": TITLE,
        "edit_text": EDIT_TEXT,
        "ids": {"acme": ACME, "derived": DERIVED, "fai": FAI, "file": FILE_ID, "starter": STARTER},
        "handle": HANDLE,
        "report_r": rid,
        "page_r": page,
        "report_a": {"id": a_id, "title": a_title, "doc_text": jcs(a_doc).decode("utf-8")},
        "report_f": {"id": f_id, "title": f_title},
        "acme_v1": dict(v1_ref, doc_text=jcs(v1).decode("utf-8")),
        "acme_v2": v2_ref,
        "derived_v1": dict(derived_ref, doc_text=jcs(derived).decode("utf-8")),
        "fai_v1": fai_ref,
        "file_v1": file_ref,
        "starter_v1": starter_v1,
        "starter_p1": starter_p1,
        "list": {"templates": rows},
        "journal_texts": [jcs(e).decode("utf-8") for e in events],
        "forged": forged(events, v1_ref, fai_ref, a_doc),
    }


def forged(events, v1_ref, fai_ref, a_doc):
    """Forged journals and their hand-labelled exact-replay outcome (the first event that is
    not identical). Each starts from the scenario journal."""
    texts = [jcs(e) for e in events]
    save = [i for i, e in enumerate(events) if e["type"] == "report.save_template@1"]
    apply = [i for i, e in enumerate(events) if e["type"] == "report.apply_template@1"]
    out = []

    def case(name, journal, outcome, at):
        out.append({"name": name, "journal_texts": [t.decode("utf-8") for t in journal], "outcome": outcome,
                    "first_mismatch": at if isinstance(at, str) else str(at)})

    # Two saves that both claim version 2 (the race the compare-and-set prevents).
    j = texts[:save[1] + 1] + [texts[save[1]]] + texts[save[1] + 1:]
    case("duplicate_version", j, "DIVERGENT", save[1] + 1)
    # A save whose version skips one.
    e = json.loads(texts[save[0]])
    e["template"]["version"] = "2"
    case("version_gap", texts[:save[0]] + [jcs(e)] + texts[save[0] + 1:], "DIVERGENT", save[0])
    # A save that names a built-in version: not a readable event.
    e = json.loads(texts[save[0]])
    e["template"]["builtin"] = True
    case("save_names_builtin", texts[:save[0]] + [jcs(e)] + texts[save[0] + 1:], "NOT_REPLAYABLE", save[0])
    # A save whose object is not in the store.
    e = json.loads(texts[save[0]])
    e["template"]["digest"] = "sha256:" + "00" * 32
    case("object_missing", texts[:save[0]] + [jcs(e)] + texts[save[0] + 1:], "DIVERGENT", save[0])
    # A save from a report whose recorded report digest is not the replayed one.
    e = json.loads(texts[save[0]])
    e["source"]["report_digest"] = "sha256:" + "11" * 32
    case("source_digest", texts[:save[0]] + [jcs(e)] + texts[save[0] + 1:], "DIVERGENT", save[0])
    # An apply of a project version that was never saved.
    e = json.loads(texts[apply[0]])
    e["template"]["version"] = "3"
    case("apply_unsaved_version", texts[:apply[0]] + [jcs(e)] + texts[apply[0] + 1:], "DIVERGENT", apply[0])
    # An apply that calls a project version a built-in one.
    e = json.loads(texts[apply[0]])
    e["template"]["builtin"] = True
    e["template"]["id"] = fai_ref["id"]
    case("apply_builtin_wrong_digest", texts[:apply[0]] + [jcs(e)] + texts[apply[0] + 1:], "DIVERGENT", apply[0])
    # An apply of the FAI form recorded as a project version.
    e = json.loads(texts[apply[1]])
    e["template"]["builtin"] = False
    case("apply_builtin_as_project", texts[:apply[1]] + [jcs(e)] + texts[apply[1] + 1:], "DIVERGENT", apply[1])
    # Documented limit (NODE.md): an apply that relabels a project version (acme@1) as a
    # built-in version the replaying release does not ship, with the report digest
    # recomputed to match, replays from the stored bytes after the digest check, so the
    # forged provenance label is accepted. It grants no authority. The journal ends there.
    e = json.loads(texts[apply[0]])
    e["template"]["builtin"] = True
    forged_a = dict(a_doc, template_ref=e["template"])
    e["report_digest"] = sha(jcs(forged_a))
    case("apply_relabelled_unshipped_builtin", texts[:apply[0]] + [jcs(e)], "IDENTICAL", "none")
    assert v1_ref["digest"] != fai_ref["digest"]
    return out


def outputs():
    out = {
        "fai_form3.json": jcs(FAI_DOC),
        "starter.json": jcs(STARTER_DOC),
        "file_ok.json": jcs(FILE_DOC),
        "file_invalid.json": jcs(FILE_INVALID),
        "label_switch.json": jcs(LABEL_SWITCH),
        "label_text.json": jcs(LABEL_TEXT),
    }
    out["cases.json"] = (json.dumps(scenario(), indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")
    return out


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, data in outputs().items():
        p = HERE / name
        if check:
            if not p.exists() or p.read_bytes() != data:
                print(f"drift: {p}")
                bad += 1
        else:
            p.write_bytes(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
