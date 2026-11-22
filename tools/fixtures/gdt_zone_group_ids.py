#!/usr/bin/env python3
"""Independent generator/checker for fixtures/gdt/zone/group/ids.json (GDT.ZONE.GROUP.IDS).

Uses only Python's standard library (uuid.uuid5, hashlib.sha256, struct, json) to compute
expected UUIDv5 group IDs and SHA-256 pose digests independently of the Rust implementation.

Usage:
    python tools/fixtures/gdt_zone_group_ids.py          # write fixtures/gdt/zone/group/ids.json
    python tools/fixtures/gdt_zone_group_ids.py --check  # verify committed fixture matches
"""

from __future__ import annotations

import argparse
import hashlib
import json
import struct
import sys
import uuid
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT_PATH = ROOT / "fixtures" / "gdt" / "zone" / "group" / "ids.json"

NAMESPACE = uuid.UUID("6ba7b810-9dad-11d1-80b4-00c04fd430c8")

# Four characteristic UUIDs (in canonical byte order: m1 < m2 < m3 < m4).
M1 = "01900000-0000-7000-8000-000000000001"
M2 = "01900000-0000-7000-8000-000000000002"
M3 = "01900000-0000-7000-8000-000000000003"
M4 = "01900000-0000-7000-8000-000000000004"


def f64_hex(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def canonical_json(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def compute_group_id(ns: uuid.UUID, kind: str, label: str, members: list[str]) -> tuple[str, str]:
    sorted_members = sorted(members, key=lambda s: uuid.UUID(s).bytes)
    payload = {"kind": kind, "label": label, "members": sorted_members}
    canon = canonical_json(payload)
    # RFC 9562 / 4122 UUIDv5: SHA-1(ns.bytes + name_bytes)
    h = hashlib.sha1(ns.bytes + canon).digest()
    raw = bytearray(h[:16])
    raw[6] = (raw[6] & 0x0F) | 0x50
    raw[8] = (raw[8] & 0x3F) | 0x80
    return str(uuid.UUID(bytes=bytes(raw))), canon.decode("utf-8")


def compute_pose_digest(origin: list[float], direction: list[float]) -> tuple[str, str]:
    payload = {
        "direction": [f64_hex(v) for v in direction],
        "origin": [f64_hex(v) for v in origin],
    }
    canon = canonical_json(payload)
    digest = "sha256:" + hashlib.sha256(canon).hexdigest()
    return digest, canon.decode("utf-8")


def build_fixture() -> dict[str, object]:
    order_a = [M3, M1, M4, M2]
    order_b = [M2, M4, M1, M3]

    pat_id, pat_canon = compute_group_id(NAMESPACE, "pattern", "4x-bolt-circle-A", order_a)
    sim_id, sim_canon = compute_group_id(NAMESPACE, "simreq", "SIM-REQ-1", order_b)

    origin = [0.0125, -0.025, 0.5]
    direction = [0.0, 0.0, 1.0]
    pose_dig, pose_canon = compute_pose_digest(origin, direction)

    neg_zero_origin = [-0.0, 0.0, 0.0]
    neg_zero_dir = [1.0, 0.0, 0.0]
    nz_dig, nz_canon = compute_pose_digest(neg_zero_origin, neg_zero_dir)

    return {
        "licence": "CC0-1.0",
        "spec_node": "GDT.ZONE.GROUP.IDS",
        "namespace": str(NAMESPACE),
        "members_sorted": [M1, M2, M3, M4],
        "group_cases": [
            {
                "name": "ac1_pattern_four_members",
                "kind": "pattern",
                "label": "4x-bolt-circle-A",
                "order_a": order_a,
                "order_b": order_b,
                "expected_canonical": pat_canon,
                "expected_group_id": pat_id,
            },
            {
                "name": "simreq_four_members",
                "kind": "simreq",
                "label": "SIM-REQ-1",
                "order_a": order_b,
                "order_b": [M4, M3, M2, M1],
                "expected_canonical": sim_canon,
                "expected_group_id": sim_id,
            },
        ],
        "pose_cases": [
            {
                "name": "ac2_known_f64_bits",
                "origin_hex": [f64_hex(v) for v in origin],
                "direction_hex": [f64_hex(v) for v in direction],
                "expected_canonical": pose_canon,
                "expected_pose_digest": pose_dig,
            },
            {
                "name": "negative_zero_bit_preserved",
                "origin_hex": [f64_hex(v) for v in neg_zero_origin],
                "direction_hex": [f64_hex(v) for v in neg_zero_dir],
                "expected_canonical": nz_canon,
                "expected_pose_digest": nz_dig,
            },
        ],
    }


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Generate or check GDT.ZONE.GROUP.IDS fixtures.")
    parser.add_argument("--check", action="store_true", help="Verify committed fixture matches generator.")
    args = parser.parse_args(argv)

    rendered = json.dumps(build_fixture(), indent=2, ensure_ascii=False) + "\n"
    if args.check:
        if not OUT_PATH.is_file():
            print(f"MISSING {OUT_PATH}", file=sys.stderr)
            return 1
        actual = OUT_PATH.read_text(encoding="utf-8")
        if actual != rendered:
            print(f"DRIFT in {OUT_PATH}; rerun tools/fixtures/gdt_zone_group_ids.py", file=sys.stderr)
            return 1
        return 0

    OUT_PATH.parent.mkdir(parents=True, exist_ok=True)
    OUT_PATH.write_text(rendered, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
