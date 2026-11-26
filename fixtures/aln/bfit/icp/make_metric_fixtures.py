#!/usr/bin/env python3
"""Independent reference for ALN.BFIT.ICP.METRIC; writes metric_rows.json and metric_fd.json.

Stdlib only. Shares no code with the Rust crate fid-align-solve: the rows below are written
from spec/aln/bfit/icp/metric/NODE.md (REQ-001 to REQ-003) and the operation order pinned in
the crate's module documentation (`icp/metric.rs`). All data is invented (CC-BY-4.0,
project-authored); nothing comes from an external dataset (D-038).

Rows of one valid correspondence with moved point x, foot q, unit normal n and combined
weight w > 0, s = sqrt(w) (correctly rounded), d = x - q componentwise:

  point_to_point  three rows, residuals d0*s, d1*s, d2*s; Jacobian rows (columns
                  tx, ty, tz, a, b, c; the twist [-[x]x | I] reordered to translation first)
                    [1, 0, 0,   0,  x2, -x1] * s
                    [0, 1, 0, -x2,   0,  x0] * s
                    [0, 0, 1,  x1, -x0,   0] * s
  point_to_plane  one row, residual ((n0*d0 + n1*d1) + n2*d2) * s; Jacobian row
                    [n0, n1, n2, x1*n2 - x2*n1, x2*n0 - x0*n2, x0*n1 - x1*n0] * s

Every product is one IEEE multiplication and every sum one IEEE addition, left to right as
bracketed (no FMA); `* s` multiplies each entry. Python floats are binary64 with correctly
rounded + - * and math.sqrt, as in Rust; repr() round-trips, so the Rust test reads the same
bits.

Outputs:
  metric_rows.json  AC-1: five correspondences with hand-chosen x, q, n (w = 1). For each
                    metric, `exact` is the analytic row computed in exact rational arithmetic
                    from the binary64 inputs and rounded once (the 1e-15 m comparison), and
                    `pinned` is the binary64 evaluation in the pinned order above (the bit pin
                    of REQ-006, D0).
  metric_fd.json    AC-2: four random poses (splitmix64, seed 5) applied to six measured points
                    each, with feet, unit normals and weights. `fd` holds the central finite
                    differences of the weighted residuals with respect to the six pose
                    parameters (x' = Rz(c) Ry(b) Rx(a) x + t around 0, step 1e-6), evaluated in
                    exact rational arithmetic (sine and cosine of the step by their Taylor
                    series, no libm) and rounded once. The Rust test requires its analytic
                    Jacobian to match within a relative 1e-6.

Run:  python fixtures/aln/bfit/icp/make_metric_fixtures.py [--check]
With --check nothing is written; exits non-zero if an output differs.
"""

import json
import math
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ------------------------------------------------------------------ pinned binary64 rows


def pinned_rows(metric, x, q, n, w):
    s = math.sqrt(w)
    d = [x[0] - q[0], x[1] - q[1], x[2] - q[2]]
    if metric == "point_to_point":
        r = [d[0] * s, d[1] * s, d[2] * s]
        j = [
            [1.0, 0.0, 0.0, 0.0, x[2], -x[1]],
            [0.0, 1.0, 0.0, -x[2], 0.0, x[0]],
            [0.0, 0.0, 1.0, x[1], -x[0], 0.0],
        ]
        return r, [[v * s for v in row] for row in j]
    dot = (n[0] * d[0] + n[1] * d[1]) + n[2] * d[2]
    c = [x[1] * n[2] - x[2] * n[1], x[2] * n[0] - x[0] * n[2], x[0] * n[1] - x[1] * n[0]]
    row = [n[0], n[1], n[2], c[0], c[1], c[2]]
    return [dot * s], [[v * s for v in row]]


# ------------------------------------------------------------------ exact analytic rows


