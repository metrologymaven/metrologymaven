#!/usr/bin/env python3
"""Generate the RPT.CATALOG.STORE.SCHEMA fixtures with Python's standard library only.

An independent reference: nothing here is produced by running the Rust crate `fid-catalog`.

Outputs (CC-BY-4.0 project data), next to this script:

  schema_v1.sqlite   a catalog at schema version 1 (AC-2). The v1 DDL is the file the crate
                     compiles in (crates/fid-catalog/src/schema/v1.sql), executed here by
                     Python's own SQLite; catalog_meta, project and entry rows are synthetic
                     and written below. WAL journal mode, fully checkpointed. Its bytes depend
                     on the SQLite version, so --check compares sqlite3 iterdump output.
  schema_v1.json     what a v2 build must find after migrating schema_v1.sqlite: version 2,
                     the catalog ID and owner, and every v1 row, column by column, as typed
                     values ({"t": "text"|"int"|"real"|"null", "v": "..."}), sorted by
                     (project_id, record_id) bytewise. Written from the row plan below, never
                     read back from SQLite.
  schema_cases.json  hand-labelled entry rows (AC-4 and the column rules): each case is a full
                     typed row plus the label "accept" or "refuse" and the rule it exercises.
                     Every label is checked against this script's own reading of the rules
                     (py_rules, written from the spec and D-045, not from the DDL), and then
                     against SQLite running the crate's v1 + v2 DDL, so the DDL, the labels
                     and the Rust test agree.

Rules (spec/rpt/catalog/store/schema/NODE.md, D-045, UNC.UEC.CLASS, GDT.CHAR.RECORD):
  - (project_id, record_id) is unique; project_id names a registered project;
  - project-content text is bounded in UTF-8 bytes: record_id, revision, run, stage, rule_id
    1..256; part, plan, characteristic 1..1024; unit 1..64; tag 1..128;
  - value_literal is 1..64 characters of 0-9 . e E + -; value_literal, unit and si_value are
    all present or all absent; si_value is finite;
  - fixed strings: decision_outcome CONFORMS|NONCONFORMS|INDETERMINATE|NOT_DECIDABLE;
    evaluation_basis POINTS|FEATURES|RESULTS_ONLY|DIRECT_READING; uec_class UEC-0..UEC-3;
    interpretation provisional|signed_off; record_status active|voided|superseded;
    entry_status ok|unverified|stale|unavailable; claimed, declared_input,
    uncharacterized_input are 0 or 1, never defaulted;
  - a claimed value (declared or specification-derived basis) is UEC-1, so it never carries a
    measured class (D-045: claimed values never look measured); UEC-2 and UEC-3 rest on no
    declared and no uncharacterized input (UNC.UEC.CLASS: either one downgrades to UEC-1);
    a UEC-0 entry has no value and sets no label at all; a UEC-1 entry has exactly one of
    claimed and uncharacterized_input (UNC.UEC.CLASS: it is UEC-1 for exactly one reason; a
    declared input alone does not make it UEC-1); a decided outcome names its rule;
  - journal_entry_hash is "sha256:" + 64 lowercase hex digits; journal_seq >= 1;
  - timestamp is an RFC 3339 UTC instant ending in Z, 20..40 characters;
  - columns are typed (STRICT): text is never a blob, a real is never non-numeric text.

Run:   python fixtures/rpt/catalog/make_schema_fixtures.py
Check: python fixtures/rpt/catalog/make_schema_fixtures.py --check
"""

import hashlib
import json
import re
import sqlite3
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
DDL_V1 = ROOT / "crates" / "fid-catalog" / "src" / "schema" / "v1.sql"
DDL_V2 = ROOT / "crates" / "fid-catalog" / "src" / "schema" / "v2.sql"

CATALOG_ID = "01936b2e-7a00-7000-8000-00000000c0de"
OWNER = "metrology lab 3"

P1 = "01936b2e-7a00-7000-8000-000000000001"
P2 = "01936b2e-7a00-7000-8000-000000000002"
P3 = "01936b2e-7a00-7000-8000-000000000003"

