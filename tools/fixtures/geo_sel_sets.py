#!/usr/bin/env python3
"""Independent reference for GEO.SEL.MODEL index sets; writes fixtures/geo/sel/sets.json.

Stdlib only (random, hashlib, struct, json). Shares no code with the Rust crate
fid-geo-sel: it builds each member set with Python's `set`, derives the half-open ranges
by a linear scan over `sorted(set)`, and hashes the canonical encoding with hashlib.

Canonical encoding of an index set (REQ-GEO.SEL.MODEL-002), all integers little-endian:

    "FIDSEL" | 0x01 | domain u8 | target element digest [32] | count u64 |
    ranges as (start u64, end u64) pairs, ascending

Domain bytes: points 1, triangles 2, faces 3, edges 4. `count` is the number of members.
Ranges are half-open, non-empty, ascending and coalesced: two ranges that touch
(end == next start) are one range. The digest is SHA-256 of that encoding.

Cases. Every random case uses one `random.Random(71)` stream (seed 71, NODE.md Test
fixtures), drawn in the order listed:

    random_<i> (i = 0..11)  domain = rng.choice(points, triangles, faces, edges);
                            bound = rng.randrange(1, 5000); members: rng.randrange(1, 9)
                            runs, each start rng.randrange(bound), length
                            rng.randrange(1, 40), clipped to bound; the list is every
                            member once, plus rng.randrange(0, 20) duplicates drawn with
                            rng.choice, then rng.shuffle'd. target digest =
                            SHA-256(ASCII "fid sel target <i>").

Hand-labelled cases (target digest SHA-256(ASCII "fid sel target <name>") unless stated):

    empty            points, bound 10, no indexes
    single_zero      points, bound 1, [0]
    last_index       triangles, bound 1000, [999]
    coalesce         points, bound 10, [7, 5, 4, 2, 1, 0, 5, 0] -> [0,3) [4,6) [7,8)
    full             edges, bound 6, [5, 4, 3, 2, 1, 0] -> [0,6)
    near_u64_max     faces, bound 2^64-1, [2^64-2, 2^64-3, 0, 2^64-2]
    pointset_small   points, bound 1000, every third index 0, 3, ..., 999 in descending
                     order plus duplicates 0 and 999; target digest = the
                     pointset_small digest from fixtures/geo/types/digest_vectors.json
                     (itself from the independent geo_digest_vectors.py), used by AC-2.

Run:  python tools/fixtures/geo_sel_sets.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.
"""

import hashlib
import json
import os
import random
import struct
import sys

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", ".."))
OUT = os.path.join(ROOT, "fixtures", "geo", "sel", "sets.json")
DIGEST_VECTORS = os.path.join(ROOT, "fixtures", "geo", "types", "digest_vectors.json")

DOMAINS = {"points": 1, "triangles": 2, "faces": 3, "edges": 4}
U64_MAX = (1 << 64) - 1


def ranges_of(members):
    """Half-open coalesced ranges of a set of non-negative integers."""
    out = []
    for m in sorted(members):
        if out and out[-1][1] == m:
            out[-1][1] = m + 1
        else:
            out.append([m, m + 1])
    return out


def encode(domain, target, members):
    ranges = ranges_of(members)
    b = b"FIDSEL" + bytes([1, DOMAINS[domain]]) + target + struct.pack("<Q", len(members))
    for s, e in ranges:
        b += struct.pack("<QQ", s, e)
    return b, ranges


def case(name, domain, bound, indexes, target):
    members = set(indexes)
    assert all(0 <= i < bound for i in members)
    enc, ranges = encode(domain, target, members)
    return {
        "name": name,
        "domain": domain,
        "bound": str(bound),
        "target_digest": target.hex(),
        "indexes": " ".join(str(i) for i in indexes),
        "count": str(len(members)),
        "ranges": " ".join(f"{s}:{e}" for s, e in ranges),
        "encoding_hex": enc.hex(),
        "sha256": hashlib.sha256(enc).hexdigest(),
    }


def tgt(label):
    return hashlib.sha256(f"fid sel target {label}".encode("ascii")).digest()


def pointset_small_digest():
    with open(DIGEST_VECTORS, encoding="utf-8") as f:
        doc = json.load(f)
    for v in doc["vectors"]:
        if v["name"] == "pointset_small":
            return bytes.fromhex(v["sha256"])
    raise SystemExit("pointset_small vector missing")


def build():
    rng = random.Random(71)
    cases = []
    names = ["points", "triangles", "faces", "edges"]
    for i in range(12):
        domain = rng.choice(names)
        bound = rng.randrange(1, 5000)
        members = set()
        for _ in range(rng.randrange(1, 9)):
            start = rng.randrange(bound)
            length = rng.randrange(1, 40)
            for m in range(start, min(start + length, bound)):
                members.add(m)
        listed = sorted(members)
        uniq = list(listed)
        for _ in range(rng.randrange(0, 20)):
            listed.append(rng.choice(uniq))
        rng.shuffle(listed)
        cases.append(case(f"random_{i}", domain, bound, listed, tgt(i)))

    cases.append(case("empty", "points", 10, [], tgt("empty")))
    cases.append(case("single_zero", "points", 1, [0], tgt("single_zero")))
    cases.append(case("last_index", "triangles", 1000, [999], tgt("last_index")))
    cases.append(case("coalesce", "points", 10, [7, 5, 4, 2, 1, 0, 5, 0], tgt("coalesce")))
    cases.append(case("full", "edges", 6, [5, 4, 3, 2, 1, 0], tgt("full")))
    cases.append(
        case("near_u64_max", "faces", U64_MAX, [U64_MAX - 1, U64_MAX - 2, 0, U64_MAX - 1], tgt("near_u64_max"))
    )
    third = list(range(999, -1, -1))
    third = [i for i in third if i % 3 == 0] + [0, 999]
    cases.append(case("pointset_small", "points", 1000, third, pointset_small_digest()))

    doc = {
        "generator": "tools/fixtures/geo_sel_sets.py (stdlib hashlib, independent of fid-geo-sel)",
        "license": "CC-BY-4.0",
        "seed": 71,
        "spec": "GEO.SEL.MODEL",
        "version": 1,
        "cases": cases,
    }
    return json.dumps(doc, indent=2, sort_keys=True) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        with open(OUT, encoding="utf-8", newline="") as f:
            if f.read() != text:
                print(f"{OUT} is stale; rerun tools/fixtures/geo_sel_sets.py", file=sys.stderr)
                sys.exit(1)
        print("sets.json up to date")
        return
    os.makedirs(os.path.dirname(OUT), exist_ok=True)
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
