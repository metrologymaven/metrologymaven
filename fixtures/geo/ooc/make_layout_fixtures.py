#!/usr/bin/env python3
"""Independent reference for GEO.OOC.LAYOUT (CC-BY-4.0 project data).

Writes
  fixtures/geo/ooc/layout_vectors.json   chunk layouts of small clouds: cells, counts, chunk
                                         byte lengths and store addresses, manifest bytes and
                                         digests, original element digests
  fixtures/geo/ooc/cloud_5e6/expected.json
                                         AC-1 / AC-2: element digest and manifest digest of the
                                         5 x 10^6 point cloud regenerated from seed 61, plus
                                         sample points that pin the generator

Run:  python fixtures/geo/ooc/make_layout_fixtures.py [--check] [--jobs N]

Without --check it rewrites both files (cloud_5e6 takes several minutes; the Philox draws run
on --jobs processes, default the CPU count). With --check nothing is written: the small
vectors and the cloud_5e6 sample points are regenerated and compared with the committed
bytes; the cloud_5e6 digests are taken as committed (the full run is too slow for CI).

Stdlib only (struct, hashlib, json, multiprocessing). It shares no code with
crates/fid-geo-ooc. It models the pinned rules of the crate documentation directly:

* point to cell: per axis s = ((v - min) / (max - min)) * 2^21 in IEEE binary64 (CPython
  floats round to nearest even and never fuse), q = 0 if not s > 0 else min(floor(s), 2^21-1);
  the octant of a point at depth d + 1 is read from bit 20 - d of each quantised coordinate
  (x bit 1, y bit 2, z bit 4), not from a Morton code;
* split rule: a cell with more than max_chunk_points points below depth 21 splits into its
  eight octants, empty octants are dropped, leaves are emitted depth first in octant order;
* chunk v1 and manifest v1 byte layouts, CORE.STO.CAS addresses (fid-chunks@1 above 8 MiB);
* the GEO.TYPES.DIGEST layout v1 of a point cloud (kind 2), with -0.0 written as +0.0.
"""

import argparse
import hashlib
import json
import os
import struct
import sys
from multiprocessing import Pool
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT_VECTORS = HERE / "layout_vectors.json"
OUT_5E6 = HERE / "cloud_5e6" / "expected.json"

MAX_DEPTH = 21
QMAX = (1 << 21) - 1
CAS_CHUNK = 8 * 1024 * 1024

# ------------------------------------------------------------------ Philox4x32-10 (KRN.NUM.RNG)

M0, M1 = 0xD2511F53, 0xCD9E8D57
W0, W1 = 0x9E3779B9, 0xBB67AE85
MASK = 0xFFFFFFFF


def philox(c0, c1, c2, c3, k0, k1):
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK, (k1 + W1) & MASK
        p0, p1 = M0 * c0, M1 * c2
        c0, c1, c2, c3 = (p1 >> 32) ^ c1 ^ k0, p1 & MASK, (p0 >> 32) ^ c3 ^ k1, p0 & MASK
    return c0, c1, c2, c3


# Published Random123 known answer for philox4x32_10.
assert philox(0, 0, 0, 0, 0, 0) == (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)


def draw(seed, index, stream=0):
    w = philox(index & MASK, index >> 32, stream & MASK, stream >> 32, seed & MASK, seed >> 32)
    return (w[1] << 32) | w[0]


def uniform(u):
    return (u >> 11) * 2.0**-53


# ------------------------------------------------------------------ bytes


def u8(v):
    return struct.pack("<B", v)


def u16(v):
    return struct.pack("<H", v)


def u32(v):
    return struct.pack("<I", v)


def u64(v):
    return struct.pack("<Q", v)


def i64(v):
    return struct.pack("<q", v)


def f64raw(v):
    return struct.pack("<d", v)


def bits64(v):
    return struct.unpack("<Q", struct.pack("<d", v))[0]


def hex64(v):
    return f"{bits64(v):016x}"


def from_hex64(h):
    return struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0]


def f32bits(v):
    return struct.unpack("<I", struct.pack("<f", v))[0]


def f64canon(v):
    b = bits64(v)
    if b == 1 << 63:
        b = 0
    return u64(b)


def f64col(col):
    """A column of f64 as canonical layout bytes: -0.0 written as +0.0 (v == 0.0 holds for both)."""
    return struct.pack("<%dd" % len(col), *[0.0 if v == 0.0 else v for v in col])