COLUMNS = [
    "project_id", "record_id", "part", "revision", "plan", "run", "stage", "characteristic", "timestamp",
    "value_literal", "unit", "si_value", "decision_outcome", "rule_id", "evaluation_basis", "uec_class",
    "claimed", "declared_input", "uncharacterized_input", "interpretation", "record_status", "journal_seq",
    "journal_entry_hash", "entry_status",
]


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


def T(v):
    return {"t": "text", "v": v}


def I(v):
    return {"t": "int", "v": str(v)}


def R(v):
    return {"t": "real", "v": repr(float(v))}


NULL = {"t": "null"}


def entry(project, record, part, char, seq, *, value=None, outcome="CONFORMS", rule="unc.dec.simple@1",
          basis="FEATURES", uec="UEC-2", claimed=0, declared=0, unchar=0, interp="signed_off",
          status="active", estatus="ok", revision="B", plan="plan-1", run="run-1", stage="released",
          ts="2026-11-01T08:00:00Z"):
    lit, unit, si = value if value is not None else (None, None, None)
    return {
        "project_id": T(project),
        "record_id": T(record),
        "part": T(part),
        "revision": T(revision) if revision is not None else NULL,
        "plan": T(plan),
        "run": T(run),
        "stage": T(stage),
        "characteristic": T(char),
        "timestamp": T(ts),
        "value_literal": T(lit) if lit is not None else NULL,
        "unit": T(unit) if unit is not None else NULL,
        "si_value": R(si) if si is not None else NULL,
        "decision_outcome": T(outcome),
        "rule_id": T(rule) if rule is not None else NULL,
        "evaluation_basis": T(basis),
        "uec_class": T(uec),
        "claimed": I(claimed),
        "declared_input": I(declared),
        "uncharacterized_input": I(unchar),
        "interpretation": T(interp),
        "record_status": T(status),
        "journal_seq": I(seq),
        "journal_entry_hash": T(sha(f"{project}/{seq}")),
        "entry_status": T(estatus),
    }


PROJECTS = [
    # project_id, handle, last_seq, head_hash, availability
    (P1, "bracket-line", 12, sha(f"{P1}/12"), "available"),
    (P2, "Housing", 7, sha(f"{P2}/7"), "available"),
    (P3, "fixture-test", None, None, "unavailable"),
]

V1_ROWS = [
    entry(P1, "r-0001", "BRK-100", "flatness A", 3, value=("0.012", "mm", 1.2e-05)),
    entry(P1, "r-0002", "BRK-100", "position 1", 4, value=("0.25", "mm", 0.00025), uec="UEC-3",
          basis="POINTS", interp="provisional"),
    entry(P1, "r-0003", "BRK-100", "diameter 2", 5, value=("10.004", "mm", 0.010004), uec="UEC-1",
          claimed=1, declared=1, basis="RESULTS_ONLY", outcome="INDETERMINATE"),
    entry(P1, "r-0010", "BRK-100", "profile 4", 9, uec="UEC-0", outcome="NOT_DECIDABLE", rule=None,
          status="voided", estatus="unverified"),
    # Hostile-looking project text is data: quotes, SQL, a bidi override, a NUL.
    entry(P1, "r-0011", "BRK'); DROP TABLE entry; --", "char ‮gnirts‬", 12,
          value=("-1.5e-3", "rad", -0.0015), unchar=1, uec="UEC-1", status="superseded"),
    entry(P2, "a", "HSG-7", "flatness top", 2, value=("0.003", "mm", 3e-06), outcome="NONCONFORMS",
          basis="DIRECT_READING", revision=None, ts="2026-10-31T23:59:59.250Z"),
    entry(P2, "b", "HSG-7", "bore ø 12", 7, value=("12.0", "mm", 0.012), uec="UEC-2",
          estatus="stale"),
    entry(P3, "x-1", "FIX-1", "nul\u0000inside", 1, value=("1", "1", 1.0), estatus="unavailable"),
]


def sort_rows(rows):
    return sorted(rows, key=lambda r: (r["project_id"]["v"].encode("utf-8"), r["record_id"]["v"].encode("utf-8")))


def to_sql(v):
    t = v["t"]
    if t == "null":
        return None
    if t == "text":
        return v["v"]
    if t == "int":
        return int(v["v"])
    if t == "real":
        return float(v["v"])
    if t == "blob":
        return bytes.fromhex(v["v"])
    raise ValueError(t)


