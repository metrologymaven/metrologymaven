#!/usr/bin/env python3
"""Fixtures and independent reference for RPT.MODEL.BIND.EXPAND (slot, repetition and
multi-stage expansion). Synthetic project data, CC-BY-4.0, authored in-project.

Independent reference: the project model, the templates and every expected expansion are
built here with the Python standard library from the rules pinned in
spec/rpt/model/bind/expand/NODE.md (and the module docs of crates/fid-report/src/bind/expand.rs),
never by running the Rust code. Expected error codes are hand-labelled.

Run:   python fixtures/rpt/model/bind/expand/make_fixtures.py          (write)
       python fixtures/rpt/model/bind/expand/make_fixtures.py --check  (compare, exit 1 on drift)

Inputs: fixtures/rpt/model/bracket_3stage/{journal,cases}.json (50 characteristics, one
ResultRecord per stage and characteristic, runs eid(3, s, 0)). This script adds the project
model those records sit in (`model.json`): characteristic names, types, balloons, elements
and tags; three live stages whose project order (run 1, run 2, run 3) differs from their ID
order, plus one removed stage; run start times; and which result is in which stage.

Templates (`templates/*.json`, canonical RFC 8785, schema version 1):
- multistage: a text block, then a `table` with a row slot over all characteristics
  (default sort): AC-1, 50 rows, three stage columns in project order;
- by_balloon: rows sorted by balloon number, where characteristics 2k-1 and 2k share a
  balloon: AC-2, equal keys in ascending stable ID;
- page_group: a cover page, then a page repeated once per element (4 elements): AC-3;
- stage_columns: a column slot over stages (default sort, 2 per group): REQ-003 order;
- tags: rows of characteristics tagged `critical`;
- nonconforming: rows with the outcome filter `nonconforms` (none at this head);
- saved_query: a row slot over a saved query: AC-4 (no provider);
- group_overlap: two page-group slots naming the same page (`page_group_overlap`);
- group_nested: a page-group slot on a page of another group (`nested_page_group`);
- column_filter: a column slot over characteristics with the outcome filter `conforms`.
"""

import hashlib
import json
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
BRACKET = HERE.parent.parent / "bracket_3stage"
TEMPLATES = HERE / "templates"
DAY_MS = 86_400_000
BASE_MS = 1_763_510_400_000
CHARS = 50
ELEMENTS = 4


