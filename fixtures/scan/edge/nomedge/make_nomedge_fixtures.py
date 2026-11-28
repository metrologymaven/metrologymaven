#!/usr/bin/env python3
"""Independent reference for SCAN.EDGE.NOMEDGE (nominal edge chains and station frames).
CC0-1.0 project data.

Stdlib only. Writes five small B-rep descriptions and the expected stations:

  fixtures/scan/edge_plate/body.txt          200 x 100 mm plate face in z = 0, normal +z (AC-1, AC-4)
  fixtures/scan/edge_cylinder_trim/body.txt  a cylinder face (r = 40 mm) bounded below by a circle
                                             arc and above by a trim curve, the ellipse cut by the
                                             plane 3 x = 4 z (AC-2)
  fixtures/scan/edge/nomedge/hole.txt        the same patch on a Reversed face (a hole), loop reversed
  fixtures/scan/edge/nomedge/band.txt        a full cylinder band with a seam edge
  fixtures/scan/edge/nomedge/notch.txt       the plate with its +x side split around a 1 mm edge
                                             (AC-3), plus a separate auxiliary face
  fixtures/scan/edge/nomedge/cases.txt       the cases and their expected stations

The Rust test `crates/fid-scan-edge/tests/nomedge.rs` reads these files; nothing here comes from
the crate. The expected stations follow the spec rules (NODE.md Requirements and Design, and the
`nominal` module documentation), computed here independently:

* Orientation: GEO.CAD keeps the face material on the left of each coedge seen from the outward
  normal (natural normal Su x Sv, negated for a Reversed face). The chain runs in the coedge's
  direction; a constructed curve runs with its parameter for material=left, against it for
  material=right. t is the unit tangent in chain direction, n the outward unit normal of the face
  at the point, b = t x n.
* Chaining (topological here; the fixtures are clean): sorted by edge label; the chain starts at
  the piece whose start vertex ends no other piece, or at the lowest label when closed; a missing
  successor is a gap, reported at the end point with the distance to the nearest remaining start.
* Stations at s_i = i d; open chain while s_i <= L + tol, closed chain while s_i < L - tol, with
  tol = 1e-12 m + 1e-12 L; a station within tol of a junction belongs to the later piece, at its
  start; one past the end of an open chain sits at the end.
* Arc length: exact for lines (|X| dt) and circles (r dt); for the ellipse by Romberg integration
  of sqrt(a^2 sin^2 t + b^2 cos^2 t) and Newton's method on the arc-length equation (not the
  crate's Gauss-Legendre quadrature and bisection).

Everything is evaluated in 60-digit decimal arithmetic on the exact binary values of the inputs
(pi by Machin's formula, sine and cosine by their Taylor series, placements normalised exactly)
and rounded once to the nearest double, so the files do not depend on the platform libm. Floats
are written with Python's shortest round-trip repr; the angles pi/2 and 2 pi in the bodies are the
doubles nearest to them (Rust's FRAC_PI_2 and TAU).

File formats (LF, one record per line, fields separated by single spaces):

  body files:
    fid-scan-edge-nomedge-body 1
    vertex <label> <x> <y> <z>
    line <label> <start> <end> <t0> <t1> <origin xyz> <axis xyz> <ref xyz>
    circle <label> <start> <end> <t0> <t1> <origin xyz> <axis xyz> <ref xyz> <r>
    ellipse <label> <start> <end> <t0> <t1> <origin xyz> <axis xyz> <ref xyz> <a> <b>
    coedge <label> <edge> same|reversed
    loop <label> outer|inner <coedge> ...
    plane <label> same|reversed <origin xyz> <axis xyz> <ref xyz> <loop> ...
    cylinder <label> same|reversed <origin xyz> <axis xyz> <ref xyz> <r> <loop> ...
    shell open|closed <face> ...

  cases.txt:
    case <name> body=<file> face=<label> edges=<l,l,...>|from=<edge>:<left|right> spacing=<d>
         stations=<n> closed=<0|1> length=<L>
    station <name> <i> s=<s> p=<x>,<y>,<z> t=<..> n=<..> b=<..> edge=<label|->
    gap <name> body=<file> face=<label> edges=<l,l> spacing=<d> at=<x>,<y>,<z> gap=<g>
        after=<label> before=<label>

(each record on one line; <file> relative to the repository root).
Run:  python fixtures/scan/edge/nomedge/make_nomedge_fixtures.py [--check]
With --check nothing is written; the script exits non-zero if a file differs.
"""

