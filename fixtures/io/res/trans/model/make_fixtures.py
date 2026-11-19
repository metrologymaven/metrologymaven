"""Independent reference for the IO.RES.TRANS.MODEL fixtures (spec/io/res/trans/model/NODE.md).

Stdlib only (jsonschema is used for the schema check when it is installed, as in CI); no
Rust is involved. All data is synthetic (D-038): no vendor file, no QIF XSD, no dataset.

It writes the rows files (inputs) and derives every expected output from the leaf's rules
as pinned in NODE.md Design, re-implemented here:

- rows_basic.json / rows_basic.out.json: AC-1 and AC-2 (five rows: pass, fail,
  undecided, no status, one with the points flag; source VendorX without version).
- rows_edge.json / rows_edge.out.json: AC-3, AC-4, AC-5 and the pinned rules (features
  flag, both flags, unrecognized status, pass outside a limit, fail inside the limits,
  non-decimal and inverted limits, a case-only duplicate key, a non-decimal value, a
  field over 4096 bytes, hostile text).
- the hand-labelled refusal files (bad_*.json) whose reasons the Rust test names.

Exact decimal comparison uses Python's decimal module (not the Rust parser). UUIDv5 uses
uuid.uuid5 over the RFC 8785 canonical bytes, which for these ASCII-keyed, string-valued
objects equal json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False).

Run from the repository root:  python fixtures/io/res/trans/model/make_fixtures.py [--check]
"""

import decimal
import hashlib
import json
import re
import sys
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
SCHEMAS = REPO / "schemas" / "io"

# Project namespace used by the tests (any UUID works; this one is fixed for the fixtures).
NAMESPACE = uuid.UUID("0b6f8c1e-3d2a-4f5b-9c7e-a1b2c3d4e5f6")
RULE = "dr:as-reported-by-source@1"
MAX_TEXT = 4096
DEC = re.compile(r"\A[+-]?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?\Z")
EXP = re.compile(r"[eE][+-]?0*([0-9]*)\Z")
ROW_FIELDS = [
    "characteristic", "part", "measurand", "value", "unit", "status_text", "uncertainty",
    "decision_rule", "outcome", "lower_limit", "upper_limit",
]


