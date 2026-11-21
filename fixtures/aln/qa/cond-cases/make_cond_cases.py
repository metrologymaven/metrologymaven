#!/usr/bin/env python3
"""Independent reference for ALN.QA.COND (conditioning and weak-DoF diagnostics).

Generates the input cases of `fixtures/aln/qa/cond-cases` from seeded generators and runs a
Python model of every rule pinned in `fid_align_solve::cond` (spec node ALN.QA.COND, Design),
writing the expected values to `expected.txt`. The QR and SVD steps reuse the reviewed bit-exact
models of `KRN.NUM.LA.QR` and `KRN.NUM.LA.SVD` in `fixtures/num/la/`; nothing here runs the Rust
code. `--check` regenerates and compares with the committed files.

It also writes `get-diagnostics-a2.json`, the exact bytes of `align.get_diagnostics@1` for
alignment A2 of `fixtures/aln/model/chain3` (ALN.QA.COND AC-4) when the diagnostics of the
`wide` case are stored with A2's residual report (the hand case `mixed_kinds_weights` of
`fixtures/aln/qa/make_report_fixtures.py`, whose canonical JSON writer is reused) and the
method is closed-form (`history` is null).

Inputs (the Rust test rebuilds them from the same rules and checks the `inputs` digests):

- `uni()` = `(next() >> 11) * 2^-53` and `sym()` = `2 * uni() - 1` on a splitmix64 stream.
- Seed 14, one stream for the five geometries in this order, 10,000 points each:
  `full_cylinder`, `half_cylinder`, `long_plane`, `sphere`, `freeform`. Seed 15: `freeform_small`
  (50 points), used twice with other parameter sets. Seed 16: `offset_cylinder` then
  `far_sphere`, 4,000 points each (few enough for the direct-SVD cross-check below).
- `unit2()`: draw `u = sym(), v = sym()`, `s2 = u*u + v*v`, reject `s2 > 1` or `s2 < 2^-10`,
  return `(u / s, v / s)` with `s = sqrt(s2)`. `unit3()` likewise with `s2 = (u*u + v*v) + w*w`.
- Cylinder (radius 2^-4 m, axis z through the origin): `(nx, ny) = unit2()`, `z = 0.1 * sym()`,
  point `(R nx, R ny, z)`, normal `(nx, ny, 0)`; the half cylinder uses `ny = |ny|`. Both lock
  `tz` (the axial slide of a bare cylinder is exactly unobservable too; see the PR).
- Long thin plane (1000 x 1 mm in z = 0): `x = 0.5 * sym()`, `y = 0.0005 * sym()`,
  `a = E1 * sym()`, `b = E2 * sym()`, normal `(a, b, 1) / sqrt((a*a + b*b) + 1)` (a form error
  that leaves the in-plane slide only weakly observable).
- Sphere (radius 2^-3 m, centre at the origin): `n = unit3()`, point `R n`, normal `n`.
- Offset cylinder: the cylinder with its axis through `(0.05, 0, 0)` (50 mm off the origin):
  point `(0.05 + R nx, R ny, z)`, `tz` locked.
- Far sphere: the sphere with its centre at `(2, 0, 0)` (2 m from the origin): point
  `(2 + R n0, R n1, R n2)`.
- Freeform patch: `x = 0.1 * sym()`, `y = 0.1 * sym()`,
  `z = ((2xx - 3yy) + 5xy) + 8xxx`, `fx = (4x + 5y) + 24xx`, `fy = 5x - 6y`,
  normal `(-fx, -fy, 1) / sqrt((fx*fx + fy*fy) + 1)`.
- Point-to-plane Jacobian row (columns `tx ty tz rx ry rz`):
  `[nx, ny, nz, y*nz - z*ny, z*nx - x*nz, x*ny - y*nx]`, then the case's columns are kept.
- `inputs` digest: SHA-256 of every point (3 little-endian binary64) then every kept Jacobian
  row (little-endian binary64), in order.

Cross-check: for every case with at most 4,096 (padded) rows, the singular values of the scaled
rows are also computed by the SVD model directly, without the QR reduction, and must agree with
the QR route within `64 n u sigma_max`.
"""

import hashlib
import math
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE.parents[2] / "num" / "la"))
import make_la_fixtures as la  # noqa: E402  (reviewed RED model)
import make_qr_fixtures as qrf  # noqa: E402  (reviewed KRN.NUM.LA.QR model)
import make_svd_fixtures as svdf  # noqa: E402  (reviewed KRN.NUM.LA.SVD model)

