#!/usr/bin/env python3
"""Independent reference for spec node ALN.DATUM.DOF (degree-of-freedom accounting).

Hand-built nominal feature lists (project-authored, CC-BY-4.0) with their expected
degree-of-freedom accounting, written to `dof-cases.json`. Nothing here runs the Rust code.

Exact part (Python `fractions`): every input coordinate is taken as the exact binary value of
its `float`, so the oracle sees the same numbers as `fid_align_solve::dof`.

- The invariance subspace of each feature (spec REQ-ALN.DATUM.DOF-001) as rational twists
  `(w, v)`: a rotation about direction `a` through `p` is `(a, p x a)`, a translation along `t`
  is `(0, t)`. Plane: two in-plane translations and the rotation about the normal; line and
  cylinder: translation along and rotation about the axis; cone and 3D circle: rotation about
  the axis; sphere and point: the three rotations about the centre. Directions are not
  normalised (spans do not depend on it).
- Free subspace after each pair: the exact intersection with the pair's invariance subspace,
  from the rational null space of `[F, -B]` (the Design's method, here without rounding). The
  effective count is the drop in dimension; the nominal count is 6 minus the subspace size.
- The projector before each pair in the length-scaled metric `S = diag(R, R, R, 1, 1, 1)`, with
  `R^2 = max |location|^2` (or 1 when every location is the origin):
  `P = S F (F^T S^2 F)^-1 F^T S`, exact except for the final `R` factor on mixed entries
  (`sqrt` of an exact square, correctly rounded).

Rank rule (80-digit `decimal`, so far beyond double rounding): the singular values of the part of
the free subspace outside each pair's invariance subspace, in the scaled metric (orthonormal bases
by Gram-Schmidt, then the eigenvalues of the small Gram matrix by cyclic Jacobi). Those above the
spec's threshold 1e-9 are the effective count; values in (1e-12, 1e-6] are `near_rank_threshold`
warnings; the smallest kept and largest dropped values are recorded. Where the rule agrees with
the exact rank the exact intersection is used; where every value is below the threshold (a plane
tilted by 1e-10) the free subspace is unchanged; any other disagreement stops the script.

Float part (only for the readable names of the final free subspace, REQ-003; Python floats and
`math.sqrt`, both IEEE 754 correctly rounded, so platform-independent): the free subspace is
split exactly into its translations `T` (the intersection with `w = 0`) and their complement
`Rot` in `F` under the scaled metric. Each part gets an orthonormal float basis by modified
Gram-Schmidt (twice), and then the documented canonical rule of the Rust module: in candidate
order `e_vx, e_vy, e_vz` (for `T`) or `e_wx, e_wy, e_wz` (for `Rot`), project the candidate onto
the part, remove the accepted vectors (twice), and accept the normalised residual when its norm
is above `0.5` (`T`) or `0.5 * sigma_min` (`Rot`, `sigma_min` the smallest singular value of the
angular block of its orthonormal basis); then a largest-residual fallback (never needed here).
The names follow the rules of the module docs (axis labels, `{:.9}` coordinates, the first pair
whose axis or centre the rotation axis meets, else the origin, else the point).

Run (stdlib only, Python 3.9+):
  python fixtures/aln/datum/make_dof_cases.py           # rewrite dof-cases.json
  python fixtures/aln/datum/make_dof_cases.py --check   # fail if stale
"""

import json
import math
import sys
from decimal import Decimal as D, localcontext
from fractions import Fraction as Fr
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "dof-cases.json"
FORMAT = "aln-dof-cases/1"

NAME_TOL = 1e-9
RANK_TOL = D("1e-9")
NEAR_LO, NEAR_HI = D("1e-12"), D("1e-6")
PREC = 80
NOMINAL = {"plane": 3, "line": 4, "cylinder": 4, "cone": 5, "circle3d": 5, "sphere": 3, "point": 3}
DIRECTIONAL = {"plane", "line", "cylinder", "cone", "circle3d"}
AXIAL = {"line", "cylinder", "cone", "circle3d"}
CENTRED = {"sphere", "point", "cone", "circle3d"}
KNOWN = set(NOMINAL) | {"torus"}

# ------------------------------------------------------------------ cases (hand-built)

P = "plane"


