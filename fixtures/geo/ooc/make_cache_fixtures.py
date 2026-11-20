#!/usr/bin/env python3
"""Independent reference for GEO.OOC.CACHE AC-1 (CC-BY-4.0 project data).

Writes fixtures/geo/ooc/evict_seq.json: the eviction sequence, hit and miss counts and the
budget of the chunk cache when every chunk of a cloud_5e6 prefix is streamed twice
(forward, then in reverse), one chunk pinned at a time.

Run:  python fixtures/geo/ooc/make_cache_fixtures.py [--check] [--jobs N]

The chunk layout (cells, counts, byte lengths, store addresses) comes from the GEO.OOC.LAYOUT
reference make_layout_fixtures.py next to this file, which shares no code with
crates/fid-geo-ooc. The cache is modelled here directly from the rules pinned in the
documentation of crates/fid-geo-ooc/src/cache.rs:

* cost of a resident chunk = count * record_bytes + ENTRY_OVERHEAD, where record_bytes is
  8 (index) + 24 (x, y, z) + the channel widths (normal 24, intensity 4);
* cost of a load in flight = the resident cost + the blob's byte length (the fetched bytes
  are held while they are verified and decoded);
* every access takes the next value of a monotonic counter; a hit stamps the entry with it,
  a miss stamps the new entry with it;
* before a load, if pinned bytes + load cost > budget the load fails; otherwise unpinned
  entries are evicted, smallest (stamp, blob address bytes) first, until
  resident bytes + load cost <= budget.

Stdlib only.
"""

import argparse
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import make_layout_fixtures as layout_ref  # noqa: E402

OUT = HERE / "evict_seq.json"

N_POINTS = 200_000
MAX_CHUNK_POINTS = 6144
RECORD_BYTES = 8 + 24 + 24 + 4
ENTRY_OVERHEAD = 2048
BUDGET_CHUNKS = 8


def resident_cost(e):
    return e["count"] * RECORD_BYTES + ENTRY_OVERHEAD


def load_cost(e):
    return resident_cost(e) + e["bytes"]


def simulate(chunks, budget, pattern):
    counter = 0
    resident = {}  # blob -> (cost, stamp)
    evictions = []
    hits = misses = 0
    peak = 0
    for i in pattern:
        e = chunks[i]
        key = e["blob"]
        counter += 1
        if key in resident:
            hits += 1
            resident[key] = (resident[key][0], counter)
            continue
        need = load_cost(e)
        assert need <= budget, "a single load must fit the budget"
        while sum(c for c, _ in resident.values()) + need > budget:
            victim = min(resident, key=lambda k: (resident[k][1], bytes.fromhex(k)))
            del resident[victim]
            cell = next(c["cell"] for c in chunks if c["blob"] == victim)
            evictions.append({"blob": victim, "cell": cell})
        peak = max(peak, sum(c for c, _ in resident.values()) + need)
        resident[key] = (resident_cost(e), counter)
        misses += 1
    final = sorted(resident, key=lambda k: (resident[k][1], bytes.fromhex(k)))
    return evictions, hits, misses, peak, final


def expected(jobs):
    cloud = layout_ref.cloud_5e6(jobs, N_POINTS)
    lay = layout_ref.layout(cloud, MAX_CHUNK_POINTS)
    chunks = lay["chunks"]
    budget = BUDGET_CHUNKS * max(resident_cost(e) for e in chunks)
    n = len(chunks)
    pattern = list(range(n)) + list(range(n - 1, -1, -1))
    evictions, hits, misses, peak, final = simulate(chunks, budget, pattern)
    assert peak <= budget
    return {
        "comment": "GEO.OOC.CACHE AC-1: first 200000 points of cloud_5e6 chunked with max_chunk_points 6144; "
        "every chunk read forward then in reverse, one handle at a time, through a cache whose budget is "
        "8 times the largest resident chunk cost. Values from make_cache_fixtures.py (CC-BY-4.0).",
        "n_points": N_POINTS,
        "max_chunk_points": MAX_CHUNK_POINTS,
        "n_chunks": n,
        "manifest_digest": lay["manifest_digest"],
        "record_bytes": RECORD_BYTES,
        "entry_overhead": ENTRY_OVERHEAD,
        "budget": budget,
        "accesses": len(pattern),
        "hits": hits,
        "misses": misses,
        "evictions": evictions,
        "final_resident": final,
    }


def dump(obj):
    return json.dumps(obj, indent=1, sort_keys=True) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=None)
    args = ap.parse_args()
    import os

    text = dump(expected(args.jobs or os.cpu_count() or 1))
    if args.check:
        if OUT.read_text(encoding="utf-8") != text:
            print("evict_seq.json differs from the reference", file=sys.stderr)
            sys.exit(1)
        return
    OUT.write_text(text, encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
