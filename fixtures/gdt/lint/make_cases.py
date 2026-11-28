#!/usr/bin/env python3
"""Fixtures and expected findings for spec node GDT.RES.LINT (the DMT lint).

An independent reference for the Rust module `fid_gdt::dmt_lint`: this script implements
the five lint rules of spec/gdt/res/lint/NODE.md directly from the spec text, with the
Python standard library only, and never runs or reads the Rust code.

Inputs (read only): fixtures/gdt/dmt/synthetic_table_v1.json (GDT.RES.SCHEMA's synthetic,
CC0 table; no standards text). Every case below is that table with one small, named edit.
Filler text is lorem ipsum only.

Outputs (this directory):
  ors_index.json            fid.ors-index/1: every ORS case the synthetic table links
  <case>.json               one edited table per case
  expected.json             per case: table file, mode, schema check, rules run, rules not
                            run [{rule, reason}], per-rule counts {checked, skipped},
                            findings [{rule, key, field}] sorted by (key, rule, field),
                            exit code

Rules (spec REQ-GDT.RES.LINT-001 to -006):
  L1  the free-text fields of a row (the schema has one: notes) together hold more than
      280 characters (Unicode code points)
  L2  a clause value does not fully match [0-9]+(\\.[0-9]+)*[a-z]?
  L3  a row with status "supported" links no ORS case
  L4  an ORS link names a case ID absent from the ORS index
  L5  (release mode only) the review state is not signed_off by two distinct reviewers

Schema checks are hand-labelled from GDT.RES.SCHEMA's rules (spec/gdt/res/schema/NODE.md
and the loader's pinned check order), not computed: a case that breaks the schema names the
one rule it breaks. When the schema refuses a table, only L1 and L2 run (over the raw JSON),
and the other rules of the mode are listed as not run (schema_refused); L1 and L2 do not run
either when there is no rows array (not_evaluable). A table with no rows runs no rule
(no_rows) and never passes. Counts: L1 rows, L2 clause values, L3 supported rows, L4 ORS
links, L5 rows; in a refused table, a row that is not an object (L1, L2), a row whose
free-text fields are not all strings (L1), a clauses value that is not an array (L2) and a
clause entry without a string clause (L2) are counted as skipped.

Usage: python make_cases.py           write the files
       python make_cases.py --check   fail if any file differs from what would be written
"""

import copy
import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[2]
SOURCE = ROOT / "fixtures" / "gdt" / "dmt" / "synthetic_table_v1.json"

LIMIT = 280
CLAUSE = re.compile(r"[0-9]+(\.[0-9]+)*[a-z]?")
FREE_TEXT_FIELDS = ["notes"]
RULES_DRAFT = ["L1", "L2", "L3", "L4"]
RULES_RELEASE = ["L1", "L2", "L3", "L4", "L5"]

LOREM = (
    "Lorem ipsum dolor sit amet, consectetur adipiscing elit, sed do eiusmod tempor "
    "incididunt ut labore et dolore magna aliqua. Ut enim ad minim veniam, quis nostrud "
    "exercitation ullamco laboris nisi ut aliquip ex ea commodo consequat. Duis aute irure "
    "dolor in reprehenderit in voluptate velit esse cillum dolore eu fugiat nulla pariatur. "
)


