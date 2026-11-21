#!/usr/bin/env python3
"""Independent reference for GEO.OOC.STREAM (CC-BY-4.0 project data).

Writes fixtures/geo/ooc/stream_vectors.json: for small clouds of the GEO.OOC.LAYOUT vectors
and for fixture cloud_5e6 (its 200 000-point prefix and the full cloud), the sequence of
points a chunked-cloud stream emits in spatial order and in original order, as index lists
(small clouds) and as sequence digests.

Run:  python fixtures/geo/ooc/make_stream_fixtures.py [--check] [--jobs N]

Without --check it rewrites the file (the full cloud_5e6 takes several minutes; its Philox
draws run on --jobs processes). With --check the small vectors and the 200 000-point prefix
are regenerated and compared with the committed values; the full cloud_5e6 values are taken
as committed (too slow for CI). --small-only rewrites everything except the full block.

Stdlib only. The clouds, the generator and the reference partition come from the GEO.OOC.LAYOUT
reference make_layout_fixtures.py next to this file, which shares no code with
crates/fid-geo-ooc. The stream rules are modelled here directly from the documentation of
crates/fid-geo-ooc/src/stream.rs:

* spatial order: the chunks in ascending Morton order of their cells, within a chunk the
  points by ascending original index; here the leaves are found from the points' sorted
  63-bit Morton codes (bit 3b + k = bit b of axis k) by counting cell ranges, and checked
  against the layout reference's recursive octant split on every small cloud;
* original order: ascending original index, 0 .. n - 1;
* the block size does not change either sequence (a stream is the concatenation of its
  blocks), so one sequence per order is pinned.

Sequence digest v1 = SHA-256 over, per emitted point in order: original index u64 | x | y | z
as raw IEEE 754 f64 bits | then every channel of the cloud in name order: colour 3 bytes,
intensity the stored f32 bits (any NaN as 0x7fc00000), normal x, y, z raw f64 bits, quality
u16, scanner_id u32, timestamp_ns i64; all little-endian.
"""

import argparse
import hashlib
import json
import os
import struct
import sys
from array import array
from bisect import bisect_left
from multiprocessing import Pool
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))

import make_layout_fixtures as L  # noqa: E402

OUT = HERE / "stream_vectors.json"
MAX_DEPTH = 21
INDEX_BITS = 23  # every cloud here has fewer than 2^23 points
LIST_LIMIT = 64  # index lists are written for clouds up to this size

# spread table: bits of an 11-bit value moved to every third bit
SPREAD = []
for v in range(1 << 11):
    s = 0
    for b in range(11):
        s |= ((v >> b) & 1) << (3 * b)
    SPREAD.append(s)


def spread(q):
    return SPREAD[q & 0x7FF] | (SPREAD[q >> 11] << 33)


def morton(qx, qy, qz):
    return spread(qx) | (spread(qy) << 1) | (spread(qz) << 2)


def leaves_from_keys(keys, max_points):
    """Leaves as (cell code, start, end) over keys = sorted (morton << INDEX_BITS | index)."""
    out = []

    def walk(depth, prefix, lo, hi):
        if hi - lo <= max_points or depth == MAX_DEPTH:
            out.append(((1 << (3 * depth)) | prefix, lo, hi))
            return
        shift = 3 * (MAX_DEPTH - depth - 1) + INDEX_BITS
        for o in range(8):
            child = prefix * 8 + o
            a = bisect_left(keys, child << shift, lo, hi)
            b = bisect_left(keys, (child + 1) << shift, lo, hi)
            if b > a:
                walk(depth + 1, child, a, b)

    walk(0, 0, 0, len(keys))
    return out


def spatial_order(xs, ys, zs, max_points, bounds=None):
    """Original indexes in spatial order, and the (cell, count) of every chunk."""
    n = len(xs)
    if bounds is None:
        lo = tuple(min(col, key=L.total_key) for col in (xs, ys, zs))
        hi = tuple(max(col, key=L.total_key) for col in (xs, ys, zs))
    else:
        lo, hi = bounds
    q = L.quantiser(lo, hi)
    keys = sorted((morton(q(xs[i], 0), q(ys[i], 1), q(zs[i], 2)) << INDEX_BITS) | i for i in range(n))
    order = []
    chunks = []
    mask = (1 << INDEX_BITS) - 1
    for cell, a, b in leaves_from_keys(keys, max_points):
        order.extend(sorted(keys[j] & mask for j in range(a, b)))
        chunks.append((cell, b - a))
    return order, chunks


def record_fn(c):
    """Bytes of point i in the sequence digest."""
    pts, ch = c["pts"], c["ch"]
    names = [n for n, _, _ in L.CHANNELS if n in ch]

    def rec(i):
        p = pts[i]
        out = [struct.pack("<Qddd", i, p[0], p[1], p[2])]
        for name in names:
            v = ch[name][i]
            if name == "colour":
                out.append(bytes(v))
            elif name == "intensity":
                out.append(L.u32(L.f32chunk(v)))
            elif name == "normal":
                out.append(struct.pack("<ddd", *v))
            elif name == "quality":
                out.append(L.u16(v))
            elif name == "scanner_id":
                out.append(L.u32(v))
            elif name == "timestamp_ns":
                out.append(L.i64(v))
        return b"".join(out)

    return rec


def seq_digest(order, rec):
    h = hashlib.sha256()
    for i in order:
        h.update(rec(i))
    return h.hexdigest()


