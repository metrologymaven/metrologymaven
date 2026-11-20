#!/usr/bin/env python3
"""Independent reference for the tied-pool basis reduction of KRN.SOLV.CERT.AS
(REQ-KRN.SOLV.CERT.AS-007) (CC-BY-4.0 project data).

Writes fixtures/solv/cert/as-ties-v1/cases.txt. Every expected value is decided here with exact
rational arithmetic (fractions.Fraction of exact dyadic data); nothing here runs or reads the Rust
code or any solver, and nothing uses the platform libm.

Problem (minimum-zone straightness, the conic form the Rust test solves): points (x_i, z_i),
parameters p = (a, c, t), dev_i(p) = z_i - a x_i - c, f(p) = t. Rows in the order
(0, 0), (0, 1), (1, 0), (1, 1), ...:
  row (i, 1): g = dev_i - t/2 <= 0, conic row A = (-x_i, -1, -1/2), b = -z_i;
  row (i, 0): g = -dev_i - t/2 <= 0, conic row A = (x_i, 1, -1/2), b = z_i;
and the conic objective q = grad f = (0, 0, 1). Conic form: A p + s = b, s >= 0.

Cases (`pool` = the rows the Rust test gives slack 0; all others get a large slack):

- tie200: 100 points on the upper line z = x/8 + 1 (x = 0, 2, ..., 198) and 100 on the lower
  line z = x/8 - 1 (x = 1, 3, ..., 199), listed in a seeded order (random.Random, so the label
  order is not the x order). Every upper row (i, 1) and lower row (i, 0) is tight at
  the unique optimum a = 1/8, c = 0, t = 2: a pool of 200 rows, C(200, 3) = 1 313 400 bases.
- tie13: the same construction with 7 upper and 6 lower points (pool 13, C(13, 3) = 286, just
  over the cap of 256).
- nobasis: the tie200 points with only the 100 upper rows in the pool: their multiplier
  system has no nonnegative solution (the second component forces sum y = 0, the third
  sum y = 2), so the reduction must propose nothing.
- fallback: the tie200 points with the lower point of tie200's basis raised by 2^-16. Its row
  stays in the pool (the test's slack threshold is 1e-6 * max|b|). A and q, and so the basis
  (`reduced`), are unchanged, but the basis's vertex now violates the other lower rows by
  `violation` (>= 1e-6, far above the 1e-9 * data_scale primal threshold), so KKT refuses it.
  The expected certificate is the first set of the lexicographic order (at most 256, the basis
  left out) whose exact vertex is feasible with nonnegative exact multipliers, each decision with
  a margin of 1e-6; `tried` counts the sets certify tries, the basis included.

The reduction (the rule of REQ-007): the system sum_k y_k a_k = -q, y >= 0 over the pool rows
a_k in label order, solved by a phase-1 simplex from the all-artificial start (rows with a
negative right-hand side negated first, one artificial per row, artificial j at column
m + j), with Bland's rule: entering column = the lowest-indexed column (structural, then
artificial) with a negative reduced cost; leaving row = the least ratio rhs_i / a_is over rows
with a_is > 0, ties to the lowest basic-variable index. At the end (phase-1 objective 0) every
artificial still basic at level 0 is driven out by the lowest-indexed nonbasic structural column
with a nonzero entry in its row. Infeasible (objective > 0) or a row with no such column: no
basis. This script implements that rule on a Fraction tableau, independently of the Rust code.

For each case with a basis the script also checks that the basis is n = 3 independent rows,
that its exact multipliers are >= 0, that its vertex is the planted optimum and that every row
is feasible there (so the KKT check must certify it).

Output lines:
  case <name> points <n>
  points <x z per point, as exact decimals of dyadic rationals>
  pool <point:side ...>
  expect basis <point:side ...> | expect nobasis
  params <a c t>            (basis cases; exact)
  end
"""

import random
from itertools import combinations, islice
import sys
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("cases.txt")


def rows_of(pts):
    """Conic rows (label, A row, b) in label order."""
    out = []
    for i, (x, z) in enumerate(pts):
        out.append(((i, 0), [x, Fraction(1), Fraction(-1, 2)], z))
        out.append(((i, 1), [-x, Fraction(-1), Fraction(-1, 2)], -z))
    return out


