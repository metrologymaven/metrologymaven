#!/usr/bin/env python3
"""Independent reference for SCAN.POLY.QUAL (mesh quality report). CC-BY-4.0 project data.

Stdlib only. The meshes are hand-written (or, for the jittered grid, generated from a fixed
integer LCG) below. The expected reports are computed here with a different method from
the Rust crate `fid-scan-mesh` (dicts of undirected edges, breadth-first searches over
triangle adjacency and over each vertex's incident triangles, and a set of used boundary
edges, where the crate uses sorted edge and incidence lists with union-find), following
the metric definitions pinned in the Design of spec/scan/poly/qual/NODE.md and in the
crate's `quality` module documentation:

* A triangle with a repeated vertex index is degenerate. It is counted in `triangles` and
  `degenerate` and takes no further part (no edges, no component, no fan, no angle).
* Every other triangle (a, b, c) has area 0.5 * sqrt((cx*cx + cy*cy) + cz*cz), where
  (cx, cy, cz) = (b - a) x (c - a), evaluated in exactly that order without FMA. It is
  degenerate when area < threshold (strict). It still takes part in the topology; it only
  leaves the angle statistics.
* Edges: undirected vertex pairs of the non-index-degenerate triangles. Uses counts each
  triangle that has the edge (duplicate triangles count separately). Boundary edge: used
  once. Non-manifold edge: used three or more times.
* Components: classes of non-index-degenerate triangles linked by a shared edge.
* Non-manifold vertex: a vertex whose incident non-index-degenerate triangles form more
  than one class when two of them are linked if they share an edge that contains the
  vertex. Vertices without triangles have no fan and are never non-manifold.
* Boundary loops: on the fan-split boundary graph, whose nodes are the (vertex, fan)
  pairs at the ends of boundary edges (a boundary edge has one triangle, so at each end it
  lies in one fan of that vertex) and whose edges are the boundary edges, undirected, each
  connected component counts max(1, k), 2k being its number of odd-degree nodes (the
  fewest fan-aware walks covering it). When every fan has zero or two boundary edges at
  each vertex, every component is a cycle and this is the number of boundary cycles (two
  for the bow tie, however numbered). The count depends only on the graph.
* Minimum angle of a triangle: for corners A, B, C (in stored order) with u, v the edge
  vectors from that corner to the next and the previous corner (A: B-A, C-A; B: C-B, A-B;
  C: A-C, B-C), cos = dot(u, v) / (sqrt(dot(u, u)) * sqrt(dot(v, v))), dot(p, q) =
  (px*qx + py*qy) + pz*qz, clamped to [-1, 1], angle = acos(cos); the
  smallest of the three angles.
* Angle statistics over the non-degenerate triangles: count, min, and nearest-rank
  percentiles p1, p5 and median (p50): the value at 1-based rank ceil(p * n / 100) of the
  ascending sorted list. With no such triangle, min, p1, p5 and median are null.
* `vertices` is the vertex count of the mesh, referenced or not.

acos here is computed in 60-digit decimal arithmetic and rounded once to the nearest
double (so the files do not depend on the platform libm). The crate uses fid-num's
vendored libm acos, which may differ by an ulp; the Rust test therefore compares angles
with a 1e-15 tolerance and everything else exactly. The canonical JSON line uses the kernel convention
(every number is a string; numbers in the ECMAScript Number-to-String form of RFC 8785
section 3.2.2.3; keys sorted; no whitespace; null for an absent statistic).

Files (next to this script), for each case:

<mesh>.txt                 the mesh: `frame <32 hex>`, `v x y z` (metres), `t a b c`
<case>.quality.txt         the expected report for that mesh and threshold

Run:  python fixtures/scan/make_mesh_quality.py [--check]

With --check nothing is written; the script exits non-zero if a file differs.
"""

import functools
import math
import os
from decimal import Decimal, getcontext
import sys
from collections import deque

