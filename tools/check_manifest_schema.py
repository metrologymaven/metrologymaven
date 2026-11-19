#!/usr/bin/env python3
"""Validate the CORE.REG.MAN golden manifests against schema/manifest.schema.json with an
independent JSON Schema 2020-12 implementation (AC-4). valid and order manifests must pass;
tampered.json also passes the schema (only its definition_digest is wrong, which a schema
cannot see; `fid registry show-manifest --check` refuses it), and a manifest with an extra
top-level field or a command without definition_digest must fail.
Needs: pip install jsonschema
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
FIX = ROOT / "fixtures" / "core" / "reg" / "manifest"


def main() -> int:
    schema = json.loads((ROOT / "schema" / "manifest.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    bad = 0
    docs = {}
    for name in ("valid.manifest.json", "order.manifest.json", "tampered.json"):
        docs[name] = json.loads((FIX / name).read_text(encoding="utf-8"))
        errs = list(v.iter_errors(docs[name]))
        if errs:
            bad += 1
            print(f"FAIL {name}: {errs[0].message}")
    extra = copy.deepcopy(docs["valid.manifest.json"])
    extra["built_at"] = "2026-11-19"
    no_digest = copy.deepcopy(docs["valid.manifest.json"])
    del no_digest["commands"][0]["definition_digest"]
    for label, doc in (("extra top-level field", extra), ("missing definition_digest", no_digest)):
        if v.is_valid(doc):
            bad += 1
            print(f"FAIL {label}: should NOT validate")
    print("manifest schema check:", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
