#!/usr/bin/env python3
"""Deterministic fixture generator for UNC.RDL.GEN.XCHECK (AC-1..AC-4).

Produces synthetic LSNULL seed-1 `rdl-dataset/v1` records and matching `oracle-result/v1`
files keyed by each record's RFC 8785 canonical SHA-256 digest:
  - `lsnull_seed1_records.json`: 6 tier-G LSNULL seed-1 records (test uncertainty 1e-30).
  - `oracle_ok.json`: independent oracle (`fiducial-oracles-mpmath`) agreeing within 1e-50.
  - `oracle_one_bad.json`: one output shifted by 1e-20 m against test uncertainty 1e-30 m.
  - `oracle_missing_two.json`: two records omitted for `require_all = false` / `true` checks.
  - `oracle_self.json`: `oracle_id = "fid-rdl-gen"` for `oracle_not_independent` check.

Usage: python3 fixtures/rdl/gen/xcheck/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import hashlib
import json
import struct
import sys
from decimal import Decimal, localcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent


def f64_hex(x: float) -> str:
    bits = struct.unpack(">Q", struct.pack(">d", x))[0]
    return f"0x{bits:016x}"


def canon_json_bytes(obj: object) -> bytes:
    """RFC 8785 canonical JSON for objects containing only strings, ints, lists, dicts."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256_digest(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def fmt_dec(d: Decimal) -> str:
    return format(d, "E").replace("E", "e")


def build_records() -> list[dict]:
    tu_hex = f64_hex(1e-30)
    with localcontext() as ctx:
        ctx.prec = 60
        specs = [
            (
                "rdl:G/ls.line.svd/seed1/0001",
                "ls.line.svd",
                [
                    ("origin_x", "length", Decimal("0.1250000000000000000000000000000000000000000000000000001")),
                    ("dir_x", "dimensionless", Decimal("0.8000000000000000000000000000000000000000000000000000000")),
                    ("dir_y", "dimensionless", Decimal("0.6000000000000000000000000000000000000000000000000000000")),
                ],
            ),
            (
                "rdl:G/ls.plane.svd/seed1/0002",
                "ls.plane.svd",
                [
                    ("offset_d", "length", Decimal("-0.0425000000000000000000000000000000000000000000000000002")),
                    ("normal_z", "dimensionless", Decimal("1.0000000000000000000000000000000000000000000000000000000")),
                ],
            ),
            (
                "rdl:G/ls.circle.gn/seed1/0003",
                "ls.circle.gn",
                [
                    ("center_x", "length", Decimal("0.0150000000000000000000000000000000000000000000000000003")),
                    ("center_y", "length", Decimal("-0.0250000000000000000000000000000000000000000000000000004")),
                    ("radius", "length", Decimal("0.0500000000000000000000000000000000000000000000000000005")),
                ],
            ),
            (
                "rdl:G/ls.sphere.gn/seed1/0004",
                "ls.sphere.gn",
                [
                    ("center_x", "length", Decimal("0.0000000000000000000000000000000000000000000000000000000")),
                    ("radius", "length", Decimal("0.0750000000000000000000000000000000000000000000000000007")),
                ],
            ),
            (
                "rdl:G/ls.cylinder.gn/seed1/0005",
                "ls.cylinder.gn",
                [
                    ("axis_z", "dimensionless", Decimal("1.0000000000000000000000000000000000000000000000000000000")),
                    ("radius", "length", Decimal("0.0320000000000000000000000000000000000000000000000000009")),
                ],
            ),
            (
                "rdl:G/ls.cone.gn/seed1/0006",
                "ls.cone.gn",
                [
                    ("half_angle", "angle", Decimal("0.2617993877991494365385536152732919070164307832812588184")),
                    ("apex_z", "length", Decimal("-0.1000000000000000000000000000000000000000000000000000001")),
                ],
            ),
        ]

        records = []
        for idx, (rec_id, aim, outputs) in enumerate(specs, start=1):
            blob_digest = sha256_digest(f"lsnull-seed1-blob-{idx}".encode("utf-8"))
            ref_outputs = []
            for name, kind, val in outputs:
                ref_outputs.append(
                    {
                        "kind": kind,
                        "name": name,
                        "test_uncertainty_hex": tu_hex,
                        "value_dec": fmt_dec(val),
                        "value_hex": f64_hex(float(val)),
                    }
                )
            records.append(
                {
                    "aim": aim,
                    "cross_check": [],
                    "generator": {
                        "id": "fid-rdl-gen",
                        "seed": "1",
                        "version": "0.0.1",
                    },
                    "id": rec_id,
                    "inputs": [
                        {
                            "digest": blob_digest,
                            "role": "points",
                            "unit": "m",
                        }
                    ],
                    "licence": "CC-BY-4.0",
                    "provenance": {
                        "method": "null-space construction seed 1 (synthetic xcheck fixture)",
                        "source": "Metrology Maven UNC.RDL.GEN.XCHECK synthetic fixture",
                    },
                    "reference": {
                        "outputs": ref_outputs,
                        "precision_digits": 55,
                    },
                    "schema": "rdl-dataset/v1",
                    "tier": "G",
                    "visibility": "public",
                }
            )
    return records


