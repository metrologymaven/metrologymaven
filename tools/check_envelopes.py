#!/usr/bin/env python3
"""Validate result envelopes against schema/envelope-v1.json (AUTO.SURF.ENV AC-1).

Every file in fixtures/envelope/valid/ must pass, every file in
fixtures/envelope/invalid/ must fail, and every extra path given on the
command line (e.g. real `fid ... --json` output) must pass.

Needs: pip install jsonschema
Usage: python tools/check_envelopes.py [extra.json ...]
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent


def main() -> int:
    schema = json.loads((ROOT / "schema" / "envelope-v1.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    bad = 0
    for f in sorted((ROOT / "fixtures" / "envelope" / "valid").glob("*.json")) + [Path(p) for p in sys.argv[1:]]:
        errs = list(v.iter_errors(json.loads(f.read_text(encoding="utf-8"))))
        if errs:
            bad += 1
            print(f"FAIL {f}: should validate: {errs[0].message}")
    for f in sorted((ROOT / "fixtures" / "envelope" / "invalid").glob("*.json")):
        if v.is_valid(json.loads(f.read_text(encoding="utf-8"))):
            bad += 1
            print(f"FAIL {f}: should NOT validate")
    print("envelope schema check:", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
