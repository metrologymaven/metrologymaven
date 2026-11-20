#!/usr/bin/env python3
"""Independent reference for KRN.SOLV.CERT.CT (CC-BY-4.0 project data).

Writes fixtures/solv/cert/ct-v1/cases.txt and fixtures/solv/cert/ct-v1/golden-cert.json.
Every expected value is decided here with exact rational arithmetic (fractions.Fraction of
exact dyadic data). Nothing here runs or reads the Rust code or any solver, and nothing uses
the platform libm: the only float operations are float(Fraction) (correctly rounded by
integer arithmetic), the IEEE-754 product of two doubles (for the golden certificate's
thresholds, exactly as the certificate rule states them) and bit manipulation.

Problems (all data dyadic, so the f64 problem equals the rational one). Points q_i with
coordinates (x_i, z_i) (dim 1) or (x_i, y_i, z_i) (dim 2); parameters p = (a, c, t) or
(a, b, c, t); deviation dev_i(p) = z_i - a x_i [- b y_i] - c. Rows, in the order
(0, 0), (0, 1), (1, 0), (1, 1), ... (point, side):

- zone (minimum-zone straightness / flatness, CertKind::Zone):
  row (i, 1): g = dev_i - t/2 <= 0; row (i, 0): g = -dev_i - t/2 <= 0.
- envelope (one-sided tangent envelope, CertKind::Envelope): the line / plane lies on or above
  every point and the largest gap below it is minimised:
  row (i, 1): g = dev_i <= 0; row (i, 0): g = -dev_i - t <= 0.

f(p) = t. Every g is affine, g(p) = A_row p - b_row, which is also the conic form
(A p + s = b, s >= 0) that the Rust test solves.

For each problem the optimum is found by brute force: every n-subset of rows (n = dim + 2) is
solved exactly; the feasible vertices are compared by t. A problem is kept only when exactly
one basis attains the least t, every other row has g <= -margin (margin = 10^-3 * data scale,
the data scale being max |coordinate|) and every exact multiplier mu (G^T mu = -grad f, the
sign convention of KRN.SOLV.CERT.KKT) is >= 10^-3. Otherwise the seed is skipped. So the
optimum is unique and nondegenerate, and its basis is the active set a certificate must name.

Expected values: the active set, the exact optimum and multipliers rounded to the nearest
double. The Rust test checks that the certified values' proven enclosures contain them.

Two solver paths per problem ("two seeded conic starting points"): path A solves the conic
problem with rows in the order above; path B solves it with the rows permuted by `perm` (row j
of path B is row perm[j] of path A) and each row r scaled by 2^scale[r]. Both are seeded
here (random.Random); the Rust test maps path B's slacks and multipliers back exactly.

Special cases:

- ac2-tie (AC-2 of KRN.SOLV.CERT.AS): x = 0..4, z = (-1, 1, 0, 1, -1), zone. Small-slack
  rows 0:0 1:1 3:1 4:0, so four candidates in lexicographic order. Each is decided exactly:
  {0:0 1:1 3:1} has a negative multiplier (dual violated), {0:0 1:1 4:0} is the first
  certified, so the certificate names it with tie_broken.
- ac3-all-fail: the same points with small-slack rows 0:0 1:1 2:1 3:1. Every candidate is
  either singular or violates a KKT condition, so certify fails after 4 tries; the first
  candidate's exact verdicts are recorded.

golden-cert.json: a synthetic certificate in schema fid.cert@1 (values chosen here, not
computed by any solver) serialised by this script's own RFC 8785 writer (json.dumps with
sorted keys and no whitespace; every value a string, floats in the CORE.JRN.CANON F64Bits
form "0x" + 16 lowercase hex digits of their IEEE-754 bits, booleans as "true"/"false") with
its SHA-256 digest from hashlib, so the Rust parser and digest rule are checked against an
independent implementation. Its per-row dual thresholds follow the certificate rule
thr_i = down(down(tol_d * gradient_lo) / ghi_i), down = one step toward -inf clamped at 0,
with IEEE-754 products and quotients of doubles.
"""

import hashlib
import json
import random
import struct
import sys
from fractions import Fraction
from itertools import combinations
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "cases.txt"
GOLDEN = HERE / "golden-cert.json"

SLACK_REL = 1e-6
MULT_REL = 1e-6
TOL = Fraction(1, 10**9)
MARGIN_REL = Fraction(1, 1000)
MU_MIN = Fraction(1, 1000)
PER_GROUP = 15
GROUPS = [("zone", 1, 8), ("zone", 2, 8), ("envelope", 1, 8), ("envelope", 2, 8)]


