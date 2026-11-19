#!/usr/bin/env python3
"""Independent reference for GEO.TYPES.DIGEST (layout v1); writes digest_vectors.json.

Stdlib only (hashlib, struct, json). Shares no code with the Rust crate fid-geo-types:
it parses the GEO.TYPES fixture files itself, works on IEEE 754 bit patterns as Python
integers, builds each canonical layout as one bytes object and hashes it with hashlib.

Layout v1 (all integers little-endian), from spec/geo/types/digest/NODE.md:

    "FIDGEO" | 0x01 | kind u8 | frame [16]
    point set (kind 1), point cloud (kind 2), mesh (kind 3):
        n_points u64 | x[n_points] f64 | y[n_points] f64 | z[n_points] f64
        n_tris u64 | n_tris x (a u64, b u64, c u64)          (n_tris = 0 for points)
        n_channels u64
        per channel, in ascending name-byte order:
            name_len u16 | name (ASCII) | type u8 | n_records u64 | records
    CAD body handle (kind 4):
        body [16] | source_digest [32 raw bytes]

Float rules: every f64 is written as its IEEE 754 bits, except that -0.0 is written as
+0.0 (REQ-GEO.TYPES.DIGEST-002) and any NaN as 0x7ff8000000000000. f32 values get the
same treatment (-0.0 -> +0.0, any NaN -> 0x7fc00000). Valid elements hold no NaN in
f64 fields (constructors reject them), but f32 intensity is not validated.

Channel types and record encodings:

    1 f64       one f64                 4 u16    one u16
    2 f32       one f32                 5 u32    one u32
    3 u8x3      three bytes (r, g, b)   6 i64    one i64 (two's complement)
    7 u64       one u64
    8 f64x3col  n_records x-values, then n_records y-values, then n_records z-values
    9 origin    per record: scanner_id u32, x f64, y f64, z f64

Channels: colour (3), grid (7, two records rows and cols, only when declared),
intensity (2), normal (8), quality (4), scan_origins (9, only when non-empty),
scanner_id (5), timestamp_ns (6), and for meshes triangle_quality (4) and
triangle_scanner_id (5) with one record per triangle.

Vectors (inputs; the Rust test builds the same elements from the same rules):

    pointset_small         PointSet from pointset_small.txt, no attributes       (AC-1)
    mesh_cube              Mesh from mesh_cube.txt, no attributes                (AC-1)
    cloud_grid_2scans      PointCloud from cloud_grid_2scans.txt: intensity,
                           scanner_id, timestamp_ns, quality, both origins,
                           grid 128 x 48                                         (AC-1)
    pointset_small_attrs   pointset_small plus normals and intensity from
                           pointset_small_attrs.txt
    pointset_small_cloud   the pointset_small points as a PointCloud
    pointset_empty         PointSet with no points in the pointset_small frame
    mesh_cube_attrs        mesh_cube plus vertex colour (i*30, 255-i, 7) for vertex
                           i, triangle_scanner_id t%2+1 and triangle_quality 3*t
                           for triangle t
    cad_body               CadBodyRef, frame 00 01 .. 0f, body 10 11 .. 1f,
                           source digest SHA-256 of ASCII "fid cad source"

Run:  python tools/fixtures/geo_digest_vectors.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.
"""

import hashlib
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", ".."))
FIX = os.path.join(ROOT, "fixtures", "geo", "types")
OUT = os.path.join(FIX, "digest_vectors.json")

KIND = {"point_set": 1, "point_cloud": 2, "mesh": 3, "cad_body_ref": 4}
T_F64, T_F32, T_U8X3, T_U16, T_U32, T_I64, T_U64, T_F64X3COL, T_ORIGIN = range(1, 10)


def f64_bits(b):
    """Canonical f64 bits: -0.0 -> +0.0, any NaN -> one quiet NaN."""
    if b == 0x8000000000000000:
        return 0
    if (b >> 52) & 0x7FF == 0x7FF and b & ((1 << 52) - 1):
        return 0x7FF8000000000000
    return b


