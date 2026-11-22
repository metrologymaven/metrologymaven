#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for CORE.RPL.RECON.FOLD (AC-1, AC-3, AC-6).

Generates deterministic synthetic fixtures without running Rust:
- `session_1200.json`: 1,200 chained durable entries on `main` (with CORE.JRN.ENTRY salted
  `events` commitments), snapshots at entries 500 and 1000, committed state digests at head
  1200, undo head 1197 (undo 3), redo head 1199 (redo 2), plus a 5-entry side branch off
  entry 500 for branch-switch reconstruction.
- `bench_10000.json`: deterministic parameters and independently computed state digests at
  entry 9,600 (snapshot) and entry 10,000 (head) for the 10,000-entry benchmark (AC-6).
- `LICENSE.txt`: CC-BY-4.0 notice.

Usage:
  python3 fixtures/core/rpl/recon/fold/make_fixtures.py          # write fixtures
  python3 fixtures/core/rpl/recon/fold/make_fixtures.py --check  # verify committed fixtures
"""

import hashlib
import json
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent


def canon(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def splitmix64(x: int) -> int:
    x = (x + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
    z = x
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
    return x, (z ^ (z >> 31)) & 0xFFFFFFFFFFFFFFFF


def salt_for(branch: str, seq: int) -> bytes:
    return hashlib.sha256(f"fold-salt:{branch}:{seq}".encode("utf-8")).digest()[:16]


def events_commitment(salt: bytes, events: list[dict]) -> str:
    return sha256(salt + canon(events))


def state_digest(kv: dict[str, str]) -> str:
    return sha256(canon(kv))


def build_session_1200() -> bytes:
    state: dict[str, str] = {}
    entries = []
    snapshots = []
    head_digests: dict[str, str] = {}
    parent: str | None = None
    snap500_state: dict[str, str] = {}
    snap500_hash = ""

    for seq in range(1, 1201):
        key = f"k_{seq % 97:02d}"
        val = f"v_{seq:04d}"
        events = [{"key": key, "kind": "set", "value": val}]
        salt = salt_for("main", seq)
        ev_commit = events_commitment(salt, events)
        header = {
            "branch": "main",
            "events_commit": ev_commit,
            "logical_parent": parent,
            "seq": seq,
        }
        entry_hash = sha256(canon(header))
        payload = {"events": events, "salts": {"events": salt.hex()}}

        state[key] = val
        entries.append(
            {
                "branch": "main",
                "entry_hash": entry_hash,
                "events_commit": ev_commit,
                "header": header,
                "logical_parent": parent,
                "payload": payload,
                "seq": seq,
            }
        )
        parent = entry_hash

        if seq in (500, 1000):
            enc = canon(state)
            snapshots.append(
                {
                    "content_digest": sha256(enc),
                    "entry_hash": entry_hash,
                    "format_version": 1,
                    "seq": seq,
                    "size": len(enc),
                    "state": dict(sorted(state.items())),
                    "state_digest": sha256(enc),
                }
            )
        if seq == 500:
            snap500_state = dict(state)
            snap500_hash = entry_hash
        if seq in (1197, 1199, 1200):
            head_digests[str(seq)] = state_digest(state)

    # 5-entry side branch starting from entry 500.
    side_state = dict(snap500_state)
    side_parent = snap500_hash
    side_entries = []
    for step in range(1, 6):
        key = f"side_{step:02d}"
        val = f"alt_{step:02d}"
        events = [{"key": key, "kind": "set", "value": val}]
        salt = salt_for("agent/alt", step)
        ev_commit = events_commitment(salt, events)
        header = {
            "branch": "agent/alt",
            "events_commit": ev_commit,
            "logical_parent": side_parent,
            "seq": 500 + step,
        }
        entry_hash = sha256(canon(header))
        payload = {"events": events, "salts": {"events": salt.hex()}}
        side_state[key] = val
        side_entries.append(
            {
                "branch": "agent/alt",
                "entry_hash": entry_hash,
                "events_commit": ev_commit,
                "header": header,
                "logical_parent": side_parent,
                "payload": payload,
                "seq": 500 + step,
            }
        )
        side_parent = entry_hash

    doc = {
        "entries": entries,
        "head_1197": entries[1196]["entry_hash"],
        "head_1199": entries[1198]["entry_hash"],
        "head_1200": entries[1199]["entry_hash"],
        "head_digests": head_digests,
        "side_branch_entries": side_entries,
        "side_branch_head": side_parent,
        "side_branch_digest": state_digest(side_state),
        "snapshots": snapshots,
        "state_digest": head_digests["1200"],
    }
    return canon(doc) + b"\n"


def build_bench_10000() -> bytes:
    """Computes exact digests for a deterministic 10,000-entry synthetic session."""
    state: dict[str, str] = {}
    snap_at = 9600
    snap_digest = ""
    for seq in range(1, 10001):
        key = f"k_{seq % 251:03d}"
        val = f"b_{seq:05d}"
        state[key] = val
        if seq == snap_at:
            snap_digest = state_digest(state)
    head_digest = state_digest(state)
    doc = {
        "count": 10000,
        "head_digest": head_digest,
        "key_modulus": 251,
        "snapshot_at": snap_at,
        "snapshot_digest": snap_digest,
    }
    return canon(doc) + b"\n"


def outputs() -> dict[Path, bytes]:
    return {
        HERE / "session_1200.json": build_session_1200(),
        HERE / "bench_10000.json": build_bench_10000(),
        HERE / "LICENSE.txt": (
            "Synthetic, project-authored test fixtures for spec node CORE.RPL.RECON.FOLD.\n"
            "Licensed under CC-BY-4.0.\n"
        ).encode("utf-8"),
    }


def main() -> int:
    files = outputs()
    if "--check" in sys.argv[1:]:
        stale = []
        for path, want in sorted(files.items()):
            if not path.is_file() or path.read_bytes() != want:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print(f"FAIL: stale CORE.RPL.RECON.FOLD fixtures: {', '.join(stale)}")
            return 1
        print(f"fixtures/core/rpl/recon/fold: ok ({len(files)} files)")
        return 0
    for path, want in sorted(files.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(want)
    print(f"wrote {len(files)} fixture files under {HERE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