def f32canon(b):
    if b == 1 << 31:
        b = 0
    return u32(b)


def total_key(x):
    b = bits64(x)
    return b ^ 0xFFFFFFFFFFFFFFFF if b >> 63 else b | (1 << 63)


# Channels: (name, type byte, width).
CHANNELS = [
    ("colour", 3, 3),
    ("intensity", 2, 4),
    ("normal", 8, 24),
    ("quality", 4, 2),
    ("scanner_id", 5, 4),
    ("timestamp_ns", 6, 8),
]

# ------------------------------------------------------------------ cloud model
#
# A cloud is a dict: frame (16 bytes), pts (list of (x, y, z) floats), ch (dict name -> list;
# normal: (nx, ny, nz) floats; intensity: f32 bit patterns; colour: (r, g, b); others ints),
# origins (list of (id, (x, y, z))), grid ((rows, cols) or None).


def element_digest(c):
    n = len(c["pts"])
    h = hashlib.sha256()
    h.update(b"FIDGEO" + u8(1) + u8(2) + c["frame"] + u64(n))
    for k in range(3):
        h.update(f64col([p[k] for p in c["pts"]]))
    h.update(u64(0))
    ch = c["ch"]
    present = []
    for name in ["colour", "grid", "intensity", "normal", "quality", "scan_origins", "scanner_id", "timestamp_ns"]:
        if name == "grid":
            if c["grid"] is not None:
                present.append(name)
        elif name == "scan_origins":
            if c["origins"]:
                present.append(name)
        elif name in ch:
            present.append(name)
    h.update(u64(len(present)))

    def head(name, ty, n_rec):
        h.update(u16(len(name)) + name.encode() + u8(ty) + u64(n_rec))

    for name in present:
        if name == "colour":
            head(name, 3, n)
            h.update(b"".join(bytes(v) for v in ch[name]))
        elif name == "grid":
            head(name, 7, 2)
            h.update(u64(c["grid"][0]) + u64(c["grid"][1]))
        elif name == "intensity":
            head(name, 2, n)
            h.update(b"".join(f32canon(b) for b in ch[name]))
        elif name == "normal":
            head(name, 8, n)
            for k in range(3):
                h.update(f64col([v[k] for v in ch[name]]))
        elif name == "quality":
            head(name, 4, n)
            h.update(b"".join(u16(v) for v in ch[name]))
        elif name == "scan_origins":
            head(name, 9, len(c["origins"]))
            for sid, o in c["origins"]:
                h.update(u32(sid) + b"".join(f64canon(v) for v in o))
        elif name == "scanner_id":
            head(name, 5, n)
            h.update(b"".join(u32(v) for v in ch[name]))
        elif name == "timestamp_ns":
            head(name, 6, n)
            h.update(b"".join(i64(v) for v in ch[name]))
    return h.hexdigest()


def tight_box(pts):
    lo = tuple(min((p[k] for p in pts), key=total_key) for k in range(3))
    hi = tuple(max((p[k] for p in pts), key=total_key) for k in range(3))
    return lo, hi


def quantiser(lo, hi):
    ext = [hi[k] - lo[k] for k in range(3)]
    assert all(e == e and e not in (float("inf"),) for e in ext)

    def q(v, k):
        if not ext[k] > 0.0:
            return 0
        s = ((v - lo[k]) / ext[k]) * 2097152.0
        if not s > 0.0:
            return 0
        return min(int(s), QMAX)

    return q


def channel_list(names):
    out = u8(len(names))
    for name, ty, _ in CHANNELS:
        if name in names:
            out += u8(len(name)) + name.encode() + u8(ty)
    return out


def encode_chunk(c, cell, idx):
    ch = c["ch"]
    names = [n for n, _, _ in CHANNELS if n in ch]
    out = [b"FIDCHK", u8(1), u64(cell), u64(len(idx)), channel_list(names)]
    out.append(b"".join(u64(i) for i in idx))
    for k in range(3):
        out.append(b"".join(f64raw(c["pts"][i][k]) for i in idx))
    for name in names:
        col = ch[name]
        if name == "colour":
            out.append(b"".join(bytes(col[i]) for i in idx))
        elif name == "intensity":
            out.append(b"".join(u32(col[i]) for i in idx))
        elif name == "normal":
            for k in range(3):
                out.append(b"".join(f64raw(col[i][k]) for i in idx))
        elif name == "quality":
            out.append(b"".join(u16(col[i]) for i in idx))
        elif name == "scanner_id":
            out.append(b"".join(u32(col[i]) for i in idx))
        elif name == "timestamp_ns":
            out.append(b"".join(i64(col[i]) for i in idx))
    return b"".join(out)