def f32_bits(b):
    if b == 0x80000000:
        return 0
    if (b >> 23) & 0xFF == 0xFF and b & ((1 << 23) - 1):
        return 0x7FC00000
    return b


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


def f64(bits):
    return u64(f64_bits(bits))


def f32(bits):
    return u32(f32_bits(bits))


def bits_of(x):
    """Bits of a Python float (used only for the hand-written decimal mesh)."""
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def header(kind, frame):
    assert len(frame) == 16
    return b"FIDGEO" + u8(1) + u8(KIND[kind]) + frame


def channel(name, ctype, n_records, records):
    nb = name.encode("ascii")
    return u16(len(nb)) + nb + u8(ctype) + u64(n_records) + records


def point_layout(kind, frame, xs, ys, zs, tris, channels):
    """xs/ys/zs are lists of f64 bit patterns; channels a dict name -> encoded channel."""
    n = len(xs)
    assert len(ys) == n and len(zs) == n
    out = [header(kind, frame), u64(n)]
    for col in (xs, ys, zs):
        out.append(b"".join(f64(b) for b in col))
    out.append(u64(len(tris)))
    out.append(b"".join(u64(a) + u64(b) + u64(c) for (a, b, c) in tris))
    out.append(u64(len(channels)))
    for name in sorted(channels, key=lambda s: s.encode("ascii")):
        out.append(channels[name])
    return b"".join(out)


def rows(name):
    with open(os.path.join(FIX, name), encoding="ascii") as fh:
        for line in fh:
            if line.startswith("#") or not line.strip():
                continue
            yield line.split()


def load_pointset_small():
    frame, xs, ys, zs = None, [], [], []
    for f in rows("pointset_small.txt"):
        if f[0] == "frame":
            frame = bytes.fromhex(f[1])
        elif f[0] == "p":
            assert int(f[1]) == len(xs)
            xs.append(int(f[2], 16))
            ys.append(int(f[3], 16))
            zs.append(int(f[4], 16))
        else:
            assert f[0] in ("count", "aabb_min", "aabb_max"), f[0]
    assert len(xs) == 1000
    return frame, xs, ys, zs


def load_small_attrs(frame):
    nx, ny, nz, inten = [], [], [], []
    for f in rows("pointset_small_attrs.txt"):
        if f[0] == "frame":
            assert bytes.fromhex(f[1]) == frame
        elif f[0] == "a":
            assert int(f[1]) == len(inten)
            nx.append(int(f[2], 16))
            ny.append(int(f[3], 16))
            nz.append(int(f[4], 16))
            inten.append(int(f[5], 16))
        else:
            assert f[0] == "count", f[0]
    assert len(inten) == 1000
    return nx, ny, nz, inten


def load_cube():
    frame, verts, tris = None, [], []
    for f in rows("mesh_cube.txt"):
        if f[0] == "frame":
            frame = bytes.fromhex(f[1])
        elif f[0] == "v":
            verts.append(tuple(bits_of(float(s)) for s in f[1:4]))
        elif f[0] == "t":
            tris.append(tuple(int(s) for s in f[1:4]))
        else:
            raise AssertionError(f[0])
    assert len(verts) == 8 and len(tris) == 12
    return frame, verts, tris


def load_grid():
    frame, shape, origins = None, None, []
    xs, ys, zs, sid, inten, ts, q = [], [], [], [], [], [], []
    for f in rows("cloud_grid_2scans.txt"):
        k = f[0]
        if k == "frame":
            frame = bytes.fromhex(f[1])
        elif k == "scan_grid":
            shape = (int(f[1]), int(f[2]))
        elif k == "origin":
            origins.append((int(f[1]), int(f[2], 16), int(f[3], 16), int(f[4], 16)))
        elif k == "p":
            assert int(f[1]) == len(xs)
            sid.append(int(f[2]))
            xs.append(int(f[3], 16))
            ys.append(int(f[4], 16))
            zs.append(int(f[5], 16))
            inten.append(int(f[6], 16))
            ts.append(int(f[7]))
            q.append(int(f[8]))
        else:
            assert k in ("count", "aabb_min", "aabb_max"), k
    assert shape == (64, 48) and len(xs) == 2 * 64 * 48
    return frame, xs, ys, zs, sid, inten, ts, q, origins, (2 * shape[0], shape[1])


