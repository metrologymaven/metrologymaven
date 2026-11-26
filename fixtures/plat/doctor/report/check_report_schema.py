#!/usr/bin/env python3
"""PLAT.DOCTOR.REPORT AC-1 and AC-5: validate the report fixtures against
`platform/doctor-report.schema.json` with an independent JSON Schema 2020-12 validator.

The schema must itself be a valid 2020-12 schema. Every golden report must validate
(report-win11.json, report-escape.json, report/identifying*.json and every report of
report/cases.json); report-schema99.json must not; each case of report/invalid_cases.json
must validate exactly when its hand label `schema_valid` says so (the parser is stricter
than the schema in a few labelled cases).

Needs: pip install jsonschema
Usage: python fixtures/plat/doctor/report/check_report_schema.py
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[4]
DOCTOR = ROOT / "fixtures" / "plat" / "doctor"
HERE = Path(__file__).resolve().parent


def no_dup(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise ValueError(f"duplicate key {k!r}")
        d[k] = v
    return d


def load(path):
    return json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=no_dup)


def main():
    schema = load(ROOT / "platform" / "doctor-report.schema.json")
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    failures = []

    def expect(name, doc, valid):
        errors = list(v.iter_errors(doc))
        if valid and errors:
            failures.append(f"{name}: expected valid, got {errors[0].message[:200]}")
        if not valid and not errors:
            failures.append(f"{name}: expected invalid, schema accepts it")

    for name in ["report-win11.json", "report-escape.json"]:
        expect(name, load(DOCTOR / name), True)
    for name in ["identifying.json", "identifying.redacted.json"]:
        expect(name, load(HERE / name), True)
    expect("report-schema99.json", load(DOCTOR / "report-schema99.json"), False)
    for case in load(HERE / "cases.json"):
        expect(f"cases.json:{case['name']}", case["report"], True)
    for case in load(HERE / "invalid_cases.json"):
        expect(f"invalid_cases.json:{case['name']}", case["report"], case["schema_valid"])

    if failures:
        for f in failures:
            print(f, file=sys.stderr)
        sys.exit(1)
    print("doctor report schema: all fixtures as labelled")


if __name__ == "__main__":
    main()