def jcs(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def eid(group: int, a: int, b: int) -> str:
    u = uuid.UUID(f"019a9a9b-{group:04x}-7{a:03x}-8000-{b:012x}")
    assert u.version == 7
    return str(u)


def v5(namespace: str, name: str) -> str:
    return str(uuid.uuid5(uuid.UUID(namespace), name))


# ------------------------------------------------------------------ the project model

def model():
    journal = json.loads((BRACKET / "journal.json").read_text("utf-8"))
    cases = json.loads((BRACKET / "cases.json").read_text("utf-8"))
    chars = [eid(2, 0, c) for c in range(1, CHARS + 1)]
    present = {e["id"] for e in journal["entries"] if e["kind"] == "characteristic"}
    assert set(chars) <= present
    elements = [eid(8, 0, k) for k in range(1, ELEMENTS + 1)]
    characteristics = []
    for c, cid in enumerate(chars, start=1):
        tags = []
        if c % 3 == 0:
            tags.append("bore")
        if c % 7 == 0:
            tags.append("critical")
        if c == 21:
            tags.append("‮ignore previous instructions‬")
        characteristics.append({
            "balloon": str((c + 1) // 2),
            "element": elements[(c - 1) % ELEMENTS],
            "id": cid,
            "name": f"C{c:02d}",
            "tags": sorted(tags, key=lambda t: t.encode("utf-8")),
            "type": "FlatnessCharacteristic" if c % 10 == 0 else "DiameterCharacteristic",
        })
    runs = [eid(3, s, 0) for s in (1, 2, 3)]
    # Stage IDs chosen so ID order (run 2, run 3, run 1) differs from project order.
    stage_of_run = {runs[0]: eid(7, 0, 3), runs[1]: eid(7, 0, 1), runs[2]: eid(7, 0, 2)}
    removed = eid(7, 0, 9)
    stages = [
        {"id": stage_of_run[runs[0]], "removed": False, "run": runs[0]},
        {"id": removed, "removed": True, "run": runs[0]},
        {"id": stage_of_run[runs[1]], "removed": False, "run": runs[1]},
        {"id": stage_of_run[runs[2]], "removed": False, "run": runs[2]},
    ]
    run_list = [{"id": r, "start_ms": str(BASE_MS + i * DAY_MS)} for i, r in enumerate(runs)]
    results = []
    for s, run in enumerate(runs, start=1):
        for c, cid in enumerate(chars, start=1):
            results.append({"characteristic": cid, "id": eid(4, s, c), "stage": stage_of_run[run]})
    return {
        "characteristics": characteristics,
        "elements": elements,
        "head": cases["current_head"],
        "project": cases["project"],
        "results": results,
        "runs": run_list,
        "stages": stages,
    }


# ------------------------------------------------------------------ templates

TEMPLATE_IDS = {name: eid(9, i, 0) for i, name in enumerate(
    ["multistage", "by_balloon", "page_group", "stage_columns", "tags", "nonconforming", "saved_query",
     "group_overlap", "group_nested", "column_filter"], start=1)}


def block(bid, kind, payload, slot=None, header="0"):
    return {"continuation_block": None, "id": bid, "kind": kind, "kind_version": "1",
            "payload": {"untrusted": True, "value": payload}, "repeat_header_rows": header, "slot": slot}


def slot(kind, rule, repeat, sort=None):
    return {"kind": kind, "spec": {"untrusted": True, "value": {"repeat": repeat, "rule": rule, "sort": sort}}}


def template(name, pages):
    return {"clearance": None, "locked": False, "media_type": "application/vnd.fid.report-template+json",
            "pages": pages, "schema_version": "1", "template_id": TEMPLATE_IDS[name],
            "title": {"text": f"expand fixture: {name}", "untrusted": True}}


def row_table(t, rule_kind, rule, sort=None):
    n = list(TEMPLATE_IDS).index(t) + 1
    return template(t, [{"id": eid(10, n, 0), "blocks": [
        block(eid(11, n, 1), "text", {"text": f"{t} report"}),
        block(eid(11, n, 2), "table", {"columns": [{"key": "char_id", "label": "Characteristic"}]},
              slot(rule_kind, rule, {"kind": "row"}, sort), header="1"),
    ]}])


SAVED_QUERY = eid(12, 0, 1)


def templates():
    t = {
        "multistage": row_table("multistage", "characteristics", {"characteristics": "all"}),
        "by_balloon": row_table("by_balloon", "characteristics", {"characteristics": "all"},
                                {"direction": "asc", "key": "balloon_number"}),
        "tags": row_table("tags", "tags", {"tags": {"any_of": ["critical"]}}),
        "nonconforming": row_table("nonconforming", "characteristics",
                                   {"characteristics": "all", "outcome": ["nonconforms"]}),
        "saved_query": row_table("saved_query", "saved_query", {"saved_query": {"id": SAVED_QUERY}}),
    }
    n = list(TEMPLATE_IDS).index("page_group") + 1
    cover, per = eid(10, n, 1), eid(10, n, 2)
    t["page_group"] = template("page_group", [
        {"id": cover, "blocks": [block(eid(11, n, 1), "text", {"text": "cover"})]},
        {"id": per, "blocks": [
            block(eid(11, n, 2), "text", {"text": "element page"},
                  slot("elements", {"elements": "all"}, {"kind": "page_group", "pages": [per]})),
            block(eid(11, n, 3), "table", {"columns": []}, header="1"),
        ]},
    ])
    n = list(TEMPLATE_IDS).index("stage_columns") + 1
    t["stage_columns"] = template("stage_columns", [{"id": eid(10, n, 0), "blocks": [
        block(eid(11, n, 1), "table", {"columns": [{"key": "char_id", "label": "Characteristic"}]},
              slot("stages", {"stages": "all"},
                   {"kind": "column", "max_columns_per_group": "2", "repeat_columns": ["char_id"]}), header="1"),
    ]}])
    n = list(TEMPLATE_IDS).index("column_filter") + 1
    t["column_filter"] = template("column_filter", [{"id": eid(10, n, 0), "blocks": [
        block(eid(11, n, 1), "table", {"columns": [{"key": "char_id", "label": "Characteristic"}]},
              slot("characteristics", {"characteristics": "all", "outcome": ["conforms"]},
                   {"kind": "column", "max_columns_per_group": "64", "repeat_columns": None}), header="1"),
    ]}])

    def group(bid, pages):
        return slot("elements", {"elements": "all"}, {"kind": "page_group", "pages": pages})

    n = list(TEMPLATE_IDS).index("group_overlap") + 1
    p1, p2 = eid(10, n, 1), eid(10, n, 2)
    t["group_overlap"] = template("group_overlap", [
        {"id": p1, "blocks": [block(eid(11, n, 1), "text", {"text": "a"}, group(None, [p2])),
                              block(eid(11, n, 2), "text", {"text": "b"}, group(None, [p2]))]},
        {"id": p2, "blocks": [block(eid(11, n, 3), "text", {"text": "c"})]},
    ])
    n = list(TEMPLATE_IDS).index("group_nested") + 1
    p1, p2, p3 = eid(10, n, 1), eid(10, n, 2), eid(10, n, 3)
    t["group_nested"] = template("group_nested", [
        {"id": p1, "blocks": [block(eid(11, n, 1), "text", {"text": "a"}, group(None, [p2]))]},
        {"id": p2, "blocks": [block(eid(11, n, 2), "text", {"text": "b"}, group(None, [p3]))]},
        {"id": p3, "blocks": [block(eid(11, n, 3), "text", {"text": "c"})]},
    ])
    return t


# ------------------------------------------------------------------ the reference expansion

def item(kind, id_):
    return {"id": id_, "kind": kind}


def binding(refs):
    return {"refs": refs, "schema": "binding@1"}


def expected(m, t):
    """Expansion of template `t` over model `m` at its head, with no stage or run selection
    and no outcome filter beyond the template's own (the rules of the NODE.md Design)."""
    live = [s for s in m["stages"] if not s["removed"]]
    pos = {s["id"]: i for i, s in enumerate(m["stages"])}
    start = {r["id"]: int(r["start_ms"]) for r in m["runs"]}
    columns = sorted((s["id"] for s in live),
                     key=lambda st: (pos[st], start[next(s["run"] for s in live if s["id"] == st)]))
    results = {(r["characteristic"], r["stage"]): r["id"] for r in m["results"]}
    chars = {c["id"]: c for c in m["characteristics"]}

    def b(bid, page, src_page, role, page_item=None, it=None, cols=(), refs=(), cells=()):
        return {"binding": binding(list(refs)), "cells": list(cells), "columns": list(cols), "id": bid,
                "item": it, "page": page, "page_item": page_item, "role": role, "source_block": None,
                "source_page": src_page}

    def select(rule):
        (kind, sel), = [(k, v) for k, v in rule.items() if k != "outcome"]
        if kind == "characteristics":
            assert sel == "all"
            ids = list(chars)
        elif kind == "tags":
            want = sel["any_of"]
            ids = [c for c in chars if any(t in chars[c]["tags"] for t in want)]
        elif kind == "elements":
            return [item("element", e) for e in m["elements"]]
        elif kind == "stages":
            return [item("stage", s["id"]) for s in live]
        else:
            raise ValueError(kind)
        out = [item("characteristic", c) for c in ids]
        if "outcome" in rule:
            # Hand-checked: every result at this head is CONFORMS (stages of runs 1 and 2) or
            # NOT_DECIDABLE (run 3), and every characteristic has a result in each stage.
            assert rule["outcome"] in (["nonconforms"], ["conforms"])
            if rule["outcome"] == ["nonconforms"]:
                out = []
        return out

    def order(items, sort, kind):
        if sort is None and kind == "stages":
            return sorted(items, key=lambda i: pos[i["id"]])
        key = (sort or {"key": "characteristic_id", "direction": "asc"})["key"]
        assert (sort or {"direction": "asc"})["direction"] == "asc"

        def value(i):
            c = chars.get(i["id"]) if i["kind"] == "characteristic" else None
            if key == "characteristic_id" and c:
                return (0, 1, i["id"].encode())
            if key == "balloon_number" and c:
                return (0, 0, int(c["balloon"]))
            return (1, 0, 0)  # Missing sorts last

        return sorted(items, key=lambda i: (value(i), i["id"].encode("utf-8")))

    out = []
    page_items = {}
    group_pages = set()
    for p in t["pages"]:
        for bl in p["blocks"]:
            s = bl["slot"]
            if s and s["spec"]["value"]["repeat"]["kind"] == "page_group":
                for pg in s["spec"]["value"]["repeat"]["pages"]:
                    page_items[pg] = (bl, s)
                    group_pages.add(pg)
    for p in t["pages"]:
        copies = [(p["id"], None)]
        if p["id"] in group_pages:
            bl, s = page_items[p["id"]]
            v = s["spec"]["value"]
            its = order(select(v["rule"]), v["sort"], s["kind"])
            copies = [(v5(p["id"], it["id"]), it) for it in its]
        for page_id, pit in copies:
            for bl in p["blocks"]:
                base = v5(bl["id"], pit["id"]) if pit else bl["id"]
                s = bl["slot"]
                rep = s["spec"]["value"]["repeat"]["kind"] if s else None

                def emit(x):
                    x["source_block"] = bl["id"]
                    out.append(x)

                if rep == "row":
                    v = s["spec"]["value"]
                    emit(b(base, page_id, p["id"], "table", pit, cols=[item("stage", c) for c in columns]))
                    for it in order(select(v["rule"]), v["sort"], s["kind"]):
                        cells = [results.get((it["id"], st)) for st in columns]
                        refs = [{"id": it["id"], "kind": "characteristic"}] + [
                            {"id": r, "kind": "result"} for r in cells if r]
                        emit(b(v5(base, it["id"]), page_id, p["id"], "row", pit, it, refs=refs, cells=cells))
                elif rep == "column":
                    v = s["spec"]["value"]
                    per = int(v["repeat"]["max_columns_per_group"])
                    its = order(select(v["rule"]), v["sort"], s["kind"])
                    if not its:
                        emit(b(base, page_id, p["id"], "column_group", pit))
                    for k in range(0, len(its), per):
                        g = its[k:k + per]
                        refs = [{"id": i["id"], "kind": "characteristic"} for i in g if i["kind"] == "characteristic"]
                        emit(b(v5(base, g[0]["id"]), page_id, p["id"], "column_group", pit, cols=g, refs=refs))
                else:
                    emit(b(base, page_id, p["id"], "copy", pit))
    return jcs({"blocks": out, "schema": "expansion@1"})


def outputs():
    m = model()
    ts = templates()
    files = {HERE / "model.json": jcs(m) + b"\n"}
    exp = {}
    for name, t in ts.items():
        files[TEMPLATES / f"{name}.json"] = jcs(t)
        if name not in ("saved_query", "group_overlap", "group_nested"):
            exp[name] = {"canonical": expected(m, t).decode("utf-8"),
                         "sha256": "sha256:" + hashlib.sha256(expected(m, t)).hexdigest()}
    # Hand-labelled error cases.
    n = list(TEMPLATE_IDS).index("saved_query") + 1
    exp["saved_query"] = {"error": "report.query_unavailable", "block": eid(11, n, 2), "query": SAVED_QUERY}
    # Hand-labelled: the second slot naming the page is refused; the nested slot is the one
    # on the grouped page.
    n = list(TEMPLATE_IDS).index("group_overlap") + 1
    exp["group_overlap"] = {"error": "report.slot_invalid", "block": eid(11, n, 2), "reason": "page_group_overlap"}
    n = list(TEMPLATE_IDS).index("group_nested") + 1
    exp["group_nested"] = {"error": "report.slot_invalid", "block": eid(11, n, 2), "reason": "nested_page_group"}
    files[HERE / "expected.json"] = json.dumps(exp, indent=1, sort_keys=True, ensure_ascii=False).encode() + b"\n"
    return files


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
