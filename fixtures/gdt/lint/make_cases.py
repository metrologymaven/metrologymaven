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
  expected.json             per case: table file, mode, schema check, rules run and not run,
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
and the other rules of the mode are listed as not run.

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
    # here is one (they come from the synthetic table unchanged).
    return r.get("key") if isinstance(r.get("key"), str) else "rows[%d]" % index


def l1_l2(index, r, out):
    k = key_of(index, r)
    total = sum(len(r[f]) for f in FREE_TEXT_FIELDS if isinstance(r.get(f), str))
    if total > LIMIT:
        out.append(("L1", k, FREE_TEXT_FIELDS[0]))
    for i, c in enumerate(r.get("clauses", [])):
        v = c.get("clause")
        if isinstance(v, str) and not CLAUSE.fullmatch(v):
            out.append(("L2", k, "clauses[%d].clause" % i))


def lint(table, index_cases, mode, schema_ok):
    out = []
    for i, r in enumerate(table["rows"]):
        l1_l2(i, r, out)
        if not schema_ok:
            continue
        k = r["key"]
        if r["status"] == "supported" and not r["ors_cases"]:
            out.append(("L3", k, "ors_cases"))
        for j, case in enumerate(r["ors_cases"]):
            if case not in index_cases:
                out.append(("L4", k, "ors_cases[%d]" % j))
        if mode == "release":
            rv = r["review"]
            distinct = {x.lower() for x in rv.get("reviewers", [])}
            if rv["state"] != "signed_off" or len(distinct) < 2:
                out.append(("L5", k, "review"))
    out.sort(key=lambda f: (f[1], f[0], f[2]))
    return [{"rule": a, "key": b, "field": c} for (a, b, c) in out]


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

    expected = []
    for name, t, mode, schema in cases:
        table_file = "../dmt/synthetic_table_v1.json" if t is base else name + ".json"
        if t is not base:
            files[name + ".json"] = dump(t)
        schema_ok = schema["status"] == "ok"
        rules = RULES_RELEASE if mode == "release" else RULES_DRAFT
        run = rules if schema_ok else ["L1", "L2"]
        findings = lint(t, set(index_cases), mode, schema_ok)
        expected.append(
            {
                "case": name,
                "table": table_file,
                "mode": mode,
                "schema_check": schema,
                "rules_run": run,
                "rules_not_run": [r for r in rules if r not in run],
                "findings": findings,
                "exit_code": 0 if (schema_ok and not findings) else 1,
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