import os
import sys
from decimal import Decimal, getcontext

getcontext().prec = 60

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", ".."))

D = Decimal
ZERO = D(0)
ONE = D(1)


def pi_machin():
    def arctan_inv(x):
        x = D(x)
        x2 = x * x
        term = ONE / x
        total = term
        k = 1
        sign = -1
        while True:
            term = term / x2
            add = term / (2 * k + 1)
            if add < D("1e-62"):
                break
            total += sign * add
            sign = -sign
            k += 1
        return total

    return 16 * arctan_inv(5) - 4 * arctan_inv(239)


PI = pi_machin()
HALF_PI_F = 1.5707963267948966  # nearest double to pi/2 (Rust FRAC_PI_2)
TAU_F = 6.283185307179586  # nearest double to 2 pi (Rust TAU)


def sin_d(x):
    x = D(x)
    term = x
    total = x
    k = 1
    while True:
        term = -term * x * x / ((2 * k) * (2 * k + 1))
        if abs(term) < D("1e-62"):
            break
        total += term
        k += 1
    return total


def cos_d(x):
    x = D(x)
    term = ONE
    total = ONE
    k = 1
    while True:
        term = -term * x * x / ((2 * k - 1) * (2 * k))
        if abs(term) < D("1e-62"):
            break
        total += term
        k += 1
    return total


def vd(v):
    return [D(c) for c in v]


def add(a, b):
    return [a[0] + b[0], a[1] + b[1], a[2] + b[2]]


def sub(a, b):
    return [a[0] - b[0], a[1] - b[1], a[2] - b[2]]


def scale(a, k):
    return [a[0] * k, a[1] * k, a[2] * k]


def dot(a, b):
    return a[0] * b[0] + a[1] * b[1] + a[2] * b[2]


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def norm(a):
    return dot(a, a).sqrt()


def unit(a):
    return scale(a, ONE / norm(a))


def frame(origin, axis, ref):
    """Exact orthonormalisation of a placement: Z, X, Y = Z x X."""
    z = unit(vd(axis))
    x = vd(ref)
    x = unit(sub(x, scale(z, dot(x, z))))
    return vd(origin), z, x, cross(z, x)


def fl(x):
    return repr(float(x))


def fv(v):
    return ",".join(fl(c) for c in v)


# ------------------------------------------------------------------ geometry descriptions

