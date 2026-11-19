#!/usr/bin/env python3
"""Validate the CORE.REG.DEF.SCHEMA fixtures against schemas/registry-keywords.schema.json
with an independent JSON Schema 2020-12 implementation (AC-1): every valid schema passes;
every invalid schema whose fault the meta-schema can see fails. The faults that need the
unit catalogue or the depth limit (unknown_quantity, angle_unit_on_length, depth_65) are
listed as not decidable by the meta-schema; depth_65 is not run through the meta-schema at
all, because jsonschema's unevaluatedProperties evaluation is exponential in nesting depth.
Needs: pip install jsonschema
"""

import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / "fixtures" / "core" / "reg" / "def" / "schema"
CATALOGUE_OR_LIMIT = {"unknown_quantity", "angle_unit_on_length", "depth_65"}


def main() -> int:
    meta = json.loads((ROOT / "schemas" / "registry-keywords.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(meta)
    v = Draft202012Validator(meta)
    bad = 0
    for f in sorted((FIX / "valid").glob("*.json")):
        errs = list(v.iter_errors(json.loads(f.read_text(encoding="utf-8"))))
        if errs:
            bad += 1
            print(f"FAIL valid/{f.name}: {errs[0].message}")
    for f in sorted((FIX / "invalid").glob("*.json")):
        if f.stem == "depth_65":
            print("note invalid/depth_65.json: depth limit only, not run (see docstring)")
            continue
        ok = v.is_valid(json.loads(f.read_text(encoding="utf-8"))["schema"])
        if f.stem in CATALOGUE_OR_LIMIT:
            print(f"note invalid/{f.name}: needs the catalogue or the depth limit (meta-schema says {'valid' if ok else 'invalid'})")
        elif ok:
            bad += 1
            print(f"FAIL invalid/{f.name}: should NOT validate")
    print("registry-keywords meta-schema check:", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