def hexf(x):
    """Hex bits of the nearest double (float() of a Fraction is correctly rounded)."""
    return struct.pack(">d", float(x)).hex()


def bits(x):
    return struct.pack(">d", x).hex()


def solve(a, b):
    """Exact Gauss-Jordan elimination; None if singular."""
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


def transpose(a):
    return [list(r) for r in zip(*a)]


class Lin:
    def __init__(self, kind, dim, pts):
        self.kind, self.dim, self.pts = kind, dim, pts
        self.n = dim + 2

    def rows(self):
        return [(i, s) for i in range(len(self.pts)) for s in (0, 1)]

    def a_row(self, row):
        i, s = row
        q = self.pts[i][: self.dim]
        if self.kind == "zone":
            if s == 1:
                return [-c for c in q] + [Fraction(-1), Fraction(-1, 2)]
            return list(q) + [Fraction(1), Fraction(-1, 2)]
        if s == 1:
            return [-c for c in q] + [Fraction(-1), Fraction(0)]
        return list(q) + [Fraction(1), Fraction(-1)]

    def b_row(self, row):
        i, s = row
        z = self.pts[i][self.dim]
        return -z if s == 1 else z

    def g(self, row, p):
        return sum(a * x for a, x in zip(self.a_row(row), p)) - self.b_row(row)

    def grad_f(self):
        return [Fraction(0)] * (self.n - 1) + [Fraction(1)]

    def data_scale(self):
        return max(abs(c) for q in self.pts for c in q)

    def vertex(self, rows):
        return solve([self.a_row(r) for r in rows], [self.b_row(r) for r in rows])

    def multipliers(self, rows):
        return solve(transpose([self.a_row(r) for r in rows]), [-v for v in self.grad_f()])


def optimum(prob):
    """The unique nondegenerate optimal basis, or None."""
    rows = prob.rows()
    best, best_sets = None, []
    for combo in combinations(rows, prob.n):
        p = prob.vertex(list(combo))
        if p is None:
            continue
        if any(prob.g(r, p) > 0 for r in rows):
            continue
        t = p[-1]
        if best is None or t < best:
            best, best_sets = t, [(list(combo), p)]
        elif t == best:
            best_sets.append((list(combo), p))
    if best is None or len(best_sets) != 1:
        return None
    s, p = best_sets[0]
    margin = MARGIN_REL * prob.data_scale()
    if any(prob.g(r, p) > -margin for r in rows if r not in s):
        return None
    mu = prob.multipliers(s)
    if mu is None or min(mu) < MU_MIN:
        return None
    return sorted(s), p, mu


def gen_points(rng, dim, count):
    pts, seen = [], set()
    while len(pts) < count:
        if dim == 1:
            x = Fraction(4 * len(pts) + rng.randint(0, 3), 2)
            key = (x,)
        else:
            x = Fraction(rng.randint(0, 64), 4)
            y = Fraction(rng.randint(0, 64), 4)
            key = (x, y)
        if key in seen:
            continue
        seen.add(key)
        z = Fraction(rng.randint(-4096, 4096), 1024)
        pts.append(list(key) + [z])
    return pts


def kkt_verdicts(prob, rows, p):
    """Exact KKT verdicts at the vertex p of rows (primal dual stationarity complementarity),
    with the default tolerances; None if any quantity is within a factor 2 of its threshold.
    Dual thresholds: tol_d |grad f| / |grad g_i| per row, compared through squares."""
    rows = sorted(rows)
    d = prob.data_scale()
    mu = prob.multipliers(rows)
    primal = max(prob.g(r, p) for r in prob.rows())
    comp = max(abs(prob.g(r, p)) for r in rows)
    out = []
    if primal <= TOL * d / 2:
        out.append("S")
    elif primal >= 2 * TOL * d:
        out.append("V")
    else:
        return None
    row_v = []
    gf2 = sum(v * v for v in prob.grad_f())
    for m, r in zip(mu, rows):
        gi2 = sum(v * v for v in prob.a_row(r))
        # thr_i^2 = tol^2 |grad f|^2 / |grad g_i|^2
        thr2 = TOL * TOL * gf2 / gi2
        if m >= 0 or m * m <= thr2 / 4:
            row_v.append("S")
        elif m * m >= 4 * thr2:
            row_v.append("V")
        else:
            return None
    out.append("V" if "V" in row_v else "S")
    out.append("S")  # stationarity is exact at mu
    out.append("S" if comp <= TOL * d / 2 else ("V" if comp >= 2 * TOL * d else None))
    if None in out:
        return None
    return out, mu


