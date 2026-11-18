#!/usr/bin/env python3
"""Validate KNP-1 findings reports against schema/knp1-findings-v1.json (KRN.NUM.LINT.FIND).

Every expected report under fixtures/knp1/find/ (report-v1/expected-report.json and
waive-v1/expected-report*.json) must pass, every file in
fixtures/knp1/find/schema-invalid/ must fail, and every extra path given on the
command line (e.g. a real knp1-findings.json) must pass.

Needs: pip install jsonschema
Usage: python tools/check_knp1_findings.py [extra.json ...]
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / "fixtures" / "knp1" / "find"


def main() -> int:
    schema = json.loads((ROOT / "schema" / "knp1-findings-v1.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    bad = 0
    valid = sorted(FIX.glob("report-v1/expected-report*.json")) + sorted(FIX.glob("waive-v1/expected-report*.json"))
    invalid = sorted((FIX / "schema-invalid").glob("*.json"))
    if not valid or not invalid:
        print("FAIL: fixture files not found")
        return 1
    for f in valid + [Path(p) for p in sys.argv[1:]]:
        errs = list(v.iter_errors(json.loads(f.read_text(encoding="utf-8"))))
        if errs:
            bad += 1
            print(f"FAIL {f}: should validate: {errs[0].message}")
    for f in invalid:
        if v.is_valid(json.loads(f.read_text(encoding="utf-8"))):
            bad += 1
            print(f"FAIL {f}: should NOT validate")
    print(f"knp1 findings schema check ({len(valid)} valid, {len(invalid)} invalid):", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