def lorem(n):
    text = LOREM * (n // len(LOREM) + 1)
    return text[:n]


def row(table, key):
    for r in table["rows"]:
        if r["key"] == key:
            return r
    raise KeyError(key)


def key_of(index, r):
    # The Rust lint names a row by its key when the key is a valid DMT key; every key used
    # here is one (they come from the synthetic table unchanged), or the row has none.
    return r.get("key") if isinstance(r.get("key"), str) else "rows[%d]" % index


def l1_l2(index, r, out, checks):
    """L1 and L2 over one row as raw JSON; counts what was checked and what was skipped."""
    if not isinstance(r, dict):
        checks["L1"]["skipped"] += 1
        checks["L2"]["skipped"] += 1
        return
    k = key_of(index, r)
    texts = [r.get(f) for f in FREE_TEXT_FIELDS]
    if all(isinstance(t, str) for t in texts):
        checks["L1"]["checked"] += 1
        if sum(len(t) for t in texts) > LIMIT:
            out.append(("L1", k, "+".join(FREE_TEXT_FIELDS)))
    else:
        checks["L1"]["skipped"] += 1
    clauses = r.get("clauses")
    if not isinstance(clauses, list):
        checks["L2"]["skipped"] += 1
        return
    for i, c in enumerate(clauses):
        v = c.get("clause") if isinstance(c, dict) else None
        if not isinstance(v, str):
            checks["L2"]["skipped"] += 1
            continue
        checks["L2"]["checked"] += 1
        if not CLAUSE.fullmatch(v):
            out.append(("L2", k, "clauses[%d].clause" % i))


def lint(table, index_cases, mode, schema_ok):
    """Returns (findings, checks, rules_run, rules_not_run) per the spec rules."""
    rules = RULES_RELEASE if mode == "release" else RULES_DRAFT
    checks = {r: {"checked": 0, "skipped": 0} for r in rules}
    out = []
    rows = table.get("rows") if isinstance(table, dict) else None
    if schema_ok:
        if not rows:
            return [], checks, [], [{"rule": r, "reason": "no_rows"} for r in rules]
        run, not_run = list(rules), []
    else:
        raw = ["L1", "L2"]
        if not isinstance(rows, list):
            run, not_run = [], [{"rule": r, "reason": "not_evaluable"} for r in raw]
        elif not rows:
            run, not_run = [], [{"rule": r, "reason": "no_rows"} for r in raw]
        else:
            run, not_run = raw, []
        not_run += [{"rule": r, "reason": "schema_refused"} for r in rules if r not in raw]
        if not isinstance(rows, list):
            rows = []
    for i, r in enumerate(rows):
        l1_l2(i, r, out, checks)
        if not schema_ok:
            continue
        k = r["key"]
        if r["status"] == "supported":
            checks["L3"]["checked"] += 1
            if not r["ors_cases"]:
                out.append(("L3", k, "ors_cases"))
        for j, case in enumerate(r["ors_cases"]):
            checks["L4"]["checked"] += 1
            if case not in index_cases:
                out.append(("L4", k, "ors_cases[%d]" % j))
        if mode == "release":
            checks["L5"]["checked"] += 1
            rv = r["review"]
            distinct = {x.lower() for x in rv.get("reviewers", [])}
            if rv["state"] != "signed_off" or len(distinct) < 2:
                out.append(("L5", k, "review"))
    out.sort(key=lambda f: (f[1], f[0], f[2]))
    findings = [{"rule": a, "key": b, "field": c} for (a, b, c) in out]
    return findings, checks, run, not_run


def dump(obj):
    return json.dumps(obj, indent=2, ensure_ascii=True) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    base = json.loads(SOURCE.read_text(encoding="utf-8"))
    index_cases = sorted({c for r in base["rows"] for c in r["ors_cases"]})
    files = {"ors_index.json": dump({"schema": "fid.ors-index/1", "cases": index_cases})}
    ok = {"status": "ok"}

    # (name, table, mode, schema check)
    cases = []

    def case(name, edit, mode, schema=ok):
        t = copy.deepcopy(base)
        edit(t)
        cases.append((name, t, mode, schema))

    cases.append(("synthetic_draft", base, "draft", ok))
    cases.append(("synthetic_release", base, "release", ok))

    def long_text(t):
        row(t, "dmt:asme/flatness/none@1")["notes"] = lorem(300)

    case("long_text", long_text, "draft", {"status": "refused", "code": "field_too_long", "reason": "notes"})

    def notes_280(t):
        row(t, "dmt:asme/flatness/none@1")["notes"] = lorem(280)

    case("notes_280", notes_280, "draft")

    def notes_281(t):
        row(t, "dmt:asme/flatness/none@1")["notes"] = lorem(281)

    case("notes_281", notes_281, "draft", {"status": "refused", "code": "field_too_long", "reason": "notes"})

    def supported_no_ors(t):
        row(t, "dmt:asme/straightness/none@1")["ors_cases"] = []

    case("supported_no_ors", supported_no_ors, "draft")

    def dangling_ors(t):
        row(t, "dmt:asme/size/none@1")["ors_cases"] = ["ors:synthetic/missing.001"]

    case("dangling_ors", dangling_ors, "draft")

    def bad_clause(t):
        row(t, "dmt:asme/angularity/none@1")["clauses"][0]["clause"] = "see text"

    case("bad_clause", bad_clause, "draft", {"status": "refused", "code": "malformed_table", "reason": "bad_clause"})

    def bad_clause_schema_ok(t):
        # Dot-separated lower-case alphanumerics pass the schema's clause grammar but are not
        # clause numbers, so only L2 catches them.
        row(t, "dmt:asme/angularity/none@1")["clauses"][0]["clause"] = "a.b"

    case("bad_clause_schema_ok", bad_clause_schema_ok, "draft")

    def release_draft(t):
        for r in t["rows"]:
            r["review"] = {"state": "draft"}

    case("release_draft", release_draft, "release")

    def release_clean(t):
        for r in t["rows"]:
            r["review"] = {"state": "signed_off", "reviewers": ["rev-a", "rev-b"]}

    case("release_clean", release_clean, "release")

    def signed_one(t):
        release_clean(t)
        row(t, "dmt:asme/flatness/none@1")["review"] = {"state": "signed_off", "reviewers": ["rev-a"]}

    case("signed_one", signed_one, "release")

    def mixed(t):
        a = row(t, "dmt:asme/angularity/none@1")
        c0 = a["clauses"][0]
        a["clauses"] = [dict(c0, clause="x.1"), dict(c0, clause="y")]
        a["ors_cases"] = []
        row(t, "dmt:asme/flatness/none@1")["ors_cases"] = ["ors:synthetic/flatness.001", "ors:zzz/none.001"]

    case("mixed", mixed, "release")

    def empty_rows(t):
        t["rows"] = []

    # The schema accepts a table with no rows; the lint never passes one.
    case("empty_rows", empty_rows, "release")

    def rows_not_array(t):
        t["rows"] = {}

    wrong_type = {"status": "refused", "code": "malformed_table", "reason": "wrong_type"}
    case("rows_not_array", rows_not_array, "draft", wrong_type)

    def raw_skips(t):
        # rows[0] is not an object (the schema refuses it first, as wrong_type); one row's
        # notes and one clause value are not strings; one row's notes are over the limit.
        row(t, "dmt:asme/flatness/none@1")["notes"] = 7
        row(t, "dmt:asme/angularity/none@1")["clauses"][0]["clause"] = 3
        row(t, "dmt:asme/straightness/none@1")["notes"] = lorem(300)
        t["rows"].insert(0, 5)

    case("raw_skips", raw_skips, "draft", wrong_type)

    expected = []
    for name, t, mode, schema in cases:
        table_file = "../dmt/synthetic_table_v1.json" if t is base else name + ".json"
        if t is not base:
            files[name + ".json"] = dump(t)
        schema_ok = schema["status"] == "ok"
        findings, checks, run, not_run = lint(t, set(index_cases), mode, schema_ok)
        expected.append(
            {
                "case": name,
                "table": table_file,
                "mode": mode,
                "schema_check": schema,
                "rules_run": run,
                "rules_not_run": not_run,
                "checks": checks,
                "findings": findings,
                "exit_code": 0 if (schema_ok and not findings and not not_run) else 1,
            }
        )
    files["expected.json"] = dump({"cases": expected})

    bad = []
    for name, text in sorted(files.items()):
        p = HERE / name
        data = text.encode("utf-8")
        if check:
            if not p.exists() or p.read_bytes() != data:
                bad.append(name)
        else:
            p.write_bytes(data)
    if bad:
        print("DIFFERS: " + ", ".join(bad))
        return 1
    print("%s %d files" % ("ok" if check else "wrote", len(files)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
