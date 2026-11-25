#!/usr/bin/env python3
"""Generate and verify deterministic fixtures for CORE.DEP.RECOMP (CC-BY-4.0).

Fixtures:
  - diamond.json: a -> b, a -> c, b -> d, c -> d with id(c) < id(b), all outdated.
  - fail_b.json: same diamond graph with b configured to fail with `fit_failed`.
  - chain1000.json: 1,000-element chain (e_0 -> e_1 -> ... -> e_999), e_1..e_999 outdated.
  - kernel_d0.json: deterministic D0 recompute & exact-replay fixture.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def make_uuid_v7(ts_ms: int, tag: int) -> str:
    """Deterministic UUIDv7 whose lexicographic order matches (ts_ms, tag)."""
    b = bytearray(16)
    b[0] = (ts_ms >> 40) & 0xFF
    b[1] = (ts_ms >> 32) & 0xFF
    b[2] = (ts_ms >> 24) & 0xFF
    b[3] = (ts_ms >> 16) & 0xFF
    b[4] = (ts_ms >> 8) & 0xFF
    b[5] = ts_ms & 0xFF
    h = hashlib.sha256(f"core.dep.recomp:{ts_ms}:{tag}".encode("utf-8")).digest()
    b[6] = 0x70 | (h[0] & 0x0F)
    b[7] = h[1]
    b[8] = 0x80 | (h[2] & 0x3F)
    b[9:16] = h[3:10]
    hex_s = b.hex()
    return f"{hex_s[0:8]}-{hex_s[8:12]}-{hex_s[12:16]}-{hex_s[16:20]}-{hex_s[20:32]}"


def canon_bytes(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def build_diamond(fail_b: bool) -> dict:
    # Ensure strict ordering: id(a) < id(c) < id(b) < id(d)
    id_a = make_uuid_v7(1_760_100_000_000, 1)
    id_c = make_uuid_v7(1_760_100_001_000, 2)
    id_b = make_uuid_v7(1_760_100_002_000, 3)
    id_d = make_uuid_v7(1_760_100_003_000, 4)
    assert id_c < id_b, "AC-1 requires id(c) < id(b)"

    elements = [
        {
            "id": id_a,
            "name": "a",
            "state": "current",
            "value": 10,
            "command": None,
            "args": None,
            "inputs": [],
            "fail_code": None,
        },
        {
            "id": id_c,
            "name": "c",
            "state": "outdated",
            "value": 0,
            "command": "test.add_const@1",
            "args": {"delta": 5, "input": id_a, "target": id_c},
            "inputs": [id_a],
            "fail_code": None,
        },
        {
            "id": id_b,
            "name": "b",
            "state": "outdated",
            "value": 0,
            "command": "test.fail_fit@1" if fail_b else "test.mul_const@1",
            "args": (
                {"code": "fit_failed", "input": id_a, "target": id_b}
                if fail_b
                else {"factor": 3, "input": id_a, "target": id_b}
            ),
            "inputs": [id_a],
            "fail_code": "fit_failed" if fail_b else None,
        },
        {
            "id": id_d,
            "name": "d",
            "state": "outdated",
            "value": 0,
            "command": "test.sum_two@1",
            "args": {"left": id_b, "right": id_c, "target": id_d},
            "inputs": [id_c, id_b],
            "fail_code": None,
        },
    ]
    return {
        "format": "fid-dep-recomp-fixture/1",
        "licence": "CC-BY-4.0",
        "seed": "0xde91" if not fail_b else "0xde92",
        "name": "fail_b" if fail_b else "diamond",
        "expected_order": [id_c, id_b, id_d],
        "expected_values": (
            {id_a: 10, id_c: 15} if fail_b else {id_a: 10, id_c: 15, id_b: 30, id_d: 45}
        ),
        "elements": elements,
    }


def build_chain1000() -> dict:
    base_ms = 1_760_200_000_000
    ids = [make_uuid_v7(base_ms + i * 10, i) for i in range(1000)]
    elements = [
        {
            "id": ids[0],
            "name": "e_0000",
            "state": "current",
            "value": 1,
            "command": None,
            "args": None,
            "inputs": [],
            "fail_code": None,
        }
    ]
    for i in range(1, 1000):
        elements.append(
            {
                "id": ids[i],
                "name": f"e_{i:04d}",
                "state": "outdated",
                "value": 0,
                "command": "test.add_const@1",
                "args": {"delta": 1, "input": ids[i - 1], "target": ids[i]},
                "inputs": [ids[i - 1]],
                "fail_code": None,
            }
        )
    return {
        "format": "fid-dep-recomp-fixture/1",
        "licence": "CC-BY-4.0",
        "seed": "0xde93",
        "name": "chain1000",
        "expected_order": ids[1:],
        "elements": elements,
    }


def build_kernel_d0() -> dict:
    base_ms = 1_760_300_000_000
    id_in = make_uuid_v7(base_ms, 1)
    id_p1 = make_uuid_v7(base_ms + 100, 2)
    id_p2 = make_uuid_v7(base_ms + 200, 3)
    id_out = make_uuid_v7(base_ms + 300, 4)
    elements = [
        {
            "id": id_in,
            "name": "datum_seed",
            "state": "current",
            "value": 100,
            "command": None,
            "args": None,
            "inputs": [],
            "fail_code": None,
        },
        {
            "id": id_p1,
            "name": "offset_a",
            "state": "outdated",
            "value": 0,
            "command": "test.add_const@1",
            "args": {"delta": 25, "input": id_in, "target": id_p1},
            "inputs": [id_in],
            "fail_code": None,
        },
        {
            "id": id_p2,
            "name": "scale_b",
            "state": "outdated",
            "value": 0,
            "command": "test.mul_const@1",
            "args": {"factor": 4, "input": id_in, "target": id_p2},
            "inputs": [id_in],
            "fail_code": None,
        },
        {
            "id": id_out,
            "name": "combined",
            "state": "outdated",
            "value": 0,
            "command": "test.sum_two@1",
            "args": {"left": id_p1, "right": id_p2, "target": id_out},
            "inputs": [id_p1, id_p2],
            "fail_code": None,
        },
    ]
    return {
        "format": "fid-dep-recomp-fixture/1",
        "licence": "CC-BY-4.0",
        "seed": "0xde94",
        "name": "kernel_d0",
        "expected_order": [id_p1, id_p2, id_out],
        "expected_values": {id_in: 100, id_p1: 125, id_p2: 400, id_out: 525},
        "elements": elements,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    files = {
        HERE / "diamond.json": canon_bytes(build_diamond(fail_b=False)) + b"\n",
        HERE / "fail_b.json": canon_bytes(build_diamond(fail_b=True)) + b"\n",
        HERE / "chain1000.json": canon_bytes(build_chain1000()) + b"\n",
        HERE / "kernel_d0.json": canon_bytes(build_kernel_d0()) + b"\n",
    }

    if args.check:
        ok = True
        for path, expected in files.items():
            if not path.exists() or path.read_bytes() != expected:
                print(f"mismatch: {path}", file=sys.stderr)
                ok = False
        if not ok:
            return 1
        print(f"core-dep-recomp fixtures ok ({len(files)} files)")
        return 0

    for path, expected in files.items():
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(expected)
    print(f"wrote {len(files)} fixture files under {HERE}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