def case_entry(name, c, max_points, bounds=None):
    pts = c["pts"]
    xs, ys, zs = [p[0] for p in pts], [p[1] for p in pts], [p[2] for p in pts]
    order, chunks = spatial_order(xs, ys, zs, max_points, bounds)
    # Cross-check against the layout reference's recursive split.
    lo, hi = bounds if bounds is not None else L.tight_box(pts)
    qf = L.quantiser(lo, hi)
    ref = L.leaves([(qf(p[0], 0), qf(p[1], 1), qf(p[2], 2)) for p in pts], max_points)
    assert [(cell, len(m)) for cell, m in ref] == chunks, name
    assert [i for _, m in ref for i in m] == order, name
    assert sorted(order) == list(range(len(pts))), name
    rec = record_fn(c)
    e = {
        "name": name,
        "n_points": len(pts),
        "n_chunks": len(chunks),
        "spatial_digest": seq_digest(order, rec),
        "original_digest": seq_digest(range(len(pts)), rec),
    }
    if len(pts) <= LIST_LIMIT:
        e["spatial_order"] = order
    return e


def small_cases():
    """Clouds of layout_vectors.json (same names, clouds and parameters)."""
    out = [
        case_entry("tiny_all_channels", L.case_tiny(), 4),
        case_entry("seeded_2000", L.case_seeded(7, 2000), 64),
        case_entry("boundaries", L.case_boundaries(), 1),
        case_entry("boundaries_bounds", L.case_boundaries(), 2, ((-1.0, -1.0, -1.0), (2.0, 2.0, 2.0))),
        case_entry("nan_intensity", L.case_nan_intensity(), 2),
        case_entry("neg_zero_minus", L.neg_zero(True), 2),
    ]
    gen = {"kind": "uniform", "seed": 11, "n": 500, "lo": [0.0, 0.0, 2.5], "span": [1.0, 2.0, 0.0], "extra": []}
    out.append(case_entry("flat_z", L.case_from_gen("flat_z", gen), 50))
    return out


# ------------------------------------------------------------------ cloud_5e6


def scan_columns(rng):
    """Points rng of cloud_5e6 as compact columns: x, y, z, normal id, intensity bits."""
    normals = [f[3] for f in L.FACETS]
    xs, ys, zs, nid, it = array("d"), array("d"), array("d"), bytearray(), array("I")
    for i in range(*rng):
        p, nrm, inten = L.scan_point(i)
        xs.append(p[0])
        ys.append(p[1])
        zs.append(p[2])
        nid.append(normals.index(nrm))
        it.append(inten)
    return xs, ys, zs, bytes(nid), it


def cloud_5e6_columns(jobs, n):
    step = 50_000
    blocks = [(s, min(s + step, n)) for s in range(0, n, step)]
    xs, ys, zs, nid, it = array("d"), array("d"), array("d"), bytearray(), array("I")
    with Pool(jobs) as pool:
        for a, b, c, d, e in pool.imap(scan_columns, blocks):
            xs.extend(a)
            ys.extend(b)
            zs.extend(c)
            nid.extend(d)
            it.extend(e)
    return xs, ys, zs, nid, it


def entry_5e6(jobs, n, max_points):
    xs, ys, zs, nid, it = cloud_5e6_columns(jobs, n)
    normals = [f[3] for f in L.FACETS]
    order, chunks = spatial_order(xs, ys, zs, max_points)

    def rec(i):
        nx, ny, nz = normals[nid[i]]
        # channels in name order: intensity, then normal
        return struct.pack("<QdddIddd", i, xs[i], ys[i], zs[i], L.f32chunk(it[i]), nx, ny, nz)

    return {
        "n_points": n,
        "max_chunk_points": max_points,
        "n_chunks": len(chunks),
        "spatial_digest": seq_digest(order, rec),
        "original_digest": seq_digest(range(n), rec),
    }


def build(jobs, full):
    out = {
        "comment": "GEO.OOC.STREAM vectors from make_stream_fixtures.py (independent Python reference). CC-BY-4.0. "
        "Clouds are those of layout_vectors.json and cloud_5e6 (same names and parameters). "
        "Sequence digest v1: SHA-256 over, per emitted point, index u64 | x y z raw f64 bits | the cloud's "
        "channels in name order (colour 3 bytes, intensity stored f32 bits, normal 3 raw f64, quality u16, "
        "scanner_id u32, timestamp_ns i64), little-endian. spatial_order lists original indexes in spatial "
        "order for clouds of at most 64 points.",
        "cases": small_cases(),
        "cloud_5e6_small": entry_5e6(jobs, L.N_SMALL, L.MAX_SMALL),
    }
    if full is not None:
        out["cloud_5e6"] = full
    return out


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    ap.add_argument("--small-only", action="store_true", help="keep the committed full cloud_5e6 block")
    args = ap.parse_args()
    if args.check or args.small_only:
        committed = json.loads(OUT.read_text(encoding="utf-8"))
        text = L.dump(build(args.jobs, committed.get("cloud_5e6")))
        if args.check:
            ok = OUT.read_text(encoding="utf-8") == text
            if not ok:
                print("stream_vectors.json differs from the reference", file=sys.stderr)
            sys.exit(0 if ok else 1)
        OUT.write_text(text, encoding="utf-8", newline="\n")
        return
    full = entry_5e6(args.jobs, L.N_5E6, L.MAX_5E6)
    OUT.write_text(L.dump(build(args.jobs, full)), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