def build_fixtures() -> dict[str, bytes]:
    records = build_records()
    digests = [sha256_digest(canon_json_bytes(r)) for r in records]

    with localcontext() as ctx:
        ctx.prec = 70
        ok_entries: dict[str, dict[str, str]] = {}
        for idx, (rec, dig) in enumerate(zip(records, digests)):
            outs: dict[str, str] = {}
            for out_idx, out in enumerate(rec["reference"]["outputs"]):
                base = Decimal(out["value_dec"])
                # Tiny deterministic discrepancy (<= 5e-50, well under test uncertainty 1e-30).
                eps = Decimal(f"{(idx + out_idx) % 5}e-50")
                outs[out["name"]] = fmt_dec(base + eps)
            ok_entries[dig] = outs

        bad_entries = {dig: dict(outs) for dig, outs in ok_entries.items()}
        bad_dig = digests[2]  # rdl:G/ls.circle.gn/seed1/0003
        bad_entries[bad_dig]["radius"] = fmt_dec(
            Decimal(records[2]["reference"]["outputs"][2]["value_dec"]) + Decimal("1e-20")
        )

        missing_two_entries = {dig: dict(ok_entries[dig]) for dig in digests[:4]}

    oracle_ok = {
        "entries": ok_entries,
        "oracle_id": "fiducial-oracles-mpmath",
        "oracle_version": "0.1.0",
        "schema": "oracle-result/v1",
    }
    oracle_one_bad = {
        "entries": bad_entries,
        "oracle_id": "fiducial-oracles-mpmath",
        "oracle_version": "0.1.0",
        "schema": "oracle-result/v1",
    }
    oracle_missing_two = {
        "entries": missing_two_entries,
        "oracle_id": "fiducial-oracles-mpmath",
        "oracle_version": "0.1.0",
        "schema": "oracle-result/v1",
    }
    oracle_self = {
        "entries": ok_entries,
        "oracle_id": "fid-rdl-gen",
        "oracle_version": "0.0.1",
        "schema": "oracle-result/v1",
    }

    def pretty(obj: object) -> bytes:
        return (json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")

    return {
        "lsnull_seed1_records.json": pretty(records),
        "oracle_ok.json": pretty(oracle_ok),
        "oracle_one_bad.json": pretty(oracle_one_bad),
        "oracle_missing_two.json": pretty(oracle_missing_two),
        "oracle_self.json": pretty(oracle_self),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    files = build_fixtures()
    if check:
        drift = []
        for name, expected in files.items():
            path = HERE / name
            if not path.exists() or path.read_bytes() != expected:
                drift.append(name)
        if drift:
            print(f"xcheck fixtures out of date: {', '.join(drift)}", file=sys.stderr)
            return 1
        print(f"xcheck fixtures: ok ({len(files)} files)")
        return 0

    for name, content in files.items():
        (HERE / name).write_bytes(content)
    print(f"wrote {len(files)} xcheck fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