HERE = os.path.dirname(os.path.abspath(__file__))
FRAME = "5c0a11ab1e5c0a11ab1e5c0a11ab1e00"
DEFAULT_THRESHOLD = "1e-18"

# --- meshes ------------------------------------------------------------------------------

CUBE_V = [
    (0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0),
    (0, 0, 1), (1, 0, 1), (1, 1, 1), (0, 1, 1),
]
CUBE_T = [
    (0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4),
    (3, 7, 6), (3, 6, 2), (0, 4, 7), (0, 7, 3), (1, 2, 6), (1, 6, 5),
]
# Regular tetrahedron, outward winding.
TETRA_V = [(1, 1, 1), (1, -1, -1), (-1, 1, -1), (-1, -1, 1)]
TETRA_T = [(0, 1, 2), (0, 3, 1), (0, 2, 3), (1, 3, 2)]
# Two fans of two triangles each, sharing only vertex 0.
BOWTIE_V = [(0, 0, 0), (1, 0, 0), (1, 1, 0), (0, 1, 0), (-1, 0, 0), (-1, -1, 0), (0, -1, 0)]
BOWTIE_T = [(0, 1, 2), (0, 2, 3), (0, 4, 5), (0, 5, 6)]
# Three triangles on the edge 0-1.
FIN_V = [(0, 0, 0), (1, 0, 0), (0, 1, 0), (0, -1, 0), (0, 0, 1)]
FIN_T = [(0, 1, 2), (1, 0, 3), (0, 1, 4)]
# Moebius strip: three quads between rails a_i = i and b_i = 3 + i; the last quad joins
# (a2, b2) to (b0, a0), i.e. with a half twist.
MOBIUS_V = [(0, 0, 0), (1, 0, 0), (2, 0, 0), (0, 1, 0), (1, 1, 0), (2, 1, 1)]
MOBIUS_T = [(0, 3, 1), (3, 4, 1), (1, 4, 2), (4, 5, 2), (2, 5, 3), (5, 0, 3)]


def open_grid():
    """4 x 3 vertices, spacing 1 m in x and 2 m in y; every quad split on the same diagonal."""
    nx, ny = 4, 3
    v = [(i, 2 * j, 0) for j in range(ny) for i in range(nx)]
    t = []
    for j in range(ny - 1):
        for i in range(nx - 1):
            a = j * nx + i
            b, c, d = a + 1, a + nx + 1, a + nx
            t += [(a, b, c), (a, c, d)]
    return v, t


def jitter_grid():
    """11 x 11 vertices on a 0.01 m grid, x and y jittered by up to 0.003 m (integer LCG,
    then exact binary fractions), 200 triangles: enough for distinct p1, p5 and median."""
    n = 11
    state = 20261119
    v = []
    for j in range(n):
        for i in range(n):
            jit = []
            for _ in range(2):
                state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
                jit.append(((state >> 40) % 601 - 300) / 100000.0)
            v.append((i / 100.0 + jit[0], j / 100.0 + jit[1], ((i * 7 + j * 3) % 5) / 1000.0))
    t = []
    for j in range(n - 1):
        for i in range(n - 1):
            a = j * n + i
            b, c, d = a + 1, a + n + 1, a + n
            if (i + j) % 2 == 0:
                t += [(a, b, c), (a, c, d)]
            else:
                t += [(a, b, d), (b, c, d)]
    return v, t


SLIVER_V = [
    (0, 0, 0), (1, 0, 0), (0, 1, 0),            # 0-2: a right triangle
    (2, 0, 0), (3, 0, 0), (4, 0, 0),            # 3-5: exactly collinear, area 0
    (0, 0, 5), (1, 0, 5), (0.5, 1e-19, 5),      # 6-8: area 5e-20, below 1e-18
    (0, 0, 10), (1, 0, 10), (0.5, 1e-6, 10),    # 9-11: needle, area 5e-7, kept at 1e-18
    (0, 0, 20), (0, 0, 20), (1, 0, 20),         # 12-14: two coincident vertices, area 0
]
SLIVER_T = [(0, 1, 2), (3, 4, 5), (6, 7, 8), (9, 10, 11), (0, 0, 1), (12, 13, 14)]

