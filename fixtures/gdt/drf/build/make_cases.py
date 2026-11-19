#!/usr/bin/env python3
"""Independent high-precision reference for GDT.DRF.BUILD fixtures (stdlib only).

Writes `cases.json`: datum reference frames built from one to three simulated datums (planes
and axes, RMB), covering every precedence sequence of plane and axis for one, two and three
datums (2 + 4 + 8 = 14 sequences), each in a local placement and again under a fixed rigid
rotation and translation (`*_rot`). For every case it records either the expected frame (the
constrained DoFs of each datum, the free DoFs, the origin and the three axes) or the expected
refusal (`degenerate_datum_pair` and the named pair).

This script does not mirror the Rust case analysis. It works with rigid-body twists in
`decimal` arithmetic at 60 significant digits:

- the inputs are the exact binary64 values of the decimal strings written to the fixture
  (what a correctly rounding parser reads), normalised in high precision;
- a datum's first-order change under a twist is computed directly (a plane: the change of its
  normal and of its offset; an axis: the change of its direction and the motion of its points
  across the axis), and the datum constrains exactly the free coordinate DoFs whose twist
  changes it. The script asserts that the twists preserving the datum are spanned by the free
  coordinate DoFs it leaves (the DRF axes are aligned with the datum's invariance class);
- the origin moves, within the still-free translations, by the minimum-norm displacement that
  brings it onto the datum (a plane) or closest to it (an axis), by a least-squares solve over
  all free translations;
- a datum fixes the rotation about the primary direction when no free twist with a rotational
  part preserves it (a null-space test, not a geometric case split). The frame's clocking then
  follows the leaf's pinned rule: the datum's projected plane normal is `y`, its projected
  axis direction is `x`, and for an axis parallel to the primary direction the direction from
  the origin to that axis is `x`. A frame whose clocking stays free takes `x` from the world
  axis least aligned with `z` (lowest index on a tie), projected and normalised;
- a datum that constrains nothing is refused; the pair names the highest-precedence earlier
  datum of the same type with a parallel direction, else the immediately preceding datum.

Zero tests use 1e-12 (far above the 1e-16 representation error of the inputs, far below every
non-degenerate quantity in the cases). The rotated variants use a rotation with exact decimal
entries (products of 3-4-5 rotations). With `--check` the file is rebuilt in memory and
compared byte for byte with the committed one.

All geometry is synthetic, in metres; no standards text; CC0.
"""

import json
import sys
from decimal import Decimal, getcontext
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent
ZERO = Decimal("1e-12")
DOFS = ["tx", "ty", "tz", "rx", "ry", "rz"]
ALG = {"plane": "fit.tan_plane.minimax@1.0.0", "axis": "fit.tan_cylinder.max_inscribed@1.0.0"}
ROW = "dmt:asme/position/none@1"


def D(s):
    # The exact value of the binary64 nearest to the decimal string.
    return Decimal(float(s))


def add(a, b):
    return tuple(x + y for x, y in zip(a, b))


def sub(a, b):
    return tuple(x - y for x, y in zip(a, b))


def mul(a, s):
    return tuple(x * s for x in a)


def dot(a, b):
    return sum((x * y for x, y in zip(a, b)), Decimal(0))


def cross(a, b):
    return (a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0])


def norm(a):
    return dot(a, a).sqrt()


def unit(a):
    n = norm(a)
    assert n > ZERO
    return mul(a, 1 / n)


def reject(a, u):
    return sub(a, mul(u, dot(a, u)))


ZV = (Decimal(0),) * 3


def change(feat, o, w, v):
    """First-order change of a datum feature under the twist (w, v) about o."""
    kind, p, u = feat
    vel = add(v, cross(w, sub(p, o)))
    if kind == "plane":
        return list(cross(w, u)) + [dot(u, vel)]
    return list(cross(w, u)) + list(reject(vel, u))


def rref_null(cols):
    """Null-space basis of the matrix whose columns are `cols` (lists), by RREF."""
    n = len(cols)
    m = len(cols[0]) if cols else 0
    a = [[cols[j][i] for j in range(n)] for i in range(m)]
    pivots = []
    r = 0
    for c in range(n):
        best = max(range(r, m), key=lambda i: abs(a[i][c]), default=None)
        if best is None or abs(a[best][c]) <= ZERO:
            continue
        a[r], a[best] = a[best], a[r]
        pv = a[r][c]
        a[r] = [x / pv for x in a[r]]
        for i in range(m):
            if i != r and a[i][c] != 0:
                f = a[i][c]
                a[i] = [x - f * y for x, y in zip(a[i], a[r])]
        pivots.append(c)
        r += 1
        if r == m:
            break
    basis = []
    for fcol in range(n):
        if fcol in pivots:
            continue
        vec = [Decimal(0)] * n
        vec[fcol] = Decimal(1)
        for i, pc in enumerate(pivots):
            vec[pc] = -a[i][fcol]
        basis.append(vec)
    return len(pivots), basis