def exact_rows(metric, x, q, n, w):
    s = F(math.sqrt(w)) if w != 1.0 else F(1)
    X = [F(v) for v in x]
    Q = [F(v) for v in q]
    N = [F(v) for v in n]
    d = [X[i] - Q[i] for i in range(3)]
    if metric == "point_to_point":
        r = [d[i] * s for i in range(3)]
        j = [
            [F(1), F(0), F(0), F(0), X[2], -X[1]],
            [F(0), F(1), F(0), -X[2], F(0), X[0]],
            [F(0), F(0), F(1), X[1], -X[0], F(0)],
        ]
        rows = [[v * s for v in row] for row in j]
    else:
        r = [sum(N[i] * d[i] for i in range(3)) * s]
        c = [X[1] * N[2] - X[2] * N[1], X[2] * N[0] - X[0] * N[2], X[0] * N[1] - X[1] * N[0]]
        rows = [[v * s for v in N + c]]
    return [float(v) for v in r], [[float(v) for v in row] for row in rows]


# ------------------------------------------------------------------ AC-1 cases (hand-chosen)

AC1 = [
    # x, q, n
    ([0.1, 0.2, 0.3], [0.1, 0.2, 0.25], [0.0, 0.0, 1.0]),
    ([-0.05, 0.4, 0.125], [-0.06, 0.41, 0.12], [3 / 13, 4 / 13, 12 / 13]),
    ([1.5, -2.25, 0.75], [1.49, -2.26, 0.74], [2 / 3, 1 / 3, 2 / 3]),
    ([0.0, 0.0, 0.0], [0.003, -0.004, 0.0], [0.6, 0.0, 0.8]),
    ([-0.7, 0.33, -1.1], [-0.69, 0.33, -1.1], [-1.0, 0.0, 0.0]),
]

METRICS = ["point_to_point", "point_to_plane"]


def build_rows_fixture():
    cases = []
    for x, q, n in AC1:
        entry = {"x": x, "q": q, "n": n, "w": 1.0}
        for m in METRICS:
            er, ej = exact_rows(m, x, q, n, 1.0)
            pr, pj = pinned_rows(m, x, q, n, 1.0)
            for a, b in zip(er + sum(ej, []), pr + sum(pj, [])):
                assert abs(a - b) <= 1e-15, (m, x, a, b)
            entry[m] = {"exact": {"r": er, "j": ej}, "pinned": {"r": pr, "j": pj}}
        cases.append(entry)
    return {
        "about": "ALN.BFIT.ICP.METRIC AC-1 rows. Generated by make_metric_fixtures.py (independent "
        "reference); hand-chosen invented inputs, weight 1, no locks. CC-BY-4.0.",
        "columns": ["tx", "ty", "tz", "a", "b", "c"],
        "cases": cases,
    }


# ------------------------------------------------------------------ AC-2 (seed 5)

MASK = (1 << 64) - 1


class SplitMix64:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def int_in(self, lo, hi):
        return lo + self.next() % (hi - lo + 1)

    def dyadic(self, half_range, bits=20):
        # an exact binary64 value k / 2^bits in [-half_range, half_range]
        k = self.int_in(-int(half_range * (1 << bits)), int(half_range * (1 << bits)))
        return F(k, 1 << bits)


def quat_rot(a, b, c, d):
    # exactly orthogonal rational rotation of the (unnormalised) quaternion (a, b, c, d)
    n = F(a * a + b * b + c * c + d * d)
    return [
        [F(a * a + b * b - c * c - d * d) / n, F(2 * (b * c - a * d)) / n, F(2 * (b * d + a * c)) / n],
        [F(2 * (b * c + a * d)) / n, F(a * a - b * b + c * c - d * d) / n, F(2 * (c * d - a * b)) / n],
        [F(2 * (b * d - a * c)) / n, F(2 * (c * d + a * b)) / n, F(a * a - b * b - c * c + d * d) / n],
    ]


def stereo_normal(u, v):
    # exactly unit rational vector (inverse stereographic projection)
    den = 1 + u * u + v * v
    return [2 * u / den, 2 * v / den, (1 - u * u - v * v) / den]


H = F(1, 10**6)


def sin_cos(h):
    s = F(0)
    c = F(0)
    term = F(1)
    for k in range(0, 14):
        if k > 0:
            term = term * h / k
        if k % 4 == 0:
            c += term
        elif k % 4 == 1:
            s += term
        elif k % 4 == 2:
            c -= term
        else:
            s -= term
    return s, c


