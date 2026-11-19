#!/usr/bin/env python3
"""Independent reference for GDT.RES.SCHEMA fixtures (stdlib only).

Writes, from the row data below:

- `synthetic_table_v1.json`: the synthetic DMT table, rows in key order (AC-1);
- `synthetic_table_v1_shuffled.json`: the same rows in a fixed shuffled order (AC-2);
- `synthetic_table_v1.digest`: the table digest, `sha256:` + hex of SHA-256 over the RFC 8785
  (JCS) canonical JSON of `{rows: [rows by key], schema, version}`, computed here with this
  script's own serializer, not with the Rust code under test;
- `synthetic_table_v1.rows.json`: per-row and per-chain digests, computed the same way.

With `--check` it recomputes everything and fails if any file differs.

A `fid.dmt-table/1` document holds only objects, arrays, strings and booleans (no JSON
numbers), so the canonicalizer needs nothing else. Object members sort by UTF-16 code units
(all names here are ASCII); strings escape `"`, `\\` and control characters, with the short
forms where RFC 8785 has them and lower-case `\\u00xx` otherwise.

All rows are synthetic placeholders: characteristic names are real, but every clause number,
operator choice, algorithm ID and parameter is invented, and no standards text appears
anywhere. CC0.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def jcs_string(s):
    out = ['"']
    for ch in s:
        if ch in SHORT:
            out.append(SHORT[ch])
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def utf16_key(s):
    return s.encode("utf-16-be")


def jcs(v):
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        items = sorted(v.items(), key=lambda kv: utf16_key(kv[0]))
        return "{" + ",".join(jcs_string(k) + ":" + jcs(x) for k, x in items) + "}"
    raise ValueError(f"value kind not allowed in a DMT table: {v!r}")


def sha(text):
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


ONE = "0x3ff0000000000000"  # IEEE-754 bits of 1.0
HALF = "0x3fe0000000000000"  # IEEE-754 bits of 0.5
NOTE = "Synthetic placeholder row for schema tests; not an interpretation of any standard."


def op(kind, alg, **params):
    return {"kind": kind, "alg": alg, "params": dict(sorted(params.items()))}


def form_chain(variant, method="min_zone"):
    return {
        "variant": variant,
        "steps": [
            op("partition", "syn.partition.whole_feature@0.1.0"),
            op("extraction", "syn.extract.all_points@0.1.0", min_points="3"),
            op("association", f"syn.assoc.{method}@0.1.0", constrained=False, scale=ONE),
            op("evaluation", "syn.eval.peak_to_valley@0.1.0"),
        ],
    }


def located_chain(variant):
    return {
        "variant": variant,
        "steps": [
            op("partition", "syn.partition.whole_feature@0.1.0"),
            op("extraction", "syn.extract.all_points@0.1.0", min_points="4"),
            op("association", "syn.assoc.min_zone@0.1.0", constrained=True, scale=ONE),
            op("construction", f"syn.construct.{variant}@0.1.0", fraction=HALF),
            op("evaluation", "syn.eval.max_deviation@0.1.0", sign="-1"),
        ],
    }


def asme(char, mods, variants, default, status="supported", limitations=(), review=None, ors=True):
    key = "dmt:asme/%s/%s@1" % (char, "+".join(mods) if mods else "none")
    return {
        "key": key,
        "dialect": "asme",
        "editions": ["y14.5-2018"],
        "characteristic": char,
        "modifiers": list(mods),
        "clauses": [{"designator": "asme-y14.5", "edition": "2018", "clause": "90.%d" % (len(char) % 9 + 1)}],
        "variants": variants,
        "default_variant": default,
        "actual_value": f"av.synthetic.{char}@1",
        "decision_rule_hint": "dr.synthetic.default@1",
        "ors_cases": [f"ors:synthetic/{char}.001"] if ors else [],
        "status": status,
        "limitations": list(limitations),
        "review": review or {"state": "draft"},
        "notes": NOTE,
        "synthetic": True,
    }


def iso_stub(char):
    return {
        "key": f"dmt:iso/{char}/none@1",
        "dialect": "iso",
        "editions": ["1101-2017"],
        "characteristic": char,
        "modifiers": [],
        "clauses": [{"designator": "iso-1101", "edition": "2017", "clause": "1"}],
        "variants": [{"variant": "placeholder", "steps": []}],
        "default_variant": "placeholder",
        "actual_value": "av.synthetic.placeholder@1",
        "decision_rule_hint": "dr.synthetic.default@1",
        "ors_cases": [],
        "status": "unsupported",
        "limitations": ["placeholder_no_interpretation"],
        "review": {"state": "draft"},
        "notes": "",
        "synthetic": True,
    }


SIGNED = {"state": "signed_off", "reviewers": ["rev-a", "rev-b"]}
UNDER = {"state": "under_review"}

ROWS = [
    asme("angularity", [], [located_chain("surface")], "surface"),
    asme("circular_runout", [], [form_chain("default")], "default", review=UNDER),
    asme("circularity", [], [form_chain("default")], "default", review=SIGNED),
    asme("concentricity", [], [{"variant": "placeholder", "steps": []}], "placeholder",
         status="unsupported", limitations=["placeholder_no_interpretation"], ors=False),
    asme("cylindricity", [], [form_chain("default")], "default"),
    asme("flatness", [], [form_chain("default")], "default", review=SIGNED),
    asme("parallelism", [], [located_chain("surface")], "surface"),
    asme("perpendicularity", [], [located_chain("axis"), located_chain("surface")], "surface",
         status="partial", limitations=["bestfit_disallowed:axis"]),
    asme("position", [], [located_chain("axis"), located_chain("surface")], "surface", review=SIGNED),
    asme("position", ["mmc"], [located_chain("axis"), located_chain("surface")], "axis"),
    asme("position", ["cz", "mmc"], [located_chain("axis"), located_chain("surface")], "axis",
         status="partial", limitations=["pattern_size_limited", "zone_def_unsupported:nonuniform"], review=UNDER),
    asme("profile_of_a_line", [], [{"variant": "placeholder", "steps": []}], "placeholder",
         status="unsupported", limitations=["placeholder_no_interpretation"], ors=False),
    asme("profile_of_a_surface", [], [located_chain("surface")], "surface"),
    asme("size", [], [form_chain("global", "least_squares"), form_chain("local", "two_point")], "local"),
    asme("straightness", [], [form_chain("default")], "default"),
    asme("total_runout", [], [form_chain("default")], "default"),
    iso_stub("cylindricity"),
    iso_stub("flatness"),
    iso_stub("position"),
    iso_stub("roundness"),
]
ROWS.sort(key=lambda r: r["key"].encode("utf-8"))
# A fixed permutation for AC-2 (not a random shuffle, so the file is stable).
SHUFFLE = [7, 19, 2, 13, 0, 16, 5, 11, 18, 3, 9, 14, 1, 17, 6, 12, 4, 15, 10, 8]
VERSION = "1.0.0"


def table(rows):
    return {"schema": "fid.dmt-table/1", "version": VERSION, "rows": rows}


def outputs():
    assert sorted(SHUFFLE) == list(range(len(ROWS)))
    keys = [r["key"] for r in ROWS]
    assert len(set(keys)) == len(keys)
    pretty = json.dumps(table(ROWS), indent=2, ensure_ascii=False) + "\n"
    shuffled = json.dumps(table([ROWS[i] for i in SHUFFLE]), indent=2, ensure_ascii=False) + "\n"
    digest = sha(jcs(table(ROWS))) + "\n"
    per_row = {
        r["key"]: {
            "row": sha(jcs(r)),
            "chains": {c["variant"]: sha(jcs(c)) for c in r["variants"]},
        }
        for r in ROWS
    }
    rows_doc = json.dumps(per_row, indent=2, sort_keys=True) + "\n"
    return {
        "synthetic_table_v1.json": pretty,
        "synthetic_table_v1_shuffled.json": shuffled,
        "synthetic_table_v1.digest": digest,
        "synthetic_table_v1.rows.json": rows_doc,
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode("utf-8"):
                bad.append(name)
        else:
            path.write_bytes(text.encode("utf-8"))
    if bad:
        print("stale fixtures: " + ", ".join(bad) + " (run fixtures/gdt/dmt/make_fixtures.py)")
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