def f(t, loc, d=None):
    return {"type": t, "location": list(loc), "direction": None if d is None else list(d)}


CASES = [
    # AC-2: plane, cylinder lying on it (axis +X at the radius height), then a point.
    ("ac2_plane_cylinder_point", "reject",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("cylinder", (0.0, 0.0, 0.01), (1.0, 0.0, 0.0)),
      f("point", (0.05, 0.03, 0.0))]),
    # AC-3: plane, then a cylinder along the plane normal.
    ("ac3_plane_coaxial_cylinder_keep", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("cylinder", (0.01, 0.02, 0.0), (0.0, 0.0, 1.0))]),
    ("ac3_plane_coaxial_cylinder_reject", "reject",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("cylinder", (0.01, 0.02, 0.0), (0.0, 0.0, 1.0))]),
    # AC-4: two parallel planes at different heights (the second faces the other way).
    ("ac4_parallel_planes", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f(P, (0.0, 0.0, 0.05), (0.0, 0.0, -1.0))]),
    # Block: plane, line, point (classic 3-2-1 by elements).
    ("block_plane_line_point", "reject",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("line", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0)),
      f("point", (0.0, 0.04, 0.02))]),
    # Plane, two crossing cylinders lying on it.
    ("plane_cylinder_cylinder_cross", "reject",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("cylinder", (0.0, 0.03, 0.005), (1.0, 0.0, 0.0)),
      f("cylinder", (0.08, 0.0, 0.005), (0.0, 1.0, 0.0))]),
    # Plane, two parallel cylinders: the second only repeats the first.
    ("plane_cylinder_cylinder_parallel", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("cylinder", (0.0, 0.03, 0.005), (1.0, 0.0, 0.0)),
      f("cylinder", (0.0, -0.03, 0.005), (-1.0, 0.0, 0.0))]),
    # Three spheres not on one line.
    ("sphere_sphere_sphere", "reject",
     [f("sphere", (0.0, 0.0, 0.0)), f("sphere", (0.1, 0.0, 0.0)), f("sphere", (0.0, 0.1, 0.0))]),
    # Two spheres: rotation about the line through the centres stays free.
    ("sphere_sphere", "keep_parent", [f("sphere", (0.0, 0.0, 0.0)), f("sphere", (0.1, 0.0, 0.0))]),
    # One sphere at the origin: the three rotations about its centre.
    ("sphere_alone", "keep_parent", [f("sphere", (0.0, 0.0, 0.0))]),
    # A cone alone: only the rotation about its axis.
    ("cone_alone", "keep_parent", [f("cone", (0.0, 0.0, 0.1), (0.0, 0.0, 1.0))]),
    # A plane, then a 3D circle on it (axis along the normal).
    ("plane_circle3d", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("circle3d", (0.02, 0.03, 0.0), (0.0, 0.0, 2.0))]),
    # A tilted plane: names that are not axis-aligned.
    ("tilted_plane", "keep_parent", [f(P, (0.0, 0.0, 0.0), (0.0, 0.6, 0.8))]),
    # A line far from the origin, then a point on it.
    ("line_point_on_line", "keep_parent",
     [f("line", (0.3, 0.4, 0.0), (0.0, 0.0, 1.0)), f("point", (0.3, 0.4, 0.2))]),
    # Plane, then a point off-origin: a rotation about the plane normal through the point.
    ("plane_point", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("point", (0.03, 0.04, 0.0))]),
    # Everything at the origin: the unit radius fallback.
    ("origin_only_point", "keep_parent", [f("point", (0.0, 0.0, 0.0))]),
    # A plane after a full constraint only repeats it.
    ("block_then_redundant_plane", "reject",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("line", (0.0, 0.0, 0.0), (1.0, 0.0, 0.0)),
      f("point", (0.0, 0.04, 0.02)), f(P, (0.1, 0.0, 0.0), (1.0, 0.0, 0.0))]),
    # A second plane tilted by atan(1e-8) about y: two DoF just above the rank threshold.
    ("tilted_planes_1e-8", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f(P, (0.0, 0.0, 0.0), (1e-8, 0.0, 1.0))]),
    # Tilted by atan(1e-10): both below the threshold, so the second plane counts as redundant.
    ("tilted_planes_1e-10", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f(P, (0.0, 0.0, 0.0), (1e-10, 0.0, 1.0))]),
    # AC-5 and other error paths.
    ("ac5_torus", "keep_parent", [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("torus", (0.0, 0.0, 0.0), (0.0, 0.0, 1.0))]),
    ("err_missing_direction", "keep_parent", [f(P, (0.0, 0.0, 0.0))]),
    ("err_unexpected_direction", "keep_parent", [f("sphere", (0.0, 0.0, 0.0), (0.0, 0.0, 1.0))]),
    ("err_zero_direction", "keep_parent",
     [f(P, (0.0, 0.0, 0.0), (0.0, 0.0, 1.0)), f("cylinder", (0.0, 0.0, 0.0), (0.0, 0.0, 0.0))]),
    ("err_empty", "keep_parent", []),
]