def cas_address(data):
    if len(data) <= CAS_CHUNK:
        return hashlib.sha256(data).hexdigest()
    parts = [data[i : i + CAS_CHUNK] for i in range(0, len(data), CAS_CHUNK)]
    digests = ",".join('"sha256:' + hashlib.sha256(p).hexdigest() + '"' for p in parts)
    manifest = '{"chunk_size":"%d","chunks":[%s],"format":"fid-chunks@1","size":"%d"}' % (CAS_CHUNK, digests, len(data))
    return hashlib.sha256(manifest.encode()).hexdigest()


def leaves(qs, max_points):
    """Leaves of the partition: list of (cell code, sorted point indexes), depth first."""
    out = []
    n = len(qs)
    stack = [(0, 0, list(range(n)))]  # depth, prefix, members
    while stack:
        depth, prefix, members = stack.pop()
        if len(members) <= max_points or depth == MAX_DEPTH:
            out.append(((1 << (3 * depth)) | prefix, sorted(members)))
            continue
        bit = 20 - depth
        kids = [[] for _ in range(8)]
        for i in members:
            qx, qy, qz = qs[i]
            o = ((qx >> bit) & 1) | (((qy >> bit) & 1) << 1) | (((qz >> bit) & 1) << 2)
            kids[o].append(i)
        for o in range(7, -1, -1):
            if kids[o]:
                stack.append((depth + 1, prefix * 8 + o, kids[o]))
    return out


def layout(c, max_points, bounds=None, keep_bytes=False):
    pts = c["pts"]
    lo, hi = bounds if bounds is not None else tight_box(pts)
    q = quantiser(lo, hi)
    qs = [(q(p[0], 0), q(p[1], 1), q(p[2], 2)) for p in pts]
    names = [n for n, _, _ in CHANNELS if n in c["ch"]]
    chunks = []
    blobs = []
    for cell, idx in leaves(qs, max_points):
        data = encode_chunk(c, cell, idx)
        chunks.append({"cell": str(cell), "count": len(idx), "bytes": len(data), "blob": cas_address(data)})
        if keep_bytes:
            blobs.append(data.hex())
    original = element_digest(c)
    m = [b"FIDCMF", u8(1), c["frame"]]
    m += [f64raw(v) for v in lo] + [f64raw(v) for v in hi]
    m += [u64(max_points), u8(MAX_DEPTH), u64(len(pts)), channel_list(names), u64(len(c["origins"]))]
    for sid, o in c["origins"]:
        m += [u32(sid)] + [f64raw(v) for v in o]
    m += [u8(0)] if c["grid"] is None else [u8(1), u64(c["grid"][0]), u64(c["grid"][1])]
    m += [bytes.fromhex(original), u64(len(chunks))]
    for e in chunks:
        m += [u64(int(e["cell"])), u64(e["count"]), u64(e["bytes"]), bytes.fromhex(e["blob"])]
    mbytes = b"".join(m)
    out = {
        "aabb_min": [hex64(v) for v in lo],
        "aabb_max": [hex64(v) for v in hi],
        "chunks": chunks,
        "original_digest": original,
        "manifest_digest": hashlib.sha256(mbytes).hexdigest(),
        "manifest_bytes": len(mbytes),
    }
    if keep_bytes:
        out["chunk_hex"] = blobs
        out["manifest_hex"] = mbytes.hex()
    return out


# ------------------------------------------------------------------ cases


def frame_of(name):
    return hashlib.sha256(("GEO.OOC.LAYOUT " + name + " frame").encode()).digest()[:16]


