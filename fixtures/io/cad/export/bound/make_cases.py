#!/usr/bin/env python3
"""Independent reference for IO.CAD.EXPORT.BOUND (export plan and extents). CC-BY-4.0 data.

Stdlib only. Writes, from the rules of spec node IO.CAD.EXPORT.BOUND and the pinned
expressions in the `fid-io-cad-export` `bound` module documentation, GEO.FRAME.XFORM,
GEO.FRAME.TREE and GEO.NURBS.ANALYTIC (nothing here was produced by running the Rust crate):

* `plane_points.json`: AC-1, a plane with 4 fitted points spanning 10 x 20 mm in its own
  axes, no extent, target its own frame;
* `cases.json`: AC-2 (caller axial extent), AC-3 (`extent_required`), AC-4 (circle in a frame
  translated by (1, 0, 0) m, target `world`), plus tilted, rotated, two-level, sibling
  (inverse plus compose) and similarity frames, every bounded kind, and every refusal.

Both files share one layout: `frames` (inserted in list order under their parent; world is
the all-zero ID), then `cases`, each with a `target` frame, a `margin`, the `elements` in
selection order, and either `expect.items` (one entry per element, every float written as
the shortest repr of the exact binary64 result, compared bit for bit) or `expect.error`.

`--check` regenerates both files in memory and fails on any difference with the committed
ones (LF line endings).

Rules reproduced (binary64; Python floats are IEEE binary64 and never fused; `sqrt` is
correctly rounded by IEEE 754, so no value depends on a libm):

* dot3(a, b) = (a0*b0 + a1*b1) + a2*b2; R x by rows; R^T x by columns; C = A B by dot3;
* apply_point: s * (R x)[i] + t[i]; apply_dir: (R x)[i]; compose a.b: R = Ra Rb,
  t[i] = sa * (Ra tb)[i] + ta[i], s = sa * sb; inverse: R^T, s' = 1/s, t'[i] = -(s' * (R^T t)[i]);
* resolve(a, b): ancestor lists child first, common tail popped, chains folded upward
  (acc = T(g) . acc), then T_a, inverse(T_b) or inverse(T_b) . T_a;
* Placement: Z = z/|z|; x' = x - (x.Z) Z; X = x'/|x'|; Y = Z x X (per component);
* to_local: d = q - O per component, (d.X, d.Y, d.Z) by dot3;
* fitted projections: min and max (total order, first point first) in the element's frame;
  a caller extent as given (lo < hi);
* moved placement: z1 = R Z, x1 = R X, z = z1/|z1|, x' = x1 - (x1.z) z, x = x'/|x'|, then
  the Placement rule above on (origin', z, x); radii s * r;
* target extent: caller s * v; fitted s * min - m and s * max + m (the margin m in target
  metres); refused (degenerate_extent) when lo >= hi; zeros written as +0.0 (v + 0.0);
* cone apex, on the moved cone and the target extent: refused when
  r0 + h0 tan(a) < 0 < r0 + h1 tan(a). Only this boolean uses tan (math.tan here, `fid-num`
  in Rust); the fixtures keep it far from the boundary.
"""

import json
import math
import os
import sys
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))
FORMAT = "fid.test.io.cad.export.bound/1"
WORLD = "0" * 32

# ---------------------------------------------------------------- pinned arithmetic