class Curve:
    def __init__(self, kind, origin, axis, ref, sizes):
        self.kind = kind
        self.raw = (origin, axis, ref, sizes)
        self.o, self.z, self.x, self.y = frame(origin, axis, ref)
        self.sizes = [D(s) for s in sizes]

    def point(self, t):
        t = D(t)
        if self.kind == "line":
            return add(self.o, scale(self.x, t))
        c, s = cos_d(t), sin_d(t)
        if self.kind == "circle":
            r = self.sizes[0]
            return add(self.o, add(scale(self.x, r * c), scale(self.y, r * s)))
        a, b = self.sizes
        return add(self.o, add(scale(self.x, a * c), scale(self.y, b * s)))

    def d1(self, t):
        t = D(t)
        if self.kind == "line":
            return self.x
        c, s = cos_d(t), sin_d(t)
        if self.kind == "circle":
            r = self.sizes[0]
            return add(scale(self.x, -r * s), scale(self.y, r * c))
        a, b = self.sizes
        return add(scale(self.x, -a * s), scale(self.y, b * c))

    def speed(self, t):
        return norm(self.d1(t))

    def length(self, t0, t1):
        t0, t1 = D(t0), D(t1)
        if self.kind == "line":
            return t1 - t0
        if self.kind == "circle":
            return self.sizes[0] * (t1 - t0)
        return romberg(self.speed, t0, t1)

    def param_at(self, t0, sigma, hint=None):
        """Parameter t >= t0 with arc length sigma from t0."""
        t0 = D(t0)
        if self.kind == "line":
            return t0 + sigma
        if self.kind == "circle":
            return t0 + sigma / self.sizes[0]
        # Newton on S(t) - sigma, S accumulated over short Romberg steps.
        t, s_t = (t0, ZERO) if hint is None else hint
        for _ in range(60):
            step = (sigma - s_t) / self.speed(t)
            if abs(step) < D("1e-38"):
                return t, (t, s_t)
            t_new = t + step
            s_t = s_t + romberg(self.speed, t, t_new)
            t = t_new
        raise RuntimeError("newton did not converge")


def romberg(f, a, b, eps=D("1e-40"), kmax=16):
    if a == b:
        return ZERO
    h = b - a
    prev = [(f(a) + f(b)) * h / 2]
    for k in range(1, kmax):
        h = h / 2
        s = ZERO
        for j in range(1, 2 ** (k - 1) + 1):
            s += f(a + (2 * j - 1) * h)
        row = [prev[0] / 2 + h * s]
        for m in range(1, k + 1):
            row.append(row[m - 1] + (row[m - 1] - prev[m - 1]) / (4 ** m - 1))
        if abs(row[k] - prev[k - 1]) < eps:
            return row[k]
        prev = row
    raise RuntimeError("romberg did not converge")


class Surface:
    def __init__(self, kind, origin, axis, ref, sizes):
        self.kind = kind
        self.raw = (origin, axis, ref, sizes)
        self.o, self.z, self.x, self.y = frame(origin, axis, ref)
        self.sizes = [D(s) for s in sizes]

    def natural_normal(self, p):
        if self.kind == "plane":
            return self.z  # X x Y
        d = sub(p, self.o)
        radial = sub(d, scale(self.z, dot(d, self.z)))
        return unit(radial)  # Su x Sv points outward


class Body:
    def __init__(self, name, rel):
        self.name = name
        self.rel = rel  # repository-relative file
        self.vertices = {}  # label -> (x, y, z) floats
        self.edges = {}  # label -> (start, end, t0, t1, Curve)
        self.coedges = {}  # label -> (edge, sense)
        self.loops = {}  # label -> (outer, [coedges])
        self.faces = {}  # label -> (Surface, sense, [loops])
        self.shells = []  # (closed, [faces])
        self.lines = ["fid-scan-edge-nomedge-body 1"]

    def vertex(self, label, p):
        self.vertices[label] = p
        self.lines.append("vertex %s %s" % (label, " ".join(fl(c) for c in p)))

    def edge(self, label, kind, start, end, t0, t1, origin, axis, ref, sizes=()):
        c = Curve(kind, origin, axis, ref, sizes)
        self.edges[label] = (start, end, t0, t1, c)
        fields = [kind, label, start, end, fl(t0), fl(t1)] + [fl(v) for v in list(origin) + list(axis) + list(ref)]
        fields += [fl(s) for s in sizes]
        self.lines.append(" ".join(fields))

    def coedge(self, label, edge, sense):
        self.coedges[label] = (edge, sense)
        self.lines.append("coedge %s %s %s" % (label, edge, sense))

    def loop(self, label, outer, coedges):
        self.loops[label] = (outer, coedges)
        self.lines.append("loop %s %s %s" % (label, "outer" if outer else "inner", " ".join(coedges)))

    def face(self, label, kind, sense, origin, axis, ref, sizes, loops):
        s = Surface(kind, origin, axis, ref, sizes)
        self.faces[label] = (s, sense, loops)
        fields = [kind, label, sense] + [fl(v) for v in list(origin) + list(axis) + list(ref)]
        fields += [fl(x) for x in sizes] + loops
        self.lines.append(" ".join(fields))

    def shell(self, closed, faces):
        self.shells.append((closed, faces))
        self.lines.append("shell %s %s" % ("closed" if closed else "open", " ".join(faces)))

    def text(self):
        return "\n".join(self.lines) + "\n"


