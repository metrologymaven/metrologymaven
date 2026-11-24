#!/usr/bin/env python3
"""Independent reference for SCAN.MESHOP.CUT.INVERT; writes invert_cases.txt next to this file.

Stdlib only (hashlib, struct). Shares no code with the Rust crate fid-scan-mesh: it reads
the shared fixtures fixtures/scan/mesh_closed_cube.txt and mesh_unused_vertex.txt itself
and applies the rules of spec/scan/meshop/cut/invert/NODE.md and the SCAN.MESHOP.PROV
builder rules.

Rules applied:

  * Inversion of a target set T of triangles: every input vertex is kept in index order;
    triangle (a, b, c) in T becomes (a, c, b) (second and third index swapped); every
    other triangle is kept as it is (REQ-001). Triangle order is kept.
  * Flags (PROV): a reversed triangle has its one source's flag, a kept triangle keeps its
    flag; an input without the attribute is all-false. Every output has the attribute.
  * Vertex normal negation (REQ-002): vertex v is negated when it has at least one
    incident triangle and every incident triangle is in T (an unreferenced vertex is
    never negated; a repeated index counts once).
  * GEO.TYPES element digest (layout v1, mesh kind 3, no channels):
        "FIDGEO" | 0x01 | 0x03 | frame [16] | n u64 | x[n] f64 | y[n] f64 | z[n] f64
        | n_tris u64 | (a, b, c) u64 each | n_channels u64 = 0     (little-endian)
  * Derived-mesh digest (PROV): "FIDMOP" | 0x01 | element digest [32] | n_tris u64
    | flags [ceil(n_tris / 8)], flags LSB-first.
  * Orientation facts (GEO.TYPES.MESH): each non-degenerate triangle (a, b, c) gives the
    directed edges a->b, b->c, c->a; an undirected edge used once is a boundary edge and an
    undirected edge used exactly twice in the same direction is a misoriented pair.
  * Inward check (AC-1): the cube centre is (0.5, 0.5, 0.5); a triangle points inward when
    its normal (b - a) x (c - a) has a negative dot product with (centroid - centre).

Cases (cube = mesh_closed_cube, 12 triangles; z0 face = triangles 0 and 1;
corner0 = the six triangles incident to vertex 0):

  whole        invert every triangle of the raw cube (AC-1)
  twice        invert every triangle of the `whole` output (AC-1, REQ-003)
  face_z0      invert triangles 0 and 1 of the raw cube (AC-2)
  flag_once    invert triangles 0 and 1 of the cube flagged with triangle 0 synthesized
  flag_twice   invert triangles 0 and 1 of the `flag_once` output (REQ-003 on a flagged
               input: the version digest comes back)
  negate_*     normal negation sets: whole, z0, corner0 on the cube; whole on
               mesh_unused_vertex (vertex 2 is unreferenced)

Run:  python fixtures/scan/meshop/make_invert_cases.py [--check]
With --check nothing is written; exits non-zero if invert_cases.txt differs.
CC-BY-4.0.
"""

import hashlib
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
SCAN = os.path.dirname(HERE)
OUT = os.path.join(HERE, "invert_cases.txt")


