#!/usr/bin/env python3
"""GDT.LOC.DERIV.TYPES AC-5: validate the serialised DerivedElement examples against the
exported JSON Schema (`crates/fid-gps/schema/derived-element-v1.schema.json`) with an
independent JSON Schema 2020-12 validator.

The schema itself must be a valid 2020-12 schema; every example of `deriv_elements.json` (all
four variants) and the AC-1 expected record of `deriv_stub_points.json` must validate; each
hand-labelled mutation below must not.

Needs: pip install jsonschema
Usage: python fixtures/gdt/loc/check_deriv_schema.py
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[3]
SCHEMA = ROOT / "crates" / "fid-gps" / "schema" / "derived-element-v1.schema.json"
HERE = Path(__file__).resolve().parent


def put(path, value):
    def f(doc):
        *parents, last = path
        node = doc
        for p in parents:
            node = node[p]
        node[last] = value
    return f


def drop(path):
    def f(doc):
        *parents, last = path
        node = doc
        for p in parents:
            node = node[p]
        del node[last]
    return f


# (example name, description, mutation): every one must be rejected.
INVALID = [
    ("axis_flagged", "unknown element kind", put(["element", "kind"], "datum_axis")),
    ("axis_flagged", "axis without extent", drop(["element", "extent"])),
    ("axis_flagged", "axis with points", put(["element", "points"], [])),
    ("axis_flagged", "float as JSON number", put(["element", "extent"], 0.03)),
    ("axis_flagged", "float in upper-case hex", put(["element", "extent"], "0x3F9EB851EB851EB8")),
    ("axis_flagged", "float with 15 hex digits", put(["element", "extent"], "0x3f9eb851eb851eb")),
    ("axis_flagged", "quiet NaN", put(["element", "extent"], "0x7ff8000000000000")),
    ("axis_flagged", "negative NaN", put(["element", "extent"], "0xfff8000000000001")),
    ("axis_flagged", "positive infinity", put(["element", "point", 0], "0x7ff0000000000000")),
    ("axis_flagged", "negative infinity", put(["element", "direction", 2], "0xfff0000000000000")),
    ("axis_flagged", "point with two coordinates", put(["element", "point"], ["0x0000000000000000"] * 2)),
    ("axis_flagged", "point with four coordinates", put(["element", "point"], ["0x0000000000000000"] * 4)),
    ("axis_flagged", "unknown flag", put(["flags"], ["suspect_axis"])),
    ("axis_flagged", "duplicate flag", put(["flags"], ["local_optimum_only", "local_optimum_only"])),
    ("axis_flagged", "flags missing", drop(["flags"])),
    ("axis_flagged", "algorithm ID without version", put(["alg"], "syn.axis.example")),
    ("axis_flagged", "algorithm ID with two segments", put(["alg"], "syn.axis@1.0.0")),
    ("axis_flagged", "algorithm ID upper case", put(["alg"], "Syn.axis.example@1.0.0")),
    ("axis_flagged", "schema 2", put(["schema"], "fid.derived-element/2")),
    ("axis_flagged", "unknown top-level field", put(["chain_digest"], "x")),
    ("median_line", "median line of one point", put(["element", "points"], [["0x0000000000000000"] * 3])),
    ("median_line", "median line with normals", put(["element", "normals"], [])),
    ("median_surface_normals", "empty normals", put(["element", "normals"], [])),
    ("median_surface_normals", "normal with a NaN", put(["element", "normals", 0, 1], "0x7ff0000000000001")),
    ("median_surface_plain", "median surface without points", put(["element", "points"], [])),
    ("median_points", "median points without points", drop(["element", "points"])),
    ("median_points", "median points kind mislabelled", put(["element", "kind"], "median_point")),
]

# Finite values at the edges of the float pattern: every one must be accepted.
FINITE_EDGES = [
    "0x7fefffffffffffff",  # largest finite
    "0xffefffffffffffff",  # most negative finite
    "0x0000000000000001",  # smallest subnormal
    "0x8000000000000000",  # negative zero
    "0x7fe0000000000000",
    "0xffe0000000000000",
    "0x7f00000000000000",
]


def main():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    failures = 0

    elements = json.loads((HERE / "deriv_elements.json").read_text(encoding="utf-8"))
    by_name = {e["name"]: e["record"] for e in elements["examples"]}
    kinds = {r["element"]["kind"] for r in by_name.values()}
    if kinds != {"axis", "median_line", "median_surface", "median_points"}:
        print(f"examples do not cover all four variants: {sorted(kinds)}", file=sys.stderr)
        failures += 1
    stub = json.loads((HERE / "deriv_stub_points.json").read_text(encoding="utf-8"))
    for name, rec in list(by_name.items()) + [("stub_expected", stub["expected"])]:
        errs = list(v.iter_errors(rec))
        if errs:
            print(f"{name}: expected valid, got {errs[0].message}", file=sys.stderr)
            failures += 1

    for name, what, mutate in INVALID:
        doc = copy.deepcopy(by_name[name])
        mutate(doc)
        if v.is_valid(doc):
            print(f"{name}: mutation '{what}' was accepted", file=sys.stderr)
            failures += 1

    for bits in FINITE_EDGES:
        doc = copy.deepcopy(by_name["axis_flagged"])
        doc["element"]["extent"] = bits
        if not v.is_valid(doc):
            print(f"finite value {bits} was rejected", file=sys.stderr)
            failures += 1

    if failures:
        print(f"{failures} schema check(s) failed", file=sys.stderr)
        return 1
    print(f"derived-element schema: {len(by_name) + 1} valid, {len(INVALID)} invalid, {len(FINITE_EDGES)} edges: ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