def square(b, prefix, x0, x1, y0, y1, z, face_label):
    """A rectangular plane face (outer loop counter-clockwise from +z) on body b."""
    vs = [(x0, y0, z), (x1, y0, z), (x1, y1, z), (x0, y1, z)]
    names = ["%sv%d" % (prefix, k + 1) for k in range(4)]
    for nme, p in zip(names, vs):
        b.vertex(nme, p)
    dirs = [(1.0, 0.0, 0.0), (0.0, 1.0, 0.0), (-1.0, 0.0, 0.0), (0.0, -1.0, 0.0)]
    lens = [x1 - x0, y1 - y0, x1 - x0, y1 - y0]
    cos = []
    for k in range(4):
        e = "%se%d" % (prefix, k + 1)
        b.edge(e, "line", names[k], names[(k + 1) % 4], 0.0, lens[k], vs[k], (0.0, 0.0, 1.0), dirs[k])
        b.coedge("%sc%d" % (prefix, k + 1), e, "same")
        cos.append("%sc%d" % (prefix, k + 1))
    b.loop("%sl1" % prefix, True, cos)
    b.face(face_label, "plane", "same", (0.0, 0.0, z), (0.0, 0.0, 1.0), (1.0, 0.0, 0.0), [], ["%sl1" % prefix])


def make_plate():
    b = Body("plate", "fixtures/scan/edge_plate/body.txt")
    square(b, "", -0.1, 0.1, -0.05, 0.05, 0.0, "top")
    b.shell(False, ["top"])
    return b


def make_notch():
    b = Body("notch", "fixtures/scan/edge/nomedge/notch.txt")
    pts = [
        ("v1", (-0.1, -0.05, 0.0)),
        ("v2", (0.1, -0.05, 0.0)),
        ("va", (0.1, 0.0, 0.0)),
        ("vb", (0.1, 0.001, 0.0)),
        ("v3", (0.1, 0.05, 0.0)),
        ("v4", (-0.1, 0.05, 0.0)),
    ]
    for nme, p in pts:
        b.vertex(nme, p)
    z = (0.0, 0.0, 1.0)
    b.edge("e1", "line", "v1", "v2", 0.0, 0.2, (-0.1, -0.05, 0.0), z, (1.0, 0.0, 0.0))
    b.edge("ea", "line", "v2", "va", 0.0, 0.05, (0.1, -0.05, 0.0), z, (0.0, 1.0, 0.0))
    b.edge("en", "line", "va", "vb", 0.0, 0.001, (0.1, 0.0, 0.0), z, (0.0, 1.0, 0.0))
    b.edge("eb", "line", "vb", "v3", 0.0, 0.049, (0.1, 0.001, 0.0), z, (0.0, 1.0, 0.0))
    b.edge("e3", "line", "v3", "v4", 0.0, 0.2, (0.1, 0.05, 0.0), z, (-1.0, 0.0, 0.0))
    b.edge("e4", "line", "v4", "v1", 0.0, 0.1, (-0.1, 0.05, 0.0), z, (0.0, -1.0, 0.0))
    cos = []
    for e in ["e1", "ea", "en", "eb", "e3", "e4"]:
        b.coedge("c" + e[1:], e, "same")
        cos.append("c" + e[1:])
    b.loop("l1", True, cos)
    b.face("top", "plane", "same", (0.0, 0.0, 0.0), z, (1.0, 0.0, 0.0), [], ["l1"])
    square(b, "x", -0.01, 0.01, -0.01, 0.01, 0.5, "aux")
    b.shell(False, ["top", "aux"])
    return b