def bland(cols, q):
    """Bland-rule phase 1 on sum_k y_k cols[k] = -q, y >= 0 (see the module docs)."""
    n, m = len(q), len(cols)
    rhs = [-v for v in q]
    t = []
    for j in range(n):
        sgn = -1 if rhs[j] < 0 else 1
        row = [sgn * c[j] for c in cols] + [Fraction(int(a == j)) for a in range(n)] + [sgn * rhs[j]]
        t.append(row)
    width = m + n + 1
    cost = [Fraction(0)] * width
    for k in range(width):
        if m <= k < m + n:
            continue
        cost[k] = -sum(t[i][k] for i in range(n))
    basis = list(range(m, m + n))

    def pivot(r, s):
        p = t[r][s]
        t[r] = [v / p for v in t[r]]
        for i in range(n):
            if i != r and t[i][s] != 0:
                f = t[i][s]
                t[i] = [a - f * b for a, b in zip(t[i], t[r])]
        f = cost[s]
        if f != 0:
            for k in range(width):
                cost[k] -= f * t[r][k]
        basis[r] = s

    while True:
        s = next((k for k in range(m + n) if cost[k] < 0), None)
        if s is None:
            break
        best = None
        for i in range(n):
            if t[i][s] > 0:
                ratio = t[i][-1] / t[i][s]
                if best is None or ratio < best[0] or (ratio == best[0] and basis[i] < basis[best[1]]):
                    best = (ratio, i)
        assert best is not None  # phase 1 is bounded
        pivot(best[1], s)
    if cost[-1] != 0:
        return None
    for r in range(n):
        if basis[r] < m:
            continue
        s = next((k for k in range(m) if k not in basis and t[r][k] != 0), None)
        if s is None:
            return None
        pivot(r, s)
    return sorted(basis)


def solve(a, b):
    """Exact Gaussian elimination; None if singular."""
    n = len(a)
    mtx = [list(r) + [v] for r, v in zip(a, b)]
    for c in range(n):
        piv = next((r for r in range(c, n) if mtx[r][c] != 0), None)
        if piv is None:
            return None
        mtx[c], mtx[piv] = mtx[piv], mtx[c]
        for r in range(n):
            if r != c and mtx[r][c] != 0:
                f = mtx[r][c] / mtx[c][c]
                mtx[r] = [x - f * y for x, y in zip(mtx[r], mtx[c])]
    return [mtx[i][n] / mtx[i][i] for i in range(n)]


def lines(upper, lower, seed, slope=Fraction(1, 8)):
    """Points on the two lines, listed in a seeded order (so label order is not x order)."""
    pts = []
    for x in range(max(2 * upper, 2 * lower)):
        if x % 2 == 0 and x // 2 < upper:
            pts.append((Fraction(x), slope * x + 1))
        elif x % 2 == 1 and x // 2 < lower:
            pts.append((Fraction(x), slope * x - 1))
    random.Random(seed).shuffle(pts)
    return pts


def tight(pts):
    return [(i, 1) if z == Fraction(1, 8) * x + 1 else (i, 0) for i, (x, z) in enumerate(pts)]


def decide(pts, pool):
    rows = {lab: (arow, b) for lab, arow, b in rows_of(pts)}
    cols = [rows[lab][0] for lab in pool]
    q = [Fraction(0), Fraction(0), Fraction(1)]
    idx = bland(cols, q)
    if idx is None:
        return None, None
    basis = [pool[k] for k in idx]
    a_b = [rows[lab][0] for lab in basis]
    # Independent rows with nonnegative multipliers: A_B^T y = -q.
    at = [[a_b[r][c] for r in range(3)] for c in range(3)]
    y = solve(at, [-v for v in q])
    assert y is not None and all(v >= 0 for v in y), (basis, y)
    p = solve(a_b, [rows[lab][1] for lab in basis])
    assert p == [Fraction(1, 8), Fraction(0), Fraction(2)], p
    for lab, (arow, b) in rows.items():
        assert sum(u * v for u, v in zip(arow, p)) - b <= 0, lab
    return basis, p


