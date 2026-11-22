#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for CORE.RPL.RECON.SNAP (AC-1..AC-4).

Generates a deterministic 1,200-entry session (`session_1200.json`) for a toy key-value DocState
(sorted string -> string map, canonical JSON encoding, SHA-256 state & content digests) plus a
side branch `b` forked from `main` at entry 700 (50 additional entries on `b`).

Usage: python3 fixtures/core/rpl/recon/snap/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "session_1200.json"


def sha256_prefixed(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def encode_state(state: dict[str, str]) -> bytes:
    """Canonical sorted JSON bytes for the toy key-value DocState (format version 1)."""
    return json.dumps(state, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def entry_hash(branch: str, index: int, parent: str | None, key: str, value: str) -> str:
    payload = json.dumps(
        {"branch": branch, "index": index, "parent": parent, "key": key, "value": value},
        sort_keys=True,
        separators=(",", ":"),
    ).encode("utf-8")
    return sha256_prefixed(payload)


def build_fixture() -> dict:
    state: dict[str, str] = {}
    main_entries = []
    parent: str | None = None
    checkpoints: dict[str, dict] = {}
    state_at_700: dict[str, str] = {}
    hash_at_700 = ""

    for i in range(1, 1201):
        key = f"k{(i * 37) % 100:02d}"
        value = f"main-v{i:04d}"
        h = entry_hash("main", i, parent, key, value)
        state[key] = value
        main_entries.append({"index": i, "hash": h, "parent": parent, "key": key, "value": value})
        parent = h
        if i == 700:
            state_at_700 = dict(state)
            hash_at_700 = h
        if i in (500, 700, 1000, 1200):
            enc = encode_state(state)
            checkpoints[f"main@{i}"] = {
                "entry_hash": h,
                "format_version": 1,
                "state_digest": sha256_prefixed(enc),
                "content_digest": sha256_prefixed(enc),
                "size": len(enc),
            }

    # Side branch `b` forked from `main` at entry 700 (AC-3).
    b_state = dict(state_at_700)
    b_entries = []
    b_parent: str | None = hash_at_700
    for j in range(1, 51):
        key = f"b{(j * 13) % 25:02d}"
        value = f"branch-b-v{j:04d}"
        h = entry_hash("b", j, b_parent, key, value)
        b_state[key] = value
        b_entries.append({"index": j, "hash": h, "parent": b_parent, "key": key, "value": value})
        b_parent = h
    b_enc = encode_state(b_state)
    checkpoints["b@50"] = {
        "entry_hash": b_parent,
        "format_version": 1,
        "state_digest": sha256_prefixed(b_enc),
        "content_digest": sha256_prefixed(b_enc),
        "size": len(b_enc),
    }

    return {
        "format": "fid-snap-fixture@1",
        "interval": 500,
        "expected_main_snapshot_indices": [500, 1000, 1200],
        "checkpoints": checkpoints,
        "main": main_entries,
        "branch_b_fork_parent": hash_at_700,
        "branch_b": b_entries,
    }


def main() -> int:
    data = build_fixture()
    raw = (json.dumps(data, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_bytes() != raw:
            print(f"FAIL: {OUT} is stale; run without --check")
            return 1
        print("core/rpl/recon/snap session_1200.json: ok")
        return 0
    OUT.write_bytes(raw)
    print(f"wrote {OUT} ({len(raw)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