R_CYL = 0.04
ELL_AXIS = (-0.6, 0.0, 0.8)
ELL_REF = (0.8, 0.0, 0.6)


def cylinder_patch(name, rel, reversed_face):
    b = Body(name, rel)
    b.vertex("p0", (0.04, 0.0, -0.01))
    b.vertex("p1", (0.0, 0.04, -0.01))
    b.vertex("p2", (0.0, 0.04, 0.0))
    b.vertex("p3", (0.04, 0.0, 0.03))
    z = (0.0, 0.0, 1.0)
    b.edge("bot", "circle", "p0", "p1", 0.0, HALF_PI_F, (0.0, 0.0, -0.01), z, (1.0, 0.0, 0.0), (R_CYL,))
    b.edge("right", "line", "p1", "p2", 0.0, 0.01, (0.0, 0.04, -0.01), (1.0, 0.0, 0.0), (0.0, 0.0, 1.0))
    b.edge("ell", "ellipse", "p3", "p2", 0.0, HALF_PI_F, (0.0, 0.0, 0.0), ELL_AXIS, ELL_REF, (0.05, 0.04))
    b.edge("left", "line", "p3", "p0", 0.0, 0.04, (0.04, 0.0, 0.03), (1.0, 0.0, 0.0), (0.0, 0.0, -1.0))
    if not reversed_face:
        b.coedge("cbot", "bot", "same")
        b.coedge("cright", "right", "same")
        b.coedge("cell", "ell", "reversed")
        b.coedge("cleft", "left", "same")
        b.loop("l1", True, ["cbot", "cright", "cell", "cleft"])
    else:
        b.coedge("cleft", "left", "reversed")
        b.coedge("cell", "ell", "same")
        b.coedge("cright", "right", "reversed")
        b.coedge("cbot", "bot", "reversed")
        b.loop("l1", True, ["cleft", "cell", "cright", "cbot"])
    sense = "reversed" if reversed_face else "same"
    b.face("side", "cylinder", sense, (0.0, 0.0, 0.0), z, (1.0, 0.0, 0.0), [R_CYL], ["l1"])
    b.shell(False, ["side"])
    return b


def make_band():
    b = Body("band", "fixtures/scan/edge/nomedge/band.txt")
    b.vertex("s0", (0.04, 0.0, 0.0))
    b.vertex("s1", (0.04, 0.0, 0.02))
    z = (0.0, 0.0, 1.0)
    b.edge("cb", "circle", "s0", "s0", 0.0, TAU_F, (0.0, 0.0, 0.0), z, (1.0, 0.0, 0.0), (R_CYL,))
    b.edge("ct", "circle", "s1", "s1", 0.0, TAU_F, (0.0, 0.0, 0.02), z, (1.0, 0.0, 0.0), (R_CYL,))
    b.edge("seam", "line", "s0", "s1", 0.0, 0.02, (0.04, 0.0, 0.0), (1.0, 0.0, 0.0), z)
    b.coedge("k1", "cb", "same")
    b.coedge("k2", "seam", "same")
    b.coedge("k3", "ct", "reversed")
    b.coedge("k4", "seam", "reversed")
    b.loop("l1", True, ["k1", "k2", "k3", "k4"])
    b.face("side", "cylinder", "same", (0.0, 0.0, 0.0), z, (1.0, 0.0, 0.0), [R_CYL], ["l1"])
    b.shell(False, ["side"])
    return b


# ------------------------------------------------------------------ the reference stations

LEN_ABS = D("1e-12")
LEN_REL = D("1e-12")


