#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.CERT.AS (CC-BY-4.0 project data).

Writes fixtures/solv/cert/as-v1/cases.txt. Every expected value (row classes, scales,
candidate sets, their order, tie_broken, over_determined, errors) is decided here with exact
rational arithmetic (fractions.Fraction of the exact f64 values). Nothing here runs or reads
the Rust code or any solver, and nothing uses the platform libm: the only float operations
are float(Fraction) (correctly rounded by integer arithmetic) and abs.

Problems are minimum-zone straightness LPs in the conic wrapper's form (minimise q'x subject
to Ax + s = b, s >= 0). Points (u_i, v_i) with u_i = i; variables (a, k, t) for the line
a + k u and the half-width t; per point two rows, labelled (point i, side 0) for the lower
bound  a + k u_i - t <= v_i  and (point i, side 1) for the upper bound  -a - k u_i - t <= -v_i.
So s_lower = v_i - a - k u_i + t and s_upper = t - (v_i - a - k u_i).

Each LP is planted: a dyadic line (a0, k0) and half-width t0, contact points with residual
exactly +t0 (upper) or -t0 (lower), every other point with |residual| <= 3/4 t0. All data are
dyadic, so the f64 problem equals the rational one. When the contacts contain an alternating
triple the planted line is the minimax line (equioscillation), so t* = t0 and the contact
rows are exactly the rows with zero slack. A basis is a 3-subset of the contact rows whose
dual system  A_B' y_B = -q  has a solution with every entry > 0 (solved exactly here).

Kinds:
- nondegenerate (30): 3 alternating contacts; one basis; the dual is that basis's solution.
- tie-ambiguous (5, AC-2's 5-point problem among them): 4 contacts with two bases; the two
  rows in both bases carry the mean of the two bases' duals and the two tied rows carry tiny
  multipliers (an iterate that has not separated the tie), so the tied rows are ambiguous and
  the candidates are exactly the two bases.
- tie-overdetermined (5): 4 contacts with two bases; the dual is the mean of the two bases'
  (the kind of point an interior-point method converges to), so all 4 rows are strongly
  active and the candidates are the 3-subsets of them.
- tie-vertex (2, beyond the 40): 4 contacts, the dual is the first basis's (a vertex), so
  the fourth contact row is ambiguous but the strongly active rows already have the expected
  size: one candidate, tie_broken 0.
- boundary: one- and two-row problems whose slack or multiplier sits at or within one ulp of
  slack_rel * scale_s or mult_rel * scale_y, where rounding the product would misclassify.
- undetermined: too few small-slack rows, and more candidates than the cap.

The solution vectors imitate a converged interior-point run: zero slacks get small noise of
either sign, multipliers get small noise, inactive rows get tiny positive multipliers. Rows
are listed in a seeded shuffled order (labels shuffled with them).

`solver_set` lines give, for the LP cases, the candidates expected when the Rust test solves
the LP itself with `solver_thresholds`: every contact row strongly active (an interior-point
method converges to the analytic centre of the optimal dual face, which is positive on every
contact row of these problems), so the 3-subsets of the contacts. That expectation rests on
the solver's accuracy (slack of contact rows below 1e-6 * scale, multipliers above
1e-6 * scale); the margins here are > 0.1 on every other row.

Format: `case <name> kind <kind>` ... `end`. Floats are IEEE-754 bit patterns in hex.
Labels are `<point>:<side>`; an empty list is the bare key. `classes` gives S (strongly active), A (ambiguous) or I
(inactive) per row in row order. `set` lines list the candidates in order. `result` is
`ok tie_broken <0|1> over_determined <0|1>` or `undetermined <TooFewRows|OverCap>`.