def split_statements(sql):
    """Splits a DDL file into complete statements with sqlite3.complete_statement."""
    out, buf = [], ""
    for line in sql.splitlines(keepends=True):
        if not buf and (line.strip() == "" or line.lstrip().startswith("--")):
            continue
        buf += line
        if sqlite3.complete_statement(buf):
            out.append(buf.strip())
            buf = ""
    assert buf.strip() == "", "trailing incomplete statement"
    return out


def insert_entry(con, row):
    cols = ", ".join(COLUMNS)
    marks = ", ".join("?" for _ in COLUMNS)
    con.execute(f"INSERT INTO entry ({cols}) VALUES ({marks})", [to_sql(row[c]) for c in COLUMNS])


def insert_projects(con):
    for p in PROJECTS:
        con.execute(
            "INSERT INTO project (project_id, handle, last_seq, head_hash, availability) VALUES (?,?,?,?,?)", p
        )


def build_v1(path):
    con = sqlite3.connect(path, isolation_level=None)
    con.execute("PRAGMA foreign_keys=ON")
    mode = con.execute("PRAGMA journal_mode=WAL").fetchone()[0]
    assert mode == "wal", mode
    con.execute("BEGIN IMMEDIATE")
    for stmt in split_statements(DDL_V1.read_text(encoding="utf-8")):
        con.execute(stmt)
    con.execute(
        "INSERT INTO catalog_meta (singleton, schema_version, catalog_id, owner) VALUES (1, 1, ?, ?)",
        (CATALOG_ID, OWNER),
    )
    insert_projects(con)
    for row in V1_ROWS:
        insert_entry(con, row)
    assert con.execute("PRAGMA foreign_key_check").fetchall() == []
    con.execute("COMMIT")
    con.execute("PRAGMA wal_checkpoint(TRUNCATE)")
    con.close()


def dump(path):
    con = sqlite3.connect(f"file:{Path(path).as_posix()}?mode=ro&immutable=1", uri=True)
    try:
        return list(con.iterdump())
    finally:
        con.close()


def expected_after_migration():
    return {
        "schema_version": 2,
        "catalog_id": CATALOG_ID,
        "owner": OWNER,
        "columns": COLUMNS,
        "projects": [
            {
                "project_id": T(p[0]),
                "handle": T(p[1]),
                "last_seq": I(p[2]) if p[2] is not None else NULL,
                "head_hash": T(p[3]) if p[3] is not None else NULL,
                "availability": T(p[4]),
            }
            for p in sorted(PROJECTS)
        ],
        "entries": [[r[c] for c in COLUMNS] for r in sort_rows(V1_ROWS)],
        "tags": 0,
    }


# ---------------------------------------------------------------- hand-labelled cases

TEXT_MAX = {
    "record_id": 256, "part": 1024, "revision": 256, "plan": 1024, "run": 256, "stage": 256,
    "characteristic": 1024, "unit": 64, "rule_id": 256,
}
NULLABLE = {"revision", "value_literal", "unit", "si_value", "rule_id"}
ENUMS = {
    "decision_outcome": {"CONFORMS", "NONCONFORMS", "INDETERMINATE", "NOT_DECIDABLE"},
    "evaluation_basis": {"POINTS", "FEATURES", "RESULTS_ONLY", "DIRECT_READING"},
    "uec_class": {"UEC-0", "UEC-1", "UEC-2", "UEC-3"},
    "interpretation": {"provisional", "signed_off"},
    "record_status": {"active", "voided", "superseded"},
    "entry_status": {"ok", "unverified", "stale", "unavailable"},
}
FLAGS = {"claimed", "declared_input", "uncharacterized_input"}
REGISTERED = {p[0] for p in PROJECTS}
DIGEST = re.compile(r"sha256:[0-9a-f]{64}")
LITERAL = re.compile(r"[0-9eE.+\-]{1,64}")
TIMESTAMP = re.compile(r"[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}[0-9TZ:.\-]*Z")