def case_tiny():
    pts = [
        (0.0, 0.0, 0.0),
        (1.0, 2.0, 4.0),
        (0.5, 1.0, 2.0),
        (-0.0, 0.5, 1.0),
        (0.25, 0.5, 3.0),
        (0.75, 1.5, 0.5),
        (0.5, 0.0, 0.0),
        (0.4999999999999999, 1.0, 2.0),
        (1.0, 0.0, 4.0),
        (0.125, 1.75, 2.5),
        (0.9, 1.9, 3.9),
        (0.5, 1.0, 2.0),
    ]
    axes = [(0.0, 0.0, 1.0), (1.0, 0.0, 0.0), (0.0, -1.0, 0.0), (-0.0, 0.0, 1.0), (0.6, 0.0, 0.8), (0.0, 0.8, -0.6)]
    ch = {
        "colour": [(i, 255 - i, 7 * i % 256) for i in range(12)],
        "intensity": [f32bits(0.5), 1 << 31, 0x7F800000, f32bits(0.25)] + [f32bits(i / 16.0) for i in range(8)],
        "normal": [axes[i % 6] for i in range(12)],
        "quality": [i * 4097 % 65536 for i in range(12)],
        "scanner_id": [1 + i % 2 for i in range(12)],
        "timestamp_ns": [-(10**15) + 1000 * i for i in range(12)],
    }
    return {
        "frame": frame_of("tiny"),
        "pts": pts,
        "ch": ch,
        "origins": [(1, (0.0, 0.0, 5.0)), (2, (1.0, 2.0, -5.0))],
        "grid": (3, 4),
    }


