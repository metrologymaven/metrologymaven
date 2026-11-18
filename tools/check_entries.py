#!/usr/bin/env python3
"""Validate journal entry headers against schema/entry.schema.json
(CORE.JRN.ENTRY REQ-011, AC-1): every header in fixtures/core/jrn/entry/vectors.json
must pass, every case in fixtures/core/jrn/entry/invalid_headers.json must fail.

Needs: pip install jsonschema
Usage: python tools/check_entries.py
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / "fixtures" / "core" / "jrn" / "entry"


def main() -> int:
    schema = json.loads((ROOT / "schema" / "entry.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    bad = 0
    vectors = json.loads((FIX / "vectors.json").read_text(encoding="utf-8"))
    for vec in vectors:
        errs = list(v.iter_errors(vec["header"]))
        if errs:
            bad += 1
            print(f"FAIL {vec['name']}: should validate: {errs[0].message}")
    for case in json.loads((FIX / "invalid_headers.json").read_text(encoding="utf-8")):
        if v.is_valid(case["header"]):
            bad += 1
            print(f"FAIL {case['name']}: should NOT validate")
    print(f"entry schema check ({len(vectors)} headers):", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
