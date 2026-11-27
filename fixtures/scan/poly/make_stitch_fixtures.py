#!/usr/bin/env python3
"""Independent reference for SCAN.POLY.SURF.STITCH (fan agreement, manifold assembly and
orientation). CC0-1.0 data.

Stdlib only. Writes into this folder; `--check` regenerates everything in memory and fails on
any difference with the committed files. Nothing here was produced by running the Rust crate
`fid-scan-poly`.

Files:

* `<case>.stitch.txt` for the FAN fixtures `hex_grid`, `hex_grid_wide`, `square_grid`,
  `square_grid_tilted`, `two_patches_gap`, `jitter_dense` and `status_mix`: the expected mesh
  and report of stitching the expected fans in `<case>.fan.txt` (themselves written by the
  independent `make_fan_fixtures.py`, read here from the committed file).
* `votes_mix.stitch.txt`: a hand-built vertex set with hand-built fans (not Delaunay fans) for
  every rule: three-vote triangles on a fin edge (cleanup by fit), three equally fitting
  triangles on one edge (cleanup tie by triple), a flipped winding, two-vote triangles accepted
  and refused in ascending order, a single vote, collinear (unorientable) triangles, and a
  vertex for every fan status, and a hidden and an all-edges-long vertex (no triangle in their
  own fans) used by kept two-vote triangles.
* `plate_gap.stitch.txt` (AC-2): a jittered triangular lattice plate at 1 mm, 24.5 x 13.9 mm,
  on a tilted plane, with a 5 mm x 7 mm slot cut out; g = 1.5 mm. The vertex set, the fans
  (computed here with `make_fan_fixtures.fan_of`), the expected mesh and report, and the
  expected boundary loops: one around the slot and one around the plate.
* `sphere_points.txt` (AC-1, AC-3): the definition of the 50 mm sphere at 0.5 mm spacing as a
  generator (125 664 points are too many to commit), with every 1009th point written out so
  the test can confirm it rebuilt the same points bit for bit. The expected properties (closed,
  no non-manifold edge, every face normal outward, one component, Euler characteristic 2) are
  hand-labelled from the spec: a sampled sphere is a closed genus-0 surface.

The sphere generator uses IEEE `+ - * /` and `sqrt` only, so it is reproducible bit for bit:
`z_i = 1 - (2i + 1) / N`, `r_i = sqrt(1 - z_i * z_i)`, `(c_i, s_i)` the golden-angle rotation
applied i times by complex multiplication `(c, s) <- (c*C - s*S, s*C + c*S)` from `(1, 0)`
with `(C, S)` = (cos, sin) of `pi * (3 - sqrt 5)` written in the file, `q = (r c, r s, z)`,
`l = sqrt((q0*q0 + q1*q1) + q2*q2)`, normal `n = q / l` per axis, position `R * n` per axis.
Point `i` has source `i`.

The expected meshes follow REQ-SCAN.POLY.SURF.STITCH-001..005 and the pinned steps of the
crate's `stitch` module documentation, computed independently here: votes with a dict over
sorted source triples, orientation and fit key with Python floats in the documented operation
order (IEEE binary64, never fused), edge uses with a dict, cleanup by sorting, topology facts
by the GEO.TYPES.MESH definitions (half-edge counts, union-find over shared edges).
"""

import math
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
sys.path.insert(0, HERE)

import make_fan_fixtures as fanref  # noqa: E402  (the independent FAN reference, same folder)

FAN_CASES = ["hex_grid", "hex_grid_wide", "square_grid", "square_grid_tilted", "two_patches_gap",
             "jitter_dense", "status_mix"]


# ---------------------------------------------------------------- reading FAN fixtures


def read_fan_fixture(name):
    with open(os.path.join(HERE, name + ".fan.txt"), encoding="utf-8") as f:
        lines = [l.rstrip("\n") for l in f if not l.startswith("#")]
    assert lines[0] == "case " + name
    n = int(lines[2].split()[1])
    rows = []
    for l in lines[3:3 + n]:
        t = l.split()
        rows.append((int(t[0]), tuple(float(x) for x in t[1:4]), tuple(float(x) for x in t[4:7]), t[7]))
    assert lines[3 + n] == "fans %d" % n
    fans = []
    for l in lines[4 + n:]:
        c, status, _nbs, tris = [p.strip() for p in l.split(";")]
        c = int(c)
        pairs = [tuple(int(x) for x in t.split(":")) for t in tris.split()]
        fans.append((c, status, [(c, j, k) for j, k in pairs]))
    assert len(fans) == n
    return rows, fans