def first_certified(pts, pool, skip):
    """The first set of the lexicographic order of `pool` (at most 256, `skip` left out) whose
    exact vertex is feasible with nonnegative exact multipliers, every decision taken with a
    margin of 1e-6 (so the Rust KKT check, with 1e-9 relative tolerances, agrees)."""
    rows = {lab: (arow, b) for lab, arow, b in rows_of(pts)}
    q = [Fraction(0), Fraction(0), Fraction(1)]
    margin = Fraction(1, 10**6)
    tried = 0
    for cand in islice(combinations(pool, 3), 256):
        cand = list(cand)
        if cand == skip:
            continue
        tried += 1
        a_b = [rows[lab][0] for lab in cand]
        p = solve(a_b, [rows[lab][1] for lab in cand])
        if p is None:
            continue  # singular: RECOMP refuses it
        worst = max(sum(u * v for u, v in zip(arow, p)) - b for arow, b in rows.values())
        y = solve([[a_b[r][c] for r in range(3)] for c in range(3)], [-v for v in q])
        assert worst <= 0 or worst >= margin, (cand, worst)
        assert min(y) >= 0 or min(y) <= -margin, (cand, y)
        if worst <= 0 and min(y) >= 0:
            return cand, p, tried
    return None, None, tried


def worst_row(pts, basis):
    rows = {lab: (arow, b) for lab, arow, b in rows_of(pts)}
    p = solve([rows[lab][0] for lab in basis], [rows[lab][1] for lab in basis])
    return max(sum(u * v for u, v in zip(arow, p)) - b for arow, b in rows.values())


def dec(f):
    """An exact decimal of a dyadic rational."""
    f = Fraction(f)
    k = 0
    while (f * 10**k).denominator != 1:
        k += 1
    v = f * 10**k
    s = str(abs(v.numerator)).rjust(k + 1, "0")
    out = s if k == 0 else s[:-k] + "." + s[-k:]
    return ("-" if f < 0 else "") + out


def fmt(labels):
    return " ".join(f"{i}:{s}" for i, s in labels)


def render():
    out = [
        "# KRN.SOLV.CERT.AS fixture as-ties-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# Over-cap tied pools of a linear straightness LP and the exact Bland-order basis they reduce to.",
    ]
    big = lines(100, 100, "as-ties-v1/tie200")
    small = lines(7, 6, "as-ties-v1/tie13")
    for name, pts, pool in [
        ("tie200", big, sorted(tight(big))),
        ("tie13", small, sorted(tight(small))),
        ("nobasis", big, sorted(lab for lab in tight(big) if lab[1] == 1)),
    ]:
        basis, p = decide(pts, pool)
        out.append(f"case {name} points {len(pts)}")
        out.append("points " + " ".join(f"{dec(x)} {dec(z)}" for x, z in pts))
        out.append("pool " + fmt(pool))
        if basis is None:
            out.append("expect nobasis")
        else:
            out.append("expect basis " + fmt(basis))
            out.append("params " + " ".join(dec(v) for v in p))
        out.append("end")
    # fallback (see the module docs).
    pool = sorted(tight(big))
    basis, _ = decide(big, pool)
    raised = next(lab[0] for lab in basis if lab[1] == 0)
    delta = Fraction(1, 2**16)
    pts = [(x, z + delta) if i == raised else (x, z) for i, (x, z) in enumerate(big)]
    rows = {lab: (arow, b) for lab, arow, b in rows_of(pts)}
    idx = bland([rows[lab][0] for lab in pool], [Fraction(0), Fraction(0), Fraction(1)])
    assert [pool[k] for k in idx] == basis
    worst = worst_row(pts, basis)
    assert worst >= Fraction(1, 10**6), worst  # primal certainly violated (threshold ~2e-7)
    cand, p, tried = first_certified(pts, pool, basis)
    assert cand is not None and p == [Fraction(1, 8), Fraction(0), Fraction(2)], (cand, p)
    out.append(f"case fallback points {len(pts)}")
    out.append("points " + " ".join(f"{dec(x)} {dec(z)}" for x, z in pts))
    out.append("pool " + fmt(pool))
    out.append("reduced " + fmt(basis))
    out.append(f"violation {dec(worst)}")
    out.append("expect basis " + fmt(cand))
    out.append("params " + " ".join(dec(v) for v in p))
    out.append(f"tried {tried + 1}")
    out.append("end")
    return "\n".join(out) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if not (OUT.exists() and OUT.read_text(encoding="utf-8") == text):
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