def fmt_rows(rows):
    return " ".join(f"{i}:{s}" for i, s in rows)


def emit_case(out, name, prob, s, p, mu, perm, scale):
    out.append(f"case {name} kind {prob.kind} dim {prob.dim}")
    out.append("points " + " ".join(hexf(c) for q in prob.pts for c in q))
    out.append("perm " + " ".join(str(v) for v in perm))
    out.append("scale " + " ".join(str(v) for v in scale))
    out.append("set " + fmt_rows(s))
    out.append("params " + " ".join(hexf(v) for v in p))
    out.append(f"value {hexf(p[-1])}")
    out.append("multipliers " + " ".join(hexf(v) for v in mu))
    out.append("end")


def tie_cases(out):
    pts = [[Fraction(i), Fraction(v)] for i, v in enumerate([-1, 1, 0, 1, -1])]
    prob = Lin("zone", 1, pts)
    def decide(rows):
        p = prob.vertex(rows)
        if p is None:
            return "singular", None, None
        res = kkt_verdicts(prob, rows, p)
        assert res is not None, rows
        v, mu = res
        if v == ["S"] * 4:
            return "certified", p, mu
        first = ["primal", "dual", "stationarity", "complementarity"][v.index("V")]
        return f"violated_{first}", p, mu

    # AC-2
    small = [(0, 0), (1, 1), (3, 1), (4, 0)]
    cands = [sorted(c) for c in combinations(sorted(small), 3)]
    outcomes = [decide(c) for c in cands]
    assert outcomes[0][0] == "violated_dual", outcomes[0][0]
    assert outcomes[1][0] == "certified"
    _, p, mu = outcomes[1]
    out.append("case ac2-tie kind zone dim 1")
    out.append("points " + " ".join(hexf(c) for q in pts for c in q))
    out.append("small " + fmt_rows(small))
    for c, o in zip(cands, outcomes):
        out.append(f"candidate {o[0]} " + fmt_rows(c))
    out.append("set " + fmt_rows(cands[1]))
    out.append("params " + " ".join(hexf(v) for v in p))
    out.append(f"value {hexf(p[-1])}")
    out.append("multipliers " + " ".join(hexf(v) for v in mu))
    out.append("end")

    # AC-3
    small = [(0, 0), (1, 1), (2, 1), (3, 1)]
    cands = [sorted(c) for c in combinations(sorted(small), 3)]
    outcomes = [decide(c) for c in cands]
    assert all(o[0] != "certified" for o in outcomes), [o[0] for o in outcomes]
    assert outcomes[0][0] == "violated_primal"
    out.append("case ac3-all-fail kind zone dim 1")
    out.append("points " + " ".join(hexf(c) for q in pts for c in q))
    out.append("small " + fmt_rows(small))
    for c, o in zip(cands, outcomes):
        out.append(f"candidate {o[0]} " + fmt_rows(c))
    out.append(f"tried {len(cands)}")
    out.append("end")


# ---------------------------------------------------------------- golden certificate


def next_down(x):
    """The next double below finite x (bit manipulation only)."""
    if x == 0.0:
        return -struct.unpack(">d", (1).to_bytes(8, "big"))[0]
    b = int.from_bytes(struct.pack(">d", x), "big")
    b = b - 1 if x > 0 else b + 1
    return struct.unpack(">d", b.to_bytes(8, "big"))[0]


def down(x):
    d = next_down(x)
    return 0.0 if d < 0 else d