def case_seeded(seed, n):
    pts, normal, inten, colour, quality, scanner, ts = [], [], [], [], [], [], []
    axes = [(1.0, 0.0, 0.0), (-1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (0.0, -1.0, 0.0), (0.0, 0.0, 1.0), (0.0, 0.0, -1.0)]
    for i in range(n):
        d = [draw(seed, 8 * i + k) for k in range(8)]
        pts.append((-1.0 + uniform(d[0]) * 3.0, -1.0 + uniform(d[1]) * 3.0, -1.0 + uniform(d[2]) * 3.0))
        normal.append(axes[d[3] % 6])
        inten.append(f32bits((d[4] >> 40) * 2.0**-24))
        colour.append((d[5] & 255, (d[5] >> 8) & 255, (d[5] >> 16) & 255))
        quality.append(d[6] & 0xFFFF)
        scanner.append(1 + (d[6] >> 16) % 3)
        ts.append(struct.unpack("<q", struct.pack("<Q", d[7]))[0])
    ch = {"colour": colour, "intensity": inten, "normal": normal, "quality": quality, "scanner_id": scanner, "timestamp_ns": ts}
    return {"frame": frame_of("seeded"), "pts": pts, "ch": ch, "origins": [(1, (0.0, 0.0, 9.0)), (2, (3.0, 0.0, 9.0)), (3, (0.0, 3.0, 9.0))], "grid": None}


def case_boundaries():
    pts = [(0.0, 0.0, 0.0), (1.0, 1.0, 1.0)]
    for x in (0.5, 0.25, 0.75):
        for y in (0.5, 0.125):
            pts.append((x, y, 0.5))
    pts += [(1.0, 0.0, 1.0), (0.0, 1.0, 0.0), (-0.0, 0.5, 0.5)]
    pts += [(0.3, 0.3, 0.3)] * 3
    pts.append((0.3, 0.3, 0.30000000000000004))
    return {"frame": frame_of("boundaries"), "pts": pts, "ch": {}, "origins": [], "grid": None}


def gen_uniform(seed, n, lo, span, extra):
    pts = []
    for i in range(n):
        pts.append(tuple(lo[k] + uniform(draw(seed, 3 * i + k)) * span[k] for k in range(3)))
    return pts + [tuple(p) for p in extra]


def case_from_gen(name, gen):
    if gen["kind"] == "uniform":
        pts = gen_uniform(gen["seed"], gen["n"], gen["lo"], gen["span"], gen["extra"])
    else:
        pts = [tuple(gen["point"])] * gen["n"] + [tuple(p) for p in gen["extra"]]
    return {"frame": frame_of(name), "pts": pts, "ch": {}, "origins": [], "grid": None}


def neg_zero(sign):
    z = -0.0 if sign else 0.0
    pts = [(z, 0.0, 0.0), (1.0, z, 1.0), (0.5, 0.5, z), (z, z, z), (1.0, 1.0, 1.0), (0.25, z, 0.75)]
    return {"frame": frame_of("neg_zero"), "pts": pts, "ch": {}, "origins": [], "grid": None}


def cloud_json(c):
    out = {
        "frame": c["frame"].hex(),
        "points": [[hex64(v) for v in p] for p in c["pts"]],
        "scan_origins": [[sid] + [hex64(v) for v in o] for sid, o in c["origins"]],
        "grid": list(c["grid"]) if c["grid"] is not None else None,
        "channels": {},
    }
    ch = c["ch"]
    for name in ch:
        if name == "normal":
            out["channels"][name] = [[hex64(v) for v in n] for n in ch[name]]
        elif name == "intensity":
            out["channels"][name] = [f"{b:08x}" for b in ch[name]]
        elif name == "colour":
            out["channels"][name] = [list(v) for v in ch[name]]
        elif name == "timestamp_ns":
            out["channels"][name] = [str(v) for v in ch[name]]
        else:
            out["channels"][name] = list(ch[name])
    return out


def vectors():
    cases = []

    def add(name, c, max_points, bounds=None, keep=False, gen=None, note=""):
        entry = {"name": name, "note": note, "max_chunk_points": max_points}
        if gen is None:
            entry["cloud"] = cloud_json(c)
        else:
            entry["generator"] = gen
            entry["frame"] = c["frame"].hex()
        entry["bounds"] = None if bounds is None else {"min": [hex64(v) for v in bounds[0]], "max": [hex64(v) for v in bounds[1]]}
        entry["expected"] = layout(c, max_points, bounds, keep)
        cases.append(entry)
        return entry

    add("tiny_all_channels", case_tiny(), 4, keep=True,
        note="every channel, -0.0 coordinate and normal, f32 -0.0 and +inf intensity, points on split planes and the max corner, a duplicate")
    add("seeded_2000", case_seeded(7, 2000), 64,
        note="Philox seed 7: 8 draws per point (x, y, z, normal, intensity, colour, quality and scanner, timestamp)")
    add("boundaries", case_boundaries(), 1,
        note="points exactly on split planes; three duplicates and a one-ulp neighbour reach the depth limit")
    add("boundaries_bounds", case_boundaries(), 2, bounds=((-1.0, -1.0, -1.0), (2.0, 2.0, 2.0)),
        note="the same points partitioned over given larger bounds")
    gen = {"kind": "uniform", "seed": 11, "n": 500, "lo": [0.0, 0.0, 2.5], "span": [1.0, 2.0, 0.0], "extra": []}
    add("flat_z", case_from_gen("flat_z", gen), 50, gen=gen, note="zero z extent: every z quantises to 0")
    gen = {"kind": "uniform", "seed": 13, "n": 20000, "lo": [0.25, 0.25, 0.25], "span": [2.0**-12] * 3,
           "extra": [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0]]}
    add("clustered_20000", case_from_gen("clustered_20000", gen), 64, gen=gen,
        note="all but two points in one depth-3 bucket, so a small writer budget splits files")
    gen = {"kind": "duplicates", "n": 300000, "point": [0.0, 0.0, 0.0], "extra": [[1.0, 1.0, 1.0]]}
    add("duplicates_object", case_from_gen("duplicates_object", gen), 1000, gen=gen,
        note="a depth-21 chunk of 300 000 points, 9.6 MB, stored as a CORE.STO object (fid-chunks@1 address)")
    a = add("neg_zero_minus", neg_zero(True), 2, keep=True, note="as neg_zero_plus with -0.0 for some zeros")
    b = add("neg_zero_plus", neg_zero(False), 2, keep=True, note="as neg_zero_minus with +0.0")
    assert a["expected"]["original_digest"] == b["expected"]["original_digest"]
    assert a["expected"]["manifest_digest"] != b["expected"]["manifest_digest"]
    big = [e for e in cases if e["name"] == "duplicates_object"][0]
    assert any(ch["bytes"] > CAS_CHUNK for ch in big["expected"]["chunks"])
    return {
        "comment": "GEO.OOC.LAYOUT vectors from make_layout_fixtures.py (independent Python reference). CC-BY-4.0. "
        "Floats are f64 bit patterns in hex (intensity: f32 bit patterns); digests and blobs are SHA-256 hex; "
        "cell codes and timestamps are decimal strings (they exceed 2^53).",
        "cases": cases,
    }


# ------------------------------------------------------------------ cloud_5e6

SEED_5E6 = 61
N_5E6 = 5_000_000
MAX_5E6 = 65536
NOISE = 0.002
# Facets of a scan-like surface: origin, edge a, edge b, unit normal (|n| within 1e-9 of 1).
FACETS = [
    ((0.0, 0.0, 0.0), (2.0, 0.0, 0.0), (0.0, 2.0, 0.0), (0.0, 0.0, 1.0)),
    ((0.0, 0.0, 0.5), (0.8, 0.0, -0.6), (0.0, 2.0, 0.0), (0.6, 0.0, 0.8)),
    ((2.0, 0.0, 0.5), (-0.8, 0.0, -0.6), (0.0, 2.0, 0.0), (-0.6, 0.0, 0.8)),
    ((0.0, 0.0, 1.0), (2.0, 0.0, 0.0), (0.0, 0.8, -0.6), (0.0, 0.6, 0.8)),
    ((0.0, 2.0, 1.0), (2.0, 0.0, 0.0), (0.0, -0.8, -0.6), (0.0, -0.6, 0.8)),
    ((2.0, 0.0, 0.0), (0.0, 2.0, 0.0), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0)),
]


