"""Independent reference for the IO.RES.CSV.MAP fixtures (spec/io/res/csv/map/NODE.md).

Stdlib only, no Rust involved. It derives every expected value the Rust tests compare:

- user_map.expected.json: ID, version and SHA-256 of the RFC 8785 canonical form of
  user_map.json (AC-1). For these maps (ASCII keys, strings, booleans and small
  integers) RFC 8785 equals json.dumps with sorted keys, no whitespace and raw UTF-8;
  the script asserts that user_map_reordered.json has the same canonical bytes.
- builtin.expected.json: the same three values for the built-in maps, read from the
  crate's JSON files, plus their lossy lists (AC-4).
- lossy.expected.json: the lossy list of export_lossy.json (AC-3), computed from the
  spec's list of lossy-check fields.
- dialect_cases.json: RFC 4180 records written and read by Python's csv module
  (QUOTE_MINIMAL, doublequote, strict reader) for the three delimiters.

The hand-labelled error fixtures (dup_column.json, unknown_field.json, comma_comma.json)
carry their expected reasons in the Rust test, taken from the spec's rules.

Run from the repository root:  python fixtures/io/res/csv/map/make_fixtures.py [--check]
"""

import csv
import hashlib
import io
import json
import random
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
BUILTIN_DIR = REPO / "crates" / "fid-io-res" / "src" / "csv" / "map"

# REQ-IO.RES.CSV.MAP-003: evaluation_basis, uncertainty class, decision rule, record
# status, source software, in this order.
LOSSY_CHECK = ["evaluation_basis", "uec", "decision_rule", "record_status", "source_software"]


def canonical(path):
    obj = json.loads(path.read_bytes().decode("utf-8"))
    return obj, json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def identity(path):
    obj, canon = canonical(path)
    return {"id": obj["id"], "version": obj["version"], "sha256": "sha256:" + hashlib.sha256(canon).hexdigest()}


def lossy(path):
    obj, _ = canonical(path)
    fields = {c["field"] for c in obj["columns"]}
    return [f for f in LOSSY_CHECK if f not in fields]


def dump(obj):
    return (json.dumps(obj, indent=2, ensure_ascii=True) + "\n").encode("ascii")


def dialect_cases():
    fixed = [
        ["a", "b", "c"],
        ["", "x", ""],
        [""],
        ["", ""],
        ["has,comma", "has;semi", "has\ttab"],
        ['say "hi"', '"', '""'],
        ["line\nbreak", "cr\rhere", "crlf\r\nboth"],
        [" lead", "trail ", "  "],
        ["=SUM(A1)", "+1", "-0.012", "@x"],
        ["Maß", "été", "日本"],
        ["0,0100", "1.5e-3", "-7"],
    ]
    rng = random.Random(20261118)
    alphabet = ["a", "b", "0", " ", ",", ";", "\t", '"', "\r", "\n", "ß", "="]
    generated = []
    for _ in range(60):
        n = rng.randint(1, 5)
        row = ["".join(rng.choice(alphabet) for _ in range(rng.randint(0, 6))) for _ in range(n)]
        generated.append(row)
    cases = []
    for name, delim in (("comma", ","), ("semicolon", ";"), ("tab", "\t")):
        for row in fixed + generated:
            buf = io.StringIO(newline="")
            # CRLF terminator so that both CR and LF in a field force quoting; the
            # terminator is stripped below, the record text is what is compared.
            w = csv.writer(buf, delimiter=delim, quotechar='"', doublequote=True,
                           quoting=csv.QUOTE_MINIMAL, lineterminator="\r\n")
            w.writerow(row)
            text = buf.getvalue()
            assert text.endswith("\r\n")
            record = text[:-2]
            back = list(csv.reader(io.StringIO(record, newline=""), delimiter=delim, strict=True))
            assert back == [row], (row, record, back)
            cases.append({"delimiter": name, "fields": row, "record": record})
    return cases


def outputs():
    a = identity(HERE / "user_map.json")
    _, c1 = canonical(HERE / "user_map.json")
    _, c2 = canonical(HERE / "user_map_reordered.json")
    assert c1 == c2, "reordered map must have the same canonical bytes"
    builtins = []
    for ref, name in (("fid.results.long@1", "long.json"), ("fid.results.wide@1", "wide.json")):
        entry = {"reference": ref}
        entry.update(identity(BUILTIN_DIR / name))
        entry["lossy"] = lossy(BUILTIN_DIR / name)
        builtins.append(entry)
    return {
        "user_map.expected.json": dump(a),
        "builtin.expected.json": dump(builtins),
        "lossy.expected.json": dump({"export_lossy.json": lossy(HERE / "export_lossy.json")}),
        "dialect_cases.json": dump(dialect_cases()),
    }


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale))
        sys.exit(1)


if __name__ == "__main__":
    main()