class Piece:
    def __init__(self, label, curve, t0, t1, reversed_, start, end):
        self.label = label
        self.curve = curve
        self.t0 = D(t0)
        self.t1 = D(t1)
        self.reversed = reversed_
        self.start = start
        self.end = end
        self.length = curve.length(t0, t1)
        self.hint = None

    def oriented_points(self):
        p0, p1 = self.curve.point(self.t0), self.curve.point(self.t1)
        return (p1, p0) if self.reversed else (p0, p1)


def face_pieces(body, face, edges):
    surf, fsense, loops = body.faces[face]
    uses = {}
    for lp in loops:
        for co in body.loops[lp][1]:
            e, sense = body.coedges[co]
            uses.setdefault(e, []).append(sense)
    pieces = []
    for e in sorted(edges):
        (sense,) = uses[e]
        start, end, t0, t1, c = body.edges[e]
        rev = sense == "reversed"
        pieces.append(Piece(e, c, t0, t1, rev, end if rev else start, start if rev else end))
    return surf, fsense, pieces


def chain(pieces):
    ends = set(p.end for p in pieces)
    open_starts = [p for p in pieces if p.start not in ends]
    first = open_starts[0] if open_starts else pieces[0]
    order = [first]
    left = [p for p in pieces if p is not first]
    while left:
        cur = order[-1]
        nxt = [p for p in left if p.start == cur.end]
        if len(nxt) != 1:
            at = cur.oriented_points()[1]
            best = min(left, key=lambda p: (norm(sub(p.oriented_points()[0], at)), p.label))
            return None, ("gap", cur, best, at, norm(sub(best.oriented_points()[0], at)))
        order.append(nxt[0])
        left.remove(nxt[0])
    return order, not open_starts


def station_frame(piece, surf, fsense, sigma_local):
    """Point and frame at arc length sigma_local from the piece's chain start."""
    sig = piece.length - sigma_local if piece.reversed else sigma_local
    if sig <= 0:
        t = piece.t0
    elif sig >= piece.length:
        t = piece.t1
    elif piece.curve.kind == "ellipse":
        t, piece.hint = piece.curve.param_at(piece.t0, sig, piece.hint)
    else:
        t = piece.curve.param_at(piece.t0, sig)
    p = piece.curve.point(t)
    tan = unit(piece.curve.d1(t))
    if piece.reversed:
        tan = scale(tan, -ONE)
    n = surf.natural_normal(p)
    if fsense == "reversed":
        n = scale(n, -ONE)
    tan = unit(sub(tan, scale(n, dot(tan, n))))
    b = unit(cross(tan, n))
    return p, tan, n, b


def run(surf, fsense, order, closed, spacing):
    total = sum((p.length for p in order), ZERO)
    tol = LEN_ABS + LEN_REL * total
    d = D(spacing)
    starts = []
    acc = ZERO
    for p in order:
        starts.append(acc)
        acc += p.length
    out = []
    i = 0
    while True:
        s = i * d
        if not ((s < total - tol) if closed else (s <= total + tol)):
            break
        k = max(j for j in range(len(starts)) if starts[j] <= s)
        if k + 1 < len(starts) and starts[k + 1] - s <= tol:
            k += 1
        local = s - starts[k]
        local = min(max(local, ZERO), order[k].length)
        p, tan, n, b = station_frame(order[k], surf, fsense, local)
        out.append((i, s, p, tan, n, b, order[k].label))
        i += 1
    return total, out


