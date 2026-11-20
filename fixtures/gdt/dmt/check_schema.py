#!/usr/bin/env python3
"""GDT.RES.SCHEMA AC-6: validate the synthetic DMT table fixtures against the exported JSON
Schema (`crates/fid-gps/schema/dmt-table-v1.schema.json`) with an independent JSON Schema
2020-12 validator.

The schema itself must be a valid 2020-12 schema; both fixtures (key order and shuffled) must
validate; each hand-labelled mutation below must not.

Needs: pip install jsonschema
Usage: python fixtures/gdt/dmt/check_schema.py
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[3]
SCHEMA = ROOT / "crates" / "fid-gps" / "schema" / "dmt-table-v1.schema.json"
HERE = Path(__file__).resolve().parent
FIXTURES = ["synthetic_table_v1.json", "synthetic_table_v1_shuffled.json"]


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


R0 = ["rows", 0]
OP0 = R0 + ["variants", 0, "steps", 0]

INVALID = [
    ("operator kind smoothing (AC-5)", put(OP0 + ["kind"], "smoothing")),
    ("operator kind upper case", put(OP0 + ["kind"], "Partition")),
    ("algorithm ID without version", put(OP0 + ["alg"], "syn.partition.whole_feature")),
    ("parameter as JSON number", put(OP0 + ["params", "x"], 1.5)),
    ("parameter NaN-looking hex in upper case", put(OP0 + ["params", "x"], "0x7FF8000000000000")),
    ("key with upper-case dialect", put(R0 + ["key"], "dmt:ASME/angularity/none@1")),
    ("key version 0", put(R0 + ["key"], "dmt:asme/angularity/none@0")),
    ("unknown row field", put(R0 + ["comment"], "x")),
    ("row field missing", drop(R0 + ["review"])),
    ("unknown table field", put(["extra"], True)),
    ("schema 2", put(["schema"], "fid.dmt-table/2")),
    ("notes of 281 characters", put(R0 + ["notes"], "x" * 281)),
    ("notes with a control character", put(R0 + ["notes"], "a\tb")),
    ("status unknown", put(R0 + ["status"], "maybe")),
    ("signed off without reviewers", put(R0 + ["review"], {"state": "signed_off"})),
    ("draft with reviewers", put(R0 + ["review"], {"state": "draft", "reviewers": ["a"]})),
    ("reviewer upper case", put(R0 + ["review"], {"state": "signed_off", "reviewers": ["Rev-a"]})),
    ("clause with free text", put(R0 + ["clauses", 0, "clause"], "see the figure")),
    ("clause with an extra field", put(R0 + ["clauses", 0, "text"], "x")),
    ("no variants", put(R0 + ["variants"], [])),
    ("version as number", put(["version"], 1)),
]


def main():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    failures = []
    for name in FIXTURES:
        doc = json.loads((HERE / name).read_text(encoding="utf-8"))
        errors = list(v.iter_errors(doc))
        if errors:
            failures.append(f"{name} should validate: {errors[0].message}")
    base = json.loads((HERE / FIXTURES[0]).read_text(encoding="utf-8"))
    for label, mutate in INVALID:
        doc = copy.deepcopy(base)
        mutate(doc)
        if v.is_valid(doc):
            failures.append(f"mutation should not validate: {label}")
    for f in failures:
        print(f)
    if failures:
        return 1
    print(f"ok: {len(FIXTURES)} fixtures valid, {len(INVALID)} mutations rejected")
    return 0


if __name__ == "__main__":
    sys.exit(main())
