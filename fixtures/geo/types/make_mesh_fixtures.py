#!/usr/bin/env python3
"""Independent reference for GEO.TYPES.MESH AC-1 to AC-3 (CC-BY-4.0 project data).

Stdlib only. The meshes are hand-written below; the expected topology facts are computed
here with a different method from the Rust crate (a dict of undirected edges and a
breadth-first search over triangle adjacency, where the crate uses a sorted half-edge
table and union-find), from the spec rules of GEO.TYPES.MESH:

* a triangle is degenerate when two of its three indexes are equal; degenerate
  triangles are counted and take no further part (no edges, no component);
* every other triangle (a, b, c) contributes the directed edges a->b, b->c, c->a;
* an undirected edge used once is a boundary edge, used more than twice a non-manifold
  edge, used exactly twice in the same direction an inconsistently oriented pair;
* components are the connected classes of non-degenerate triangles, two triangles being
  adjacent when they share an undirected edge;
* closed = no boundary edge, no non-manifold edge, and at least one non-degenerate
  triangle; oriented = no misoriented pair and no non-manifold edge.

Files (next to this script):

mesh_cube.txt                 existing GEO.TYPES.ENV fixture; checked, never rewritten
mesh_cube.facts.json          expected facts of the closed cube (AC-1)
mesh_open_flipped.txt         the cube without triangle 11 and with triangle 0 reversed (AC-2)
mesh_open_flipped.facts.json
mesh_nonmanifold.txt          three triangles on the edge 0-1 (AC-3)
mesh_nonmanifold.facts.json
mesh_mobius.txt               a six-triangle Moebius strip (and .facts.json)
mesh_bowtie.txt               two triangles sharing one vertex (and .facts.json)
mesh_degenerate_dup.txt       the cube plus a degenerate triangle and a duplicate of triangle 2
mesh_degenerate_dup.facts.json

Run:  python fixtures/geo/types/make_mesh_fixtures.py [--check]

With --check nothing is written; the script exits non-zero if a file differs.
"""

import json
import os
import sys
from collections import deque

HERE = os.path.dirname(os.path.abspath(__file__))

CUBE_FRAME = "0f1e2d3c4b5a69788796a5b4c3d2e1f0"
CUBE_V = [
    (0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0),
    (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1),
]
CUBE_T = [
    (0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4),
    (3, 7, 6), (3, 6, 2), (0, 4, 7), (0, 7, 3), (1, 2, 6), (1, 6, 5),
]


def open_flipped():
    t = [tri for i, tri in enumerate(CUBE_T) if i != 11]
    a, b, c = t[0]
    t[0] = (a, c, b)
    return t


# Moebius strip: three quads between rails a_i = i and b_i = 3 + i; the last quad joins
# (a2, b2) to (b0, a0), i.e. with a half twist.
MOBIUS_V = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (0, 1, 0), (1, 1, 0), (2, 1, 1)]
MOBIUS_T = [(0, 3, 1), (3, 4, 1), (1, 4, 2), (4, 5, 2), (2, 5, 3), (5, 0, 3)]

# Bow tie: two triangles meeting only at vertex 0 (a non-manifold vertex, not reported).
BOWTIE_V = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (-1, 0, 0), (-1, -1, 0)]
BOWTIE_T = [(0, 1, 2), (0, 3, 4)]

NONMANIFOLD_V = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1)]
NONMANIFOLD_T = [(0, 1, 2), (1, 0, 3), (0, 1, 4)]

CASES = [
    # name, header, frame, vertices, triangles, rewrite the mesh file?
    ("mesh_cube", None, CUBE_FRAME, CUBE_V, CUBE_T, False),
    (
        "mesh_open_flipped",
        "GEO.TYPES.MESH AC-2 fixture: mesh_cube without triangle 11 and with triangle 0\n"
        "# reversed (0 2 1 -> 0 1 2). Hand-written; facts in mesh_open_flipped.facts.json.",
        CUBE_FRAME,
        CUBE_V,
        open_flipped(),
        True,
    ),
    (
        "mesh_nonmanifold",
        "GEO.TYPES.MESH AC-3 fixture: three triangles sharing the edge 0-1.\n"
        "# Hand-written; facts in mesh_nonmanifold.facts.json.",
        "00112233445566778899aabbccddeeff",
        NONMANIFOLD_V,
        NONMANIFOLD_T,
        True,
    ),
    (
        "mesh_mobius",
        "GEO.TYPES.MESH fixture: a Moebius strip of six triangles (one half twist).\n"
        "# Hand-labelled: 6 boundary edges, 1 misoriented pair, 1 component, not oriented.",
        "0123456789abcdef0123456789abcdef",
        MOBIUS_V,
        MOBIUS_T,
        True,
    ),
    (
        "mesh_bowtie",
        "GEO.TYPES.MESH fixture: two triangles sharing only vertex 0.\n"
        "# Hand-labelled: 2 components, 6 boundary edges; the non-manifold vertex is not a fact.",
        "fedcba9876543210fedcba9876543210",
        BOWTIE_V,
        BOWTIE_T,
        True,
    ),
    (
        "mesh_degenerate_dup",
        "GEO.TYPES.MESH fixture: mesh_cube plus a degenerate triangle (0 0 1) and a\n"
        "# duplicate of triangle 2 (4 5 6). Facts are reported, nothing is repaired.",
        CUBE_FRAME,
        CUBE_V,
        CUBE_T + [(0, 0, 1), (4, 5, 6)],
        True,
    ),
]