def py_rules(row, existing):
    """This script's own reading of the rules: 'accept' or 'refuse'. Independent of the DDL."""
    vals = {c: row[c] for c in COLUMNS}
    for c, v in vals.items():
        if v["t"] == "null":
            if c not in NULLABLE:
                return "refuse"
            continue
        if c in ("journal_seq",) or c in FLAGS:
            if v["t"] != "int":
                return "refuse"
        elif c == "si_value":
            if v["t"] not in ("real", "int"):
                return "refuse"
        elif v["t"] != "text":
            return "refuse"
    get = lambda c: None if vals[c]["t"] == "null" else to_sql(vals[c])
    for c, n in TEXT_MAX.items():
        s = get(c)
        if s is not None and not (1 <= len(s.encode("utf-8")) <= n):
            return "refuse"
    for c, allowed in ENUMS.items():
        if get(c) not in allowed:
            return "refuse"
    for c in FLAGS:
        if get(c) not in (0, 1):
            return "refuse"
    if get("project_id") not in REGISTERED:
        return "refuse"
    if not DIGEST.fullmatch(get("journal_entry_hash")):
        return "refuse"
    if get("journal_seq") < 1:
        return "refuse"
    ts = get("timestamp")
    if not (20 <= len(ts) <= 40 and TIMESTAMP.fullmatch(ts)):
        return "refuse"
    lit, unit, si = get("value_literal"), get("unit"), get("si_value")
    if not ((lit is None) == (unit is None) == (si is None)):
        return "refuse"
    if lit is not None and not LITERAL.fullmatch(lit):
        return "refuse"
    if get("uec_class") == "UEC-0" and lit is not None:
        return "refuse"
    if get("claimed") == 1 and get("uec_class") != "UEC-1":
        return "refuse"
    labels = (get("claimed"), get("declared_input"), get("uncharacterized_input"))
    if get("uec_class") in ("UEC-2", "UEC-3") and (labels[1] == 1 or labels[2] == 1):
        return "refuse"
    if get("uec_class") == "UEC-0" and any(v == 1 for v in labels):
        return "refuse"
    if get("uec_class") == "UEC-1" and labels[0] + labels[2] != 1:
        return "refuse"
    if get("decision_outcome") != "NOT_DECIDABLE" and get("rule_id") is None:
        return "refuse"
    if (get("project_id"), get("record_id")) in existing:
        return "refuse"
    return "accept"


BASE = entry(P1, "case-base", "CASE-1", "flatness", 1, value=("0.010", "mm", 1e-05))


def with_(**changes):
    row = dict(BASE)
    row.update(changes)
    return row