def dot3(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def mat_vec(r, x):
    return [dot3(r[i], x) for i in range(3)]


def mat_t_vec(r, x):
    return [dot3([r[0][i], r[1][i], r[2][i]], x) for i in range(3)]


def mat_mul(a, b):
    return [[dot3(a[i], [b[0][j], b[1][j], b[2][j]]) for j in range(3)] for i in range(3)]


def transpose(r):
    return [[r[0][i], r[1][i], r[2][i]] for i in range(3)]


class Xf:
    def __init__(self, r, t, s, rigid):
        self.r, self.t, self.s, self.rigid = r, t, s, rigid

    def compose(self, other):
        r = mat_mul(self.r, other.r)
        u = mat_vec(self.r, other.t)
        t = [self.s * u[i] + self.t[i] for i in range(3)]
        return Xf(r, t, self.s * other.s, self.rigid and other.rigid)

    def inverse(self):
        si = 1.0 / self.s
        u = mat_t_vec(self.r, self.t)
        return Xf(transpose(self.r), [-(si * ui) for ui in u], si, self.rigid)

    def point(self, x):
        u = mat_vec(self.r, x)
        y = [self.s * u[i] + self.t[i] for i in range(3)]
        assert all(math.isfinite(c) for c in y)
        return y

    def dir(self, x):
        return mat_vec(self.r, x)


IDENTITY = Xf([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0], 1.0, True)


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def placement(origin, axis, ref):
    """GEO.NURBS.ANALYTIC Placement::new: validation, then the pinned orthonormalisation."""
    assert abs(math.sqrt(dot3(axis, axis)) - 1.0) <= 1e-12
    assert abs(math.sqrt(dot3(ref, ref)) - 1.0) <= 1e-12
    assert abs(dot3(axis, ref)) <= 1e-12
    nz = math.sqrt(dot3(axis, axis))
    z = [axis[i] / nz for i in range(3)]
    p = dot3(ref, z)
    xp = [ref[i] - p * z[i] for i in range(3)]
    nx = math.sqrt(dot3(xp, xp))
    x = [xp[i] / nx for i in range(3)]
    return {"origin": list(origin), "axis": z, "ref_dir": x, "y_dir": cross(z, x)}


def to_local(pl, q):
    d = [q[i] - pl["origin"][i] for i in range(3)]
    return [dot3(d, pl["ref_dir"]), dot3(d, pl["y_dir"]), dot3(d, pl["axis"])]


def unit(v):
    n = math.sqrt(dot3(v, v))
    assert n > 0.0
    return [v[i] / n for i in range(3)]


def move_placement(pl, xf):
    origin = xf.point(pl["origin"])
    z1 = xf.dir(pl["axis"])
    x1 = xf.dir(pl["ref_dir"])
    z = unit(z1)
    p = dot3(x1, z)
    x = unit([x1[i] - p * z[i] for i in range(3)])
    return placement(origin, z, x)


def pz(v):
    return v + 0.0


# ---------------------------------------------------------------- frames

ROT_Z = [[0.6, -0.8, 0.0], [0.8, 0.6, 0.0], [0.0, 0.0, 1.0]]
ROT_3D = [[2 / 3, -1 / 3, 2 / 3], [2 / 3, 2 / 3, -1 / 3], [-1 / 3, 2 / 3, 2 / 3]]
ROT_X = [[1.0, 0.0, 0.0], [0.0, 0.28, -0.96], [0.0, 0.96, 0.28]]
I3 = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]

FRAMES = [
    # (id, name, parent, rotation, translation, scale or None)
    ("10000000000000000000000000000001", "shift_x", WORLD, I3, [1.0, 0.0, 0.0], None),
    ("10000000000000000000000000000002", "rot_z", WORLD, ROT_Z, [0.1, -0.2, 0.05], None),
    ("10000000000000000000000000000003", "rot_3d", "10000000000000000000000000000002", ROT_3D,
     [0.003, 0.002, -0.001], None),
    ("10000000000000000000000000000004", "scaled", WORLD, I3, [0.0, 0.0, 0.5], 1.5),
    ("10000000000000000000000000000005", "sibling", WORLD, ROT_X, [0.0, 0.5, 0.0], None),
]
F_SHIFT, F_ROTZ, F_ROT3, F_SCALED, F_SIB = (f[0] for f in FRAMES)
F_MISSING = "20000000000000000000000000000009"


def check_rotation(r):
    """XFORM's acceptance in exact arithmetic of the binary64 entries (well inside 1e-12)."""
    fr = [[Fraction(v) for v in row] for row in r]
    s = Fraction(0)
    for i in range(3):
        for j in range(3):
            g = sum(fr[k][i] * fr[k][j] for k in range(3))
            s += (g - (1 if i == j else 0)) ** 2
    assert s <= Fraction(1, 10 ** 26)
    det = (fr[0][0] * (fr[1][1] * fr[2][2] - fr[1][2] * fr[2][1])
           - fr[0][1] * (fr[1][0] * fr[2][2] - fr[1][2] * fr[2][0])
           + fr[0][2] * (fr[1][0] * fr[2][1] - fr[1][1] * fr[2][0]))
    assert abs(det - 1) <= Fraction(1, 10 ** 13)