def case_lines(name, body, face, spacing, edges=None, constructed=None):
    if constructed is None:
        surf, fsense, pieces = face_pieces(body, face, edges)
        sel = "edges=" + ",".join(edges)
    else:
        edge, side = constructed
        surf, fsense, _ = body.faces[face]
        start, end, t0, t1, c = body.edges[edge]
        pieces = [Piece("-", c, t0, t1, side == "right", "a", "b")]
        sel = "from=%s:%s" % (edge, side)
    order, closed = chain(pieces)
    if constructed is not None:
        p0, p1 = pieces[0].oriented_points()
        closed = norm(sub(p0, p1)) <= D("1e-9")
    total, sts = run(surf, fsense, order, closed, spacing)
    lines = [
        "case %s body=%s face=%s %s spacing=%s stations=%d closed=%d length=%s"
        % (name, body.rel, face, sel, fl(spacing), len(sts), 1 if closed else 0, fl(total))
    ]
    for i, s, p, tan, n, b, lab in sts:
        lines.append(
            "station %s %d s=%s p=%s t=%s n=%s b=%s edge=%s" % (name, i, fl(s), fv(p), fv(tan), fv(n), fv(b), lab)
        )
    return lines


def gap_lines(name, body, face, edges, spacing):
    surf, fsense, pieces = face_pieces(body, face, edges)
    order, info = chain(pieces)
    assert order is None
    _, cur, best, at, g = info
    return [
        "gap %s body=%s face=%s edges=%s spacing=%s at=%s gap=%s after=%s before=%s"
        % (name, body.rel, face, ",".join(edges), fl(spacing), fv(at), fl(g), cur.label, best.label)
    ]


def build():
    plate = make_plate()
    trim = cylinder_patch("cylinder_trim", "fixtures/scan/edge_cylinder_trim/body.txt", False)
    hole = cylinder_patch("hole", "fixtures/scan/edge/nomedge/hole.txt", True)
    band = make_band()
    notch = make_notch()
    files = {}
    for b in (plate, trim, hole, band, notch):
        files[b.rel] = b.text()
    lines = [
        "# SCAN.EDGE.NOMEDGE expected stations; written by make_nomedge_fixtures.py (do not edit)",
    ]
    lines += case_lines("plate_xp", plate, "top", 0.01, edges=["e2"])
    lines += case_lines("plate_loop", plate, "top", 0.1, edges=["e1", "e2", "e3", "e4"])
    lines += case_lines("plate_loop_007", plate, "top", 0.07, edges=["e3", "e1", "e4", "e2"])
    lines += case_lines("plate_open3", plate, "top", 0.03, edges=["e4", "e1", "e2"])
    lines += case_lines("cyl_trim", trim, "side", 0.005, edges=["ell"])
    lines += case_lines("cyl_trim_chain", trim, "side", 0.004, edges=["ell", "left", "bot"])
    lines += case_lines("cyl_trim_loop", trim, "side", 0.006, edges=["bot", "right", "ell", "left"])
    lines += case_lines("cyl_constructed_right", trim, "side", 0.005, constructed=("ell", "right"))
    lines += case_lines("cyl_constructed_left", trim, "side", 0.005, constructed=("ell", "left"))
    lines += case_lines("hole_trim", hole, "side", 0.005, edges=["ell"])
    lines += case_lines("band_bottom", band, "side", 0.01, edges=["cb"])
    lines += case_lines("band_top", band, "side", 0.013, edges=["ct"])
    lines += gap_lines("notch_gap", notch, "top", ["ea", "eb"], 0.01)
    files["fixtures/scan/edge/nomedge/cases.txt"] = "\n".join(lines) + "\n"
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    bad = 0
    for rel, text in sorted(files.items()):
        full = os.path.join(ROOT, *rel.split("/"))
        data = text.encode("utf-8")
        if check:
            try:
                with open(full, "rb") as fh:
                    have = fh.read()
            except OSError:
                have = None
            if have != data:
                print("DIFFERS: %s" % rel)
                bad += 1
        else:
            os.makedirs(os.path.dirname(full), exist_ok=True)
            with open(full, "wb") as fh:
                fh.write(data)
    if check:
        if bad:
            sys.exit(1)
        print("nomedge fixtures: %d files up to date" % len(files))


if __name__ == "__main__":
    main()
