#!/usr/bin/env python3
"""Independent top-level XSD declaration counter for IO.QIF.GEN.PARSE (AC-6).

Uses only Python's standard library (`tomllib` + `xml.etree.ElementTree`) — zero shared
code with `crates/fid-qif-gen` — to count top-level declarations across every pinned XSD
in a schema directory and compare or emit `qif3-counts.json`.

Usage:
  python fixtures/io/qif/gen/parse/count_decls.py [--dir <schema-dir>] [--out <json>] [--check]
"""

import argparse
import json
import sys
import tomllib
import xml.etree.ElementTree as ET
from pathlib import Path

HERE = Path(__file__).resolve().parent
XS_NS = "http://www.w3.org/2001/XMLSchema"

DECL_TAGS = {
    f"{{{XS_NS}}}complexType": "complex_types",
    f"{{{XS_NS}}}simpleType": "simple_types",
    f"{{{XS_NS}}}element": "elements",
    f"{{{XS_NS}}}attribute": "attributes",
    f"{{{XS_NS}}}group": "groups",
    f"{{{XS_NS}}}attributeGroup": "attribute_groups",
}


def count_schema_dir(schema_dir: Path) -> dict:
    manifest_path = schema_dir / "qif-xsd.manifest.toml"
    manifest = tomllib.loads(manifest_path.read_text(encoding="utf-8"))
    xsd_paths = sorted(row["path"] for row in manifest["file"] if row["role"] == "xsd")

    totals = {
        "complex_types": 0,
        "simple_types": 0,
        "elements": 0,
        "attributes": 0,
        "groups": 0,
        "attribute_groups": 0,
    }
    per_file: dict[str, dict[str, int]] = {}

    for rel in xsd_paths:
        tree = ET.parse(schema_dir / rel)
        root = tree.getroot()
        if root.tag != f"{{{XS_NS}}}schema":
            raise ValueError(f"{rel}: expected xs:schema root, got {root.tag}")
        file_counts = {k: 0 for k in totals}
        for child in root:
            key = DECL_TAGS.get(child.tag)
            if key is not None:
                file_counts[key] += 1
                totals[key] += 1
        file_counts["total"] = sum(file_counts.values())
        per_file[rel] = file_counts

    return {
        "schema": "qif-parse-counts@1",
        "source_rev": manifest["source"]["rev"],
        "files": xsd_paths,
        "counts": {**totals, "total": sum(totals.values())},
        "per_file": per_file,
    }


def render_json(data: dict) -> bytes:
    return (json.dumps(data, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--dir", type=Path, default=HERE / "qif3_bundle", help="Pinned schema directory")
    ap.add_argument("--out", type=Path, default=HERE / "qif3-counts.json", help="Output JSON path")
    ap.add_argument("--check", action="store_true", help="Verify --out matches recomputed counts")
    args = ap.parse_args()

    payload = render_json(count_schema_dir(args.dir))
    if args.check:
        if not args.out.exists():
            print(f"MISSING {args.out}", file=sys.stderr)
            return 1
        actual = args.out.read_bytes()
        if actual != payload:
            print(f"DRIFT {args.out}", file=sys.stderr)
            return 1
        return 0

    args.out.write_bytes(payload)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