def vectors():
    out = []

    def add(name, kind, layout):
        out.append({
            "name": name,
            "kind": kind,
            "layout_len": len(layout),
            "sha256": hashlib.sha256(layout).hexdigest(),
        })

    frame, xs, ys, zs = load_pointset_small()
    add("pointset_small", "point_set", point_layout("point_set", frame, xs, ys, zs, [], {}))

    cframe, verts, tris = load_cube()
    cx, cy, cz = ([v[k] for v in verts] for k in range(3))
    add("mesh_cube", "mesh", point_layout("mesh", cframe, cx, cy, cz, tris, {}))

    gframe, gx, gy, gz, sid, inten, ts, q, origins, grid = load_grid()
    n = len(gx)
    gch = {
        "intensity": channel("intensity", T_F32, n, b"".join(f32(b) for b in inten)),
        "scanner_id": channel("scanner_id", T_U32, n, b"".join(u32(v) for v in sid)),
        "timestamp_ns": channel("timestamp_ns", T_I64, n, b"".join(i64(v) for v in ts)),
        "quality": channel("quality", T_U16, n, b"".join(u16(v) for v in q)),
        "scan_origins": channel(
            "scan_origins", T_ORIGIN, len(origins),
            b"".join(u32(i) + f64(x) + f64(y) + f64(z) for (i, x, y, z) in origins)),
        "grid": channel("grid", T_U64, 2, u64(grid[0]) + u64(grid[1])),
    }
    add("cloud_grid_2scans", "point_cloud", point_layout("point_cloud", gframe, gx, gy, gz, [], gch))

    nx, ny, nz, sint = load_small_attrs(frame)
    sch = {
        "normal": channel("normal", T_F64X3COL, 1000, b"".join(f64(b) for b in nx + ny + nz)),
        "intensity": channel("intensity", T_F32, 1000, b"".join(f32(b) for b in sint)),
    }
    add("pointset_small_attrs", "point_set", point_layout("point_set", frame, xs, ys, zs, [], sch))

    add("pointset_small_cloud", "point_cloud", point_layout("point_cloud", frame, xs, ys, zs, [], {}))

    add("pointset_empty", "point_set", point_layout("point_set", frame, [], [], [], [], {}))

    colour = [(i * 30, 255 - i, 7) for i in range(8)]
    mch = {
        "colour": channel("colour", T_U8X3, 8, b"".join(bytes(c) for c in colour)),
        "triangle_scanner_id": channel("triangle_scanner_id", T_U32, 12,
                                       b"".join(u32(t % 2 + 1) for t in range(12))),
        "triangle_quality": channel("triangle_quality", T_U16, 12, b"".join(u16(3 * t) for t in range(12))),
    }
    add("mesh_cube_attrs", "mesh", point_layout("mesh", cframe, cx, cy, cz, tris, mch))

    body_frame = bytes(range(16))
    body = bytes(range(16, 32))
    source = hashlib.sha256(b"fid cad source").digest()
    add("cad_body", "cad_body_ref", header("cad_body_ref", body_frame) + body + source)
    return out


def render():
    doc = {
        "spec": "GEO.TYPES.DIGEST",
        "layout_version": 1,
        "generator": "tools/fixtures/geo_digest_vectors.py (stdlib hashlib, independent of fid-geo-types)",
        "license": "CC-BY-4.0",
        "cad_body_source_digest": "sha256:" + hashlib.sha256(b"fid cad source").hexdigest(),
        "vectors": vectors(),
    }
    return (json.dumps(doc, indent=2, sort_keys=True) + "\n").encode("ascii")


def main():
    data = render()
    if "--check" in sys.argv[1:]:
        with open(OUT, "rb") as fh:
            if fh.read() != data:
                print(f"stale: {OUT}", file=sys.stderr)
                return 1
        print("digest_vectors.json is up to date")
        return 0
    with open(OUT, "wb") as fh:
        fh.write(data)
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
