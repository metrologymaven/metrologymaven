#!/usr/bin/env python3
"""Independent stdlib-Python fixture & initial taxonomy generator for MIG.COVER.TAXO (CC0-1.0).

Writes:
- `taxonomies/{qif-project,dmis-output,csv-points,form-xlsx}.json`: initial launch-format
  construct taxonomies authored in original project wording.
- `fixtures/mig/cover/taxo/ok/`: valid taxonomies plus `dmis-conformance.json` extract (AC-1).
- `fixtures/mig/cover/taxo/dmis-missing/`: DMIS taxonomy omitting `SNSET` (AC-2).
- `fixtures/mig/cover/taxo/unknown-report/`: producer report naming `point_set.weighted` (AC-3).
- `fixtures/mig/cover/taxo/renamed/`: construct renamed with minor version bump only (AC-4).
- `fixtures/mig/cover/taxo/no-attest/`: taxonomy with empty reviewer attestation (AC-5).
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parent.parent.parent.parent
TAXO_DIR = REPO_ROOT / "taxonomies"


def canonical_bytes(obj: object) -> bytes:
    return (json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")


def attestation() -> dict[str, str]:
    return {
        "reviewer": "Metrology Maven maintainers",
        "date": "2026-11-22",
        "statement": "Construct descriptions authored in original project wording with no text copied from incumbent documentation.",
    }


def initial_taxonomies() -> dict[str, dict[str, object]]:
    qif_project = {
        "schema": "mig-taxonomy@1",
        "format_id": "qif-project",
        "version": "1.0.0",
        "reviewer_attestation": attestation(),
        "constructs": [
            {"id": "alignment.known_transform", "kind": "record", "description": "Explicit rigid coordinate transformation between named frames."},
            {"id": "characteristic.nominal", "kind": "record", "description": "Declared dimensional or geometric characteristic nominal and limits."},
            {"id": "characteristic.actual", "kind": "record", "description": "Reported measured value and status for a characteristic."},
            {"id": "datum.definition", "kind": "record", "description": "Datum label bound to one or more geometric features."},
            {"id": "feature.nominal", "kind": "record", "description": "Nominal geometric feature parameters and coordinate frame."},
            {"id": "feature.actual", "kind": "record", "description": "Evaluated geometric feature actual parameters and point references."},
            {"id": "point_set.record", "kind": "record", "description": "Ordered list of 3D measured coordinates in a declared frame."},
            {"id": "unit.declaration", "kind": "field", "description": "Length and angle unit specification for numeric values."},
        ],
    }

    dmis_output = {
        "schema": "mig-taxonomy@1",
        "format_id": "dmis-output",
        "version": "1.0.0",
        "reviewer_attestation": attestation(),
        "constructs": [
            {"id": "stmt.dmismn", "kind": "statement", "description": "Program header identifying the DMIS output stream.", "dmis_keyword": "DMISMN"},
            {"id": "stmt.units", "kind": "statement", "description": "Active length and angular unit declaration.", "dmis_keyword": "UNITS"},
            {"id": "stmt.feat", "kind": "statement", "description": "Nominal or actual geometric feature statement.", "dmis_keyword": "FEAT"},
            {"id": "stmt.tol", "kind": "statement", "description": "Tolerance definition or evaluated tolerance result statement.", "dmis_keyword": "TOL"},
            {"id": "stmt.datdef", "kind": "statement", "description": "Datum assignment associating a feature with a datum label.", "dmis_keyword": "DATDEF"},
            {"id": "stmt.snset", "kind": "statement", "description": "Sensor setting configuration statement.", "dmis_keyword": "SNSET"},
            {"id": "stmt.endfil", "kind": "statement", "description": "End-of-file terminator statement.", "dmis_keyword": "ENDFIL"},
        ],
    }

    csv_points = {
        "schema": "mig-taxonomy@1",
        "format_id": "csv-points",
        "version": "1.0.0",
        "reviewer_attestation": attestation(),
        "constructs": [
            {"id": "column.header", "kind": "layout", "description": "Header row naming exported coordinate or result columns."},
            {"id": "point_set.xyz", "kind": "record", "description": "Three-column Cartesian coordinate row in declared length units."},
            {"id": "point_set.ijk_normal", "kind": "field", "description": "Optional unit surface normal vector components per point."},
            {"id": "result.row", "kind": "record", "description": "Tabular characteristic value, limits, and reported status row."},
            {"id": "unit.column", "kind": "field", "description": "Column carrying per-row length or angle unit symbols."},
        ],
    }

    form_xlsx = {
        "schema": "mig-taxonomy@1",
        "format_id": "form-xlsx",
        "version": "1.0.0",
        "reviewer_attestation": attestation(),
        "constructs": [
            {"id": "cell.merged_header", "kind": "layout", "description": "Merged cell range forming a multi-column report header."},
            {"id": "cell.formula_summary", "kind": "field", "description": "Summary cell computing pass counts or statistics."},
            {"id": "sheet.characteristic_table", "kind": "layout", "description": "Repeating row block populated with per-characteristic results."},
            {"id": "sheet.part_metadata", "kind": "field", "description": "Header fields holding part identifier, lot, and inspection date."},
        ],
    }

    return {
        "qif-project": qif_project,
        "dmis-output": dmis_output,
        "csv-points": csv_points,
        "form-xlsx": form_xlsx,
    }


def dmis_conformance_extract() -> dict[str, object]:
    keywords = ["DATDEF", "DMISMN", "ENDFIL", "FEAT", "SNSET", "TOL", "UNITS"]
    return {
        "schema": "fid.dmis-conformance",
        "version": 1,
        "standard": "DMIS 5.3",
        "module_levels": [],
        "statements": [
            {"keyword": kw, "import": "full", "export": "none", "module_levels": [], "tests": []}
            for kw in keywords
        ],
    }


def expected_files() -> dict[Path, bytes]:
    taxos = initial_taxonomies()
    files: dict[Path, bytes] = {}

    # Committed initial taxonomies under taxonomies/ and fixtures/mig/cover/taxo/ok/
    for fmt, doc in taxos.items():
        data = canonical_bytes(doc)
        files[TAXO_DIR / f"{fmt}.json"] = data
        files[HERE / "ok" / f"{fmt}.json"] = data

    files[HERE / "ok" / "dmis-conformance.json"] = canonical_bytes(dmis_conformance_extract())

    # AC-2: dmis-missing/ lacks SNSET
    dmis_missing = json.loads(json.dumps(taxos["dmis-output"]))
    dmis_missing["constructs"] = [c for c in dmis_missing["constructs"] if c.get("dmis_keyword") != "SNSET"]
    files[HERE / "dmis-missing" / "dmis-output.json"] = canonical_bytes(dmis_missing)

    # AC-3: unknown-report/ names point_set.weighted (not in csv-points)
    files[HERE / "unknown-report" / "csv-mapping-report.json"] = canonical_bytes({
        "format_id": "csv-points",
        "constructs": ["point_set.xyz", "point_set.weighted"],
    })

    # AC-4: renamed/ renames point_set.xyz -> point_set.cartesian with minor bump 1.1.0 only
    renamed = json.loads(json.dumps(taxos["csv-points"]))
    renamed["version"] = "1.1.0"
    for c in renamed["constructs"]:
        if c["id"] == "point_set.xyz":
            c["id"] = "point_set.cartesian"
    files[HERE / "renamed" / "csv-points.json"] = canonical_bytes(renamed)

    # AC-5: no-attest/ has empty attestation fields
    no_attest = json.loads(json.dumps(taxos["qif-project"]))
    no_attest["reviewer_attestation"] = {"reviewer": "", "date": "2026-11-22", "statement": ""}
    files[HERE / "no-attest" / "qif-project.json"] = canonical_bytes(no_attest)

    return files


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or verify MIG.COVER.TAXO fixtures and initial taxonomies.")
    ap.add_argument("--check", action="store_true", help="Check committed files match generator output.")
    args = ap.parse_args()

    files = expected_files()
    if args.check:
        for path, expected in sorted(files.items()):
            if not path.is_file():
                raise SystemExit(f"missing file: {path}")
            actual = path.read_bytes()
            if actual != expected:
                raise SystemExit(f"drift in {path}")
        print(f"ok ({len(files)} files checked)")
        return 0

    for path, content in sorted(files.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    print(f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