CASES = [
    # name, row, setup rows inserted first (same transaction), hand label, rule
    ("valid_base", BASE, [], "accept", "a complete row"),
    ("duplicate_key", BASE, [BASE], "refuse", "unique (project_id, record_id) (AC-4)"),
    ("same_record_other_project", with_(project_id=T(P2)), [BASE], "accept", "the key is the pair"),
    ("unregistered_project", with_(project_id=T("01936b2e-7a00-7000-8000-0000000000ff")), [], "refuse",
     "project must be registered"),
    ("claimed_uec1", with_(claimed=I(1), declared_input=I(1), uec_class=T("UEC-1")), [], "accept",
     "a claimed value is UEC-1"),
    ("claimed_uec2", with_(claimed=I(1), uec_class=T("UEC-2")), [], "refuse",
     "D-045: a claimed value never carries a measured class"),
    ("claimed_uec3", with_(claimed=I(1), uec_class=T("UEC-3")), [], "refuse",
     "D-045: a claimed value never carries a measured class"),
    ("uec0_without_value", with_(uec_class=T("UEC-0"), value_literal=NULL, unit=NULL, si_value=NULL), [],
     "accept", "UEC-0 carries no value"),
    ("uec0_with_value", with_(uec_class=T("UEC-0")), [], "refuse", "UEC-0 carries no value"),
    ("uec2_declared_input", with_(declared_input=I(1)), [], "refuse",
     "a declared input downgrades to UEC-1"),
    ("uec3_declared_input", with_(uec_class=T("UEC-3"), declared_input=I(1)), [], "refuse",
     "a declared input downgrades to UEC-1"),
    ("uec2_uncharacterized_input", with_(uncharacterized_input=I(1)), [], "refuse",
     "an uncharacterized contributor downgrades to UEC-1"),
    ("uec3_uncharacterized_input", with_(uec_class=T("UEC-3"), uncharacterized_input=I(1)), [], "refuse",
     "an uncharacterized contributor downgrades to UEC-1"),
    ("uec1_declared_input", with_(uec_class=T("UEC-1"), declared_input=I(1)), [], "refuse",
     "UNC.UEC.CLASS: a declared input alone does not make a value UEC-1"),
    ("uec1_no_label", with_(uec_class=T("UEC-1")), [], "refuse",
     "UNC.UEC.CLASS: a UEC-1 entry is claimed or rests on an uncharacterized contributor"),
    ("uec1_uncharacterized_and_declared",
     with_(uec_class=T("UEC-1"), uncharacterized_input=I(1), declared_input=I(1)), [], "accept",
     "UEC-1 on an uncharacterized contributor may also rest on a declared input"),
    ("uec1_claimed_and_uncharacterized",
     with_(uec_class=T("UEC-1"), claimed=I(1), uncharacterized_input=I(1)), [], "refuse",
     "UNC.UEC.CLASS: exactly one of claimed and uncharacterized_input"),
    ("uec1_uncharacterized_input", with_(uec_class=T("UEC-1"), uncharacterized_input=I(1)), [], "accept",
     "UEC-1 may rest on an uncharacterized contributor"),
    ("uec0_claimed", with_(uec_class=T("UEC-0"), claimed=I(1), value_literal=NULL, unit=NULL, si_value=NULL), [], "refuse",
     "UEC-0 sets no labels"),
    ("uec0_declared_input", with_(uec_class=T("UEC-0"), declared_input=I(1), value_literal=NULL, unit=NULL, si_value=NULL), [], "refuse",
     "UEC-0 sets no labels"),
    ("uec0_uncharacterized_input", with_(uec_class=T("UEC-0"), uncharacterized_input=I(1), value_literal=NULL, unit=NULL, si_value=NULL), [],
     "refuse", "UEC-0 sets no labels"),
    ("claimed_missing", with_(claimed=NULL), [], "refuse", "labels are never defaulted"),
    ("claimed_two", with_(claimed=I(2)), [], "refuse", "labels are 0 or 1"),
    ("claimed_text", with_(claimed=T("yes")), [], "refuse", "STRICT integer column"),
    ("interpretation_missing", with_(interpretation=NULL), [], "refuse", "D-045 label is never defaulted"),
    ("interpretation_settled", with_(interpretation=T("settled")), [], "refuse", "fixed strings only"),
    ("value_without_unit", with_(unit=NULL), [], "refuse", "value, unit and SI value go together"),
    ("si_without_literal", with_(value_literal=NULL, unit=NULL), [], "refuse",
     "value, unit and SI value go together"),
    ("literal_with_letters", with_(value_literal=T("1.0mm")), [], "refuse", "decimal literal charset"),
    ("literal_65_chars", with_(value_literal=T("1" * 65)), [], "refuse", "literal at most 64"),
    ("si_text", with_(si_value=T("one")), [], "refuse", "STRICT real column"),
    ("basis_measured", with_(evaluation_basis=T("MEASURED")), [], "refuse", "fixed strings only"),
    ("entry_status_upper", with_(entry_status=T("OK")), [], "refuse", "fixed strings, exact case"),
    ("record_status_deleted", with_(record_status=T("deleted")), [], "refuse", "fixed strings only"),
    ("outcome_pass", with_(decision_outcome=T("PASS")), [], "refuse", "fixed strings only"),
    ("decided_without_rule", with_(rule_id=NULL), [], "refuse", "a decided outcome names its rule"),
    ("not_decidable_without_rule", with_(decision_outcome=T("NOT_DECIDABLE"), rule_id=NULL), [], "accept",
     "NOT_DECIDABLE may have no rule"),
    ("hash_upper_hex", with_(journal_entry_hash=T("sha256:" + "A" * 64)), [], "refuse", "lowercase digest"),
    ("hash_sha1", with_(journal_entry_hash=T("sha1:" + "0" * 40)), [], "refuse", "sha256 digest"),
    ("hash_short", with_(journal_entry_hash=T("sha256:" + "0" * 63)), [], "refuse", "64 hex digits"),
    ("seq_zero", with_(journal_seq=I(0)), [], "refuse", "journal positions start at 1"),
    ("timestamp_offset", with_(timestamp=T("2026-11-01T08:00:00+01:00")), [], "refuse", "UTC, ends in Z"),
    ("timestamp_date_only", with_(timestamp=T("2026-11-01")), [], "refuse", "full instant"),
    ("record_id_empty", with_(record_id=T("")), [], "refuse", "record ID 1..256 bytes"),
    ("record_id_256_bytes", with_(record_id=T("r" * 256)), [], "accept", "record ID 1..256 bytes"),
    ("record_id_257_bytes", with_(record_id=T("r" * 257)), [], "refuse", "record ID 1..256 bytes"),
    ("part_1024_bytes", with_(part=T("p" * 1024)), [], "accept", "part 1..1024 bytes"),
    ("part_342_euro_signs", with_(part=T("€" * 342)), [], "refuse",
     "bounds are UTF-8 bytes (1026), not characters (342)"),
    ("part_blob", with_(part={"t": "blob", "v": "41"}), [], "refuse", "STRICT text column"),
    ("part_sql_text", with_(part=T("x'); DELETE FROM entry; --")), [], "accept", "text is data"),
    ("characteristic_bidi", with_(characteristic=T("‮evil‬")), [], "accept", "text is data"),
    ("revision_null", with_(revision=NULL), [], "accept", "revision is optional"),
    ("revision_empty", with_(revision=T("")), [], "refuse", "present text is never empty"),
    ("unit_65_bytes", with_(unit=T("u" * 65)), [], "refuse", "unit 1..64 bytes"),
]