# ---------------------------------------------------------------- the reference stitch


def tkey(x):
    """Total order on non-NaN floats: -0.0 before +0.0."""
    return (x, math.copysign(1.0, x))


def dot(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def orient(p, nrm, t):
    """(flip, key) or None for the ascending source triple t."""
    pa, pb, pc = (p[s] for s in t)
    na, nb, nc = (nrm[s] for s in t)
    u = (pb[0] - pa[0], pb[1] - pa[1], pb[2] - pa[2])
    w = (pc[0] - pa[0], pc[1] - pa[1], pc[2] - pa[2])
    f = (u[1] * w[2] - u[2] * w[1], u[2] * w[0] - u[0] * w[2], u[0] * w[1] - u[1] * w[0])
    m = ((na[0] + nb[0]) + nc[0], (na[1] + nb[1]) + nc[1], (na[2] + nb[2]) + nc[2])
    s = dot(f, m)
    assert math.isfinite(s)
    if s == 0.0:
        return None
    flip = s < 0.0
    if flip:
        f = (-f[0], -f[1], -f[2])
    big = max(abs(f[0]), abs(f[1]), abs(f[2]))
    assert big > 0.0
    w = (f[0] / big, f[1] / big, f[2] / big)
    l = math.sqrt((w[0] * w[0] + w[1] * w[1]) + w[2] * w[2])
    e = (w[0] / l, w[1] / l, w[2] / l)
    key = min((dot(e, na), dot(e, nb), dot(e, nc)), key=tkey)
    return flip, key


def edges3(t):
    return [(t[0], t[1]), (t[0], t[2]), (t[1], t[2])]


def stitch(rows, fans):
    """Returns a dict with the expected mesh and report (sources throughout, indices for triangles)."""
    p = {s: q for s, q, _, _ in rows}
    nrm = {s: n for s, _, n, _ in rows}
    srcs = sorted(p)
    index = {s: i for i, s in enumerate(srcs)}
    status = {c: st for c, st, _ in fans}
    assert sorted(status) == srcs and len(fans) == len(srcs)
    votes = {}
    for c, st, tris in fans:
        assert (st == "ok") == bool(tris)
        seen = set()
        for t in tris:
            assert t[0] == c and len(set(t)) == 3
            key = tuple(sorted(t))
            assert key not in seen
            seen.add(key)
            votes[key] = votes.get(key, 0) + 1
    fate = {}
    info = {}
    for t in sorted(votes):
        v = votes[t]
        assert 1 <= v <= 3
        if v == 1:
            fate[t] = ("single_vote",)
            continue
        o = orient(p, nrm, t)
        info[t] = o
        if o is None:
            fate[t] = ("unorientable",)
    uses = {}
    for t in sorted(votes):
        if votes[t] == 3 and t not in fate:
            for e in edges3(t):
                uses[e] = uses.get(e, 0) + 1
    for t in sorted(votes):
        if votes[t] == 2 and t not in fate:
            full = [e for e in edges3(t) if uses.get(e, 0) >= 2]
            if full:
                fate[t] = ("edge_full", full[0])
            else:
                for e in edges3(t):
                    uses[e] = uses.get(e, 0) + 1
    on = {}
    for t in votes:
        if t not in fate:
            for e in edges3(t):
                on.setdefault(e, []).append(t)
    for e in sorted(on):
        alive = [t for t in on[e] if t not in fate]
        if len(alive) <= 2:
            continue
        ranked = sorted(alive, key=lambda t: ((-info[t][1], -math.copysign(1.0, info[t][1])), t))
        for t in ranked[2:]:
            fate[t] = ("non_manifold", e)
    tris = []
    kept = {2: 0, 3: 0}
    for t in sorted(votes):
        if t in fate:
            continue
        a, b, c = (index[s] for s in t)
        tris.append((a, c, b) if info[t][0] else (a, b, c))
        kept[votes[t]] += 1
    used = {s for t in votes if t not in fate for s in t}
    rejected = [(t, votes[t], fate[t]) for t in sorted(fate)]
    unused = [(s, status[s]) for s in srcs if s not in used]
    assert len(rejected) + kept[2] + kept[3] == len(votes)
    return {
        "srcs": srcs, "candidates": len(votes), "kept3": kept[3], "kept2": kept[2], "tris": tris,
        "rejected": rejected, "unused": unused, "facts": facts(tris, len(srcs)),
        "ok_fans": sum(1 for _, st, _ in fans if st == "ok"),
        "fan_status": [status[s] for s in srcs],
        "used_without_ok_fan": sum(1 for s in srcs if s in used and status[s] != "ok"),
    }


def facts(tris, nverts):
    """GEO.TYPES.MESH topology facts of a triangle list without degenerate triangles."""
    half = {}
    for k, (a, b, c) in enumerate(tris):
        assert len({a, b, c}) == 3
        for x, y in ((a, b), (b, c), (c, a)):
            half.setdefault((min(x, y), max(x, y)), []).append((k, x < y))
    boundary = sum(1 for v in half.values() if len(v) == 1)
    nonmanifold = sum(1 for v in half.values() if len(v) > 2)
    misoriented = sum(1 for v in half.values() if len(v) == 2 and v[0][1] == v[1][1])
    parent = list(range(len(tris)))

    def find(x):
        while parent[x] != x:
            parent[x] = parent[parent[x]]
            x = parent[x]
        return x

    for v in half.values():
        for k, _ in v[1:]:
            ra, rb = find(v[0][0]), find(k)
            if ra != rb:
                parent[ra] = rb
    components = len({find(k) for k in range(len(tris))})
    closed = boundary == 0 and nonmanifold == 0 and len(tris) > 0
    oriented = misoriented == 0 and nonmanifold == 0
    return (boundary, nonmanifold, misoriented, components, closed, oriented)


def boundary_loops(tris, srcs):
    """Boundary loops as source lists, each walked along the triangle winding from its smallest
    source; requires every boundary vertex to have one outgoing boundary edge."""
    count = {}
    for a, b, c in tris:
        for x, y in ((a, b), (b, c), (c, a)):
            e = (min(x, y), max(x, y))
            count[e] = count.get(e, 0) + 1
    nxt = {}
    for a, b, c in tris:
        for x, y in ((a, b), (b, c), (c, a)):
            if count[(min(x, y), max(x, y))] == 1:
                assert x not in nxt, "boundary vertex with two outgoing boundary edges"
                nxt[x] = y
    loops = []
    left = set(nxt)
    while left:
        start = min(left)
        loop = [start]
        left.discard(start)
        x = nxt[start]
        while x != start:
            loop.append(x)
            left.discard(x)
            x = nxt[x]
        loops.append([srcs[i] for i in loop])
    loops.sort(key=lambda l: l[0])
    return loops


# ---------------------------------------------------------------- rendering


def render_expect(r, loops=None):
    lines = ["expect", "candidates %d" % r["candidates"], "kept3 %d" % r["kept3"], "kept2 %d" % r["kept2"]]
    b, nm, mis, comp, closed, oriented = r["facts"]
    lines.append("facts %d %d %d %d %d %d" % (b, nm, mis, comp, int(closed), int(oriented)))
    lines.append("triangles %d" % len(r["tris"]))
    lines += ["%d %d %d" % t for t in r["tris"]]
    lines.append("rejected %d" % len(r["rejected"]))
    for t, v, fate in r["rejected"]:
        extra = " %d %d" % fate[1] if len(fate) > 1 else ""
        lines.append("%d %d %d %d %s%s" % (t + (v, fate[0], extra)))
    lines.append("unused %d" % len(r["unused"]))
    lines += ["%d %s" % u for u in r["unused"]]
    lines.append("used_without_ok_fan %d" % r["used_without_ok_fan"])
    lines.append("fan_status " + " ".join(r["fan_status"]))
    if loops is not None:
        lines.append("loops %d" % len(loops))
        lines += [" ".join(map(str, l)) for l in loops]
    return lines


def render_vertices_and_fans(rows, fans):
    lines = ["vertices %d" % len(rows)]
    for s, q, n, tag in rows:
        lines.append(" ".join([str(s)] + [repr(x) for x in q] + [repr(x) for x in n] + [tag]))
    lines.append("fans %d" % len(fans))
    for c, st, tris in fans:
        lines.append("%d ; %s ; %s" % (c, st, " ".join("%d:%d" % (t[1], t[2]) for t in tris)))
    return lines


HEADER = "# SCAN.POLY.SURF.STITCH fixture, written by make_stitch_fixtures.py (CC0-1.0)."
EXPECT_DOC = [
    "# expect: candidates, kept3/kept2 (output triangles with 3/2 votes), facts = boundary_edges",
    "# nonmanifold_edges misoriented_pairs components closed oriented; triangles as mesh indices",
    "# (vertex i = i-th smallest source) in output order and winding; rejected = sources a b c,",
    "# votes, reason [edge sources]; unused = source and fan status; used_without_ok_fan = vertices",
    "# in an output triangle whose own fan is not ok; fan_status = each vertex's own fan, by index.",
]


def from_fan_case(name):
    rows, fans = read_fan_fixture(name)
    r = stitch(rows, fans)
    lines = [HEADER, "# Stitching the expected fans of %s.fan.txt." % name] + EXPECT_DOC
    lines += ["case " + name, "from %s.fan.txt" % name] + render_expect(r)
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- votes_mix


def votes_mix():
    z = (0.0, 0.0, 1.0)
    tilt = (0.0, -0.6, 0.8)
    named = [
        # A fin: A B on the x axis, C and D in the plane, E standing up (tilted normal).
        ("A", (0.0, 0.0, 0.0), z), ("B", (1.0, 0.0, 0.0), z), ("C", (0.5, 1.0, 0.0), z),
        ("D", (0.5, -1.0, 0.0), z), ("E", (0.5, 0.0, 1.0), tilt),
        # Three equally fitting flat triangles on F G.
        ("F", (10.0, 0.0, 0.0), z), ("G", (11.0, 0.0, 0.0), z), ("H1", (10.5, 1.0, 0.0), z),
        ("H2", (10.5, 2.0, 0.0), z), ("H3", (10.5, -1.0, 0.0), z),
        # Two-vote rules around P Q R S.
        ("P", (20.0, 0.0, 0.0), z), ("Q", (21.0, 0.0, 0.0), z), ("R", (21.0, 1.0, 0.0), z),
        ("S", (20.0, 1.0, 0.0), z), ("T", (20.5, 2.0, 0.0), z), ("U", (22.0, 0.5, 0.0), z),
        ("W", (22.0, 1.5, 0.0), z), ("X", (19.0, 0.5, 0.0), z),
        # Single vote and an Ok fan whose only triangle is rejected.
        ("K1", (30.0, 0.0, 0.0), z), ("K2", (31.0, 0.0, 0.0), z), ("K3", (30.0, 1.0, 0.0), z),
        # Collinear triples (two and three votes).
        ("L1", (40.0, 0.0, 0.0), z), ("L2", (41.0, 0.0, 0.0), z), ("L3", (42.0, 0.0, 0.0), z),
        ("L4", (43.0, 0.0, 0.0), z),
        # One vertex per non-Ok fan status.
        ("N1", (50.0, 0.0, 0.0), z), ("N2", (51.0, 0.0, 0.0), z), ("N3", (52.0, 0.0, 0.0), z),
        ("N4", (53.0, 0.0, 0.0), z),
        # Vertices whose own fan is not Ok (hidden, all edges long) in kept two-vote triangles.
        ("Y1", (60.0, 0.0, 0.0), z), ("Y2", (61.0, 0.0, 0.0), z), ("Y3", (60.5, 1.0, 0.0), z),
        ("Z1", (70.0, 0.0, 0.0), z), ("Z2", (71.0, 0.0, 0.0), z), ("Z3", (70.5, 1.0, 0.0), z),
    ]
    # (triangle as names in fan order (centre first is set per voter), voters).
    tri = [
        (("A", "B", "C"), "ABC"), (("A", "D", "B"), "ABD"), (("A", "B", "E"), "ABE"),
        (("F", "G", "H1"), "FGH1"), (("F", "G", "H2"), "FGH2"), (("F", "G", "H3"), "FGH3"),
        # P Q R and P R S use the diagonal P R twice.
        (("P", "Q", "R"), "PQR"), (("P", "R", "S"), "PRS"),
        # Two votes: P R T is refused on the full edge P R; Q U R is accepted (Q R used once),
        # Q R W then finds Q R full; S X P (two votes) is accepted.
        (("P", "R", "T"), "PT"), (("Q", "U", "R"), "QU"), (("Q", "W", "R"), "QW"),
        (("S", "X", "P"), "SX"),
        # A single vote.
        (("K1", "K2", "K3"), "K1"),
        # Collinear: three votes and two votes.
        (("L1", "L2", "L3"), "L1L2L3"), (("L2", "L3", "L4"), "L2L4"),
        # Two votes from the neighbours of a hidden and of an all-edges-long vertex.
        (("Y1", "Y2", "Y3"), "Y1Y2"), (("Z1", "Z2", "Z3"), "Z1Z2"),
    ]
    names = [n for n, _, _ in named]
    for t, voters in tri:
        for x in t:
            assert x in names
    # Voters string: a run of names; parse greedily against the triangle's own names.
    def voter_list(t, s):
        out = []
        while s:
            for x in sorted(t, key=len, reverse=True):
                if s.startswith(x):
                    out.append(x)
                    s = s[len(x):]
                    break
            else:
                raise AssertionError(s)
        return out

    rng = fanref.Rng(301)
    ids = fanref.shuffle(rng, range(len(named)))
    src = {n: 5 * ids[k] + 3 for k, (n, _, _) in enumerate(named)}
    status = {n: "ok" for n in names}
    status.update({"N1": "no_neighbours", "N2": "degenerate", "N3": "hidden:%d" % src["N4"],
                   "N4": "all_edges_long", "K2": "no_neighbours", "K3": "no_neighbours",
                   "Y3": "hidden:%d" % src["Y1"], "Z3": "all_edges_long"})
    fan_tris = {n: [] for n in names}
    for t, voters in tri:
        for v in voter_list(t, voters):
            k = t.index(v)
            rot = t[k:] + t[:k]
            fan_tris[v].append(tuple(src[x] for x in rot))
    for n in names:
        if status[n] == "ok":
            assert fan_tris[n], n
        else:
            assert not fan_tris[n], n
    rows = [(src[n], q, nrm, n) for n, q, nrm in named]
    rows = fanref.shuffle(rng, rows)
    fans = [(src[n], status[n], fan_tris[n]) for n in names]
    fans = fanref.shuffle(rng, fans)
    r = stitch(rows, fans)
    lines = [HEADER, "# Hand-built vertex set and fans (not Delaunay fans) for every STITCH rule;"
             " tag = name."] + EXPECT_DOC
    lines += ["case votes_mix"] + render_vertices_and_fans(rows, fans) + render_expect(r)
    return "\n".join(lines) + "\n", r


# ---------------------------------------------------------------- plate_gap


PLATE_G = 1.5e-3
PLATE_S = 1e-3
PLATE_NI, PLATE_NJ = 25, 17
SLOT_A, SLOT_B = (12e-3, 8 * math.sqrt(3.0) / 2.0 * 1e-3), (2.5e-3, 3.5e-3)


def plate_rows():
    rng = fanref.Rng(401)
    h = math.sqrt(3.0) / 2.0
    verts = []
    for j in range(PLATE_NJ):
        for i in range(PLATE_NI):
            a = (i + 0.5 * (j % 2)) * PLATE_S
            b = (j * h) * PLATE_S
            if abs(a - SLOT_A[0]) < SLOT_B[0] and abs(b - SLOT_A[1]) < SLOT_B[1]:
                continue
            da = (rng.unit() - 0.5) * 0.3 * PLATE_S
            db = (rng.unit() - 0.5) * 0.3 * PLATE_S
            w = (rng.unit() - 0.5) * 20e-6
            verts.append((fanref.on_plane(a + da, b + db, w), fanref.jitter_normal(rng, 0.1), "%d:%d" % (i, j)))
    # Sources are point indices (so a cloud in this order prepares to the same sources).
    return [(k, q, n, tag) for k, (q, n, tag) in enumerate(verts)]


def plate_gap():
    rows = plate_rows()
    # The slot is wider than g on every crossing: points on opposite sides are far apart.
    g = PLATE_G
    for _, q, _, _ in rows:
        assert all(math.isfinite(x) for x in q)
    # PREP with h = 0.25 mm keeps every point: no two points share a 0.25 mm cell.
    assert min(math.dist(a[1], b[1]) for a in rows for b in rows if a[0] < b[0]) > 0.25e-3 * math.sqrt(3.0)
    vs_rows = list(rows)
    fans = []
    for k in range(len(vs_rows)):
        c, st, _nbs, tris, _n = fanref.fan_of(vs_rows, k, g)
        fans.append((c, st, [(c, j, kk) for j, kk in tris]))
    r = stitch(vs_rows, fans)
    loops = boundary_loops(r["tris"], r["srcs"])
    assert len(loops) == 2, len(loops)
    # One loop is the slot's: every vertex of it lies within 2 mm of the slot rectangle.
    pos = {s: q for s, q, _, _ in rows}

    def near_slot(s):
        q = pos[s]
        d = tuple(q[k] - fanref.ORIGIN[k] for k in range(3))
        a, b = fanref.dot(d, fanref.U), fanref.dot(d, fanref.V)
        return abs(a - SLOT_A[0]) < SLOT_B[0] + 2e-3 and abs(b - SLOT_A[1]) < SLOT_B[1] + 2e-3

    slot = [l for l in loops if all(near_slot(s) for s in l)]
    assert len(slot) == 1
    assert r["facts"][1] == 0 and r["facts"][3] == 1
    lines = [HEADER,
             "# Jittered triangular lattice plate, 1 mm, 25 x 17 on a tilted plane, with a 5 mm x 7 mm slot;",
             "# g = 1.5 mm; sources are point indices; tag = i:j. Fans by make_fan_fixtures.fan_of.",
             "# loops: the two boundary loops as sources along the winding, each from its smallest source;",
             "# the slot loop is the one starting %d." % slot[0][0]] + EXPECT_DOC
    lines += ["case plate_gap", "g " + repr(g)] + render_vertices_and_fans(rows, fans) + render_expect(r, loops)
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- sphere_points


SPHERE_R = 0.05
SPHERE_S = 0.5e-3
SPHERE_G = 1.5e-3
SPHERE_EVERY = 1009


def sphere_points():
    n = round(4.0 * math.pi * SPHERE_R * SPHERE_R / (SPHERE_S * SPHERE_S))
    theta = math.pi * (3.0 - math.sqrt(5.0))
    cc, ss = math.cos(theta), math.sin(theta)
    c, s = 1.0, 0.0
    pts = []
    for i in range(n):
        z = 1.0 - (2 * i + 1) / n
        r = math.sqrt(1.0 - z * z)
        q = (r * c, r * s, z)
        l = math.sqrt((q[0] * q[0] + q[1] * q[1]) + q[2] * q[2])
        nn = (q[0] / l, q[1] / l, q[2] / l)
        assert fanref.is_unit(nn)
        pts.append(((SPHERE_R * nn[0], SPHERE_R * nn[1], SPHERE_R * nn[2]), nn))
        c, s = c * cc - s * ss, s * cc + c * ss
    return n, cc, ss, pts


def sphere_file():
    n, cc, ss, pts = sphere_points()
    lines = [
        HEADER,
        "# AC-1: oriented points on a 50 mm sphere at 0.5 mm spacing (N = round(4 pi R^2 / s^2)),",
        "# by the golden-angle generator of the script's documentation; point i has source i.",
        "# sample lines: i x y z nx ny nz for every %dth point and the last." % SPHERE_EVERY,
        "# Expected (hand-labelled, a closed genus-0 surface): closed, no non-manifold edge, every",
        "# face normal outward, one component, Euler characteristic V - E + F = 2 over used vertices.",
        "case sphere_points",
        "radius " + repr(SPHERE_R),
        "points %d" % n,
        "rot_cos " + repr(cc),
        "rot_sin " + repr(ss),
        "g " + repr(SPHERE_G),
    ]
    idx = list(range(0, n, SPHERE_EVERY))
    if idx[-1] != n - 1:
        idx.append(n - 1)
    lines.append("samples %d" % len(idx))
    for i in idx:
        q, nn = pts[i]
        lines.append(" ".join([str(i)] + [repr(x) for x in q] + [repr(x) for x in nn]))
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- main


def outputs():
    out = [(name + ".stitch.txt", from_fan_case(name)) for name in FAN_CASES]
    out.append(("votes_mix.stitch.txt", votes_mix()[0]))
    out.append(("plate_gap.stitch.txt", plate_gap()))
    out.append(("sphere_points.txt", sphere_file()))
    return out


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, text in outputs():
        target = os.path.join(HERE, name)
        if check:
            try:
                with open(target, encoding="utf-8", newline="") as f:
                    old = f.read()
            except OSError:
                old = None
            if old != text:
                print("MISMATCH " + target)
                bad += 1
        else:
            with open(target, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
            print("wrote " + target)
    if bad:
        sys.exit(1)
    if check:
        print("stitch fixtures OK")


if __name__ == "__main__":
    main()