def min_norm_solve(g, h):
    """Minimum-norm solution of the consistent symmetric system g c = h (g may be singular):
    a particular solution by RREF with free variables zero, then its component in the null
    space of g removed by orthogonal projection."""
    n = len(h)
    cols = [[g[i][j] for i in range(n)] for j in range(n)] + [list(h)]
    rank, null = rref_null(cols)
    # The augmented column is not a pivot (consistency); its null vector gives -particular.
    part = [vec for vec in null if vec[n] == 1]
    assert len(part) == 1
    c = [-x for x in part[0][:n]]
    basis = []
    for vec in null:
        if vec[n] != 0:
            continue
        v = vec[:n]
        for q in basis:
            k = dot(v, q)
            v = [x - k * y for x, y in zip(v, q)]
        nv = norm(v)
        basis.append([x / nv for x in v])
    for q in basis:
        k = dot(c, q)
        c = [x - k * y for x, y in zip(c, q)]
    return c


def default_clock(z):
    mags = [abs(c) for c in z]
    k = min(range(3), key=lambda i: (mags[i], i))
    second = sorted(mags)[1]
    # No near-tie: the f64 code under test must pick the same world axis.
    assert second == mags[k] or second - mags[k] > Decimal("1e-6")
    e = tuple(Decimal(1) if i == k else Decimal(0) for i in range(3))
    x = unit(reject(e, z))
    return x, cross(z, x)


def twist(dof, frame):
    x, y, z = frame
    e = {"x": x, "y": y, "z": z}[dof[1]]
    return (ZV, e) if dof[0] == "t" else (e, ZV)


def relocate(feat, o, free, frame):
    """Minimum-norm move of o within the free translations onto (or closest to) the feature."""
    trans = [twist(d, frame)[1] for d in free if d[0] == "t"]
    if not trans:
        return o
    kind, p, u = feat
    # Residual r(c) = A c + b; columns of A are the changes of the residual per translation.
    if kind == "plane":
        cols = [[dot(u, e)] for e in trans]
        b = [dot(u, sub(o, p))]
    else:
        cols = [list(reject(e, u)) for e in trans]
        b = list(reject(sub(o, p), u))
    g = [[dot(ci, cj) for cj in cols] for ci in cols]
    h = [-dot(ci, b) for ci in cols]
    for ci, e in zip(min_norm_solve(g, h), trans):
        o = add(o, mul(e, ci))
    return o


def build(datums):
    feats = []
    for d in datums:
        p = tuple(D(s) for s in d["point"])
        u = unit(tuple(D(s) for s in d["dir"]))
        feats.append((d["kind"], p, u))
    z = feats[0][2]
    o = feats[0][1]
    clock = None
    free = list(DOFS)
    dof = []
    for i, feat in enumerate(feats):
        frame = (clock or default_clock(z)) + (z,)
        if i > 0:
            o = relocate(feat, o, free, frame)
        cols = [change(feat, o, *twist(f, frame)) for f in free]
        _, null = rref_null(cols)
        if "rz" in free and clock is None:
            k = free.index("rz")
            if all(abs(vec[k]) <= ZERO for vec in null):
                kind, p, u = feat
                w = reject(u, z)
                if kind == "plane":
                    y = unit(w)
                    clock = (cross(y, z), y)
                elif norm(w) > ZERO:
                    x = unit(w)
                    clock = (x, cross(z, x))
                else:
                    x = unit(reject(sub(p, o), z))
                    clock = (x, cross(z, x))
                frame = clock + (z,)
                cols = [change(feat, o, *twist(f, frame)) for f in free]
        rank, _ = rref_null(cols)
        hit = [f for f, c in zip(free, cols) if max(abs(x) for x in c) > ZERO]
        assert rank == len(hit), ("invariance not axis-aligned", datums[i]["label"])
        if not hit:
            same = [j for j in range(i) if feats[j][0] == feat[0] and norm(cross(feats[j][2], feat[2])) <= ZERO]
            j = same[0] if same else i - 1
            return {"error": {"code": "degenerate_datum_pair", "pair": [datums[j]["label"], datums[i]["label"]]}}
        dof.append(hit)
        free = [f for f in free if f not in hit]
    x, y = clock or default_clock(z)
    return {"ok": {"origin": [fmt(c) for c in o], "axes": [[fmt(c) for c in a] for a in (x, y, z)], "dof": dof, "free": free}}