sys.path.insert(0, str(HERE.parent))
import make_report_fixtures as rpt  # noqa: E402  (RFC 8785 writer, chain3 IDs, report digest)

OUT = HERE / "expected.txt"
OUT_READ = HERE / "get-diagnostics-a2.json"

MAX_DIM = 4096
FLOOR = 1e-15
GUARD = 8.0
U = 2.0 ** -53
AXIS = 0.9
DEFAULT_THRESHOLD = 1e8
DOFS = ["tx", "ty", "tz", "rx", "ry", "rz"]
N_POINTS = 10_000
E1 = 1e-10
E2 = 1e-9


def hx(x):
    return "0x%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


class CondFail(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


# ---------------------------------------------------------------- model: ALN.QA.COND


def diagnose(jac, params, points, threshold):
    """jac: list of rows (one value per param). Returns a dict of the diagnostics."""
    n = len(params)
    if not 1 <= n <= 6 or any(p not in DOFS for p in params):
        raise CondFail("bad_params")
    idx = [DOFS.index(p) for p in params]
    if any(idx[i] >= idx[i + 1] for i in range(n - 1)):
        raise CondFail("bad_params")
    flat = [x for r in jac for x in r]
    if not flat:
        raise CondFail("empty_jacobian")
    if any(len(r) != n for r in jac):
        raise CondFail("bad_shape")
    m = len(jac)
    for x in flat:
        if math.isnan(x):
            raise CondFail("nan_input")
        if math.isinf(x):
            raise CondFail("infinite_input")
    if not points:
        raise CondFail("empty_points")
    for p in points:
        for x in p:
            if math.isnan(x):
                raise CondFail("nan_input")
            if math.isinf(x):
                raise CondFail("infinite_input")
    if not la.finite(threshold) or threshold < 1:
        raise CondFail("bad_threshold")

    try:
        cent = [la.fixed_sum([p[k] for p in points]) / len(points) for k in range(3)]
    except la.Overflow:
        raise CondFail("non_finite")
    dev = [p[k] - cent[k] for p in points for k in range(3)]
    if not all(la.finite(x) for x in dev):
        raise CondFail("non_finite")
    try:
        big_l = la.norm2(dev) / math.sqrt(float(len(points)))
    except la.Overflow:
        raise CondFail("non_finite")
    if not la.finite(big_l):
        raise CondFail("non_finite")
    has_rot = any(d >= 3 for d in idx)
    if has_rot and big_l == 0:
        raise CondFail("degenerate_scale")
    # Each rotation pivots on the centroid when the two translations perpendicular to its axis
    # are free (rx: ty, tz; ry: tx, tz; rz: tx, ty).
    perp = {3: (1, 2), 4: (0, 2), 5: (0, 1)}
    moved = {d: all(t in idx for t in perp[d]) for d in (3, 4, 5)}
    pivots = []
    for d in (3, 4, 5):
        pivots.append(("centroid" if moved[d] else "frame_origin") if d in idx else "-")
    q = la.norm2(cent) / big_l if has_rot else 0.0
    guard = ((GUARD * U) * float(n)) * (1.0 + q)
    if not la.finite(guard):
        raise CondFail("non_finite")
    ratio = FLOOR if guard < FLOOR else guard
    col = {d: j for j, d in enumerate(idx)}
    cx, cy, cz = cent

    def scaled(row):
        out = []
        for j, d in enumerate(idx):
            x = row[j]
            if d >= 3:
                if moved[d]:
                    if d == 3:
                        x = (x + cz * row[col[1]]) - cy * row[col[2]]
                    elif d == 4:
                        x = (x - cz * row[col[0]]) + cx * row[col[2]]
                    else:
                        x = (x + cy * row[col[0]]) - cx * row[col[1]]
                x = x / big_l
                if not la.finite(x):
                    raise CondFail("non_finite")
            out.append(x)
        return out

    rows = [scaled(r) for r in jac] + [[0.0] * n for _ in range(max(0, n - m))]
    total = len(rows)
    try:
        first = min(total, MAX_DIM)
        r = qrf.qr(first, n, [x for row in rows[:first] for x in row], 0.0, False).r()
        pos = first
        while pos < total:
            take = min(total - pos, MAX_DIM - n)
            block = r + [x for row in rows[pos:pos + take] for x in row]
            r = qrf.qr(n + take, n, block, 0.0, False).r()
            pos += take
        s = svdf.svd(n, n, r, 0.0)
    except qrf.Fail:
        raise CondFail("numeric")
    sigma = s.sigma
    if total <= MAX_DIM:
        direct = svdf.svd(total, n, [x for row in rows for x in row], 0.0).sigma
        tol = 64 * n * U * sigma[0]
        for a, b in zip(direct, sigma):
            assert abs(a - b) <= tol, ("direct SVD disagrees", a, b, tol)
    v = [s.v[j * n + (n - 1)] for j in range(n)]
    smax = sigma[0]
    cond = s.cond
    if smax == 0:
        nu = n
    else:
        lim = ratio * smax
        nu = sum(1 for x in sigma if x < lim)
    best = 0
    for j in range(1, n):
        if abs(v[j]) > abs(v[best]):
            best = j
    weak_pivot = "-"
    if abs(v[best]) > AXIS:
        kind = "translation" if idx[best] < 3 else "rotation"
        axis = "xyz"[idx[best] % 3]
        if idx[best] >= 3:
            weak_pivot = pivots[idx[best] - 3]
    else:
        kind, axis = "twist", "-"
    angular, linear = [0.0] * 3, [0.0] * 3
    for j, d in enumerate(idx):
        if d < 3:
            linear[d] = v[j]
        else:
            angular[d - 3] = v[j] / big_l
    if nu > 0:
        warning = "unobservable_dof"
    elif cond == math.inf or cond > threshold:
        warning = "ill_conditioned"
    else:
        warning = "none"
    return {
        "length_scale": [big_l],
        "centroid": cent,
        "pivots": pivots,
        "ratio": ratio,
        "weak_pivot": weak_pivot,
        "sigma": sigma,
        "cond": cond,
        "n_unobservable": nu,
        "weakest": [kind, axis],
        "vector": v,
        "angular": angular,
        "linear": linear,
        "warning": warning,
    }


# ---------------------------------------------------------------- inputs


class Stream:
    def __init__(self, seed):
        self.g = la.Gen(seed)

    def uni(self):
        return (self.g.next() >> 11) * 2.0 ** -53

    def sym(self):
        return 2.0 * self.uni() - 1.0

    def unit2(self):
        while True:
            u, v = self.sym(), self.sym()
            s2 = u * u + v * v
            if s2 > 1.0 or s2 < 2.0 ** -10:
                continue
            s = math.sqrt(s2)
            return u / s, v / s

    def unit3(self):
        while True:
            u, v, w = self.sym(), self.sym(), self.sym()
            s2 = (u * u + v * v) + w * w
            if s2 > 1.0 or s2 < 2.0 ** -10:
                continue
            s = math.sqrt(s2)
            return u / s, v / s, w / s


def cylinder(st, half):
    r = 0.0625
    nx, ny = st.unit2()
    if half:
        ny = abs(ny)
    z = 0.1 * st.sym()
    return (r * nx, r * ny, z), (nx, ny, 0.0)


def long_plane(st):
    x = 0.5 * st.sym()
    y = 0.0005 * st.sym()
    a = E1 * st.sym()
    b = E2 * st.sym()
    s = math.sqrt((a * a + b * b) + 1.0)
    return (x, y, 0.0), (a / s, b / s, 1.0 / s)


def offset_cylinder(st):
    (x, y, z), nrm = cylinder(st, False)
    return (0.05 + x, y, z), nrm


def far_sphere(st):
    (x, y, z), nrm = sphere(st)
    return (2.0 + x, y, z), nrm


def sphere(st):
    r = 0.125
    n = st.unit3()
    return (r * n[0], r * n[1], r * n[2]), n


def freeform(st):
    x = 0.1 * st.sym()
    y = 0.1 * st.sym()
    z = ((2.0 * x * x - 3.0 * y * y) + 5.0 * x * y) + 8.0 * x * x * x
    fx = (4.0 * x + 5.0 * y) + 24.0 * x * x
    fy = 5.0 * x - 6.0 * y
    s = math.sqrt((fx * fx + fy * fy) + 1.0)
    return (x, y, z), (-fx / s, -fy / s, 1.0 / s)


def full_row(p, nrm):
    x, y, z = p
    nx, ny, nz = nrm
    return [nx, ny, nz, y * nz - z * ny, z * nx - x * nz, x * ny - y * nx]


def build(gen, count):
    pts, rows = [], []
    for _ in range(count):
        p, nrm = gen()
        pts.append(p)
        rows.append(full_row(p, nrm))
    return pts, rows


def keep(rows, params):
    idx = [DOFS.index(p) for p in params]
    return [[r[i] for i in idx] for r in rows]


def digest(points, jac):
    h = hashlib.sha256()
    for p in points:
        h.update(struct.pack("<3d", *p))
    for r in jac:
        h.update(struct.pack("<%dd" % len(r), *r))
    return h.hexdigest()


ALL6 = DOFS
CYL = ["tx", "ty", "rx", "ry", "rz"]


def cases():
    """(name, params, threshold, points, jacobian)."""
    st = Stream(14)
    geo = {}
    geo["full_cylinder"] = build(lambda: cylinder(st, False), N_POINTS)
    geo["half_cylinder"] = build(lambda: cylinder(st, True), N_POINTS)
    geo["long_plane"] = build(lambda: long_plane(st), N_POINTS)
    geo["sphere"] = build(lambda: sphere(st), N_POINTS)
    geo["freeform"] = build(lambda: freeform(st), N_POINTS)
    small = Stream(15)
    geo["freeform_small"] = build(lambda: freeform(small), 50)
    far = Stream(16)
    geo["offset_cylinder"] = build(lambda: offset_cylinder(far), 4000)
    geo["far_sphere"] = build(lambda: far_sphere(far), 4000)
    out = []
    for name, src, params, thr in [
        ("full_cylinder", "full_cylinder", CYL, DEFAULT_THRESHOLD),
        ("half_cylinder", "half_cylinder", CYL, DEFAULT_THRESHOLD),
        ("long_plane", "long_plane", ALL6, DEFAULT_THRESHOLD),
        ("long_plane_1e12", "long_plane", ALL6, 1e12),
        ("sphere", "sphere", ALL6, DEFAULT_THRESHOLD),
        ("freeform", "freeform", ALL6, DEFAULT_THRESHOLD),
        ("small_translations", "freeform_small", ["tx", "ty", "tz"], DEFAULT_THRESHOLD),
        ("small_tilt", "freeform_small", ["tz", "rx", "ry"], DEFAULT_THRESHOLD),
        ("offset_cylinder", "offset_cylinder", CYL, DEFAULT_THRESHOLD),
        ("far_sphere", "far_sphere", ALL6, DEFAULT_THRESHOLD),
    ]:
        pts, rows = geo[src]
        out.append((name, params, thr, pts, keep(rows, params)))
    # Fewer rows than parameters: two rows, six parameters (padded with zero rows).
    out.append(
        (
            "wide",
            ALL6,
            DEFAULT_THRESHOLD,
            [(1.0, 0.0, 0.0), (-1.0, 0.0, 0.0)],
            [[1.0, 0.0, 0.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0, 0.0, 0.5]],
        )
    )
    return out


# ---------------------------------------------------------------- output


def render():
    lines = [
        "# ALN.QA.COND cases: generated by make_cond_cases.py (seeds 14 and 15). Do not edit by hand.",
        "# <case> <key> <values>; floats as 0x + 16 hex digits of their binary64 bits.",
    ]
    for name, params, thr, pts, jac in cases():
        d = diagnose(jac, params, pts, thr)
        put = lambda k, *vals: lines.append(" ".join([name, k] + [str(v) for v in vals]))  # noqa: E731
        put("inputs", len(pts), len(jac), digest(pts, jac))
        put("params", ",".join(params))
        put("threshold", hx(thr))
        put("length_scale", hx(d["length_scale"][0]))
        put("centroid", *[hx(x) for x in d["centroid"]])
        put("pivots", *d["pivots"])
        put("ratio", hx(d["ratio"]))
        put("sigma", *[hx(x) for x in d["sigma"]])
        put("cond", "inf" if d["cond"] == math.inf else hx(d["cond"]))
        put("n_unobservable", d["n_unobservable"])
        put("weakest", *d["weakest"], d["weak_pivot"])
        put("vector", *[hx(x) for x in d["vector"]])
        put("angular", *[hx(x) for x in d["angular"]])
        put("linear", *[hx(x) for x in d["linear"]])
        put("warning", d["warning"])
        sys.stderr.write(
            "%-20s cond=%-12s nu=%d weakest=%s warning=%s\n"
            % (name, "inf" if d["cond"] == math.inf else "%.3e" % d["cond"], d["n_unobservable"],
               "/".join(d["weakest"]), d["warning"])
        )
    return "\n".join(lines) + "\n"


def name_of(kind, axis, pivot):
    if kind == "translation":
        return "translation along " + axis
    if kind == "rotation":
        through = "the centroid" if pivot == "centroid" else "the frame origin"
        return "rotation about %s through %s" % (axis, through)
    return "generic twist"


def read_payload():
    """align.get_diagnostics@1 for chain3's A2 with the `wide` diagnostics (AC-4)."""
    a2, report_id = rpt.chain3_ids()
    entries = next(c[3] for c in rpt.hand_cases() if c[1] == "mixed_kinds_weights")
    rep_ = rpt.Report(entries)
    report_digest = rep_.digest(rep_.stats(exact=True))
    name, params, thr, pts, jac = next(c for c in cases() if c[0] == "wide")
    d = diagnose(jac, params, pts, thr)
    kind, axis = d["weakest"]
    wname = name_of(kind, axis, d["weak_pivot"])
    threshold = {"basis": "default_unverified", "value": hx(thr)}

    def m(xs):
        return {"unit": "m", "value": list(xs)}

    conditioning = {
        "centroid": m(d["centroid"]),
        "condition_number": None if d["cond"] == math.inf else hx(d["cond"]),
        "length_scale": {"unit": "m", "value": d["length_scale"][0]},
        "n_points": len(pts),
        "n_rows": len(jac),
        "n_unobservable": d["n_unobservable"],
        "params": params,
        "pivots": {k: (None if v == "-" else v) for k, v in zip(["rx", "ry", "rz"], d["pivots"])},
        "singular_values": [hx(x) for x in d["sigma"]],
        "threshold": threshold,
        "unobservable_ratio": {"basis": "default_unverified", "value": hx(d["ratio"])},
        "weakest_dof": {
            "angular": {"unit": "rad", "value": d["angular"]},
            "axis": None if axis == "-" else axis,
            "kind": kind,
            "linear": m(d["linear"]),
            "name": wname,
            "vector": [hx(x) for x in d["vector"]],
        },
    }
    if d["warning"] == "unobservable_dof":
        warnings = [{"code": "unobservable_dof", "count": d["n_unobservable"], "weakest_dof": wname}]
    elif d["warning"] == "ill_conditioned":
        cond = None if d["cond"] == math.inf else hx(d["cond"])
        warnings = [
            {"code": "ill_conditioned", "condition_number": cond, "threshold": threshold, "weakest_dof": wname}
        ]
    else:
        warnings = []
    return rpt.canon(
        {
            "alignment": a2,
            "conditioning": conditioning,
            "history": None,
            "report": {"digest": report_digest, "id": report_id},
            "warnings": warnings,
        }
    )


def self_test():
    """Error paths of the model (the Rust tests hand-label the same refusals)."""
    pts = [(0.0, 0.0, 0.0), (1.0, 0.0, 0.0)]
    for args, code in [
        (([[1.0]], ["tx", "tx"], pts, 1e8), "bad_params"),
        (([[1.0, 0.0]], ["ty", "tx"], pts, 1e8), "bad_params"),
        (([], ["tx"], pts, 1e8), "empty_jacobian"),
        (([[math.nan]], ["tx"], pts, 1e8), "nan_input"),
        (([[math.inf]], ["tx"], pts, 1e8), "infinite_input"),
        (([[1.0]], ["tx"], [], 1e8), "empty_points"),
        (([[1.0]], ["tx"], pts, 0.5), "bad_threshold"),
        (([[1.0]], ["rx"], [(1.0, 2.0, 3.0)], 1e8), "degenerate_scale"),
    ]:
        try:
            diagnose(*args)
        except CondFail as e:
            assert e.code == code, (e.code, code)
        else:
            raise AssertionError(code)


def main():
    self_test()
    outputs = {OUT: render(), OUT_READ: read_payload()}
    if "--check" in sys.argv[1:]:
        for path, text in outputs.items():
            if path.read_text(encoding="utf-8") != text:
                sys.exit("%s is stale: run make_cond_cases.py" % path.name)
        print("cond-cases: OK")
        return
    for path, text in outputs.items():
        path.write_text(text, encoding="utf-8", newline="\n")
        print("wrote", path)

if __name__ == "__main__":
    main()