def frame_table(frames):
    table = {WORLD: (None, IDENTITY)}
    for fid, _name, parent, r, t, s in frames:
        check_rotation(r)
        assert parent in table
        table[fid] = (parent, Xf(r, t, 1.0 if s is None else s, s is None))
    return table


def ancestors(table, f):
    out = [f]
    while table[f][0] is not None:
        f = table[f][0]
        out.append(f)
    return out


def chain(table, frames):
    if not frames:
        return None
    acc = table[frames[0]][1]
    for g in frames[1:]:
        acc = table[g][1].compose(acc)
    return acc


def resolve(table, a, b):
    if a == b:
        return IDENTITY
    la, lb = ancestors(table, a), ancestors(table, b)
    while la and lb and la[-1] == lb[-1]:
        la.pop()
        lb.pop()
    ta, tb = chain(table, la), chain(table, lb)
    if tb is None:
        return ta
    if ta is None:
        return tb.inverse()
    return tb.inverse().compose(ta)


# ---------------------------------------------------------------- elements and the plan

UNBOUNDED = {"line": "line", "plane": "rect", "cylinder": "axial", "cone": "axial"}
AXES = {"line": ["t"], "rect": ["u", "v"], "axial": ["h"]}
COMPS = {"line": [0], "rect": [0, 1], "axial": [2]}


def uid(n):
    return "00000000-0000-7000-8000-%012x" % n


class Refused(Exception):
    def __init__(self, code, element=None, **detail):
        super().__init__(code)
        self.code, self.element, self.detail = code, element, detail


def forbidden(c):
    """fid_types::text::FORBIDDEN_RANGES, the subset these fixtures exercise (Cc and the BOM)."""
    u = ord(c)
    return u <= 0x1F or 0x7F <= u <= 0x9F or u == 0xFEFF or (0x202A <= u <= 0x202E)


def source_placement(g):
    return placement(g["origin"], g["axis"], g["ref_dir"])


def raw_bounds(e):
    """The input extent rules in the element's own frame: None (bounded), or (kind, caller
    (lo, hi) or fitted (min, max) per axis, source)."""
    g = e["geom"]
    kind = g["kind"]
    ek = UNBOUNDED.get(kind)
    if ek is None:
        if e["extent"] is not None:
            raise Refused("extent_not_applicable", e["id"], kind=kind)
        if e["points"] is not None:
            raise Refused("points_not_applicable", e["id"], reason="bounded")
        return None
    if e["extent"] is not None:
        x = e["extent"]
        if x["kind"] != ek:
            raise Refused("extent_kind_mismatch", e["id"], expected=ek, found=x["kind"])
        if e["points"] is not None:
            raise Refused("points_not_applicable", e["id"], reason="caller_extent")
        vals = x["values"]
        for i, axis in enumerate(AXES[ek]):
            if not vals[2 * i] < vals[2 * i + 1]:
                raise Refused("bad_extent", e["id"], fault="empty", axis=axis)
        return ek, list(vals), {"source": "caller"}
    pts = e["points"]
    if pts is None:
        raise Refused("extent_required", e["id"])
    if not pts:
        raise Refused("empty_points", e["id"])
    pl = source_placement(g)
    values = []
    locals_ = [to_local(pl, q) for q in pts]
    for c in COMPS[ek]:
        vs = [l[c] for l in locals_]
        lo, hi = vs[0], vs[0]
        for v in vs[1:]:
            if v < lo or (v == lo and math.copysign(1, v) < math.copysign(1, lo)):
                lo = v
            if v > hi or (v == hi and math.copysign(1, v) > math.copysign(1, hi)):
                hi = v
        values += [lo, hi]
    return ek, values, {"source": "fitted_points", "points": len(pts)}


def target_bounds(e, raw, s, margin, moved):
    """Scale into the target frame, add the margin there (fitted only), refuse zero width,
    then the apex rule on the moved cone."""
    ek, vals, source = raw
    out = []
    fitted = source["source"] == "fitted_points"
    for i, axis in enumerate(AXES[ek]):
        a, b = vals[2 * i], vals[2 * i + 1]
        lo, hi = (s * a - margin, s * b + margin) if fitted else (s * a, s * b)
        assert math.isfinite(lo) and math.isfinite(hi)
        if lo >= hi:
            raise Refused("degenerate_extent", e["id"], axis=axis)
        out += [pz(lo), pz(hi)]
    if moved["kind"] == "cone":
        ta = math.tan(moved["half_angle"])
        r0 = moved["radius"]
        a0, a1 = r0 + out[0] * ta, r0 + out[1] * ta
        # Keep the decision far from rounding (math.tan and fid-num may differ by an ulp).
        assert abs(a0) > 1e-9 and abs(a1) > 1e-9
        if a0 < 0.0 < a1:
            raise Refused("extent_crosses_apex", e["id"])
    if fitted:
        source = dict(source, margin=margin)
    return dict(extent=ek, values=out, **source)


