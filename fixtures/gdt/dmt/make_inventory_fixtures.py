#!/usr/bin/env python3
"""Independent stdlib reference generator for GDT.RES.ROWASME.INV fixtures (CC0).

Generates synthetic `dmt-inventory/v1` fixtures paired against the existing
`synthetic_table_v1.json` in this directory and computes expected coverage findings
and RFC 8785 SHA-256 digests independently of the Rust implementation.

- `inventory_synthetic.json`: 6 keys (4 present in `synthetic_table_v1.json`,
  one of which is `unsupported`, plus 2 absent) for AC-1.
- `inventory_all_supported.json`: 4 keys all present as `supported` in
  `synthetic_table_v1.json` for AC-2 (zero findings).
- `inventory_expected.json`: independent expected findings and canonical digests.

Usage:
    python3 fixtures/gdt/dmt/make_inventory_fixtures.py
    python3 fixtures/gdt/dmt/make_inventory_fixtures.py --check
"""

from __future__ import annotations

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def canon_bytes(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def sha256_obj(obj: object) -> str:
    return "sha256:" + hashlib.sha256(canon_bytes(obj)).hexdigest()


def make_inventory(dialect: str, editions: list[str], phase: str, entries: list[dict]) -> dict:
    return {
        "dialect": dialect,
        "editions": sorted(set(editions)),
        "entries": sorted(entries, key=lambda e: e["key"].encode("utf-8")),
        "phase": phase,
        "schema": "dmt-inventory/v1",
    }


def compute_coverage(table: dict, inv: dict) -> list[dict]:
    by_key = {row["key"]: row["status"] for row in table["rows"]}
    findings: list[dict] = []
    for entry in sorted(inv["entries"], key=lambda e: e["key"].encode("utf-8")):
        key = entry["key"]
        req = entry["required_status"]
        if key not in by_key:
            findings.append({"key": key, "kind": "missing_row"})
        else:
            st = by_key[key]
            ok = True if req == "unsupported_ok" else st in ("supported", "partial")
            if not ok:
                findings.append(
                    {
                        "key": key,
                        "kind": "status_below_required",
                        "required_status": req,
                        "status": st,
                    }
                )
    return findings


def outputs() -> dict[str, str]:
    table = json.loads((HERE / "synthetic_table_v1.json").read_text(encoding="utf-8"))

    # AC-1: 6 keys; 4 in synthetic_table_v1.json (flatness=supported, perpendicularity=partial,
    # position/mmc=supported, concentricity=unsupported), 2 missing (caliper, drf_planar).
    inv_synth = make_inventory(
        dialect="asme",
        editions=["y14.5-2018"],
        phase="P1a",
        entries=[
            {"family": "SIZE", "key": "dmt:asme/caliper/none@1", "required_status": "supported_or_partial"},
            {"family": "POS", "key": "dmt:asme/concentricity/none@1", "required_status": "supported_or_partial"},
            {"family": "DRF", "key": "dmt:asme/drf_planar/none@1", "required_status": "supported_or_partial"},
            {"family": "FORM", "key": "dmt:asme/flatness/none@1", "required_status": "supported_or_partial"},
            {"family": "ORIENT", "key": "dmt:asme/perpendicularity/none@1", "required_status": "supported_or_partial"},
            {"family": "POS", "key": "dmt:asme/position/mmc@1", "required_status": "supported_or_partial"},
        ],
    )

    # AC-2: 4 keys all present in synthetic_table_v1.json with status `supported`.
    inv_all_sup = make_inventory(
        dialect="asme",
        editions=["y14.5-2018"],
        phase="P1a",
        entries=[
            {"family": "FORM", "key": "dmt:asme/flatness/none@1", "required_status": "supported_or_partial"},
            {"family": "ORIENT", "key": "dmt:asme/parallelism/none@1", "required_status": "supported_or_partial"},
            {"family": "POS", "key": "dmt:asme/position/none@1", "required_status": "supported_or_partial"},
            {"family": "SIZE", "key": "dmt:asme/size/none@1", "required_status": "supported_or_partial"},
        ],
    )

    ac1_findings = compute_coverage(table, inv_synth)
    ac2_findings = compute_coverage(table, inv_all_sup)
    assert len(ac1_findings) == 3
    assert [f["kind"] for f in ac1_findings] == ["missing_row", "status_below_required", "missing_row"]
    assert ac2_findings == []

    expected = {
        "ac1_findings": ac1_findings,
        "ac2_findings": ac2_findings,
        "all_supported_digest": sha256_obj(inv_all_sup),
        "synthetic_digest": sha256_obj(inv_synth),
    }

    pretty = lambda obj: json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=True) + "\n"
    return {
        "inventory_synthetic.json": pretty(inv_synth),
        "inventory_all_supported.json": pretty(inv_all_sup),
        "inventory_expected.json": pretty(expected),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad: list[str] = []
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.is_file() or path.read_text(encoding="utf-8") != text:
                bad.append(name)
        else:
            path.write_text(text, encoding="utf-8")
    if bad:
        print("stale inventory fixtures: " + ", ".join(bad), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