# ------------------------------------------------------------------ exact linear algebra


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def total(xs, zero):
    """Left-to-right sum (never builtin `sum`, whose float result changed in Python 3.12)."""
    acc = zero
    for x in xs:
        acc = acc + x
    return acc


def dot(a, b):
    return total((x * y for x, y in zip(a, b)), Fr(0))


def nullspace(rows, ncols):
    """Basis of {x : A x = 0} for a rational matrix given as a list of rows."""
    a = [list(r) for r in rows]
    pivots = []
    r = 0
    for c in range(ncols):
        piv = next((i for i in range(r, len(a)) if a[i][c] != 0), None)
        if piv is None:
            continue
        a[r], a[piv] = a[piv], a[r]
        inv = 1 / a[r][c]
        a[r] = [x * inv for x in a[r]]
        for i in range(len(a)):
            if i != r and a[i][c] != 0:
                k = a[i][c]
                a[i] = [x - k * y for x, y in zip(a[i], a[r])]
        pivots.append(c)
        r += 1
    free = [c for c in range(ncols) if c not in pivots]
    out = []
    for fc in free:
        x = [Fr(0)] * ncols
        x[fc] = Fr(1)
        for i, pc in enumerate(pivots):
            x[pc] = -a[i][fc]
        out.append(x)
    return out


def combine(cols, coeff):
    return [total((c[i] * k for c, k in zip(cols, coeff)), Fr(0)) for i in range(6)]


def intersect(fb, bb):
    """Exact basis of span(fb) intersected with span(bb) (both independent lists of 6-vectors)."""
    n = len(fb) + len(bb)
    rows = [[v[i] for v in fb] + [-v[i] for v in bb] for i in range(6)]
    return [combine(fb, x[: len(fb)]) for x in nullspace(rows, n)]


def inverse(m):
    n = len(m)
    a = [list(r) + [Fr(int(i == j)) for j in range(n)] for i, r in enumerate(m)]
    for c in range(n):
        piv = next(i for i in range(c, n) if a[i][c] != 0)
        a[c], a[piv] = a[piv], a[c]
        inv = 1 / a[c][c]
        a[c] = [x * inv for x in a[c]]
        for i in range(n):
            if i != c and a[i][c] != 0:
                k = a[i][c]
                a[i] = [x - k * y for x, y in zip(a[i], a[c])]
    return [r[n:] for r in a]


def invariance(t, p, d):
    if t == "plane":
        k = min(range(3), key=lambda i: (abs(d[i]), i))
        e = [Fr(int(i == k)) for i in range(3)]
        t1 = cross(d, e)
        t2 = cross(d, t1)
        z = [Fr(0)] * 3
        return [z + t1, z + t2, d + cross(p, d)]
    if t in ("line", "cylinder"):
        return [[Fr(0)] * 3 + d, d + cross(p, d)]
    if t in ("cone", "circle3d"):
        return [d + cross(p, d)]
    out = []
    for i in range(3):
        e = [Fr(int(i == j)) for j in range(3)]
        out.append(e + cross(p, e))
    return out


def metric(r2):
    return [r2, r2, r2, Fr(1), Fr(1), Fr(1)]