def load_mesh(name):
    frame, verts, tris = None, [], []
    with open(os.path.join(SCAN, name), encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            k, *rest = line.split(" ")
            if k == "frame":
                frame = bytes.fromhex(rest[0])
            elif k == "v":
                verts.append(tuple(float(s) for s in rest))
            elif k == "t":
                tris.append(tuple(int(s) for s in rest))
            else:
                raise AssertionError(k)
    assert frame is not None and len(frame) == 16
    return frame, verts, tris


def f64le(x):
    if x == 0.0:
        x = 0.0
    return struct.pack("<d", x)


def u64le(v):
    return struct.pack("<Q", v)


def element_digest(frame, verts, tris):
    out = [b"FIDGEO", bytes([1, 3]), frame, u64le(len(verts))]
    for k in range(3):
        out.append(b"".join(f64le(p[k]) for p in verts))
    out.append(u64le(len(tris)))
    out.append(b"".join(u64le(a) + u64le(b) + u64le(c) for (a, b, c) in tris))
    out.append(u64le(0))
    return hashlib.sha256(b"".join(out)).digest()


def flag_bytes(flags):
    b = bytearray((len(flags) + 7) // 8)
    for t, f in enumerate(flags):
        if f:
            b[t // 8] |= 1 << (t % 8)
    return bytes(b)


def derived_digest(frame, verts, tris, flags):
    assert len(flags) == len(tris)
    e = element_digest(frame, verts, tris)
    return hashlib.sha256(b"FIDMOP" + bytes([1]) + e + u64le(len(tris)) + flag_bytes(flags)).digest()


def wire(d):
    return "sha256:" + d.hex()


def invert(tris, flags, target):
    out_t, out_f = [], []
    for t, (a, b, c) in enumerate(tris):
        out_t.append((a, c, b) if t in target else (a, b, c))
        out_f.append(flags[t])
    return out_t, out_f


def negated(n_verts, tris, target):
    inc = [0] * n_verts
    tgt = [0] * n_verts
    for t, tri in enumerate(tris):
        for v in set(tri):
            inc[v] += 1
            if t in target:
                tgt[v] += 1
    return [v for v in range(n_verts) if inc[v] > 0 and tgt[v] == inc[v]]


def orientation(tris):
    directed = {}
    for tri in tris:
        if len(set(tri)) < 3:
            continue
        a, b, c = tri
        for e in ((a, b), (b, c), (c, a)):
            key = (min(e), max(e))
            directed.setdefault(key, []).append(e)
    boundary = sum(1 for uses in directed.values() if len(uses) == 1)
    misoriented = sum(1 for uses in directed.values() if len(uses) == 2 and uses[0] == uses[1])
    return boundary, misoriented


def sub(p, q):
    return (p[0] - q[0], p[1] - q[1], p[2] - q[2])


def cross(u, v):
    return (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])


def dot(u, v):
    return u[0] * v[0] + u[1] * v[1] + u[2] * v[2]


def inward_signs(verts, tris):
    """'-' where the triangle normal points toward the cube centre, '+' outward."""
    signs = []
    for a, b, c in tris:
        pa, pb, pc = verts[a], verts[b], verts[c]
        n = cross(sub(pb, pa), sub(pc, pa))
        centroid = tuple((pa[k] + pb[k] + pc[k]) / 3.0 for k in range(3))
        d = dot(n, sub(centroid, (0.5, 0.5, 0.5)))
        assert d != 0.0
        signs.append("-" if d < 0 else "+")
    return "".join(signs)


def tri_text(tris):
    return " ".join("%d,%d,%d" % t for t in tris)


def mesh_case(name, frame, verts, in_digest, tris, flags):
    boundary, misoriented = orientation(tris)
    return [
        "case %s" % name,
        "input_digest %s" % in_digest,
        "triangles %s" % tri_text(tris),
        "flags %s" % "".join("1" if f else "0" for f in flags),
        "element_digest %s" % wire(element_digest(frame, verts, tris)),
        "output_digest %s" % wire(derived_digest(frame, verts, tris, flags)),
        "boundary_edges %d" % boundary,
        "misoriented_pairs %d" % misoriented,
        "orientation %s" % inward_signs(verts, tris),
        "end",
    ]


def build():
    frame, verts, tris = load_mesh("mesh_closed_cube.txt")
    n = len(tris)
    every = set(range(n))
    z0 = {0, 1}
    corner0 = {t for t, tri in enumerate(tris) if 0 in tri}
    assert corner0 == {0, 1, 4, 5, 8, 9}
    zeros = [False] * n
    raw = wire(element_digest(frame, verts, tris))

    lines = [
        "# Expected results of SCAN.MESHOP.CUT.INVERT, written by make_invert_cases.py from the",
        "# spec's rules (not by running the Rust crate). CC-BY-4.0.",
        "case input",
        "element_digest %s" % raw,
        "orientation %s" % inward_signs(verts, tris),
        "misoriented_pairs %d" % orientation(tris)[1],
        "end",
    ]

    w_t, w_f = invert(tris, zeros, every)
    lines += mesh_case("whole", frame, verts, raw, w_t, w_f)
    w_digest = wire(derived_digest(frame, verts, w_t, w_f))
    t_t, t_f = invert(w_t, w_f, every)
    assert t_t == tris
    lines += mesh_case("twice", frame, verts, w_digest, t_t, t_f)

    f_t, f_f = invert(tris, zeros, z0)
    lines += mesh_case("face_z0", frame, verts, raw, f_t, f_f)

    marked = [t == 0 for t in range(n)]
    marked_digest = wire(derived_digest(frame, verts, tris, marked))
    o_t, o_f = invert(tris, marked, z0)
    lines += mesh_case("flag_once", frame, verts, marked_digest, o_t, o_f)
    once_digest = wire(derived_digest(frame, verts, o_t, o_f))
    tw_t, tw_f = invert(o_t, o_f, z0)
    assert (tw_t, tw_f) == (tris, marked)
    lines += mesh_case("flag_twice", frame, verts, once_digest, tw_t, tw_f)

    def neg_case(name, n_verts, tris_, target):
        vs = negated(n_verts, tris_, target)
        return ["case %s" % name, "targets %s" % " ".join(str(t) for t in sorted(target)),
                ("negated " + " ".join(str(v) for v in vs)).rstrip(), "end"]

    lines += neg_case("negate_whole", len(verts), tris, every)
    lines += neg_case("negate_z0", len(verts), tris, z0)
    lines += neg_case("negate_corner0", len(verts), tris, corner0)
    _, u_verts, u_tris = load_mesh("mesh_unused_vertex.txt")
    lines += neg_case("negate_unused_whole", len(u_verts), u_tris, set(range(len(u_tris))))
    return "\n".join(lines) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        with open(OUT, encoding="utf-8") as f:
            if f.read() != text:
                print("invert_cases.txt is stale; run make_invert_cases.py", file=sys.stderr)
                return 1
        return 0
    with open(OUT, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