def move(g, xf):
    kind = g["kind"]
    s = xf.s
    out = {"kind": kind}
    if kind == "point":
        out["point"] = xf.point(g["point"])
    elif kind in ("nurbs_curve", "nurbs_surface"):
        out["ctrl"] = [xf.point(p) for p in g["ctrl"]]
    else:
        pl = move_placement(source_placement(g), xf)
        out.update({"origin": pl["origin"], "axis": pl["axis"], "ref_dir": pl["ref_dir"]})
        for key in ("radius", "major_radius", "minor_radius", "semi_axis_1", "semi_axis_2"):
            if key in g:
                out[key] = s * g[key]
        if "half_angle" in g:
            out["half_angle"] = g["half_angle"]
    return out


def run_case(table, case):
    target, margin = case["target"], case["margin"]
    if target not in table:
        raise Refused("frame_not_found", None, frame=target)
    if not (math.isfinite(margin) and margin >= 0.0):
        raise Refused("bad_margin", None)
    margin = margin + 0.0
    if not case["elements"]:
        raise Refused("empty_selection", None)
    seen = {}
    items = []
    for index, e in enumerate(case["elements"]):
        name = e["name"]
        if name == "":
            raise Refused("bad_name", e["id"], fault="empty")
        if len(name.encode("utf-8")) > 255:
            raise Refused("bad_name", e["id"], fault="too_long")
        for off, ch in ((len(name[:i].encode("utf-8")), c) for i, c in enumerate(name)):
            if forbidden(ch):
                raise Refused("bad_name", e["id"], fault="forbidden_char", offset=off)
        if e["id"] in seen:
            raise Refused("duplicate_element", e["id"], first=seen[e["id"]], index=index)
        seen[e["id"]] = index
        if e["frame"] not in table:
            raise Refused("frame_not_found", e["id"], frame=e["frame"])
        xf = resolve(table, e["frame"], target)
        raw = raw_bounds(e)
        geom = move(e["geom"], xf)
        item = {"id": e["id"], "geom": geom}
        item["bounds"] = "intrinsic" if raw is None else target_bounds(e, raw, xf.s, margin, geom)
        items.append(item)
    return items


# ---------------------------------------------------------------- the cases

Z = [0.0, 0.0, 1.0]
X = [1.0, 0.0, 0.0]


def el(n, name, frame, geom, points=None, extent=None):
    return {"id": uid(n), "name": name, "frame": frame, "geom": geom, "points": points, "extent": extent}


def plane(o, axis, ref):
    return {"kind": "plane", "origin": o, "axis": axis, "ref_dir": ref}


def line(o, axis, ref):
    return {"kind": "line", "origin": o, "axis": axis, "ref_dir": ref}


def cyl(o, axis, ref, r):
    return {"kind": "cylinder", "origin": o, "axis": axis, "ref_dir": ref, "radius": r}


def cone(o, axis, ref, r, a):
    return {"kind": "cone", "origin": o, "axis": axis, "ref_dir": ref, "radius": r, "half_angle": a}


def on_placement(pl, u, v, w):
    """A data point O + u X + v Y + w Z (inputs only; any rounding is part of the data)."""
    o, x, y, z = pl["origin"], pl["ref_dir"], pl["y_dir"], pl["axis"]
    return [o[i] + u * x[i] + v * y[i] + w * z[i] for i in range(3)]