# AC-4: the tetrahedron plus one zero-area (collinear) triangle on its own vertices.
ZERO_V = TETRA_V + [(5, 0, 0), (6, 0, 0), (7, 0, 0)]
ZERO_T = TETRA_T + [(4, 5, 6)]

# The tetrahedron with triangle 0 repeated (same winding), plus an isolated pair of one
# triangle in both windings.
DUP_V = TETRA_V + [(5, 0, 0), (6, 0, 0), (5, 1, 0)]
DUP_T = TETRA_T + [(0, 1, 2), (4, 5, 6), (4, 6, 5)]

# The bow tie renumbered (old -> new: 0->1, 1->0, 3->4, 4->3): with the plain
# smallest-neighbour walk this numbering merged the two fans into one loop.
BOWTIE_MAP = [1, 0, 2, 4, 3, 5, 6]
BOWTIE_R_V = [BOWTIE_V[BOWTIE_MAP.index(k)] for k in range(7)]
BOWTIE_R_T = [tuple(BOWTIE_MAP[i] for i in t) for t in BOWTIE_T]

# The tetrahedron with an extra vertex at index 2 that no triangle uses.
UNUSED_V = TETRA_V[:2] + [(9, 9, 9)] + TETRA_V[2:]
UNUSED_T = [tuple(i + 1 if i >= 2 else i for i in t) for t in TETRA_T]

GRID_V, GRID_T = open_grid()
JIT_V, JIT_T = jitter_grid()

MESHES = [
    # name, description, vertices, triangles
    ("mesh_closed_cube", "unit cube, 12 triangles, watertight (AC-1)", CUBE_V, CUBE_T),
    ("mesh_open_cube", "mesh_closed_cube without the z = 0 face (triangles 0 and 1) (AC-2)",
     CUBE_V, CUBE_T[2:]),
    ("mesh_bowtie", "two two-triangle fans sharing only vertex 0 (AC-3)", BOWTIE_V, BOWTIE_T),
    ("mesh_bowtie_renumbered", "mesh_bowtie with vertices renumbered 0<->1 and 3<->4",
     BOWTIE_R_V, BOWTIE_R_T),
    ("mesh_unused_vertex", "mesh_tetra plus vertex 2, which no triangle uses",
     UNUSED_V, UNUSED_T),
    ("mesh_fin", "three triangles on the edge 0-1 (AC-3)", FIN_V, FIN_T),
    ("mesh_tetra", "regular tetrahedron", TETRA_V, TETRA_T),
    ("mesh_open_grid", "4 x 3 vertex grid, 1 m by 2 m cells", GRID_V, GRID_T),
    ("mesh_jitter_grid", "11 x 11 jittered grid, 200 triangles", JIT_V, JIT_T),
    ("mesh_mobius", "six-triangle Moebius strip", MOBIUS_V, MOBIUS_T),
    ("mesh_slivers", "slivers: collinear, sub-threshold, needle, repeated index, coincident",
     SLIVER_V, SLIVER_T),
    ("mesh_zero_area", "mesh_tetra plus one zero-area triangle (AC-4)", ZERO_V, ZERO_T),
    ("mesh_duplicate", "mesh_tetra with triangle 0 repeated, plus a two-sided pair",
     DUP_V, DUP_T),
    ("mesh_empty", "no vertices, no triangles", [], []),
]

CASES = [
    # case name, mesh name, threshold (decimal string)
    (name, name, DEFAULT_THRESHOLD) for (name, _, _, _) in MESHES
] + [("mesh_slivers_t1e-6", "mesh_slivers", "1e-6")]

# --- reference computation ----------------------------------------------------------------


def sub(p, q):
    return (p[0] - q[0], p[1] - q[1], p[2] - q[2])


