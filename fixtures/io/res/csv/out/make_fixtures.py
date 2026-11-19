"""Independent reference for the IO.RES.CSV.OUT fixtures (spec/io/res/csv/out/NODE.md).

Stdlib only, no Rust involved. It re-implements the export rules from the spec and the
crate docs in plain Python and writes the expected tables with Python's own `csv` module
(QUOTE_MINIMAL, doubled quotes):

- rows sorted by characteristic ID, then part ID (absent first), then result ID, by
  code point (equal to UTF-8 byte order);
- decimal fields: the authored text, `.` replaced by the map's decimal mark only;
- text and enumeration cells (and header cells) starting with = + - @ TAB CR get a
  leading apostrophe (REQ-IO.RES.CSV.OUT-003);
- columns: a column with an `index` sits at that index, the others fill the lowest free
  places in map order, places left over are empty; an index-only column is headed with
  its field name;
- wide layout: one row per part, part-scope columns first, then one group per
  characteristic (characteristic-scope columns in map order) headed `<id>:<header>`;
- the map's line ending after every record, UTF-8 without BOM.

Outputs: r.golden.csv (fid.results.long@1, AC-1), r.wide.golden.csv (fid.results.wide@1),
r.semicolon.golden.csv (semicolon_comma.json, AC-4), r.no_decision_rule.golden.csv
(no_decision_rule.json, AC-2) and expected.json (lossy lists, row counts, SHA-256).

Run from the repository root:  python fixtures/io/res/csv/out/make_fixtures.py [--check]
"""

import csv
import hashlib
import io
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[4]
BUILTIN_DIR = REPO / "crates" / "fid-io-res" / "src" / "csv" / "map"

DECIMAL = {"nominal", "actual", "deviation", "lower_limit", "upper_limit"}
PART_SCOPE = {"part_id", "timestamp_text", "source_software", "source_version"}
LOSSY_CHECK = ["evaluation_basis", "uec", "decision_rule", "record_status", "source_software"]
TRIGGERS = ("=", "+", "-", "@", "\t", "\r")
DELIMS = {"comma": ",", "semicolon": ";", "tab": "\t"}
TERMS = {"lf": "\n", "crlf": "\r\n"}
DECIMAL_RE = re.compile(r"[+-]?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?\Z")


def neutralise(text):
    return "'" + text if text.startswith(TRIGGERS) else text


def value(field, rec, mark):
    v = rec.get(field)
    if v is None:
        return ""
    if field in DECIMAL:
        assert DECIMAL_RE.match(v), v
        return v.replace(".", mark)
    return neutralise(v)


def header(col):
    return col.get("header", col["field"])


def place(cols):
    width = max([len(cols)] + [c["index"] + 1 for c in cols if "index" in c])
    slots = [None] * width
    for c in cols:
        if "index" in c:
            slots[c["index"]] = c
    free = 0
    for c in cols:
        if "index" not in c:
            while slots[free] is not None:
                free += 1
            slots[free] = c
    return slots


def write_row(cells, delim, term):
    buf = io.StringIO(newline="")
    # CRLF terminator so that CR or LF in a field forces quoting; replaced below.
    csv.writer(buf, delimiter=delim, quotechar='"', doublequote=True,
               quoting=csv.QUOTE_MINIMAL, lineterminator="\r\n").writerow(cells)
    text = buf.getvalue()
    assert text.endswith("\r\n")
    return text[:-2] + term


def render(records, cmap):
    d = cmap["dialect"]
    delim, term, mark = DELIMS[d["delimiter"]], TERMS[d["line_ending"]], d["decimal_mark"]
    recs = sorted(records, key=lambda r: (r["characteristic_id"], r.get("part_id") or "", r["result_id"]))
    # Maintainer decision: a map without record_status may not carry transcribed results.
    if not any(c["field"] == "record_status" for c in cmap["columns"]):
        assert not any(r.get("record_status") == "transcribed" for r in recs), "transcription_status_missing"
    out = []
    if cmap["layout"] == "long":
        slots = place(cmap["columns"])
        if d["header"]:
            out.append(write_row([neutralise(header(c)) if c else "" for c in slots], delim, term))
        for r in recs:
            out.append(write_row([value(c["field"], r, mark) if c else "" for c in slots], delim, term))
        rows = len(recs)
    else:
        part_cols = [c for c in cmap["columns"] if c["field"] in PART_SCOPE]
        group = [c for c in cmap["columns"] if c["field"] not in PART_SCOPE]
        slots = place(part_cols)
        chars = sorted({r["characteristic_id"] for r in recs})
        parts = sorted({r.get("part_id") or "" for r in recs})
        if d["header"]:
            cells = [neutralise(header(c)) if c else "" for c in slots]
            cells += [neutralise(ch + ":" + header(c)) for ch in chars for c in group]
            out.append(write_row(cells, delim, term))
        for p in parts:
            mine = {r["characteristic_id"]: r for r in recs if (r.get("part_id") or "") == p}
            cells = []
            for c in slots:
                if c is None:
                    cells.append("")
                    continue
                vals = {r.get(c["field"]) for r in mine.values()} - {None, ""}
                assert len(vals) <= 1, (p, c["field"], vals)
                cells.append(value(c["field"], {c["field"]: vals.pop()} if vals else {}, mark))
            for ch in chars:
                r = mine.get(ch, {})
                cells += [value(c["field"], r, mark) for c in group]
            out.append(write_row(cells, delim, term))
        rows = len(parts)
    return "".join(out).encode("utf-8"), rows


def lossy(cmap):
    fields = {c["field"] for c in cmap["columns"]}
    return [f for f in LOSSY_CHECK if f not in fields]


def load(path):
    return json.loads(path.read_bytes().decode("utf-8"))


def outputs():
    records = [{k: v for k, v in r.items() if v is not None} for r in load(HERE / "records.json")["records"]]
    cases = [
        ("long", BUILTIN_DIR / "long.json", "r.golden.csv"),
        ("wide", BUILTIN_DIR / "wide.json", "r.wide.golden.csv"),
        ("semicolon", HERE / "semicolon_comma.json", "r.semicolon.golden.csv"),
        ("no_decision_rule", HERE / "no_decision_rule.json", "r.no_decision_rule.golden.csv"),
    ]
    files = {}
    expected = {}
    for name, map_path, golden in cases:
        cmap = load(map_path)
        data, rows = render(records, cmap)
        files[golden] = data
        expected[name] = {
            "golden": golden,
            "lossy": lossy(cmap),
            "rows": rows,
            "sha256": "sha256:" + hashlib.sha256(data).hexdigest(),
            "bytes": len(data),
        }
    files["expected.json"] = (json.dumps(expected, indent=2, sort_keys=True) + "\n").encode("ascii")
    return files


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