def projector(fb, r2):
    """Scaled-metric orthogonal projector onto span(fb), 36 floats row-major."""
    if not fb:
        return [0.0] * 36
    g = metric(r2)
    gram = [[total((a[i] * g[i] * b[i] for i in range(6)), Fr(0)) for b in fb] for a in fb]
    gi = inverse(gram)
    out = []
    for i in range(6):
        for j in range(6):
            inner = total((fb[a][i] * gi[a][b] * fb[b][j] for a in range(len(fb)) for b in range(len(fb))), Fr(0))
            si, sj = i < 3, j < 3
            if si and sj:
                out.append(float(inner * r2))
            elif si or sj:
                out.append(math.copysign(math.sqrt(float(inner * inner * r2)), inner) if inner != 0 else 0.0)
            else:
                out.append(float(inner))
    return out


# ------------------------------------------------------------------ the rank rule (80 digits)


def dec(x):
    return D(x.numerator) / D(x.denominator)


def d_orthonormal(vecs):
    out = []
    for v in vecs:
        w = list(v)
        for _ in range(2):
            for q in out:
                g = total((a * b for a, b in zip(q, w)), D(0))
                w = [a - g * b for a, b in zip(w, q)]
        n = total((a * a for a in w), D(0)).sqrt()
        out.append([a / n for a in w])
    return out


def d_eigvals(m):
    """Eigenvalues of a small symmetric Decimal matrix by cyclic Jacobi."""
    a = [list(row) for row in m]
    n = len(a)
    for _ in range(200):
        off = total((a[i][j] * a[i][j] for i in range(n) for j in range(n) if i != j), D(0))
        if off < D("1e-140"):
            break
        for p in range(n):
            for q in range(p + 1, n):
                if a[p][q] == 0:
                    continue
                th = (a[q][q] - a[p][p]) / (2 * a[p][q])
                sg = D(1) if th >= 0 else D(-1)
                t = sg / (abs(th) + (th * th + 1).sqrt())
                c = 1 / (t * t + 1).sqrt()
                sn = t * c
                for k in range(n):
                    akp, akq = a[k][p], a[k][q]
                    a[k][p], a[k][q] = c * akp - sn * akq, sn * akp + c * akq
                for k in range(n):
                    apk, aqk = a[p][k], a[q][k]
                    a[p][k], a[q][k] = c * apk - sn * aqk, sn * apk + c * aqk
    else:
        raise SystemExit("Jacobi did not converge")
    return [a[i][i] for i in range(n)]


def sigmas(fb, bb, r2):
    """Singular values (descending, 80 digits) of (I - P_B) restricted to span(fb), both in the
    scaled metric: the quantities the Rust rank rule classifies."""
    with localcontext() as ctx:
        ctx.prec = PREC
        r = dec(r2).sqrt()

        def scaled(v):
            return [dec(v[i]) * r for i in range(3)] + [dec(v[i]) for i in range(3, 6)]

        fo = d_orthonormal([scaled(v) for v in fb])
        bo = d_orthonormal([scaled(v) for v in bb])
        ms = []
        for col in fo:
            m = list(col)
            for b in bo:
                g = total((x * y for x, y in zip(b, col)), D(0))
                m = [x - g * y for x, y in zip(m, b)]
            ms.append(m)
        gram = [[total((x * y for x, y in zip(a, b)), D(0)) for b in ms] for a in ms]
        lam = d_eigvals(gram)
        return sorted(((x if x > 0 else D(0)).sqrt() for x in lam), reverse=True)


# ------------------------------------------------------------------ float naming


def fnorm(v):
    return math.sqrt(total((x * x for x in v), 0.0))


def fdot(a, b):
    return total((x * y for x, y in zip(a, b)), 0.0)


def orthonormal(vecs):
    out = []
    for v in vecs:
        r = list(v)
        for _ in range(2):
            for q in out:
                g = fdot(q, r)
                r = [x - g * y for x, y in zip(r, q)]
        n = fnorm(r)
        out.append([x / n for x in r])
    return out


def sym_eig_min(m):
    """Smallest eigenvalue of a small symmetric float matrix by cyclic Jacobi."""
    a = [list(r) for r in m]
    n = len(a)
    for _ in range(100):
        off = total((a[i][j] ** 2 for i in range(n) for j in range(n) if i != j), 0.0)
        if off < 1e-40:
            break
        for p in range(n):
            for q in range(p + 1, n):
                if a[p][q] == 0.0:
                    continue
                th = (a[q][q] - a[p][p]) / (2 * a[p][q])
                t = math.copysign(1.0, th) / (abs(th) + math.sqrt(th * th + 1))
                c = 1 / math.sqrt(t * t + 1)
                s = t * c
                for k in range(n):
                    akp, akq = a[k][p], a[k][q]
                    a[k][p], a[k][q] = c * akp - s * akq, s * akp + c * akq
                for k in range(n):
                    apk, aqk = a[p][k], a[q][k]
                    a[p][k], a[q][k] = c * apk - s * aqk, s * apk + c * aqk
    return min(a[i][i] for i in range(n))