Run:  python fixtures/solv/cert/as-v1/make_cases.py [--check]
"""

import struct
import sys
from fractions import Fraction
from itertools import combinations
from pathlib import Path

OUT = Path(__file__).with_name("cases.txt")
MASK = (1 << 64) - 1
CAP = 256
TH = (1e-9, 1e-9)  # (slack_rel, mult_rel) of the fixture solutions
SOLVER_TH = (1e-6, 1e-6)  # thresholds for the solver-run test


class SplitMix64:
    def __init__(self, seed):
        self.state = seed & MASK

    def next(self):
        self.state = (self.state + 0x9E3779B97F4A7C15) & MASK
        z = self.state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)

    def int_in(self, lo, hi):
        """Uniform-ish integer in [lo, hi] (modulo bias is irrelevant here)."""
        return lo + self.next() % (hi - lo + 1)

    def shuffle(self, xs):
        for i in range(len(xs) - 1, 0, -1):
            j = self.next() % (i + 1)
            xs[i], xs[j] = xs[j], xs[i]


def f64(q):
    """Correctly rounded double of a rational (integer arithmetic, no libm)."""
    return float(Fraction(q))


def hexf(v):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", v))[0]


def fr(v):
    return Fraction(v)


def lab(l):
    return "%d:%d" % l


# ---- exact classification and enumeration (the oracle) ---------------------------------


def classify(b, s, y, th):
    scale_b = max([abs(fr(v)) for v in b] + [Fraction(0)])
    scale_s = Fraction(1) if scale_b == 0 else scale_b
    scale_y = max([abs(fr(v)) for v in y] + [Fraction(0)])
    ts = fr(th[0]) * scale_s
    ty = fr(th[1]) * scale_y
    classes = []
    for si, yi in zip(s, y):
        if fr(si) > ts:
            classes.append("I")
        elif fr(yi) > 0 and fr(yi) >= ty:
            classes.append("S")
        else:
            classes.append("A")
    return classes, f64(scale_s), f64(scale_y)


def candidates(labels, classes, expected):
    strong = sorted(l for l, c in zip(labels, classes) if c == "S")
    amb = sorted(l for l, c in zip(labels, classes) if c == "A")
    if len(strong) > expected:
        sets = [sorted(c) for c in combinations(strong, expected)]
        over = True
    else:
        k = expected - len(strong)
        if len(amb) < k:
            return strong, amb, ("undetermined", "TooFewRows")
        sets = [sorted(strong + list(c)) for c in combinations(amb, k)]
        over = False
    if len(sets) > CAP:
        return strong, amb, ("undetermined", "OverCap")
    sets.sort()
    return strong, amb, ("ok", sets, len(sets) > 1, over)


# ---- planted straightness LPs -----------------------------------------------------------


def row_of(u, side):
    # (coefficients of (a, k, t), sign of v in b)
    return ([1, u, -1], 1) if side == 0 else ([-1, -u, -1], -1)


def solve3(m, rhs):
    """Exact solution of a 3x3 system, or None if singular (Gauss-Jordan on Fractions)."""
    a = [[Fraction(x) for x in r] + [Fraction(v)] for r, v in zip(m, rhs)]
    for c in range(3):
        p = next((r for r in range(c, 3) if a[r][c] != 0), None)
        if p is None:
            return None
        a[c], a[p] = a[p], a[c]
        for r in range(3):
            if r != c and a[r][c] != 0:
                f = a[r][c] / a[c][c]
                a[r] = [x - f * y for x, y in zip(a[r], a[c])]
    return [a[i][3] / a[i][i] for i in range(3)]


def basis_dual(rows):
    """Dual of a 3-row basis: sum_j y_j * A_j = -q = (0, 0, -1); None unless all > 0."""
    cols = [[rows[j][0][i] for j in range(3)] for i in range(3)]
    y = solve3(cols, [0, 0, -1])
    if y is None or any(v <= 0 for v in y):
        return None
    return y


def plant(rng, n, contacts):
    """contacts: {point: side}. Returns problem data and exact optimal slacks."""
    a0 = Fraction(rng.int_in(-64, 64), 64)
    k0 = Fraction(rng.int_in(-64, 64), 256)
    t0 = Fraction(32 + rng.int_in(0, 64), 64)
    v = []
    for i in range(n):
        if i in contacts:
            r = t0 if contacts[i] == 1 else -t0
        else:
            r = Fraction(rng.int_in(-48, 48), 64) * t0
        v.append(a0 + k0 * i + r)
    rows, b, s_exact = [], [], []
    for i in range(n):
        for side in (0, 1):
            coef, sg = row_of(i, side)
            rows.append(((i, side), coef))
            b.append(sg * v[i])
            res = v[i] - a0 - k0 * i
            s_exact.append(res + t0 if side == 0 else t0 - res)
    for x in b:
        assert f64(x) == x, "data must be dyadic"
    return rows, b, s_exact, (a0, k0, t0)


def contact_pattern(rng, n, kind):
    """Points and sides of the contacts: an alternating triple, or 4 contacts with 2 bases."""
    while True:
        pts = sorted(rng.int_in(0, n - 1) for _ in range(3 if kind == "nondegenerate" else 4))
        if len(set(pts)) != len(pts):
            continue
        first = rng.int_in(0, 1)
        if kind == "nondegenerate":
            return {p: (first + j) % 2 for j, p in enumerate(pts)}
        # 4 contacts: an alternating triple with one of its sides repeated next to it, so
        # exactly two alternating triples (two bases) exist, as in L U U L or L L U L
        triple = [first, 1 - first, first]
        dup = rng.int_in(0, 2)
        sides = triple[: dup + 1] + triple[dup:]
        return dict(zip(pts, sides))


def mean_dual(bases):
    y_exact = {}
    for tri, y in bases:
        for l, v in zip(tri, y):
            y_exact[l] = y_exact.get(l, Fraction(0)) + v / len(bases)
    return y_exact


def lp_case(rng, name, kind):
    n = rng.int_in(6, 12)
    contacts = contact_pattern(rng, n, kind)
    rows, b, s_exact, _ = plant(rng, n, contacts)
    labels = [l for l, _ in rows]
    contact_rows = sorted((p, sd) for p, sd in contacts.items())
    by_label = {l: (c, 0) for l, c in rows}
    bases = []
    for tri in combinations(contact_rows, 3):
        y = basis_dual([by_label[l] for l in tri])
        if y is not None:
            bases.append((list(tri), y))
    if kind == "nondegenerate":
        assert len(bases) == 1
        y_exact = dict(zip(bases[0][0], bases[0][1]))
    else:
        assert len(bases) == 2, bases
        if kind == "tie-vertex":
            y_exact = dict(zip(bases[0][0], bases[0][1]))
        else:
            y_exact = mean_dual(bases)
            assert sorted(y_exact) == contact_rows and all(v > 0 for v in y_exact.values())
            if kind == "tie-ambiguous":
                common = set(bases[0][0]) & set(bases[1][0])
                for l in contact_rows:
                    if l not in common:
                        del y_exact[l]
    # zero slack exactly on the contact rows, as planted
    zero = [l for l, s in zip(labels, s_exact) if s == 0]
    assert zero == contact_rows, (zero, contact_rows)
    # imitate interior-point output
    s, y = [], []
    for l, se in zip(labels, s_exact):
        if se == 0:
            s.append(f64(Fraction(rng.int_in(-8, 15), 1 << 44)))
        else:
            s.append(f64(se))
        if l in y_exact:
            y.append(f64(y_exact[l] + Fraction(rng.int_in(-15, 15), 1 << 50)))
        else:
            y.append(f64(Fraction(rng.int_in(1, 1000), 1 << 60)))
    # shuffle the rows (labels move with them)
    order = list(range(len(rows)))
    rng.shuffle(order)
    a_rows = [rows[i][1] for i in order]
    labels = [labels[i] for i in order]
    b = [f64(b[i]) for i in order]
    s = [s[i] for i in order]
    y = [y[i] for i in order]
    x = [0.0, 0.0, 0.0]
    solver_sets = [sorted(c) for c in combinations(contact_rows, 3)]
    return dict(
        name=name, kind=kind, n=3, a_rows=a_rows, b=b, q=[0.0, 0.0, 1.0], x=x, y=y, s=s,
        labels=labels, expected=3, th=TH,
        known=[tri for tri, _ in bases], solver_sets=solver_sets,
    )


def ac2_case():
    """AC-2's 5-point problem: points 1 and 3 both on the upper boundary (v = -1, 1, 0, 1, -1,
    line 0, t = 1). Contacts 0:0, 1:1, 3:1, 4:0, bases {0:0, 1:1, 4:0} and {0:0, 3:1, 4:0}."""
    v = [-1, 1, 0, 1, -1]
    rows, b, s = [], [], []
    for i in range(5):
        for side in (0, 1):
            coef, sg = row_of(i, side)
            rows.append(((i, side), coef))
            b.append(float(sg * v[i]))
            s.append(float(v[i] + 1 if side == 0 else 1 - v[i]))
    labels = [l for l, _ in rows]
    # rows 0:0 and 4:0 (in both bases) carry the mean dual 1/4 each; the tied rows 1:1 and
    # 3:1 carry tiny multipliers, so they are ambiguous and the two bases are the candidates
    common = {(0, 0): Fraction(1, 4), (4, 0): Fraction(1, 4)}
    y = [f64(common.get(l, Fraction(1, 1 << 60))) for l in labels]
    contact_rows = [(0, 0), (1, 1), (3, 1), (4, 0)]
    return dict(
        name="ac2-five-point-tie", kind="tie-ambiguous", n=3, a_rows=[r for _, r in rows], b=b,
        q=[0.0, 0.0, 1.0], x=[0.0, 0.0, 1.0], y=y, s=s, labels=labels, expected=3, th=TH,
        known=[[(0, 0), (1, 1), (4, 0)], [(0, 0), (3, 1), (4, 0)]],
        solver_sets=[sorted(c) for c in combinations(contact_rows, 3)],
    )


# ---- boundary and undetermined cases ----------------------------------------------------


def one_var(name, kind, b, s, y, labels, expected, th):
    return dict(
        name=name, kind=kind, n=1, a_rows=[[1] for _ in b], b=b, q=[1.0], x=[0.0], y=y, s=s,
        labels=labels, expected=expected, th=th, known=None, solver_sets=None,
    )


def next_up(v):
    bits = struct.unpack("<Q", struct.pack("<d", v))[0]
    return struct.unpack("<d", struct.pack("<Q", bits + 1))[0]


def next_down(v):
    bits = struct.unpack("<Q", struct.pack("<d", v))[0]
    return struct.unpack("<d", struct.pack("<Q", bits - 1))[0]


def boundary_cases(rng):
    out = []
    pairs = [(1e-9, 3.0), (0.1, 3.0), (1e-9, 7.25), (0.3, 0.1), (1e-6, 123.456), (2.0 ** -30, 4.0),
             (1e-9, 0.0)]
    for idx, (t, scale) in enumerate(pairs):
        exact = fr(t) * (fr(scale) if scale != 0 else 1)
        near = f64(exact)
        vals = [("below", next_down(near)), ("near", near), ("above", next_up(near))]
        if f64(exact) == exact:
            vals.append(("exact", near))
        for tag, v in vals:
            out.append(one_var("slack-%d-%s" % (idx, tag), "boundary", [scale], [v], [1.0], [(0, 0)], 1,
                               (t, 0.5)))
    for idx, (t, ymax) in enumerate([(1e-9, 0.75), (0.1, 3.0), (0.3, 0.1), (1e-6, 0.123456)]):
        near = f64(fr(t) * fr(ymax))
        for tag, v in [("below", next_down(near)), ("near", near), ("above", next_up(near))]:
            out.append(one_var("mult-%d-%s" % (idx, tag), "boundary", [1.0, 1.0], [0.0, 0.0], [ymax, v],
                               [(0, 0), (1, 0)], 1, (0.5, t)))
    # zero multiplier with mult_rel 0: small slack but not clearly positive -> ambiguous
    out.append(one_var("mult-zero-threshold", "boundary", [1.0, 1.0], [0.0, 0.0], [0.0, 0.0],
                       [(0, 0), (1, 0)], 1, (0.0, 0.0)))
    # negative slack and negative multiplier
    out.append(one_var("negative-slack-and-mult", "boundary", [2.0, 2.0, 2.0], [-1e-12, -1e-12, 0.5],
                       [0.5, -1e-3, 1e-3], [(4, 1), (2, 0), (9, 0)], 1, TH))
    return out


def undetermined_cases():
    out = []
    # AC-3 shape: 2 rows of small slack, expected 4
    out.append(one_var("too-few-rows", "undetermined", [1.0] * 6, [0.0, 0.0, 0.5, 0.5, 0.5, 0.5],
                       [0.5, 0.5, 0.0, 0.0, 0.0, 0.0], [(i, 0) for i in range(6)], 4, TH))
    # 1 strong + 12 ambiguous, expected 7: C(12, 6) = 924 > 256
    out.append(one_var("over-cap", "undetermined", [1.0] * 13, [0.0] * 13, [0.5] + [0.0] * 12,
                       [(i, 1) for i in range(13)], 7, TH))
    # exactly at the cap: 1 strong + C(256, 1)... kept small: C(10, 5) = 252 <= 256
    out.append(one_var("at-cap-252", "boundary", [1.0] * 11, [0.0] * 11, [0.5] + [0.0] * 10,
                       [(i, 0) for i in range(11)], 6, TH))
    # over-determined over the cap: 12 strong rows, expected 5: C(12, 5) = 792 > 256
    out.append(one_var("over-determined-over-cap", "undetermined", [1.0] * 12, [0.0] * 12,
                       [0.25] * 12, [(i, 0) for i in range(12)], 5, TH))
    return out


# ---- rendering ---------------------------------------------------------------------------


def csc_rows(a_rows, n):
    colptr, rowval, nzval = [0], [], []
    for j in range(n):
        for i, r in enumerate(a_rows):
            if r[j] != 0:
                rowval.append(i)
                nzval.append(float(r[j]))
        colptr.append(len(rowval))
    return colptr, rowval, nzval


def render_case(c):
    m = len(c["b"])
    colptr, rowval, nzval = csc_rows(c["a_rows"], c["n"])
    classes, scale_s, scale_y = classify(c["b"], c["s"], c["y"], c["th"])
    strong, amb, res = candidates(c["labels"], classes, c["expected"])
    hx = lambda xs: " ".join(hexf(float(v)) for v in xs)
    ls = lambda xs: " ".join(lab(l) for l in xs)
    out = ["case %s kind %s" % (c["name"], c["kind"]), "n %d" % c["n"], "m %d" % m,
           "a_colptr " + " ".join(map(str, colptr)), "a_rowval " + " ".join(map(str, rowval)),
           "a_nzval " + hx(nzval), "b " + hx(c["b"]), "q " + hx(c["q"]), "x " + hx(c["x"]),
           "y " + hx(c["y"]), "s " + hx(c["s"]), "labels " + ls(c["labels"]),
           "expected %d" % c["expected"], "thresholds " + hx(c["th"]),
           "scales " + hx([scale_s, scale_y]), "classes " + " ".join(classes),
           "strongly_active " + ls(strong), "ambiguous " + ls(amb)]
    if res[0] == "ok":
        _, sets, tie, over = res
        out.append("result ok tie_broken %d over_determined %d" % (tie, over))
        out += ["set " + ls(s) for s in sets]
        if c["known"] is not None:
            # every planted basis must be among the candidates, in order
            idx = [sets.index(k) for k in c["known"] if k in sets]
            assert idx == sorted(idx), c["name"]
            if c["kind"] == "nondegenerate":
                assert sets == c["known"] and not tie, c["name"]
            elif c["kind"] == "tie-vertex":
                assert sets == c["known"][:1] and not tie and amb, c["name"]
            elif c["kind"] == "tie-ambiguous":
                assert sets == c["known"] and tie and not over, c["name"]
            else:
                assert tie and over and len(sets) == 4, c["name"]
    else:
        out.append("result undetermined %s" % res[1])
    if c["known"] is not None:
        out += ["known " + ls(k) for k in c["known"]]
    if c["solver_sets"] is not None:
        out += ["solver_set " + ls(s) for s in c["solver_sets"]]
    out.append("end")
    return out


def render():
    rng = SplitMix64(20261120)
    cases = [lp_case(rng, "nondeg-%02d" % i, "nondegenerate") for i in range(30)]
    cases += [lp_case(rng, "tie-amb-%02d" % i, "tie-ambiguous") for i in range(4)]
    cases.append(ac2_case())
    cases += [lp_case(rng, "tie-over-%02d" % i, "tie-overdetermined") for i in range(5)]
    cases += [lp_case(rng, "tie-vertex-%02d" % i, "tie-vertex") for i in range(2)]
    cases += boundary_cases(rng)
    cases += undetermined_cases()
    lines = [
        "# KRN.SOLV.CERT.AS fixture as-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# 40 planted minimum-zone straightness LPs (30 nondegenerate, 10 tied), boundary and",
        "# undetermined cases. Expected values are exact-rational decisions; see make_cases.py.",
        "solver_thresholds " + " ".join(hexf(v) for v in SOLVER_TH),
    ]
    for c in cases:
        lines += [ln.rstrip() for ln in render_case(c)]
    return "\n".join(lines) + "\n"


def main():
    text = render()
    if "--check" in sys.argv[1:]:
        if OUT.read_text(encoding="utf-8") != text:
            print(f"{OUT} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} is up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