def facts(nv, tris):
    for tri in tris:
        for i in tri:
            assert 0 <= i < nv, "fixture index out of range"
    degenerate = 0
    live = []
    for k, (a, b, c) in enumerate(tris):
        if a == b or b == c or a == c:
            degenerate += 1
        else:
            live.append(k)
    uses = {}  # undirected edge -> list of (triangle, directed edge)
    for k in live:
        a, b, c = tris[k]
        for (p, q) in ((a, b), (b, c), (c, a)):
            uses.setdefault(frozenset((p, q)), []).append((k, (p, q)))
    boundary = nonmanifold = misoriented = 0
    adj = {k: set() for k in live}
    for edge, us in uses.items():
        if len(us) == 1:
            boundary += 1
        elif len(us) > 2:
            nonmanifold += 1
        elif us[0][1] == us[1][1]:
            misoriented += 1
        for (k1, _) in us:
            for (k2, _) in us:
                if k1 != k2:
                    adj[k1].add(k2)
    seen = set()
    components = 0
    for k in live:
        if k in seen:
            continue
        components += 1
        queue = deque([k])
        seen.add(k)
        while queue:
            x = queue.popleft()
            for y in adj[x]:
                if y not in seen:
                    seen.add(y)
                    queue.append(y)
    return {
        "degenerate": degenerate,
        "boundary_edges": boundary,
        "nonmanifold_edges": nonmanifold,
        "misoriented_pairs": misoriented,
        "components": components,
        "closed": boundary == 0 and nonmanifold == 0 and len(live) > 0,
        "oriented": misoriented == 0 and nonmanifold == 0,
    }


def mesh_text(header, frame, verts, tris):
    lines = ["# " + header, "# CC-BY-4.0. Vertices `v x y z` (metres) and triangles `t a b c`, in index order."]
    lines.append("frame " + frame)
    lines += ["v %d %d %d" % v for v in verts]
    lines += ["t %d %d %d" % t for t in tris]
    return "\n".join(lines) + "\n"


def parse_mesh(text):
    frame, verts, tris = None, [], []
    for line in text.splitlines():
        if not line.strip() or line.startswith("#"):
            continue
        f = line.split()
        if f[0] == "frame":
            frame = f[1]
        elif f[0] == "v":
            verts.append(tuple(int(x) for x in f[1:4]))
        elif f[0] == "t":
            tris.append(tuple(int(x) for x in f[1:4]))
        else:
            raise ValueError("unknown line kind " + f[0])
    return frame, verts, tris


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    # Hand-checked expectations from the leaf's acceptance criteria.
    want = {
        "mesh_cube": dict(degenerate=0, boundary_edges=0, nonmanifold_edges=0, misoriented_pairs=0,
                          components=1, closed=True, oriented=True),
        "mesh_open_flipped": dict(boundary_edges=3, misoriented_pairs=3, closed=False, oriented=False),
        "mesh_nonmanifold": dict(nonmanifold_edges=1),
        "mesh_mobius": dict(degenerate=0, boundary_edges=6, nonmanifold_edges=0, misoriented_pairs=1,
                            components=1, closed=False, oriented=False),
        "mesh_bowtie": dict(degenerate=0, boundary_edges=6, nonmanifold_edges=0, misoriented_pairs=0,
                            components=2, closed=False, oriented=True),
    }
    for name, header, frame, verts, tris, write_mesh in CASES:
        outputs = {}
        if write_mesh:
            outputs[name + ".txt"] = mesh_text(header, frame, verts, tris)
        else:
            with open(os.path.join(HERE, name + ".txt"), encoding="utf-8") as fh:
                got = parse_mesh(fh.read())
            assert got == (frame, verts, tris), name + ".txt differs from the reference mesh"
        f = facts(len(verts), tris)
        for k, v in want.get(name, {}).items():
            assert f[k] == v, "%s: %s = %r, acceptance criterion says %r" % (name, k, f[k], v)
        outputs[name + ".facts.json"] = json.dumps(f, indent=2, sort_keys=True) + "\n"
        for fname, text in outputs.items():
            path = os.path.join(HERE, fname)
            if check:
                try:
                    with open(path, encoding="utf-8", newline="") as fh:
                        cur = fh.read()
                except FileNotFoundError:
                    cur = None
                if cur != text:
                    print("stale: " + fname)
                    bad += 1
            else:
                with open(path, "w", encoding="utf-8", newline="\n") as fh:
                    fh.write(text)
    if bad:
        sys.exit(1)
    print("mesh fixtures ok" if check else "mesh fixtures written")


if __name__ == "__main__":
    main()
