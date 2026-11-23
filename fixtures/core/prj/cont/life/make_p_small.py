#!/usr/bin/env python3
"""Deterministic CC-BY-4.0 generator for `fixtures/core/prj/cont/life/p_small/manifest.json`
(spec node `CORE.PRJ.CONT.LIFE` AC-2, AC-3, AC-6, AC-8).

Generates the fixed-seed specification of a 20-entity project:
- 1 Part (`part-1`)
- 2 Revisions (`rev-1`, `rev-2`)
- 4 Plans (`plan-1`..`plan-4`, two per revision)
- 13 Runs (`run-1`..`run-13`, distributed across the 4 plans)
Total = 20 container entities with deterministic UUIDv7 IDs generated from fixed seed `0x5011`.

Usage:
  python3 fixtures/core/prj/cont/life/make_p_small.py          # write fixture
  python3 fixtures/core/prj/cont/life/make_p_small.py --check  # verify byte-exact match
"""

from __future__ import annotations

import json
import pathlib
import random
import sys

HERE = pathlib.Path(__file__).resolve().parent
OUT_DIR = HERE / "p_small"
OUT_FILE = OUT_DIR / "manifest.json"


def make_uuid_v7(unix_ms: int, rng: random.Random) -> str:
    rand_a = rng.getrandbits(12)
    rand_b = rng.getrandbits(62)
    hi = ((unix_ms & 0xFFFFFFFFFFFF) << 16) | (0x7 << 12) | rand_a
    lo = (0b10 << 62) | rand_b
    raw = (hi << 64) | lo
    h = f"{raw:032x}"
    return f"{h[0:8]}-{h[8:12]}-{h[12:16]}-{h[16:20]}-{h[20:32]}"


def build_manifest() -> dict:
    rng = random.Random(0x5011)
    base_ms = 1_763_856_000_000  # 2025-11-23T00:00:00Z
    step = 1000
    t = base_ms

    entities: list[dict] = []

    # 1 Part
    part_id = make_uuid_v7(t, rng)
    t += step
    entities.append({"id": part_id, "kind": "Part", "label": "Fixture Bracket"})

    # 2 Revisions
    revs: list[str] = []
    for r in range(1, 3):
        rid = make_uuid_v7(t, rng)
        t += step
        revs.append(rid)
        entities.append({"id": rid, "kind": "Revision", "label": f"Rev {r}", "part": part_id})

    # 4 Plans (2 per revision)
    plans: list[tuple[str, str]] = []
    p_idx = 0
    for rid in revs:
        for _ in range(2):
            p_idx += 1
            pid = make_uuid_v7(t, rng)
            t += step
            plans.append((pid, rid))
            entities.append({"id": pid, "kind": "Plan", "label": f"Plan {p_idx}", "revision": rid})

    # 13 Runs across the 4 plans -> total 1 + 2 + 4 + 13 = 20 entities.
    for k in range(13):
        pid, rid = plans[k % len(plans)]
        run_id = make_uuid_v7(t, rng)
        t += step
        entities.append({"id": run_id, "kind": "Run", "label": f"Run {k + 1:02d}", "plan": pid, "revision": rid})

    assert len(entities) == 20
    return {
        "entities": entities,
        "format": "fid-p-small-fixture/1",
        "label": "Small Benchmark Project",
        "licence": "CC-BY-4.0",
        "seed": "0x5011",
    }


def main() -> int:
    doc = build_manifest()
    payload = json.dumps(doc, sort_keys=True, separators=(",", ":")) + "\n"
    if "--check" in sys.argv[1:]:
        if not OUT_FILE.exists() or OUT_FILE.read_text(encoding="utf-8") != payload:
            sys.stderr.write(f"mismatch: {OUT_FILE}\n")
            return 1
        print(f"ok: {OUT_FILE}")
        return 0
    OUT_DIR.mkdir(parents=True, exist_ok=True)
    OUT_FILE.write_text(payload, encoding="utf-8")
    print(f"wrote {OUT_FILE}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