def dot(p, q):
    return (p[0] * q[0] + p[1] * q[1]) + p[2] * q[2]


def area(a, b, c):
    u, v = sub(b, a), sub(c, a)
    cx = u[1] * v[2] - u[2] * v[1]
    cy = u[2] * v[0] - u[0] * v[2]
    cz = u[0] * v[1] - u[1] * v[0]
    return 0.5 * math.sqrt((cx * cx + cy * cy) + cz * cz)


PI = Decimal(
    "3.14159265358979323846264338327950288419716939937510582097494459230781640628620899"
)


def atan_dec(t):
    """atan of a Decimal t >= 0 at the current precision (halving reduction + Taylor)."""
    halvings = 0
    while t > Decimal("0.01"):
        t = t / (1 + (1 + t * t).sqrt())
        halvings += 1
    term, total, k, t2 = t, Decimal(0), 0, t * t
    eps = Decimal(10) ** (-(getcontext().prec + 2))
    while abs(term) > eps:
        total += term / (2 * k + 1) if k % 2 == 0 else -term / (2 * k + 1)
        term *= t2
        k += 1
    return total * (2 ** halvings)


@functools.lru_cache(maxsize=None)
def acos_cr(x):
    """acos of a float in [-1, 1], correctly rounded to a float (60-digit evaluation)."""
    getcontext().prec = 60
    d = Decimal(x)
    if d == 1:
        return 0.0
    if d == -1:
        return float(PI)
    return float(2 * atan_dec(((1 - d) / (1 + d)).sqrt()))


def corner_angle(p, nxt, prv):
    u, v = sub(nxt, p), sub(prv, p)
    c = dot(u, v) / (math.sqrt(dot(u, u)) * math.sqrt(dot(v, v)))
    if c > 1.0:
        c = 1.0
    if c < -1.0:
        c = -1.0
    return acos_cr(c)


def min_angle(a, b, c):
    return min(corner_angle(a, b, c), corner_angle(b, c, a), corner_angle(c, a, b))


