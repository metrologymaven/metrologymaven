#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.CERT.KKT (CC-BY-4.0 project data).

Writes fixtures/solv/cert/kkt-v1/cases.txt. Every expected value is decided here with exact
rational arithmetic (fractions.Fraction). The only irrational quantities are the point
distances of the circle problems; they are replaced by rationals within 10^-70 of the true
square root (integer square roots), which is many orders of magnitude below every margin the
test relies on. Nothing here runs or reads the Rust code, any solver or the platform libm: the
only float operations are float(Fraction) (correctly rounded) and float.hex parsing of the
input points.

Input data: the points and optimal active sets of fixtures/solv/cert/recomp-v1/cases.txt
(straightness, flatness and minimum-zone circle). Their optimality is NOT taken from that
file: this script re-derives the exact point of every set and proves (or refutes) its
optimality by the exact KKT conditions, which for these convex problems are sufficient.

Conventions (the same as the Rust test problems):

- straightness (dim 1, points (x, y), p = (a, c, t)) and flatness (dim 2, points (x, y, z),
  p = (a, b, c, t)): dev_i(p) = z_i - sum_k p_k coord_ik - c. Row (i, 1) is the upper
  bound g = dev - t/2 <= 0, row (i, 0) the lower bound g = -dev - t/2 <= 0. f(p) = t.
- circle (points (x, y), p = (cx, cy, r_in, r_out)): d_i = |pt_i - c|. Row (i, 0) is the
  inner bound g = r_in - d_i <= 0, row (i, 1) the outer bound g = d_i - r_out <= 0.
  f(p) = r_out - r_in.

KKT at the exact point p* of a set S (|S| = n params): multipliers mu solve
G(p*)^T mu = -grad f (G rows = gradients of the rows of S, in sorted label order), i.e. the
Lagrangian grad f + sum mu_i grad g_i = 0 with mu >= 0 for an optimum. Quantities:

- primal = max over all rows of g_i(p*);
- dual = max(0, -min mu_i);
- stationarity = 0 at mu (exact), so every case expects it satisfied;
- complementarity = max over the rows of S of |g_i(p*)| (0 by construction).