def build_cases():
    ac1 = {
        "name": "ac1_plane_points",
        "ac": "AC-1",
        "target": WORLD,
        "margin": 0.0,
        "elements": [el(1, "PLN-1", WORLD, plane([0.0, 0.0, 0.0], Z, X),
                        points=[[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.0, 0.02, 0.0], [0.01, 0.02, 0.0]])],
    }
    tilt = placement([0.01, 0.02, 0.03], [0.0, 0.6, 0.8], X)
    tilt_pts = [on_placement(tilt, u, v, w) for (u, v, w) in
                [(0.0, 0.0, 1e-6), (0.012, 0.001, -2e-6), (0.004, 0.018, 0.0), (-0.003, 0.009, 3e-6),
                 (0.011, 0.017, -1e-6), (0.006, -0.002, 0.0)]]
    sib_line = placement([0.002, -0.001, 0.004], [0.0, 0.0, 1.0], [0.6, 0.8, 0.0])
    sib_pts = [on_placement(sib_line, t, 1e-6 * k, -1e-6 * k) for k, t in enumerate([0.015, -0.004, 0.009, 0.031])]
    cone_pl = placement([0.0, 0.0, 0.0], Z, X)
    cone_pts = [on_placement(cone_pl, 0.0, 0.0, h) for h in (0.002, 0.02, 0.011)]
    cyl_pl = placement([0.01, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0])
    cyl_pts = [on_placement(cyl_pl, 0.004, 0.0, h) for h in (-0.003, 0.012, 0.007)]
    nurbs = {"kind": "nurbs_curve", "degree": 1, "knots": [0.0, 0.0, 1.0, 1.0],
             "ctrl": [[0.0, 0.0, 0.0], [0.01, 0.005, -0.002]]}
    # Bilinear patch, control net row-major (i * n_v + j).
    nurbs_srf = {"kind": "nurbs_surface", "degree_u": 1, "degree_v": 1, "knots_u": [0.0, 0.0, 1.0, 1.0],
                 "knots_v": [0.0, 0.0, 1.0, 1.0], "n_u": 2, "n_v": 2,
                 "ctrl": [[0.0, 0.0, 0.0], [0.0, 0.01, 0.001], [0.02, 0.0, -0.001], [0.02, 0.01, 0.002]]}
    # Two points with the same t on a line whose direction (ref_dir) is +z.
    same = [[0.01, 0.0, 0.002], [0.01, 0.003, 0.002]]

    cases = [
        {"name": "ac2_cylinder_caller", "ac": "AC-2", "target": WORLD, "margin": 0.0,
         "elements": [el(2, "CYL-1", F_ROT3, cyl([0.0, 0.0, 0.0], Z, X, 0.004),
                         extent={"kind": "axial", "values": [-0.005, 0.015]})]},
        {"name": "ac3_line_no_points", "ac": "AC-3", "target": WORLD, "margin": 0.0,
         "elements": [el(3, "LIN-1", WORLD, line([0.0, 0.0, 0.0], Z, X))]},
        {"name": "ac4_circle_shift", "ac": "AC-4", "target": WORLD, "margin": 0.0,
         "elements": [el(4, "CIR-1", F_SHIFT, {"kind": "circle", "origin": [0.25, -0.5, 0.125], "axis": Z,
                                               "ref_dir": X, "radius": 0.01})]},
        {"name": "tilted_plane_margin_two_levels", "ac": "REQ-002, REQ-004", "target": WORLD, "margin": 0.0005,
         "elements": [el(5, "PLN-2", F_ROT3, plane([0.01, 0.02, 0.03], [0.0, 0.6, 0.8], X), points=tilt_pts)]},
        {"name": "line_fitted_into_sibling", "ac": "REQ-002, REQ-004", "target": F_ROTZ, "margin": 0.0,
         "elements": [el(6, "LIN-2", F_SIB, line([0.002, -0.001, 0.004], Z, [0.6, 0.8, 0.0]), points=sib_pts)]},
        {"name": "cone_fitted", "ac": "REQ-002", "target": WORLD, "margin": 0.001,
         "elements": [el(7, "CON-1", F_ROTZ, cone([0.0, 0.0, 0.0], Z, X, 0.01, 0.3), points=cone_pts)]},
        {"name": "similarity_cylinder", "ac": "REQ-002, REQ-004", "target": WORLD, "margin": 0.0,
         "elements": [el(8, "CYL-2", F_SCALED, cyl([0.01, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0.004),
                         points=cyl_pts)]},
        {"name": "similarity_cylinder_margin", "ac": "REQ-002, REQ-004", "target": WORLD, "margin": 0.001,
         "elements": [el(38, "CYL-3", F_SCALED, cyl([0.01, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0], 0.004),
                         points=cyl_pts)]},
        {"name": "bounded_kinds_rotated", "ac": "REQ-004", "target": WORLD, "margin": 0.0,
         "elements": [
             el(9, "PT-1", F_ROT3, {"kind": "point", "point": [0.001, -0.002, 0.003]}),
             el(10, "SPH-1", F_ROT3, {"kind": "sphere", "origin": [0.01, 0.0, 0.0], "axis": Z, "ref_dir": X,
                                      "radius": 0.005}),
             el(11, "TOR-1", F_ROT3, {"kind": "torus", "origin": [0.0, 0.01, 0.0], "axis": [0.0, 0.6, 0.8],
                                      "ref_dir": X, "major_radius": 0.02, "minor_radius": 0.003}),
             el(12, "ELL-1", F_ROT3, {"kind": "ellipse", "origin": [0.0, 0.0, 0.01], "axis": Z,
                                      "ref_dir": [0.6, 0.8, 0.0], "semi_axis_1": 0.008, "semi_axis_2": 0.005}),
             el(13, "NRB-1", F_ROT3, nurbs),
             el(37, "NRB-2", F_ROT3, nurbs_srf),
             el(14, "CIR-2", F_SCALED, {"kind": "circle", "origin": [0.0, 0.0, 0.0], "axis": Z, "ref_dir": X,
                                        "radius": 0.002}),
         ]},
        {"name": "degenerate_with_margin_ok", "ac": "REQ-002", "target": WORLD, "margin": 0.001,
         "elements": [el(15, "LIN-3", WORLD, line([0.0, 0.0, 0.0], X, Z), points=same)]},
        # refusals
        {"name": "degenerate_line", "ac": "REQ-002", "target": WORLD, "margin": 0.0,
         "elements": [el(16, "LIN-4", WORLD, line([0.0, 0.0, 0.0], X, Z), points=same)]},
        {"name": "degenerate_plane_v", "ac": "REQ-002", "target": WORLD, "margin": 0.0,
         "elements": [el(17, "PLN-3", WORLD, plane([0.0, 0.0, 0.0], Z, X),
                         points=[[0.0, 0.0, 0.0], [0.01, 0.0, 0.0], [0.004, 0.0, 0.0]])]},
        {"name": "cone_caller_crosses_apex", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(18, "CON-2", WORLD, cone([0.0, 0.0, 0.0], Z, X, 0.01, 0.3),
                         extent={"kind": "axial", "values": [-0.05, 0.01]})]},
        {"name": "cone_caller_negative_nappe_ok", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(19, "CON-3", WORLD, cone([0.0, 0.0, 0.0], Z, X, 0.01, 0.3),
                         extent={"kind": "axial", "values": [-0.06, -0.05]})]},
        {"name": "duplicate_element", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(20, "PT-2", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]}),
                      el(21, "PT-3", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]}),
                      el(20, "PT-4", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "extent_on_bounded", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(22, "CIR-3", WORLD, {"kind": "circle", "origin": [0.0, 0.0, 0.0], "axis": Z, "ref_dir": X,
                                              "radius": 0.01}, extent={"kind": "axial", "values": [0.0, 0.01]})]},
        {"name": "extent_kind_mismatch", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(23, "PLN-4", WORLD, plane([0.0, 0.0, 0.0], Z, X),
                         extent={"kind": "axial", "values": [0.0, 0.01]})]},
        {"name": "points_on_bounded", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(24, "SPH-2", WORLD, {"kind": "sphere", "origin": [0.0, 0.0, 0.0], "axis": Z, "ref_dir": X,
                                              "radius": 0.01}, points=[[0.01, 0.0, 0.0]])]},
        {"name": "points_and_caller_extent", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(25, "LIN-5", WORLD, line([0.0, 0.0, 0.0], X, Z), points=same,
                         extent={"kind": "line", "values": [0.0, 0.01]})]},
        {"name": "empty_points", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(26, "LIN-6", WORLD, line([0.0, 0.0, 0.0], X, Z), points=[])]},
        {"name": "caller_extent_reversed", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(27, "PLN-5", WORLD, plane([0.0, 0.0, 0.0], Z, X),
                         extent={"kind": "rect", "values": [0.0, 0.01, 0.02, 0.02]})]},
        {"name": "element_frame_missing", "ac": "frame_not_found", "target": WORLD, "margin": 0.0,
         "elements": [el(28, "PT-5", F_MISSING, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "target_frame_missing", "ac": "frame_not_found", "target": F_MISSING, "margin": 0.0,
         "elements": [el(29, "PT-6", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "name_control_char", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(30, "PT\u0007-7", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "name_bom", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(31, "﻿PT-8", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "name_empty", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(32, "", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "empty_selection", "ac": "fail closed", "target": WORLD, "margin": 0.0, "elements": []},
        {"name": "negative_margin", "ac": "fail closed", "target": WORLD, "margin": -0.001,
         "elements": [el(33, "PT-9", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]})]},
        {"name": "first_failure_wins", "ac": "fail closed", "target": WORLD, "margin": 0.0,
         "elements": [el(34, "PT-10", WORLD, {"kind": "point", "point": [0.0, 0.0, 0.0]}),
                      el(35, "LIN-7", WORLD, line([0.0, 0.0, 0.0], X, Z)),
                      el(36, "LIN-8", WORLD, line([0.0, 0.0, 0.0], X, Z), points=[])]},
    ]
    return ac1, cases


def check_ac4(items):
    """AC-4 in exact arithmetic: the centre moves by exactly (1, 0, 0) m."""
    got = items[0]["geom"]["origin"]
    want = [Fraction(0.25) + 1, Fraction(-0.5), Fraction(0.125)]
    assert [Fraction(v) for v in got] == want, got


def check_ac1(items):
    """AC-1 exactly: [0, 0.01] x [0, 0.02] m from 4 fitted points."""
    b = items[0]["bounds"]
    assert b["extent"] == "rect" and b["source"] == "fitted_points" and b["points"] == 4
    assert b["values"] == [0.0, 0.01, 0.0, 0.02], b["values"]


def check_similarity_margin(items):
    """The margin is added in the target frame: the export is widened by exactly the reported
    margin, after the scale 1.5 (projections -3 mm and 12 mm in the element's frame)."""
    b = items[0]["bounds"]
    assert b["margin"] == 0.001, b
    assert b["values"] == [pz(1.5 * -0.003 - 0.001), pz(1.5 * 0.012 + 0.001)], b["values"]


def check_ac2(items):
    """AC-2: the caller's [-5, 15] mm survives a rigid two-level move bit for bit."""
    b = items[0]["bounds"]
    assert b["source"] == "caller" and b["values"] == [-0.005, 0.015], b


def expect(table, case):
    try:
        items = run_case(table, case)
    except Refused as r:
        err = {"code": r.code, "element": r.element}
        err.update(r.detail)
        return {"error": err}
    return {"items": items}


def document(frames, cases):
    table = frame_table(frames)
    out = []
    for case in cases:
        c = dict(case)
        c["expect"] = expect(table, case)
        if case["name"] == "ac1_plane_points":
            check_ac1(c["expect"]["items"])
        if case["name"] == "similarity_cylinder_margin":
            check_similarity_margin(c["expect"]["items"])
        if case["name"] == "ac2_cylinder_caller":
            check_ac2(c["expect"]["items"])
        if case["name"] == "ac4_circle_shift":
            check_ac4(c["expect"]["items"])
        out.append(c)
    doc = {
        "format": FORMAT,
        "generator": "make_cases.py (independent stdlib Python); synthetic data, CC-BY-4.0",
        "frames": [{"id": f[0], "name": f[1], "parent": f[2], "rotation": f[3], "translation": f[4],
                    "scale": f[5]} for f in frames],
        "cases": out,
    }
    return json.dumps(doc, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    ac1, cases = build_cases()
    outputs = {
        "plane_points.json": document([], [ac1]),
        "cases.json": document(FRAMES, cases),
    }
    bad = False
    for name, text in outputs.items():
        target = os.path.join(HERE, name)
        if check:
            try:
                with open(target, encoding="utf-8", newline="") as f:
                    old = f.read()
            except FileNotFoundError:
                old = None
            if old != text:
                print("DIFFERS: " + name)
                bad = True
        else:
            with open(target, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
    if bad:
        sys.exit(1)
    print("ok" if check else "written")


if __name__ == "__main__":
    main()