def canonical(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def is_decimal(text):
    if not DEC.match(text):
        return False
    m = EXP.search(text)
    return m is None or len(m.group(1)) <= 9


def to_dec(text):
    with decimal.localcontext() as ctx:
        ctx.prec = 10000
        ctx.Emax = 10**12
        ctx.Emin = -(10**12)
        return decimal.Decimal(text)


def side(value, lower, upper):
    v = to_dec(value)
    if lower is not None and is_decimal(lower) and v < to_dec(lower):
        return "BELOW_LOWER"
    if upper is not None and is_decimal(upper) and v > to_dec(upper):
        return "ABOVE_UPPER"
    lims = [x for x in (lower, upper) if x is not None]
    if not lims or any(not is_decimal(x) for x in lims):
        return "UNDECIDABLE"
    return "INSIDE"


def blank(x):
    return x is None or x.strip() == ""


def transcribe(doc):
    src = doc["source"]
    fsha = src["file_sha256"]
    version = src.get("version")
    record = {
        "schema": "transcription-record@1",
        "id": str(uuid.uuid5(NAMESPACE, canonical({"kind": "transcription_record", "file_sha256": fsha}).decode())),
        "source_software": src["software"],
        "source_version": version if version is not None and version.strip() != "" else "unknown",
        "file_sha256": fsha,
        "imported_at": src["imported_at"],
        "format": src["format"],
    }
    rdigest = sha(canonical(record))
    results, dropped, keys = [], [], set()
    for n, row in enumerate(doc["rows"]):
        g = lambda k: row.get(k)
        if blank(g("characteristic")) or blank(g("value")):
            dropped.append({"row": str(n), "reason": "row_incomplete"})
            continue
        if any(g(k) is not None and len(g(k).encode("utf-8")) > MAX_TEXT for k in ROW_FIELDS):
            dropped.append({"row": str(n), "reason": "field_too_long"})
            continue
        if not is_decimal(g("value")):
            dropped.append({"row": str(n), "reason": "value_not_decimal"})
            continue
        key =(ascii_lower(g("characteristic")), None if g("part") is None else ascii_lower(g("part")))
        if key in keys:
            dropped.append({"row": str(n), "reason": "duplicate_row_key"})
            continue
        keys.add(key)
        sd = side(g("value"), g("lower_limit"), g("upper_limit"))
        status = g("status")
        conflict = None
        if status == "pass":
            if sd in ("BELOW_LOWER", "ABOVE_UPPER"):
                outcome, conflict = "NOT_DECIDABLE", "pass_outside_limit"
            else:
                outcome = "CONFORMS"
        elif status == "fail":
            outcome = "NONCONFORMS"
        elif status == "undecided":
            outcome = "INDETERMINATE"
        else:
            outcome = "NOT_DECIDABLE"
        basis = "POINTS" if g("has_points") else "FEATURES" if g("has_features") else "RESULTS_ONLY"
        rid = uuid.uuid5(NAMESPACE, canonical({
            "kind": "transcribed_result", "file_sha256": fsha,
            "characteristic": g("characteristic"), "part": g("part"),
        }).decode())
        results.append({
            "schema": "transcribed-result@1",
            "id": str(rid),
            "record": {"id": record["id"], "digest": rdigest},
            "source_row": str(n),
            "characteristic": g("characteristic"),
            "part": g("part"),
            "measurand": g("measurand"),
            "evaluation_basis": basis,
            "status": "transcribed",
            "value": {"text": g("value"), "unit": g("unit")},
            "uncertainty": {"uec": "UEC-0", "uec0_reason": "u_missing"},
            "decision": {"rule": RULE, "outcome": outcome, "source_conflict": conflict},
            "side": sd,
            "claim": {"claimed": True, "declared_input": True, "uncharacterized_input": True},
            "spec_context": None,
            "algorithm": None,
            "kernel_version": None,
            "input_digests": [fsha],
            "as_reported": {
                "value": g("value"), "unit": g("unit"), "status": g("status_text"),
                "uncertainty": g("uncertainty"), "decision_rule": g("decision_rule"),
                "outcome": g("outcome"), "lower_limit": g("lower_limit"), "upper_limit": g("upper_limit"),
            },
        })
    return {"schema": "transcription@1", "record": record, "results": results, "dropped": dropped}


def ascii_lower(s):
    return "".join(c.lower() if "A" <= c <= "Z" else c for c in s)


def source(name, version, tag):
    return {
        "software": name, "version": version,
        "file_sha256": sha(("synthetic IO.RES.TRANS.MODEL source " + tag).encode()),
        "imported_at": "2026-11-17T10:00:00Z", "format": "csv",
    }


BASIC = {
    "schema": "transcription-rows@1",
    "source": source("VendorX", None, "rows_basic"),
    "rows": [
        {"characteristic": "C1", "measurand": "diameter", "value": "10.012", "unit": "mm",
         "status": "pass", "status_text": "OK", "lower_limit": "9.98", "upper_limit": "10.02"},
        {"characteristic": "C2", "measurand": "diameter", "value": "10.031", "unit": "mm",
         "status": "fail", "status_text": "NOK", "lower_limit": "9.98", "upper_limit": "10.02"},
        {"characteristic": "C3", "measurand": "position", "value": "0.048", "unit": "mm",
         "status": "undecided", "status_text": "?", "upper_limit": "0.05"},
        {"characteristic": "C4", "measurand": "flatness", "value": "0.007", "unit": "mm"},
        {"characteristic": "C5", "measurand": "length", "value": "25.004", "unit": "mm",
         "status": "pass", "status_text": "OK", "has_points": True},
    ],
}

EDGE = {
    "schema": "transcription-rows@1",
    "source": source("Vendor‮Y\u0007", "", "rows_edge"),
    "rows": [
        # 0 AC-3: authored decimal text and unit kept.
        {"characteristic": "E1", "value": "0.0100", "unit": "mm", "status": "pass",
         "lower_limit": "0", "upper_limit": "0.0100"},
        # 1 AC-4: source uncertainty stays as_reported text; still UEC-0.
        {"characteristic": "E2", "value": "5.000", "unit": "mm", "status": "pass",
         "uncertainty": "U=0.002 mm", "decision_rule": "ISO 14253-1 simple", "outcome": "in tolerance"},
        # 2 AC-5: no characteristic identifier.
        {"value": "1.0", "unit": "mm", "status": "pass"},
        # 3 no value.
        {"characteristic": "E4", "unit": "mm", "status": "fail"},
        # 4 whitespace-only characteristic counts as none.
        {"characteristic": " \t", "value": "1.0"},
        # 5 features flag; 6 both flags (POINTS wins).
        {"characteristic": "E6", "value": "-0.5e-1", "unit": "mm", "has_features": True},
        {"characteristic": "E7", "value": "1E2", "unit": "deg", "has_points": True, "has_features": True},
        # 7 unrecognized status stays NOT_DECIDABLE.
        {"characteristic": "E8", "value": "3", "unit": "mm", "status": "unrecognized", "status_text": "WARN"},
        # 8 pass but above the upper limit: never CONFORMS.
        {"characteristic": "E9", "value": "10.021", "unit": "mm", "status": "pass", "status_text": "OK",
         "lower_limit": "9.98", "upper_limit": "10.02"},
        # 9 pass but below the lower limit (only a lower limit).
        {"characteristic": "E10", "value": "-0.001", "unit": "mm", "status": "pass", "lower_limit": "0"},
        # 10 fail inside the limits stays NONCONFORMS (the source's claim, conservative).
        {"characteristic": "E11", "value": "10.000", "unit": "mm", "status": "fail",
         "lower_limit": "9.98", "upper_limit": "10.02"},
        # 11 pass, upper limit violated, lower limit unreadable: the violation is still shown.
        {"characteristic": "E12", "value": "50", "unit": "mm", "status": "pass",
         "lower_limit": "n/a", "upper_limit": "10"},
        # 12 inverted limits: the violated lower limit is shown; 13 unreadable limit, no violation.
        {"characteristic": "E13", "value": "5", "unit": "mm", "status": "pass",
         "lower_limit": "6", "upper_limit": "4"},
        {"characteristic": "E13b", "value": "5", "unit": "mm", "status": "pass",
         "lower_limit": "n/a", "upper_limit": "10"},
        # 14 case-only duplicate of E1: dropped.
        {"characteristic": "e1", "value": "0.0100", "unit": "mm", "status": "pass"},
        # 15 same characteristic, other part: kept; 16 case-only duplicate part: dropped.
        {"characteristic": "E1", "part": "P-2", "value": "0.0101", "unit": "mm"},
        {"characteristic": "E1", "part": "p-2", "value": "0.0102", "unit": "mm"},
        # 17 value in comma form: not decimal text.
        {"characteristic": "E17", "value": "0,0100", "unit": "mm"},
        # 18 exponent with too many digits: not decimal text (bounded).
        {"characteristic": "E18", "value": "1e1234567890", "unit": "mm"},
        # 19 a field over 4096 bytes.
        {"characteristic": "E19", "value": "1", "status_text": "x" * 4097},
        # 20 hostile text kept verbatim, typed, escaped only on display.
        {"characteristic": "E20\u0000‮", "measurand": "</script> \"\\", "value": "+7.25",
         "unit": "m​m", "status": "pass", "status_text": "OK\r\nrm -rf /"},
    ],
}

REFUSALS = {
    "bad_dup_key.json": b'{"schema":"transcription-rows@1","schema":"transcription-rows@1","source":{},"rows":[]}',
    "bad_case_dup_key.json": (
        b'{"schema":"transcription-rows@1","source":{"software":"S","file_sha256":"sha256:'
        + b"0" * 64
        + b'","imported_at":"2026-11-17T10:00:00Z","format":"csv"},'
        b'"rows":[{"characteristic":"C1","value":"1","Value":"2"}]}'
    ),
    "bad_number_value.json": (
        b'{"schema":"transcription-rows@1","source":{"software":"S","file_sha256":"sha256:'
        + b"0" * 64
        + b'","imported_at":"2026-11-17T10:00:00Z","format":"csv"},'
        b'"rows":[{"characteristic":"C1","value":0.0100}]}'
    ),
    "bad_status.json": (
        b'{"schema":"transcription-rows@1","source":{"software":"S","file_sha256":"sha256:'
        + b"0" * 64
        + b'","imported_at":"2026-11-17T10:00:00Z","format":"csv"},'
        b'"rows":[{"characteristic":"C1","value":"1","status":"OK"}]}'
    ),
}


def pretty(obj):
    return (json.dumps(obj, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def outputs():
    files = {}
    for name, doc in (("rows_basic", BASIC), ("rows_edge", EDGE)):
        files[name + ".json"] = pretty(doc)
        files[name + ".out.json"] = canonical(transcribe(doc))
    files.update(REFUSALS)
    return files


def check_schemas(files):
    try:
        import jsonschema
    except ImportError:
        print("jsonschema not installed: schema check skipped")
        return True
    rec = json.loads((SCHEMAS / "transcription-record-1.schema.json").read_text(encoding="utf-8"))
    res = json.loads((SCHEMAS / "transcribed-result-1.schema.json").read_text(encoding="utf-8"))
    ok = True
    for name in ("rows_basic.out.json", "rows_edge.out.json"):
        doc = json.loads(files[name])
        for inst, schema in [(doc["record"], rec)] + [(r, res) for r in doc["results"]]:
            errs = list(jsonschema.Draft202012Validator(schema).iter_errors(inst))
            if errs:
                ok = False
                print(f"{name}: {errs[0].message}")
    return ok


def main():
    check = "--check" in sys.argv[1:]
    files = outputs()
    stale = []
    for name, data in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    ok = check_schemas(files)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
    if stale or not ok:
        sys.exit(1)
    print("ok" if check else "written")


if __name__ == "__main__":
    main()