def fmt(v):
    if abs(v) < Decimal("1e-40"):
        return "0"
    return format(v, ".25e")


# ---------------------------------------------------------------------------------------------
# Cases (local placement). P: plane (point, normal); X: axis (point, direction).
Z = ("0", "0", "1")
Y = ("0", "1", "0")
X = ("1", "0", "0")


def P(label, point, n):
    return {"label": label, "kind": "plane", "point": list(point), "dir": list(n)}


def A(label, point, d):
    return {"label": label, "kind": "axis", "point": list(point), "dir": list(d)}


PA = P("A", ("0.1", "0.2", "0.01"), Z)
XA = A("A", ("0.01", "0.02", "0.5"), Z)
CASES = [
    # one datum
    ("p_single", [PA]),
    ("x_single", [XA]),
    # plane, plane
    ("pp_perp", [PA, P("B", ("0.3", "0.02", "0.05"), Y)]),
    ("pp_oblique", [PA, P("B", ("0.3", "0.05", "0.05"), ("0", "0.6", "0.8"))]),
    ("pp_parallel", [PA, P("B", ("0.3", "0.02", "0.05"), Z)]),
    # plane, axis
    ("plate_bore", [PA, A("B", ("0.04", "0.03", "0.2"), Z)]),
    ("px_inplane", [PA, A("B", ("0.2", "0.05", "0.02"), X)]),
    ("px_oblique", [PA, A("B", ("0.1", "0.2", "0.3"), ("0.6", "0", "0.8"))]),
    # axis, plane
    ("xp_perp", [XA, P("B", ("0.3", "0.4", "0.1"), Z)]),
    ("xp_parallel", [XA, P("B", ("0.3", "0.07", "0.1"), Y)]),
    ("xp_oblique", [XA, P("B", ("0.3", "0.07", "0.1"), ("0", "0.6", "0.8"))]),
    # axis, axis
    ("xx_offset", [XA, A("B", ("0.06", "0.02", "0.3"), Z)]),
    ("xx_coaxial", [XA, A("B", ("0.01", "0.02", "0.3"), Z)]),
    ("xx_perp", [XA, A("B", ("0.4", "0.02", "0.2"), X)]),
    ("xx_skew", [XA, A("B", ("0.1", "0", "0.2"), ("0.48", "0.64", "0.6"))]),
    # plane, plane, plane
    ("block_abc", [P("A", ("0.1", "0.2", "0.01"), Z), P("B", ("0.3", "0.02", "0.05"), Y), P("C", ("0.03", "0.4", "0.06"), X)]),
    ("ppp_oblique", [PA, P("B", ("0.3", "0.02", "0.05"), Y), P("C", ("0.03", "0.4", "0.06"), ("0.8", "0.6", "0"))]),
    ("ppp_c_par_a", [PA, P("B", ("0.3", "0.02", "0.05"), Y), P("C", ("0.03", "0.4", "0.06"), Z)]),
    ("ppp_c_par_b", [PA, P("B", ("0.3", "0.02", "0.05"), Y), P("C", ("0.03", "0.4", "0.06"), Y)]),
    ("ppp_c_in_yz", [PA, P("B", ("0.3", "0.02", "0.05"), Y), P("C", ("0.03", "0.4", "0.06"), ("0", "0.6", "0.8"))]),
    # plane, plane, axis
    ("ppx_hole", [PA, P("B", ("0.3", "0.02", "0.05"), Y), A("C", ("0.05", "0.07", "0.3"), Z)]),
    ("ppx_along_y", [PA, P("B", ("0.3", "0.02", "0.05"), Y), A("C", ("0.04", "0.2", "0.03"), Y)]),
    ("ppx_along_x", [PA, P("B", ("0.3", "0.02", "0.05"), Y), A("C", ("0.04", "0.2", "0.03"), X)]),
    # plane, axis, plane
    ("pxp_clock", [PA, A("B", ("0.04", "0.03", "0.2"), Z), P("C", ("0.3", "0.08", "0.05"), ("0.6", "0.8", "0"))]),
    ("pxp_c_par_a", [PA, A("B", ("0.04", "0.03", "0.2"), Z), P("C", ("0.3", "0.08", "0.05"), Z)]),
    ("pxp_inplane", [PA, A("B", ("0.2", "0.05", "0.02"), X), P("C", ("0.09", "0.3", "0.05"), X)]),
    ("pxp_full", [PA, A("B", ("0.1", "0.2", "0.3"), ("0.6", "0", "0.8")), P("C", ("0.09", "0.3", "0.05"), X)]),
    # plane, axis, axis
    ("pxx_two_holes", [PA, A("B", ("0.04", "0.03", "0.2"), Z), A("C", ("0.1", "0.11", "0.2"), Z)]),
    ("pxx_coaxial", [PA, A("B", ("0.04", "0.03", "0.2"), Z), A("C", ("0.04", "0.03", "0.5"), Z)]),
    ("pxx_inplane_hole", [PA, A("B", ("0.2", "0.05", "0.02"), X), A("C", ("0.07", "0.3", "0.1"), Z)]),
    # axis, plane, plane
    ("xpp_clock", [XA, P("B", ("0.3", "0.4", "0.1"), Z), P("C", ("0.3", "0.07", "0.2"), Y)]),
    ("xpp_tz", [XA, P("B", ("0.3", "0.07", "0.1"), Y), P("C", ("0.3", "0.4", "0.1"), Z)]),
    ("xpp_c_par_b", [XA, P("B", ("0.3", "0.4", "0.1"), Z), P("C", ("0.3", "0.4", "0.2"), Z)]),
    # axis, plane, axis
    ("xpx_offset", [XA, P("B", ("0.3", "0.4", "0.1"), Z), A("C", ("0.06", "0.05", "0.3"), Z)]),
    ("xpx_tz", [XA, P("B", ("0.3", "0.07", "0.1"), Y), A("C", ("0.4", "0.02", "0.2"), X)]),
    ("xpx_coaxial", [XA, P("B", ("0.3", "0.4", "0.1"), Z), A("C", ("0.01", "0.02", "0.3"), Z)]),
    # axis, axis, plane
    ("xxp_tz", [XA, A("B", ("0.06", "0.02", "0.3"), Z), P("C", ("0.3", "0.4", "0.1"), Z)]),
    ("xxp_full", [XA, A("B", ("0.4", "0.02", "0.2"), X), P("C", ("0.3", "0.4", "0.1"), Z)]),
    # axis, axis, axis
    ("xxx_tz", [XA, A("B", ("0.06", "0.02", "0.3"), Z), A("C", ("0.3", "0.05", "0.25"), Y)]),
    ("xxx_c_par_a", [XA, A("B", ("0.06", "0.02", "0.3"), Z), A("C", ("-0.03", "0.08", "0.3"), Z)]),
]