def canonical(basis, cands, tau):
    d = len(basis)
    out, used = [], set()

    def residual(c):
        r = [total((b[c] * b[i] for b in basis), 0.0) for i in range(6)]
        for _ in range(2):
            for a in out:
                g = fdot(a, r)
                r = [x - g * y for x, y in zip(r, a)]
        return r, fnorm(r)

    for c in cands:
        if len(out) == d:
            break
        r, n = residual(c)
        if n > tau:
            out.append([x / n for x in r])
            used.add(c)
    if len(out) < d:
        raise SystemExit("canonical fallback needed: not expected for these hand-built cases")
    return out


def fmt9(x):
    s = "%.9f" % x
    return "0.000000000" if s == "-0.000000000" else s


def label(d):
    for k, name in enumerate("xyz"):
        if abs(d[k]) >= 1 - NAME_TOL and all(abs(d[j]) <= NAME_TOL for j in range(3) if j != k):
            return ("+" if d[k] > 0 else "-") + name
    return "(%s, %s, %s)" % tuple(fmt9(x) for x in d)


def sign_fix(t, part):
    k = max(range(3), key=lambda i: (abs(t[part + i]), -i))
    return [-x for x in t] if t[part + k] < 0 else t


def names(fb, r2, pairs):
    if not fb:
        return []
    r = math.sqrt(float(r2))
    g = metric(r2)
    tb = intersect(fb, [[Fr(0)] * 3 + [Fr(int(i == j)) for j in range(3)] for i in range(3)])
    # Complement of T in F under the scaled metric.
    if tb:
        rows = [[total((t[i] * g[i] * v[i] for i in range(6)), Fr(0)) for v in fb] for t in tb]
        rb = [combine(fb, c) for c in nullspace(rows, len(fb))]
    else:
        rb = list(fb)

    def scaled(v):
        return [float(v[i]) * r for i in range(3)] + [float(v[i]) for i in range(3, 6)]

    t_on = orthonormal([scaled(v) for v in tb])
    r_on = orthonormal([scaled(v) for v in rb])
    out = []
    for t in canonical(t_on, [3, 4, 5], 0.5):
        t = sign_fix(t, 3)
        v = t[3:]
        nv = fnorm(v)
        out.append("translation along " + label([x / nv for x in v]))
    if r_on:
        gram = [[fdot(a[:3], b[:3]) for b in r_on] for a in r_on]
        smin = math.sqrt(sym_eig_min(gram))
        for t in canonical(r_on, [0, 1, 2], 0.5 * smin):
            t = sign_fix(t, 0)
            w, v = t[:3], t[3:]
            nw, nv = fnorm(w), fnorm(v)
            if abs(fdot(w, v)) > NAME_TOL * nw * nv:
                out.append("generic twist")
                continue
            a = [x / nw for x in w]
            c = cross(w, v)
            nw2 = fdot(w, w)
            q = [(x / nw2) * r for x in c]
            out.append("rotation about %s through %s" % (label(a), through(a, q, r, pairs)))
    return out


def through(a, q, r, pairs):
    for i, (t, p, d) in enumerate(pairs):
        if t in AXIAL:
            pf = [float(x) for x in p]
            df = [float(x) for x in d]
            nd = fnorm(df)
            ai = [x / nd for x in df]
            if fnorm(cross(a, ai)) <= NAME_TOL and fnorm(cross([x - y for x, y in zip(pf, q)], a)) <= NAME_TOL * r:
                return "the axis of pair %d (%s)" % (i, t)
    for i, (t, p, d) in enumerate(pairs):
        if t in CENTRED:
            pf = [float(x) for x in p]
            if fnorm(cross([x - y for x, y in zip(pf, q)], a)) <= NAME_TOL * r:
                return "the centre of pair %d (%s)" % (i, t)
    if fnorm(q) <= NAME_TOL * r:
        return "the alignment origin"
    return "(%s, %s, %s) m" % tuple(fmt9(x) for x in q)


