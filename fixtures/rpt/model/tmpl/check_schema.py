#!/usr/bin/env python3
"""RPT.MODEL.TMPL.FORMAT AC-5: validate the template fixtures against the published JSON
Schema (`schemas/report-template.v1.json`) with an off-the-shelf JSON Schema 2020-12
validator.

The schema must itself be a valid 2020-12 schema; valid_v1.json, its canonical form and the
migrated v0 sample must validate; unknown_field.json must not; and each case of
invalid_cases.json carries a hand-labelled schema verdict ("pass", "fail", or null for a
rule JSON Schema cannot express), which must hold.

Needs: pip install jsonschema
Usage: python fixtures/rpt/model/tmpl/check_schema.py
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SCHEMA = ROOT / "schemas" / "report-template.v1.json"


def no_duplicates(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError("duplicate key")
    return dict(pairs)


def load(text):
    return json.loads(text, object_pairs_hook=no_duplicates)


def main() -> int:
    schema = load(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    bad = 0

    def expect(name, doc, verdict):
        nonlocal bad
        ok = v.is_valid(doc)
        if ok != (verdict == "pass"):
            print(f"FAIL {name}: expected {verdict}, validator said {'pass' if ok else 'fail'}")
            bad += 1

    for f in ["valid_v1.json", "valid_v1.canonical.json", "v0_sample.migrated.json"]:
        expect(f, load((HERE / f).read_text(encoding="utf-8")), "pass")
    expect("unknown_field.json", load((HERE / "unknown_field.json").read_text(encoding="utf-8")), "fail")
    expect("v0_sample.json (not v1)", load((HERE / "v0_sample.json").read_text(encoding="utf-8")), "fail")

    cases = load((HERE / "invalid_cases.json").read_text(encoding="utf-8"))["cases"]
    checked = 0
    for c in cases:
        if c["schema"] is None:
            continue
        expect(c["name"], load(c["text"]), c["schema"])
        checked += 1
    print(f"report-template.v1.json: {checked + 5} documents checked, {bad} failures")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