# Rigid placement of the `*_rot` variants: R = Rz(c=0.8, s=0.6) . Rx(c=0.6, s=0.8), exact
# decimals, and translation T.
R = [
    [Decimal("0.8"), Decimal("-0.36"), Decimal("0.48")],
    [Decimal("0.6"), Decimal("0.48"), Decimal("-0.64")],
    [Decimal("0"), Decimal("0.8"), Decimal("0.6")],
]
T = [Decimal("0.125"), Decimal("-0.25"), Decimal("0.375")]


def rot(v, translate):
    out = []
    for i in range(3):
        s = sum((R[i][j] * Decimal(v[j]) for j in range(3)), Decimal(0))
        if translate:
            s += T[i]
        out.append(format(s.normalize(), "f"))
    return out


def rotated(datums):
    return [dict(d, point=rot(d["point"], True), dir=rot(d["dir"], False)) for d in datums]


def build_all():
    cases = []
    for name, datums in CASES:
        for suffix, ds in (("", datums), ("_rot", rotated(datums))):
            for d in ds:
                d["alg"] = ALG[d["kind"]]
                d["dmt_row"] = ROW
            cases.append({"name": name + suffix, "datums": ds, "expect": build(ds)})
    seqs = {"".join("P" if d["kind"] == "plane" else "X" for d in ds) for _, ds in CASES}
    assert len(seqs) == 14, sorted(seqs)
    doc = {"note": "generated by make_cases.py (GDT.DRF.BUILD); synthetic, CC0", "cases": cases}
    return {"cases.json": json.dumps(doc, indent=1, sort_keys=True) + "\n"}


def main():
    check = "--check" in sys.argv[1:]
    failed = False
    for name, text in build_all().items():
        data = text.encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"{name}: differs from make_cases.py output", file=sys.stderr)
                failed = True
        else:
            path.write_bytes(data)
    if failed:
        sys.exit(1)
    print("ok" if check else "written")


if __name__ == "__main__":
    main()