def scan_point(i):
    """Point i of cloud_5e6: draws 5i .. 5i+4 of Philox seed 61, stream 0."""
    d = [draw(SEED_5E6, 5 * i + k) for k in range(5)]
    o, a, b, nrm = FACETS[d[0] % 6]
    u, v, w = uniform(d[1]), uniform(d[2]), uniform(d[3])
    noise = (w - 0.5) * NOISE
    p = tuple(((o[k] + u * a[k]) + v * b[k]) + noise * nrm[k] for k in range(3))
    return p, nrm, f32bits((d[4] >> 40) * 2.0**-24)


def scan_block(rng):
    return [scan_point(i) for i in range(*rng)]


def cloud_5e6(jobs):
    step = 50_000
    blocks = [(s, min(s + step, N_5E6)) for s in range(0, N_5E6, step)]
    with Pool(jobs) as pool:
        parts = pool.map(scan_block, blocks)
    rows = [r for part in parts for r in part]
    c = {
        "frame": frame_of("cloud_5e6"),
        "pts": [r[0] for r in rows],
        "ch": {"normal": [r[1] for r in rows], "intensity": [r[2] for r in rows]},
        "origins": [],
        "grid": None,
    }
    return c


SAMPLES = [0, 1, 2, 3, 999, 65536, 1234567, 4999999]


def sample_points():
    out = []
    for i in SAMPLES:
        p, n, inten = scan_point(i)
        out.append({"index": i, "point": [hex64(v) for v in p], "normal": [hex64(v) for v in n], "intensity": f"{inten:08x}"})
    return out


def expected_5e6(jobs):
    c = cloud_5e6(jobs)
    lay = layout(c, MAX_5E6)
    return {
        "comment": "GEO.OOC.LAYOUT AC-1/AC-2 fixture cloud_5e6, regenerated from the seed in tests; values from "
        "make_layout_fixtures.py (CC-BY-4.0). Point i takes Philox4x32-10 seed 61 stream 0 draws 5i..5i+4: "
        "facet = d0 mod 6, u, v, w = uniform(d1..d3), noise = (w - 0.5) * 0.002, coordinate k = "
        "((o[k] + u * a[k]) + v * b[k]) + noise * n[k], normal = n, intensity = f32((d4 >> 40) * 2^-24).",
        "seed": SEED_5E6,
        "n_points": N_5E6,
        "max_chunk_points": MAX_5E6,
        "noise": NOISE,
        "facets": [[list(o), list(a), list(b), list(n)] for o, a, b, n in FACETS],
        "frame": c["frame"].hex(),
        "samples": sample_points(),
        "element_digest": lay["original_digest"],
        "manifest_digest": lay["manifest_digest"],
        "n_chunks": len(lay["chunks"]),
        "aabb_min": lay["aabb_min"],
        "aabb_max": lay["aabb_max"],
    }


def dump(obj):
    return json.dumps(obj, indent=1, sort_keys=True) + "\n"


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--check", action="store_true")
    ap.add_argument("--jobs", type=int, default=os.cpu_count() or 1)
    args = ap.parse_args()
    vec = dump(vectors())
    if args.check:
        ok = OUT_VECTORS.read_text(encoding="utf-8") == vec
        committed = json.loads(OUT_5E6.read_text(encoding="utf-8"))
        ok_samples = committed["samples"] == sample_points() and committed["frame"] == frame_of("cloud_5e6").hex()
        if not ok:
            print("layout_vectors.json differs from the reference", file=sys.stderr)
        if not ok_samples:
            print("cloud_5e6 samples differ from the generator", file=sys.stderr)
        sys.exit(0 if ok and ok_samples else 1)
    OUT_VECTORS.write_text(vec, encoding="utf-8", newline="\n")
    OUT_5E6.parent.mkdir(parents=True, exist_ok=True)
    OUT_5E6.write_text(dump(expected_5e6(args.jobs)), encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