# ------------------------------------------------------------------ the accounting


def validate(pairs):
    """The first refusal in pair order, by the module's check order, else None."""
    if not pairs:
        return {"code": "empty_pairs", "pair": None}
    for i, pr in enumerate(pairs):
        t, d = pr["type"], pr["direction"]
        if t not in NOMINAL:
            assert t in KNOWN
            return {"code": "unsupported_feature_type", "pair": i}
        if t in DIRECTIONAL and d is None:
            return {"code": "missing_direction", "pair": i}
        if t not in DIRECTIONAL and d is not None:
            return {"code": "unexpected_direction", "pair": i}
        if d is not None and all(x == 0.0 for x in d):
            return {"code": "zero_direction", "pair": i}
    return None


def account(policy, pairs):
    err = validate(pairs)
    if err is not None:
        return {"error": err}
    ex = [(p["type"], [Fr(x) for x in p["location"]], None if p["direction"] is None else [Fr(x) for x in p["direction"]])
          for p in pairs]
    r2 = max(dot(p, p) for _, p, _ in ex)
    basis = "max_location"
    if r2 == 0:
        r2, basis = Fr(1), "unit_fallback"
    fb = [[Fr(int(i == j)) for j in range(6)] for i in range(6)]
    per_pair, warnings = [], []
    for i, (t, p, d) in enumerate(ex):
        proj = projector(fb, r2)
        inv = invariance(t, p, d)
        kept_min = dropped_max = None
        if fb:
            exact = intersect(fb, inv)
            sig = sigmas(fb, inv, r2)
            eff = total((1 for x in sig if x > RANK_TOL), 0)
            # The rank rule against the exact rank: equal (use the exact intersection), or every
            # value below the threshold (the free subspace is unchanged). Nothing else is needed
            # for these hand-built cases.
            if eff == len(fb) - len(exact):
                new = exact
            elif eff == 0:
                new = fb
            else:
                raise SystemExit("partial rank-rule disagreement: not supported by this reference")
            warnings.extend("near_rank_threshold:%d" % i for x in sig if NEAR_LO < x <= NEAR_HI)
            kept_min = float(sig[eff - 1]) if eff > 0 else None
            dropped_max = float(sig[eff]) if eff < len(sig) else None
        else:
            eff, new = 0, []
        assert 0 <= eff <= NOMINAL[t]
        if eff == 0:
            warnings.append("redundant_pair:%d" % i)
        per_pair.append({"nominal": NOMINAL[t], "effective": eff, "free_before": len(fb), "free_after": len(new),
                         "projector_before": proj, "kept_min": kept_min, "dropped_max": dropped_max})
        fb = new
    nm = names(fb, r2, ex)
    if nm and policy == "reject":
        return {"error": {"code": "underconstrained", "pair": None, "free_names": nm}}
    return {"ok": {
        "radius_basis": basis,
        "radius_sq": "%d/%d" % (r2.numerator, r2.denominator),
        "per_pair": per_pair,
        "free_count": len(fb),
        "free_names": nm,
        "directive": "hold_at_parent" if nm else "fully_constrained",
        "warnings": warnings,
    }}


def render():
    lines = []
    for name, policy, pairs in CASES:
        case = {"name": name, "policy": policy, "pairs": pairs, "expect": account(policy, pairs)}
        lines.append("  " + json.dumps(case, sort_keys=True, separators=(",", ":")))
    return '{\n "format": "%s",\n "cases": [\n%s\n ]\n}\n' % (FORMAT, ",\n".join(lines))


def main(argv):
    check = "--check" in argv[1:]
    if [a for a in argv[1:] if a != "--check"]:
        sys.stderr.write("usage: %s [--check]\n" % argv[0])
        return 2
    text = render()
    if check:
        old = OUT.read_bytes().decode("utf-8") if OUT.exists() else None
        if old != text:
            sys.stderr.write("%s is stale; rerun without --check\n" % OUT)
            return 1
        print("ok: %s" % OUT.name)
        return 0
    with open(OUT, "w", encoding="utf-8", newline="\n") as fh:
        fh.write(text)
    print("wrote %s (%d cases)" % (OUT, len(CASES)))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