def nearest_rank(sorted_vals, p):
    n = len(sorted_vals)
    rank = -(-p * n // 100)
    if rank < 1:
        rank = 1
    return sorted_vals[rank - 1]


def report(verts, tris, threshold):
    fverts = [tuple(float(x) for x in p) for p in verts]
    live = [i for i, t in enumerate(tris) if len(set(t)) == 3]
    degenerate = len(tris) - len(live)
    angles = []
    for i in live:
        a, b, c = (fverts[k] for k in tris[i])
        ar = area(a, b, c)
        assert math.isfinite(ar)
        if ar < threshold:
            degenerate += 1
        else:
            angles.append(min_angle(a, b, c))
    # Edge uses.
    uses = {}
    for i in live:
        x, y, z = tris[i]
        for p, q in ((x, y), (y, z), (z, x)):
            uses.setdefault(frozenset((p, q)), []).append(i)
    boundary = [e for e, ts in uses.items() if len(ts) == 1]
    nonmanifold = sum(1 for ts in uses.values() if len(ts) >= 3)
    # Components: BFS over triangle adjacency.
    adj = {i: set() for i in live}
    for ts in uses.values():
        for s in ts:
            adj[s].update(ts)
    seen, components = set(), 0
    for i in live:
        if i in seen:
            continue
        components += 1
        queue = deque([i])
        seen.add(i)
        while queue:
            s = queue.popleft()
            for r in adj[s]:
                if r not in seen:
                    seen.add(r)
                    queue.append(r)
    # Fans: per vertex, BFS over incident triangles linked by an edge containing the vertex.
    incident = {}
    for i in live:
        for v in tris[i]:
            incident.setdefault(v, []).append(i)
    nm_vertices = 0
    fan_of = {}  # (vertex, triangle) -> fan label (unique per vertex)
    for v, ts in incident.items():
        fan_seen, fans = set(), 0
        for t0 in ts:
            if t0 in fan_seen:
                continue
            fans += 1
            stack = [t0]
            fan_seen.add(t0)
            fan_of[(v, t0)] = fans
            while stack:
                s = stack.pop()
                for w in tris[s]:
                    if w == v:
                        continue
                    for r in uses[frozenset((v, w))]:
                        if r not in fan_seen:
                            fan_seen.add(r)
                            fan_of[(v, r)] = fans
                            stack.append(r)
        if fans > 1:
            nm_vertices += 1
    # Boundary loops on the fan-split boundary graph: BFS components, max(1, odd // 2) each.
    gadj = {}
    for e in boundary:
        p, q = sorted(e)
        (t,) = uses[e]
        np_, nq = (p, fan_of[(p, t)]), (q, fan_of[(q, t)])
        gadj.setdefault(np_, []).append(nq)
        gadj.setdefault(nq, []).append(np_)
    loops, gseen = 0, set()
    for n0 in gadj:
        if n0 in gseen:
            continue
        gseen.add(n0)
        queue, odd = deque([n0]), 0
        while queue:
            n = queue.popleft()
            odd += len(gadj[n]) % 2
            for m in gadj[n]:
                if m not in gseen:
                    gseen.add(m)
                    queue.append(m)
        loops += max(1, odd // 2)
    angles.sort()
    if angles:
        stats = {
            "count": len(angles),
            "min": angles[0],
            "p1": nearest_rank(angles, 1),
            "p5": nearest_rank(angles, 5),
            "median": nearest_rank(angles, 50),
        }
    else:
        stats = {"count": 0, "min": None, "p1": None, "p5": None, "median": None}
    return {
        "triangles": len(tris),
        "vertices": len(verts),
        "edges": len(uses),
        "boundary_edges": len(boundary),
        "boundary_loops": loops,
        "nonmanifold_edges": nonmanifold,
        "nonmanifold_vertices": nm_vertices,
        "components": components,
        "degenerate": degenerate,
        "min_angle": stats,
    }


# --- output ------------------------------------------------------------------------------


def es_number(x):
    """ECMAScript Number::toString of a finite float, from Python's shortest repr."""
    if x == 0:
        return "0"
    sign = "-" if x < 0 else ""
    r = repr(abs(x))
    if "e" in r:
        mant, exp = r.split("e")
        exp = int(exp)
    else:
        mant, exp = r, 0
    if "." in mant:
        ip, fp = mant.split(".")
    else:
        ip, fp = mant, ""
    digits = (ip + fp).lstrip("0")
    # value = 0.<all digits of ip+fp> * 10^(len(ip) + exp); strip leading zeros.
    lead = len(ip + fp) - len((ip + fp).lstrip("0"))
    n = len(ip) + exp - lead
    digits = digits.rstrip("0")
    k = len(digits)
    if k <= n <= 21:
        out = digits + "0" * (n - k)
    elif 0 < n <= 21:
        out = digits[:n] + "." + digits[n:]
    elif -6 < n <= 0:
        out = "0." + "0" * (-n) + digits
    else:
        e = n - 1
        out = digits[0] + ("." + digits[1:] if k > 1 else "") + "e" + ("+" if e >= 0 else "-") + str(abs(e))
    return sign + out


def jstr(s):
    return '"' + s + '"'


def canonical(rep):
    def val(v, is_float):
        if v is None:
            return "null"
        return jstr(es_number(v) if is_float else str(v))

    st = rep["min_angle"]
    inner = {k: val(st[k], k != "count") for k in st}
    inner_s = "{" + ",".join(jstr(k) + ":" + inner[k] for k in sorted(inner)) + "}"
    outer = {k: (inner_s if k == "min_angle" else jstr(str(rep[k]))) for k in rep}
    return "{" + ",".join(jstr(k) + ":" + outer[k] for k in sorted(outer)) + "}"


def fmt_coord(x):
    return repr(float(x)) if isinstance(x, float) else str(x)


def mesh_text(name, desc, verts, tris):
    lines = [
        f"# SCAN.POLY.QUAL fixture {name}: {desc}.",
        "# CC-BY-4.0. Written by make_mesh_quality.py. Vertices `v x y z` (metres) and",
        "# triangles `t a b c`, in index order.",
        f"frame {FRAME}",
    ]
    lines += ["v " + " ".join(fmt_coord(x) for x in p) for p in verts]
    lines += ["t " + " ".join(str(i) for i in t) for t in tris]
    return "\n".join(lines) + "\n"


def report_text(case, mesh, threshold, rep):
    st = rep["min_angle"]

    def f(v):
        return "null" if v is None else repr(v)

    lines = [
        f"# SCAN.POLY.QUAL expected report for {mesh}.txt at threshold {threshold} m^2.",
        "# CC-BY-4.0. Computed by make_mesh_quality.py (independent of the Rust crate).",
        f"mesh {mesh}",
        f"threshold {threshold}",
    ]
    for k in ("triangles", "vertices", "edges", "boundary_edges", "boundary_loops",
              "nonmanifold_edges", "nonmanifold_vertices", "components", "degenerate"):
        lines.append(f"{k} {rep[k]}")
    lines.append(f"angle_count {st['count']}")
    for k in ("min", "p1", "p5", "median"):
        lines.append(f"angle_{k} {f(st[k])}")
    lines.append("canonical " + canonical(rep))
    return "\n".join(lines) + "\n"


def renumbered(verts, tris, perm):
    """Vertex k moves to index perm[k]; triangle corners keep their order."""
    nv = [None] * len(verts)
    for k, p in enumerate(verts):
        nv[perm[k]] = p
    return nv, [tuple(perm[i] for i in t) for t in tris]


def renumbering_check(by):
    """The report does not depend on vertex numbering: all 5040 renumberings of the bow
    tie, and 200 pseudo-random renumberings of every other mesh."""
    import itertools
    v, t = by["mesh_bowtie"][2], by["mesh_bowtie"][3]
    want = report(v, t, 1e-18)
    for perm in itertools.permutations(range(len(v))):
        assert report(*renumbered(v, t, perm), 1e-18) == want, perm
    state = 7
    for name, _, v, t in MESHES:
        want = report(v, t, 1e-18)
        for _ in range(200):
            perm = list(range(len(v)))
            for i in range(len(perm) - 1, 0, -1):
                state = (state * 6364136223846793005 + 1442695040888963407) % (1 << 64)
                j = (state >> 33) % (i + 1)
                perm[i], perm[j] = perm[j], perm[i]
            assert report(*renumbered(v, t, perm), 1e-18) == want, (name, perm)


def self_check():
    # Hand-labelled expectations from the spec's acceptance criteria.
    by = {m[0]: m for m in MESHES}
    r = report(by["mesh_closed_cube"][2], by["mesh_closed_cube"][3], 1e-18)
    assert (r["triangles"], r["vertices"], r["edges"], r["boundary_edges"], r["boundary_loops"],
            r["nonmanifold_edges"], r["nonmanifold_vertices"], r["components"]) == (12, 8, 18, 0, 0, 0, 0, 1)
    assert abs(r["min_angle"]["min"] - math.pi / 4) < 1e-15
    r = report(by["mesh_open_cube"][2], by["mesh_open_cube"][3], 1e-18)
    assert (r["boundary_edges"], r["boundary_loops"]) == (4, 1)
    r = report(by["mesh_bowtie"][2], by["mesh_bowtie"][3], 1e-18)
    assert (r["nonmanifold_vertices"], r["nonmanifold_edges"], r["boundary_loops"]) == (1, 0, 2)
    r = report(by["mesh_bowtie_renumbered"][2], by["mesh_bowtie_renumbered"][3], 1e-18)
    assert by["mesh_bowtie_renumbered"][3] == [(1, 0, 2), (1, 2, 4), (1, 3, 5), (1, 5, 6)]
    assert (r["nonmanifold_vertices"], r["boundary_loops"]) == (1, 2)
    r = report(by["mesh_unused_vertex"][2], by["mesh_unused_vertex"][3], 1e-18)
    t = report(TETRA_V, TETRA_T, 1e-18)
    assert r["vertices"] == 5 and r["nonmanifold_vertices"] == 0
    assert {k: v for k, v in r.items() if k != "vertices"} == {k: v for k, v in t.items() if k != "vertices"}
    renumbering_check(by)
    r = report(by["mesh_fin"][2], by["mesh_fin"][3], 1e-18)
    # One component; vertices 0 and 1 each have three boundary edges in one fan: one walk
    # 0-2-1-3-0-4-1 covers all six.
    assert (r["nonmanifold_edges"], r["nonmanifold_vertices"], r["boundary_loops"]) == (1, 0, 1)
    r = report(by["mesh_zero_area"][2], by["mesh_zero_area"][3], 1e-18)
    t = report(TETRA_V, TETRA_T, 1e-18)
    assert r["degenerate"] == 1 and r["min_angle"] == t["min_angle"]
    r = report(by["mesh_mobius"][2], by["mesh_mobius"][3], 1e-18)
    assert (r["boundary_edges"], r["boundary_loops"], r["components"]) == (6, 1, 1)
    r = report(by["mesh_slivers"][2], by["mesh_slivers"][3], 1e-18)
    assert (r["degenerate"], r["min_angle"]["count"]) == (4, 2)
    r = report(by["mesh_slivers"][2], by["mesh_slivers"][3], 1e-6)
    assert (r["degenerate"], r["min_angle"]["count"]) == (5, 1)
    r = report([], [], 1e-18)
    assert r["min_angle"]["min"] is None and r["triangles"] == 0
    # acos reference against hand-known values.
    # pi/3 = 1.04719755119659774615...; the nearest double is 1.0471975511965979 (math.pi / 3
    # rounds to the double below it).
    assert acos_cr(0.0) == math.pi / 2 and acos_cr(-1.0) == math.pi and acos_cr(0.5) == 1.0471975511965979
    for x in (0.1, 0.7, -0.3, 0.999, -0.999999):
        assert abs(acos_cr(x) - math.acos(x)) <= 2e-16 * 4
    # ES number form, hand-labelled (RFC 8785 appendix B style).
    for x, s in ((0.5, "0.5"), (1e-7, "1e-7"), (1e21, "1e+21"), (1e20, "100000000000000000000"),
                 (0.000001, "0.000001"), (123.456, "123.456"), (5e-324, "5e-324"),
                 (2.0, "2"), (1.5e300, "1.5e+300"), (0.7853981633974483, "0.7853981633974483")):
        assert es_number(x) == s, (x, es_number(x), s)


def main():
    check = "--check" in sys.argv[1:]
    self_check()
    files = {}
    by = {}
    for name, desc, verts, tris in MESHES:
        files[f"{name}.txt"] = mesh_text(name, desc, verts, tris)
        by[name] = (verts, tris)
    for case, mesh, threshold in CASES:
        verts, tris = by[mesh]
        rep = report(verts, tris, float(threshold))
        files[f"{case}.quality.txt"] = report_text(case, mesh, threshold, rep)
    bad = 0
    for fname, text in sorted(files.items()):
        path = os.path.join(HERE, fname)
        if check:
            try:
                with open(path, encoding="utf-8", newline="") as fh:
                    if fh.read() != text:
                        print(f"differs: {fname}")
                        bad += 1
            except FileNotFoundError:
                print(f"missing: {fname}")
                bad += 1
        else:
            with open(path, "w", encoding="utf-8", newline="\n") as fh:
                fh.write(text)
    if bad:
        sys.exit(1)
    print(("checked " if check else "wrote ") + f"{len(files)} files")


if __name__ == "__main__":
    main()
