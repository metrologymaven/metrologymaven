#!/usr/bin/env python3
"""Validate command-definition fixtures against schema/command-def.schema.json
(CORE.REG.DEF.FIELD AC-1, REQ-010): every valid definition passes, every invalid
case fails. Needs: pip install jsonschema
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / "fixtures" / "core" / "reg" / "def" / "field"


def main() -> int:
    schema = json.loads((ROOT / "schema" / "command-def.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    bad = 0
    for i, d in enumerate(json.loads((FIX / "defs_valid.json").read_text(encoding="utf-8"))):
        errs = list(v.iter_errors(d))
        if errs:
            bad += 1
            print(f"FAIL defs_valid[{i}]: {errs[0].message}")
    for f in sorted((FIX / "defs_invalid").glob("*.json")):
        if v.is_valid(json.loads(f.read_text(encoding="utf-8"))["def"]):
            bad += 1
            print(f"FAIL {f.name}: should NOT validate")
    print("command-def schema check:", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