Thresholds: data_scale D = max |point coordinate| (the caller's data scale); thr_p = tol_p D,
thr_c = tol_c D, thr_d = tol_d max |mu_i|. A condition is expected "S" (satisfied) when the
exact quantity is at most thr / 2 and "V" (violated) when it is at least 2 thr; a candidate set
with any quantity in between is not emitted, so the expectation never rests on a borderline.

Cases:

- the 30 optimal sets of recomp-v1 (expect certified, multipliers within 1e-12 of mu);
- non-optimal sets (AC-1): for each linear problem the first set in lexicographic order (rows
  of distinct points, exact infinity-norm condition at most 1e3) that is infeasible, and the
  first that is feasible but has a negative multiplier; for the first three circle problems
  the same two kinds among the sets defined by 3 + 1 or 2 + 2 equidistant points;
- ac2-negative-multiplier: a straightness vertex whose exact multiplier is 1 - 1.2 (as
  doubles), about -0.2;
- ac3-inactive-violation: strt-00 with one extra point 1e-6 above the upper zone line,
  tol_p = 1e-12.
"""

import struct
import sys
from fractions import Fraction
from itertools import combinations
from math import isqrt
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "cases.txt"
SRC = HERE.parent / "recomp-v1" / "cases.txt"

TOL = Fraction(1, 10**9)  # default relative tolerance of every condition
SQRT_DIGITS = 70
COND_MAX = Fraction(1000)
CIRCLE_SEARCH = 3


def hexf(x):
    """Hex bits of the nearest double (float() of a Fraction is correctly rounded)."""
    return struct.pack(">d", float(x)).hex()


def unhex(h):
    return Fraction(struct.unpack(">d", bytes.fromhex(h))[0])


def dec(q, digits=30):
    """A decimal string of the Fraction q, rounded to `digits` places after the point."""
    s = "-" if q < 0 else ""
    q = abs(q)
    scaled = (q.numerator * 10**digits * 2 + q.denominator) // (2 * q.denominator)
    ip, fp = divmod(scaled, 10**digits)
    return f"{s}{ip}.{fp:0{digits}d}"


def solve(a, b):
    """Exact Gaussian elimination; None if singular."""
    n = len(a)
    m = [list(r) + [bb] for r, bb in zip(a, b)]
    for c in range(n):
        piv = next((r for r in range(c, n) if m[r][c] != 0), None)
        if piv is None:
            return None
        m[c], m[piv] = m[piv], m[c]
        for r in range(n):
            if r != c and m[r][c] != 0:
                f = m[r][c] / m[c][c]
                m[r] = [x - f * y for x, y in zip(m[r], m[c])]
    return [m[i][n] / m[i][i] for i in range(n)]


def cond_inf(a):
    n = len(a)
    cols = []
    for j in range(n):
        c = solve(a, [Fraction(int(i == j)) for i in range(n)])
        if c is None:
            return None
        cols.append(c)
    inv_norm = max(sum(abs(cols[j][i]) for j in range(n)) for i in range(n))
    return max(sum(abs(x) for x in r) for r in a) * inv_norm


def transpose(a):
    return [list(r) for r in zip(*a)]


def sqrt_q(q):
    """A rational within 10^-SQRT_DIGITS of sqrt(q) (q >= 0), monotone in q and equal for
    equal q, so rows at the same exact distance get the same approximation."""
    s = 10**SQRT_DIGITS
    return Fraction(isqrt(q.numerator * q.denominator * s * s), q.denominator * s)


# ---------------------------------------------------------------- problems


class Lin:
    def __init__(self, dim, pts):
        self.dim, self.pts = dim, pts
        self.n = dim + 2

    def rows(self):
        return [(i, s) for i in range(len(self.pts)) for s in (0, 1)]

    def grad_row(self, row, _p=None):
        """Gradient of g for row (exact, constant)."""
        i, s = row
        sgn = 1 if s == 1 else -1
        q = self.pts[i]
        return [-sgn * c for c in q[: self.dim]] + [Fraction(-sgn), Fraction(-1, 2)]

    def g(self, row, p):
        i, s = row
        sgn = 1 if s == 1 else -1
        q = self.pts[i]
        dev = q[self.dim] - sum(pk * ck for pk, ck in zip(p, q[: self.dim])) - p[self.dim]
        return sgn * dev - p[self.dim + 1] / 2

    def grad_f(self):
        return [Fraction(0)] * (self.dim + 1) + [Fraction(1)]

    def point_of(self, rows):
        """The exact point where the rows are tight, and the exact condition of the system."""
        a = [self.grad_row(r) for r in rows]
        k = cond_inf(a)
        if k is None:
            return None, None
        # g(p) = G p + sgn z = 0
        b = []
        for i, s in rows:
            sgn = 1 if s == 1 else -1
            b.append(-sgn * self.pts[i][self.dim])
        return solve(a, b), k


class Circle:
    n = 4

    def __init__(self, pts):
        self.pts = pts

    def rows(self):
        return [(i, s) for i in range(len(self.pts)) for s in (0, 1)]

    def d2(self, i, c):
        x, y = self.pts[i]
        return (x - c[0]) ** 2 + (y - c[1]) ** 2

    def d(self, i, c):
        return sqrt_q(self.d2(i, c))

    def g(self, row, p):
        i, s = row
        di = self.d(i, p[:2])
        return p[2] - di if s == 0 else di - p[3]

    def grad_row(self, row, p):
        i, s = row
        x, y = self.pts[i]
        di = self.d(i, p[:2])
        ux, uy = (x - p[0]) / di, (y - p[1]) / di
        return [ux, uy, Fraction(1), Fraction(0)] if s == 0 else [-ux, -uy, Fraction(0), Fraction(-1)]

    def grad_f(self):
        return [Fraction(0), Fraction(0), Fraction(-1), Fraction(1)]

    def point_of(self, rows):
        """Centre equidistant from the set's outer points and from its inner points; radii the
        distances to them. None unless exactly two bisector equations determine the centre."""
        outer = [i for i, s in rows if s == 1]
        inner = [i for i, s in rows if s == 0]
        if not outer or not inner or len({i for i, _ in rows}) != len(rows):
            return None, None
        a, b = [], []
        for grp in (outer, inner):
            x0, y0 = self.pts[grp[0]]
            for k in grp[1:]:
                x1, y1 = self.pts[k]
                a.append([2 * (x1 - x0), 2 * (y1 - y0)])
                b.append(x1 * x1 + y1 * y1 - x0 * x0 - y0 * y0)
        if len(a) != 2:
            return None, None
        c = solve(a, b)
        if c is None:
            return None, None
        p = [c[0], c[1], self.d(inner[0], c), self.d(outer[0], c)]
        # Condition of the residual Jacobian of the active-set equations d_i - r_side = 0.
        jac = []
        for i, s in rows:
            x, y = self.pts[i]
            di = self.d(i, c)
            row = [-(x - c[0]) / di, -(y - c[1]) / di, Fraction(0), Fraction(0)]
            row[2 + s] = Fraction(-1)
            jac.append(row)
        return p, cond_inf(jac)


# ---------------------------------------------------------------- KKT oracle


def kkt(prob, rows, p, data_scale, tol):
    """Exact KKT quantities and expected verdicts, or None if any quantity is borderline."""
    tol_p, tol_d, _tol_s, tol_c = tol
    rows = sorted(rows)
    gmat = [prob.grad_row(r, p) for r in rows]
    gf = prob.grad_f()
    mu = solve(transpose(gmat), [-v for v in gf])
    if mu is None:
        return None
    primal = max(prob.g(r, p) for r in prob.rows())
    comp = max(abs(prob.g(r, p)) for r in rows)
    dual = max(Fraction(0), -min(mu))
    mu_scale = max(abs(m) for m in mu) or Fraction(1)
    verdicts = []
    for q, thr in ((primal, tol_p * data_scale), (dual, tol_d * mu_scale), (Fraction(0), Fraction(1)),
                   (comp, tol_c * data_scale)):
        if q <= thr / 2:
            verdicts.append("S")
        elif q >= 2 * thr:
            verdicts.append("V")
        else:
            return None
    return {"rows": rows, "mu": mu, "primal": primal, "dual": dual, "comp": comp, "verdicts": verdicts}


def expectation(verdicts):
    names = ["primal", "dual", "stationarity", "complementarity"]
    for v, nm in zip(verdicts, names):
        if v == "V":
            return f"violated {nm}"
    return "certified"


# ---------------------------------------------------------------- input


def read_source():
    cases, cur = [], None
    for line in SRC.read_text(encoding="utf-8").splitlines():
        if line.startswith("#") or not line.strip():
            continue
        if line.startswith("case "):
            h = line.split()
            cur = {"name": h[1], "kind": h[3]}
        elif line == "end":
            cases.append(cur)
        else:
            k, _, v = line.partition(" ")
            cur[k] = v
    out = []
    for c in cases:
        if c["kind"] not in ("straightness", "flatness", "circle"):
            continue
        vals = [unhex(h) for h in c["points"].split()]
        rows = [tuple(int(t) for t in tok.split(":")) for tok in c["set"].split()]
        out.append((c["name"], c["kind"], vals, rows))
    return out


def make_problem(kind, vals):
    if kind == "straightness":
        return Lin(1, [vals[i:i + 2] for i in range(0, len(vals), 2)])
    if kind == "flatness":
        return Lin(2, [vals[i:i + 3] for i in range(0, len(vals), 3)])
    return Circle([vals[i:i + 2] for i in range(0, len(vals), 2)])


# ---------------------------------------------------------------- output


def emit(out, name, kind, vals, res, data_scale, tol):
    out.append(f"case {name} kind {kind}")
    out.append("points " + " ".join(hexf(v) for v in vals))
    out.append("set " + " ".join(f"{i}:{s}" for i, s in res["rows"]))
    out.append("tol " + " ".join(hexf(t) for t in tol))
    out.append(f"data_scale {hexf(data_scale)}")
    out.append("multipliers " + " ".join(hexf(m) for m in res["mu"]))
    out.append("multipliers_dec " + " ".join(dec(m) for m in res["mu"]))
    out.append(f"primal {hexf(res['primal'])}")
    out.append(f"dual {hexf(res['dual'])}")
    out.append(f"complementarity {hexf(res['comp'])}")
    out.append("verdicts " + " ".join(res["verdicts"]))
    out.append(f"expect {expectation(res['verdicts'])}")
    out.append("end")


def candidate_sets(prob, kind):
    rows = prob.rows()
    for combo in combinations(rows, prob.n):
        if len({i for i, _ in combo}) != prob.n:
            continue
        yield list(combo)


def render():
    out = [
        "# KRN.SOLV.CERT.KKT fixture kkt-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# Optimal and non-optimal active sets of minimum-zone straightness, flatness and circle",
        "# problems with exact multipliers and expected KKT verdicts (primal dual stationarity",
        "# complementarity; S satisfied, V violated). See make_cases.py.",
    ]
    tol = (TOL, TOL, TOL, TOL)
    circle_searched = 0
    count = {"opt": 0, "infeasible": 0, "negmult": 0}
    for name, kind, vals, opt_rows in read_source():
        prob = make_problem(kind, vals)
        data_scale = max(abs(v) for v in vals)
        p, k = prob.point_of(sorted(opt_rows))
        assert p is not None, name
        res = kkt(prob, opt_rows, p, data_scale, tol)
        assert res is not None and res["verdicts"] == ["S"] * 4, (name, res and res["verdicts"])
        assert min(res["mu"]) > 0, name
        emit(out, name, kind, vals, res, data_scale, tol)
        count["opt"] += 1

        if kind == "circle":
            if circle_searched >= CIRCLE_SEARCH:
                continue
            circle_searched += 1
        found = {}
        for rows in candidate_sets(prob, kind):
            if sorted(rows) == sorted(opt_rows):
                continue
            p, k = prob.point_of(sorted(rows))
            if p is None or k is None:
                continue
            res = kkt(prob, rows, p, data_scale, tol)
            if res is None:
                continue
            v = res["verdicts"]
            tag = "infeasible" if v[0] == "V" else ("negmult" if v[1] == "V" else None)
            if tag is None or tag in found or k > COND_MAX:
                continue
            assert v[2] == "S" and v[3] == "S"
            found[tag] = res
            if len(found) == 2:
                break
        for tag in ("infeasible", "negmult"):
            if tag in found:
                emit(out, f"{name}-{tag}", kind, vals, found[tag], data_scale, tol)
                count[tag] += 1

    # AC-2: upper bound tight at x = 0 and x = 1, lower bound at x = 1.2 (outside [0, 1]), so
    # the multiplier of the point at x = 0 is (1 - 1.2) / (1 - 0) = -0.2 (exactly, with the
    # double nearest 1.2). The vertex is feasible: only dual feasibility fails.
    x2 = Fraction(1.2)
    vals = [Fraction(0), Fraction(0), x2, Fraction(-1), Fraction(1), Fraction(0)]
    prob = make_problem("straightness", vals)
    rows = [(0, 1), (1, 0), (2, 1)]
    p, _ = prob.point_of(rows)
    res = kkt(prob, rows, p, max(abs(v) for v in vals), tol)
    assert res["verdicts"] == ["S", "V", "S", "S"], res["verdicts"]
    assert res["mu"][0] == 1 - x2 and res["dual"] == x2 - 1
    emit(out, "ac2-negative-multiplier", "straightness", vals, res, max(abs(v) for v in vals), tol)

    # AC-3: strt-00 plus one point 1e-6 above its optimal upper zone line, tol_p = 1e-12.
    name, kind, vals, opt_rows = read_source()[0]
    assert name == "strt-00"
    prob = make_problem(kind, vals)
    p, _ = prob.point_of(sorted(opt_rows))
    a, c, t = p
    x = vals[0]  # at the first point's abscissa, so x is an exact double
    y = Fraction(float(a * x + c + t / 2 + Fraction(1, 10**6)))
    vals = vals + [x, y]
    prob = make_problem(kind, vals)
    tol3 = (Fraction(1, 10**12), TOL, TOL, TOL)
    data_scale = max(abs(v) for v in vals)
    res = kkt(prob, opt_rows, p, data_scale, tol3)
    viol = prob.g((len(vals) // 2 - 1, 1), p)
    assert abs(viol - Fraction(1, 10**6)) < Fraction(1, 10**13), float(viol)
    assert res["primal"] == viol and res["verdicts"] == ["V", "S", "S", "S"], res["verdicts"]
    emit(out, "ac3-inactive-violation", kind, vals, res, data_scale, tol3)

    out.insert(4, f"# counts: optimal {count['opt']}, infeasible {count['infeasible']}, "
                  f"negative multiplier {count['negmult']}, plus ac2 and ac3")
    return "\n".join(out) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
