#!/usr/bin/env python3
"""Generate and verify deterministic fixture for CORE.DEP.REPL (CC-BY-4.0).

Fixture `replace_chain.json`:
  - Input `m` (type `test.mesh`, value 10, name "Measured Mesh", group "scans", tags ["raw"], metadata {"source": "scan_1"})
  - Dependent `s1 = m + 1` (value 11)
  - Dependent `s2 = s1 * 2` (value 22)
  - Staged replacement `m_prime` (type `test.mesh`, value 20, no dependents)
  - Mismatched-type replacement `cad_other` (type `test.cad`, value 99, no dependents)
  - In-use replacement `m_used` (type `test.mesh`, value 30, has dependent `u1`)
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def make_uuid_v7(ts_ms: int, tag: int) -> str:
    b = bytearray(16)
    b[0] = (ts_ms >> 40) & 0xFF
    b[1] = (ts_ms >> 32) & 0xFF
    b[2] = (ts_ms >> 24) & 0xFF
    b[3] = (ts_ms >> 16) & 0xFF
    b[4] = (ts_ms >> 8) & 0xFF
    b[5] = ts_ms & 0xFF
    h = hashlib.sha256(f"core.dep.repl:{ts_ms}:{tag}".encode("utf-8")).digest()
    b[6] = 0x70 | (h[0] & 0x0F)
    b[7] = h[1]
    b[8] = 0x80 | (h[2] & 0x3F)
    b[9:16] = h[3:10]
    hex_s = b.hex()
    return f"{hex_s[0:8]}-{hex_s[8:12]}-{hex_s[12:16]}-{hex_s[16:20]}-{hex_s[20:32]}"


def canon_bytes(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def build_replace_chain() -> dict:
    base_ms = 1_760_400_000_000
    id_m = make_uuid_v7(base_ms, 1)
    id_s1 = make_uuid_v7(base_ms + 100, 2)
    id_s2 = make_uuid_v7(base_ms + 200, 3)
    id_m_prime = make_uuid_v7(base_ms + 300, 4)
    id_cad = make_uuid_v7(base_ms + 400, 5)
    id_m_used = make_uuid_v7(base_ms + 500, 6)
    id_u1 = make_uuid_v7(base_ms + 600, 7)

    elements = [
        {
            "id": id_m,
            "name": "Measured Mesh",
            "kind": "test.mesh",
            "group": "scans",
            "tags": ["raw", "primary"],
            "metadata": {"operator": "alice", "source": "scan_1"},
            "state": "current",
            "content": {"value": 10},
            "command": None,
            "args": None,
            "inputs": [],
        },
        {
            "id": id_s1,
            "name": "s1",
            "kind": "test.scalar",
            "group": "features",
            "tags": [],
            "metadata": {},
            "state": "current",
            "content": {"value": 11},
            "command": "test.add_const@1",
            "args": {"delta": 1, "input": id_m, "target": id_s1},
            "inputs": [id_m],
        },
        {
            "id": id_s2,
            "name": "s2",
            "kind": "test.scalar",
            "group": "checks",
            "tags": [],
            "metadata": {},
            "state": "current",
            "content": {"value": 22},
            "command": "test.mul_const@1",
            "args": {"factor": 2, "input": id_s1, "target": id_s2},
            "inputs": [id_s1],
        },
        {
            "id": id_m_prime,
            "name": "Staged Replacement Mesh",
            "kind": "test.mesh",
            "group": "staging",
            "tags": ["staged"],
            "metadata": {"source": "scan_2"},
            "state": "current",
            "content": {"value": 20},
            "command": None,
            "args": None,
            "inputs": [],
        },
        {
            "id": id_cad,
            "name": "Nominal CAD",
            "kind": "test.cad",
            "group": "cad",
            "tags": [],
            "metadata": {},
            "state": "current",
            "content": {"value": 99},
            "command": None,
            "args": None,
            "inputs": [],
        },
        {
            "id": id_m_used,
            "name": "Referenced Mesh",
            "kind": "test.mesh",
            "group": "scans",
            "tags": [],
            "metadata": {},
            "state": "current",
            "content": {"value": 30},
            "command": None,
            "args": None,
            "inputs": [],
        },
        {
            "id": id_u1,
            "name": "u1",
            "kind": "test.scalar",
            "group": "features",
            "tags": [],
            "metadata": {},
            "state": "current",
            "content": {"value": 31},
            "command": "test.add_const@1",
            "args": {"delta": 1, "input": id_m_used, "target": id_u1},
            "inputs": [id_m_used],
        },
    ]

    return {
        "format": "fid-dep-repl-fixture/1",
        "licence": "CC-BY-4.0",
        "seed": "0xde95",
        "ids": {
            "m": id_m,
            "s1": id_s1,
            "s2": id_s2,
            "m_prime": id_m_prime,
            "cad_other": id_cad,
            "m_used": id_m_used,
            "u1": id_u1,
        },
        "expected_after_replace": {id_m: 20, id_s1: 21, id_s2: 42},
        "expected_after_undo": {id_m: 10, id_s1: 11, id_s2: 22},
        "elements": elements,
    }


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    args = ap.parse_args()

    out_path = HERE / "replace_chain.json"
    expected = canon_bytes(build_replace_chain()) + b"\n"

    if args.check:
        if not out_path.exists() or out_path.read_bytes() != expected:
            print(f"mismatch: {out_path}", file=sys.stderr)
            return 1
        print(f"core-dep-repl fixtures ok (1 file: {out_path.name})")
        return 0

    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_bytes(expected)
    print(f"wrote {out_path}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