def moved(x, k, h):
    # x' = Rz(c) Ry(b) Rx(a) x + t with only parameter k equal to h
    x = list(x)
    if k < 3:
        x[k] += h
        return x
    s, c = sin_cos(h)
    if k == 3:  # Rx
        return [x[0], c * x[1] - s * x[2], s * x[1] + c * x[2]]
    if k == 4:  # Ry
        return [c * x[0] + s * x[2], x[1], -s * x[0] + c * x[2]]
    return [c * x[0] - s * x[1], s * x[0] + c * x[1], x[2]]  # Rz


def residuals(metric, x, q, n, s):
    d = [x[i] - q[i] for i in range(3)]
    if metric == "point_to_point":
        return [d[i] * s for i in range(3)]
    return [sum(n[i] * d[i] for i in range(3)) * s]


def fd_jacobian(metric, x, q, n, w):
    s = F(math.sqrt(w))
    X = [F(v) for v in x]
    Q = [F(v) for v in q]
    N = [F(v) for v in n]
    nrows = 3 if metric == "point_to_point" else 1
    cols = []
    for k in range(6):
        rp = residuals(metric, moved(X, k, H), Q, N, s)
        rm = residuals(metric, moved(X, k, -H), Q, N, s)
        cols.append([(rp[i] - rm[i]) / (2 * H) for i in range(nrows)])
    return [[float(cols[k][i]) for k in range(6)] for i in range(nrows)]


WEIGHTS = [0.5, 1.0, 3.0, 4.0]


def build_fd_fixture():
    rng = SplitMix64(5)
    poses = []
    for _ in range(4):
        qa = [rng.int_in(-1000, 1000) for _ in range(4)]
        if all(v == 0 for v in qa):
            qa[0] = 1
        rot = quat_rot(*qa)
        t = [rng.dyadic(1.0) for _ in range(3)]
        pts = []
        for _ in range(6):
            p = [rng.dyadic(0.5) for _ in range(3)]
            x = [float(sum(rot[i][j] * p[j] for j in range(3)) + t[i]) for i in range(3)]
            q = [float(F(x[i]) + rng.dyadic(0.002, bits=30)) for i in range(3)]
            u = F(rng.int_in(-900, 900), 1000)
            v = F(rng.int_in(-900, 900), 1000)
            n = [float(c) for c in stereo_normal(u, v)]
            w = WEIGHTS[rng.int_in(0, len(WEIGHTS) - 1)]
            entry = {"x": x, "q": q, "n": n, "w": w}
            for m in METRICS:
                fd = fd_jacobian(m, x, q, n, w)
                _, aj = pinned_rows(m, x, q, n, w)
                for fr, ar in zip(fd, aj):
                    scale = max(abs(v) for v in fr)
                    for a, b in zip(ar, fr):
                        assert abs(a - b) <= 1e-9 * scale, (m, a, b)
                entry[m] = fd
            pts.append(entry)
        poses.append({"quaternion": qa, "t": [float(v) for v in t], "points": pts})
    return {
        "about": "ALN.BFIT.ICP.METRIC AC-2 central finite differences (step 1e-6, exact rational "
        "arithmetic) of the weighted residuals at random poses, splitmix64 seed 5. Generated by "
        "make_metric_fixtures.py (independent reference); invented data. CC-BY-4.0.",
        "seed": 5,
        "step": 1e-6,
        "columns": ["tx", "ty", "tz", "a", "b", "c"],
        "poses": poses,
    }


def dump(obj):
    return json.dumps(obj, indent=1, ensure_ascii=True, allow_nan=False) + "\n"


def main():
    check_only = "--check" in sys.argv[1:]
    outputs = {"metric_rows.json": dump(build_rows_fixture()), "metric_fd.json": dump(build_fd_fixture())}
    bad = 0
    for name, text in outputs.items():
        path = HERE / name
        if check_only:
            have = path.read_bytes().decode("utf-8") if path.exists() else None
            if have != text:
                print(f"{name}: differs from the reference output", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(text.encode("utf-8"))
    if bad:
        sys.exit(1)
    print("make_metric_fixtures: ok" if check_only else "make_metric_fixtures: written")


if __name__ == "__main__":
    main()
