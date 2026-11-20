#!/usr/bin/env python3
"""KRN.CONS.DECL AC-5: validate the sample construction command rows against the published JSON
Schema (`schemas/construct/command-row-1.schema.json`) with an independent JSON Schema 2020-12
validator.

The schema itself must be a valid 2020-12 schema; every sample row in
fixtures/cons/decl-toy-v1/canon.txt (`row_bytes`, `row_offset_bytes`, which the Rust tests
reproduce byte for byte) must validate; each hand-labelled mutation below must not.

Needs: pip install jsonschema
Usage: python fixtures/cons/check_row_schema.py
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[2]
SCHEMA = ROOT / "schemas" / "construct" / "command-row-1.schema.json"
CANON = Path(__file__).resolve().parent / "decl-toy-v1" / "canon.txt"
ROW_KEYS = ("row_bytes", "row_offset_bytes")


def rows():
    out = {}
    for line in CANON.read_text(encoding="utf-8").splitlines():
        key, _, value = line.partition("=")
        if key in ROW_KEYS:
            out[key] = json.loads(value)
    assert sorted(out) == sorted(ROW_KEYS), out.keys()
    return out


def field(row, list_name, name):
    return next(f for f in row[list_name] if f["name"] == name)


def mutations(row):
    """(label, mutate) pairs; each mutated row must be rejected."""

    def missing_unit(r):
        del field(r, "params", "thresholds.distance")["x-unit"]

    def missing_quantity(r):
        del field(r, "result", "element.p")["x-quantity"]

    def wrong_unit_length(r):
        field(r, "params", "thresholds.distance")["x-unit"] = "rad"

    def wrong_unit_angle(r):
        field(r, "params", "thresholds.angular")["x-unit"] = "m"

    def wrong_unit_count(r):
        field(r, "params", "params.branch")["x-unit"] = "m"

    def quantity_on_string(r):
        f = field(r, "params", "params.rule")
        f["x-quantity"] = "length"
        f["x-unit"] = "m"

    def quantity_on_element_ref(r):
        f = field(r, "result", "inputs")
        f["x-quantity"] = "count"
        f["x-unit"] = "1"

    def unit_alone_on_boolean(r):
        field(r, "params", "params.strict")["x-unit"] = "1"

    def unknown_quantity(r):
        field(r, "result", "element.n")["x-quantity"] = "mass"

    def side_effect(r):
        r["side_effect"] = "W1"

    def dry_run(r):
        r["dry_run"] = "R0"

    def observed(r):
        r["determinism"] = "N"

    def fit_command(r):
        r["command"] = "fit.ls_plane@1"

    def bad_algorithm(r):
        r["algorithms"] = ["fit.plane.ls@1.0.0"]

    def no_algorithm(r):
        r["algorithms"] = []

    def extra_key(r):
        r["notes"] = "x"

    return [
        ("missing unit", missing_unit),
        ("missing quantity", missing_quantity),
        ("wrong unit: length in rad", wrong_unit_length),
        ("wrong unit: angle in m", wrong_unit_angle),
        ("wrong unit: count in m", wrong_unit_count),
        ("quantity on a string field", quantity_on_string),
        ("quantity on an element_ref_array field", quantity_on_element_ref),
        ("unit alone on a boolean field", unit_alone_on_boolean),
        ("unknown quantity kind", unknown_quantity),
        ("side effect not W2", side_effect),
        ("dry run not R1", dry_run),
        ("determinism N", observed),
        ("non-construct command", fit_command),
        ("non-construct algorithm", bad_algorithm),
        ("no algorithm", no_algorithm),
        ("unknown top-level key", extra_key),
    ]


def main():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    failures = 0
    samples = rows()
    for key, row in samples.items():
        errs = list(v.iter_errors(row))
        if errs:
            print(f"FAIL {key}: valid sample rejected: {errs[0].message}")
            failures += 1
        else:
            print(f"ok   {key}: valid")
    # The offset row has every field the mutations touch.
    base = samples["row_offset_bytes"]
    for label, mutate in mutations(base):
        doc = copy.deepcopy(base)
        mutate(doc)
        if v.is_valid(doc):
            print(f"FAIL {label}: invalid row accepted")
            failures += 1
        else:
            print(f"ok   {label}: rejected")
    if failures:
        sys.exit(1)


if __name__ == "__main__":
    main()