def sqlite_verdict(row, setup):
    con = sqlite3.connect(":memory:", isolation_level=None)
    try:
        con.execute("PRAGMA foreign_keys=ON")
        for ddl in (DDL_V1, DDL_V2):
            for stmt in split_statements(ddl.read_text(encoding="utf-8")):
                con.execute(stmt)
        insert_projects(con)
        for s in setup:
            insert_entry(con, s)
        try:
            insert_entry(con, row)
        except sqlite3.IntegrityError:
            return "refuse"
        return "accept"
    finally:
        con.close()


def cases():
    out = []
    names = set()
    for name, row, setup, label, rule in CASES:
        assert name not in names, name
        names.add(name)
        existing = {(to_sql(s["project_id"]), to_sql(s["record_id"])) for s in setup}
        mine = py_rules(row, existing)
        assert mine == label, f"{name}: hand label {label}, py_rules {mine}"
        db = sqlite_verdict(row, setup)
        assert db == label, f"{name}: hand label {label}, SQLite with the crate DDL {db}"
        out.append({
            "name": name,
            "rule": rule,
            "expect": label,
            "setup": [[s[c] for c in COLUMNS] for s in setup],
            "row": [row[c] for c in COLUMNS],
        })
    return {"columns": COLUMNS, "projects": [list(p) for p in PROJECTS], "cases": out}


def dump_json(data):
    return (json.dumps(data, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")


def main():
    check = sys.argv[1:] == ["--check"]
    if sys.argv[1:] not in ([], ["--check"]):
        print("usage: make_schema_fixtures.py [--check]")
        return 2
    texts = {
        HERE / "schema_v1.json": dump_json(expected_after_migration()),
        HERE / "schema_cases.json": dump_json(cases()),
    }
    with tempfile.TemporaryDirectory() as tmp:
        fresh = Path(tmp) / "schema_v1.sqlite"
        build_v1(str(fresh))
        if check:
            stale = [p.name for p, b in texts.items() if not p.exists() or p.read_bytes() != b]
            db = HERE / "schema_v1.sqlite"
            if not db.exists() or dump(str(fresh)) != dump(str(db)):
                stale.append(db.name)
            if stale:
                print("stale fixtures (run make_schema_fixtures.py):", ", ".join(stale))
                return 1
            print("fixtures up to date")
            return 0
        (HERE / "schema_v1.sqlite").write_bytes(fresh.read_bytes())
    for p, b in texts.items():
        p.write_bytes(b)
    print("wrote schema_v1.sqlite, schema_v1.json, schema_cases.json")
    return 0


if __name__ == "__main__":
    sys.exit(main())