def golden():
    """A synthetic, internally consistent fid.cert@1 certificate and its canonical bytes."""
    def h(x):
        return "0x" + bits(x)

    n = 3
    params = [float(Fraction(3, 8)), float(Fraction(-5, 4)), float(Fraction(13, 16))]
    mult = [0.25, 0.5, 0.25]
    pb = [2.0**-40] * n
    mb = [2.0**-40] * n
    data = 12.5
    tol = 1e-9
    thr_p = tol * data  # IEEE product of two doubles, as the rule states
    thr_s = tol * 1.0
    grad_lo = next_down(1.0)
    ghi = [1.25, 1.0, 1.5]
    dual_thr = [down(down(tol * grad_lo) / g) for g in ghi]
    # dual enclosure: lo = max(0, -min next_up(l + k)), hi = max(0, -min next_down(l - k));
    # every multiplier is positive, so both are +0.
    assert all(next_down(m - k) >= -t for m, k, t in zip(mult, mb, dual_thr))
    cond = {"hi": h(2.0**-41), "lo": h(-(2.0**-41)), "threshold": h(thr_p)}
    body = {
        "active_set": ["1:0", "4:1", "6:0"],
        "algorithm": "KRN.SOLV.CERT@1",
        "cond": h(17.25),
        "flags": {"local_optimum_only": "false"},
        "is_linear": "true",
        "kind": "zone",
        "kkt": {
            "complementarity": {"hi": h(2.0**-41), "lo": h(0.0), "threshold": h(thr_p)},
            "dual": {"hi": h(0.0), "lo": h(0.0), "threshold": h(min(dual_thr))},
            "dual_thresholds": [h(v) for v in dual_thr],
            "gradient_norms_hi": [h(v) for v in ghi],
            "primal": cond,
            "scales": {"data": h(data), "gradient": h(1.0), "gradient_lo": h(grad_lo)},
            # Stationarity is proven exact at the proven multipliers ([0, 0]); the residual at the
            # float multipliers is recorded for information (KRN.SOLV.CERT.KKT step 8).
            "stationarity": {"hi": h(0.0), "lo": h(0.0), "threshold": h(thr_s)},
            "stationarity_float": {"hi": h(2.0**-45), "lo": h(0.0), "threshold": h(thr_s)},
            "status": "certified",
            "tolerances": {"rank_tol": h(2.0**-50), "tol_c": h(tol), "tol_d": h(tol), "tol_p": h(tol),
                           "tol_s": h(tol)},
        },
        "multiplier_bounds": [h(v) for v in mb],
        "multipliers": [h(v) for v in mult],
        "param_bounds": [h(v) for v in pb],
        "params": [h(v) for v in params],
        "recomp": {"cond_max": h(1e12), "gn_steps": "8", "rel_tol": h(2.0**-50)},
        "schema": "fid.cert@1",
        "steps": "1",
        "thresholds": {"mult_rel": h(MULT_REL), "slack_rel": h(SLACK_REL)},
        "value": h(params[-1]),
        "value_bound": h(2.0**-40),
    }

    def canon(v):
        return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")

    digest = "sha256:" + hashlib.sha256(canon(body)).hexdigest()
    full = dict(body)
    full["digest"] = digest
    return canon(full), digest


# ---------------------------------------------------------------- output


def render():
    out = [
        "# KRN.SOLV.CERT.CT fixture ct-v1 (CC-BY-4.0). Generated by make_cases.py; do not edit.",
        "# 60 minimum-zone and envelope LPs with unique nondegenerate optima (exact brute force),",
        "# two seeded solver paths each, and the AC-2 tie and AC-3 all-fail cases. See make_cases.py.",
        f"thresholds {bits(SLACK_REL)} {bits(MULT_REL)}",
    ]
    for kind, dim, count in GROUPS:
        rng = random.Random(f"ct-v1/{kind}/{dim}")
        made = 0
        while made < PER_GROUP:
            pts = gen_points(rng, dim, count)
            prob = Lin(kind, dim, pts)
            res = optimum(prob)
            m = 2 * count
            perm = list(range(m))
            rng.shuffle(perm)
            scale = [rng.randint(-3, 3) for _ in range(m)]
            if res is None:
                continue
            s, p, mu = res
            emit_case(out, f"{kind}{dim}-{made:02d}", prob, s, p, mu, perm, scale)
            made += 1
    tie_cases(out)
    golden_bytes, golden_digest = golden()
    out.append(f"golden_digest {golden_digest}")
    return "\n".join(out) + "\n", golden_bytes


def main():
    text, golden_bytes = render()
    if "--check" in sys.argv[1:]:
        ok = OUT.exists() and OUT.read_text(encoding="utf-8") == text
        ok = ok and GOLDEN.exists() and GOLDEN.read_bytes() == golden_bytes
        if not ok:
            print(f"{OUT} or {GOLDEN} is stale; rerun without --check", file=sys.stderr)
            return 1
        print(f"{OUT} and {GOLDEN} are up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    GOLDEN.write_bytes(golden_bytes)
    print(f"wrote {OUT} and {GOLDEN}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
