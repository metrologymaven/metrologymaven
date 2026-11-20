#!/usr/bin/env python3
"""Independent reference for UNC.PROP.MCM.ADAPT fixtures (stdlib only; CC-BY-4.0 data).

Nothing here runs or imports the Rust code. Every f64 is written as its 16 hex digits of
IEEE-754 bits, with a decimal repr next to it for people. All block summaries in the
`summaries_*.json` files are hand-chosen synthetic numbers: they are neither measured
data nor simulated by the crate.

The rule `mcm-adapt@1` paraphrases the adaptive procedure of JCGM 101:2008 §7.9
[unverified: clause numbering]; it is restated here in code, independently of the crate:

- block size M = max(10 000, J), J the smallest integer >= 100 / (1 - p), with p taken as
  the decimal number it was written as (Python's shortest repr), in exact rationals;
- tolerance: u(y) rounded half up to n_dig significant digits is c 10^l with c an
  n_dig-digit integer (a carry to 10^n_dig raises l by one), delta = 10^l / 2; computed
  here by exact rational rounding (the crate uses exact comparisons instead);
- after block h >= 2: s_k^2 = sum_r (x_r - mean)^2 / (h (h - 1)) for each of estimate, u,
  low, high, in exact rationals; stop iff 2 s_k <= delta for all k, tested exactly as
  16 s_k^2 <= 10^(2 l); else cap_reached if (h + 1) M > cap; else continue.
  The expected s_k are the exact square roots rounded once to the nearest double (60-digit
  decimal sqrt). Every decision written is checked to be at least 1e-9 (relative) away
  from the stop boundary, except in `summaries_edge.json`, whose boundary steps are
  hand-labelled (h = 2, values whose s the crate computes without rounding).

Files:
  block_size.json        AC-4 and more: p -> M.
  tolerance.json         AC-3 and more: (u, n_dig) -> (l, delta); and exact tests of
                         2 s <= delta for doubles s next to the boundary.
  summaries_converge.json  AC-1: a sequence that stops at the expected block count.
  summaries_slow.json      AC-2: cap 30 000, M = 10 000: cap_reached after block 3.
  summaries_edge.json      boundary steps: 2 s == delta exactly (stop), one ulp above
                           (continue), the stop test passing on the last block the cap
                           allows (stop, not cap_reached), and u(y) = 0.
  ac_h_simulated.json    [simulated] AC-H1 reference simulation, written only by --simulate:
                         10 000 runs per example of the JCGM 101:2008 §7.9.4 rule as
                         restated above, with Python's own generator (random.Random,
                         Mersenne Twister; gauss for normal inputs), its own block
                         summaries (fsum mean and standard deviation, symmetric ranks
                         from the exact binary64 p) and the exact tolerance and stop test
                         of this file. It never runs or imports the crate. Per example: the
                         count of runs whose final estimate, u(y), lower and upper endpoint
                         (from all trials) lie within delta of the analytic value, runs
                         stopping at h = 2, cap hits and the median block count. About
                         47 minutes; not run in CI (--check reads the stored file).
  ac_h_reference.json    AC-H1 and AC-H2 (as restated in the leaf's NODE.md): analytic
                         output distributions of the additive model Y = X1 + X2 + X3 + X4
                         of JCGM 101:2008 [unverified: clause 9.2] with standard normal
                         inputs (normal4) and with rectangular inputs of unit standard
                         deviation (rect4); estimate 0, u(y) = 2, symmetric 95 % endpoints,
                         the predicted per-block standard deviation of each block statistic
                         (estimate sigma / sqrt(M); u by sigma sqrt((kappa - 1) / (4 M)),
                         kappa the kurtosis; endpoint sqrt(q (1 - q) / M) / f(x_q)), the
                         predicted stopping block h* = max(2, ceil(max_k (2 sigma_k /
                         delta)^2)) for AC-H2; for AC-H1 the [simulated] predicted rates
                         pred_* from ac_h_simulated.json and, for the crate's 1000 seeds,
                         the acceptance band band_* of the two-sided exact conditional
                         (Fisher) binomial comparison at level 0.01 between the crate's
                         count and the reference count (it accounts for the reference's
                         own simulation error; a one-sample test against a predicted rate
                         of exactly 1 would reject a single miss). Clopper-Pearson
                         two-sided 95 % bounds for k = 800..1000 (6 decimals) for reporting.
                         Each AC-H1 quantity is a pass/fail count per seed, so binomial
                         models are the right ones. Decimal strings with 12 significant
                         digits, so the file is the same on every platform.

Usage: make_fixtures.py [--simulate] [--check]   (--check compares with the files on disk, writes
nothing; --simulate also reruns the reference simulation)
"""

import math
import random
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path
from statistics import NormalDist

HERE = Path(__file__).parent
getcontext().prec = 60
LICENSE = "CC-BY-4.0"
# Seeds per example at the hypothesis scale of AC-H1 and AC-H2 (as restated).
N_SEEDS = 1000


def hexbits(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def next_up(x):
    return math.nextafter(x, math.inf)


def next_down(x):
    return math.nextafter(x, -math.inf)


def q(x):
    return '"%s"' % hexbits(x)


def sig12(x):
    return "%.12g" % x


# --------------------------------------------------------------------------- block size


def block_size(p):
    pd = Fraction(Decimal(repr(p)))
    assert 0 < pd < 1
    j = math.ceil(Fraction(100) / (1 - pd))
    return max(10_000, j)


def block_size_file():
    ps = [0.5, 0.6827, 0.9, 0.95, 0.9545, 0.99, 0.995, 0.996, 0.9973, 0.999, 0.9999, 0.99999,
          0.9999999999999999, 1e-20, 5e-324, 0.25]
    want = {0.99: 10_000, 0.9999: 1_000_000, 0.95: 10_000, 0.995: 20_000, 0.999: 100_000}
    lines = []
    for p in ps:
        m = block_size(p)
        if p in want:
            assert m == want[p], (p, m)
        lines.append('    {"p": %s, "p_repr": "%r", "block_size": %d}' % (q(p), p, m))
    return (
        '{\n  "title": "UNC.PROP.MCM.ADAPT block size M = max(10 000, ceil(100 / (1 - p))), p as written",\n'
        '  "license": "%s",\n  "cases": [\n%s\n  ]\n}\n' % (LICENSE, ",\n".join(lines))
    )


# --------------------------------------------------------------------------- tolerance


def round_half_up(fr):
    return math.floor(fr + Fraction(1, 2))


def tolerance(u, n):
    """(l, delta) for u > 0, or (None, 0.0) for u == 0."""
    uf = Fraction(u)
    assert uf >= 0
    if uf == 0:
        return None, 0.0
    # Decimal exponent by search on exact powers of ten.
    e = 0
    while Fraction(10) ** e > uf:
        e -= 1
    while Fraction(10) ** (e + 1) <= uf:
        e += 1
    l = e - n + 1
    c = round_half_up(uf / Fraction(10) ** l)
    assert 10 ** (n - 1) <= c <= 10 ** n
    if c == 10 ** n:
        l += 1
    return l, float(Fraction(10) ** l / 2)


def admits(s, l):
    return 16 * Fraction(s) ** 2 <= Fraction(10) ** (2 * l)


def tolerance_file():
    us = [
        (0.0123456, 2),
        (1.0, 2), (2.0, 2), (0.99, 2), (0.995, 2), (next_down(0.995), 2), (next_up(0.995), 2),
        (0.0995, 2), (9.95, 2), (99.5, 2), (next_down(99.5), 2), (99.5, 1), (99.5, 3),
        (0.001, 2), (next_down(0.001), 2), (next_up(0.001), 2), (1e22, 2), (next_down(1e23), 2),
        (1e23, 2), (123456789.0, 1), (123456789.0, 17), (0.1, 17), (1.0 / 3.0, 3),
        (1e-300, 2), (5e-324, 2), (2.2250738585072014e-308, 2), (1.7976931348623157e308, 2),
        (1.7976931348623157e308, 1), (0.0, 2), (3.0, 1), (2.0, 1),
    ]
    lines = []
    for u, n in us:
        l, d = tolerance(u, n)
        if (u, n) == (0.0123456, 2):
            assert l == -3 and d == 0.0005  # AC-3
        if (u, n) == (99.5, 2):
            assert l == 1  # 99.5 -> 100 (carry): 10 x 10^1
        el = "null" if l is None else str(l)
        lines.append('    {"u": %s, "u_repr": "%r", "n_dig": %d, "exp10": %s, "delta": %s}' % (q(u), u, n, el, q(d)))
    adm = []
    # Each case gives the tolerance through (u, n_dig), the only way the crate builds one;
    # u = 3 10^l at n_dig = 1 (or an extreme u) yields the wanted l.
    MAX = 1.7976931348623157e308
    for s, u, n, want_l in [
        (0.025, 0.3, 1, -1), (next_down(0.025), 0.3, 1, -1), (0.25, 3.0, 1, 0), (next_up(0.25), 3.0, 1, 0),
        (next_down(0.25), 3.0, 1, 0), (25.0, 300.0, 1, 2), (next_up(25.0), 300.0, 1, 2), (0.0, 3e-5, 1, -5),
        (5e-324, 5e-324, 17, -340), (MAX, MAX, 1, 308), (2.5e307, MAX, 1, 308), (next_up(2.5e307), MAX, 1, 308),
        (2.5e-4, 3e-3, 1, -3), (next_down(2.5e-4), 3e-3, 1, -3), (2.5e21, 3e22, 1, 22), (next_up(2.5e21), 3e22, 1, 22),
    ]:
        l, _ = tolerance(u, n)
        assert l == want_l, (u, n, l)
        adm.append(
            '    {"s": %s, "s_repr": "%r", "u": %s, "n_dig": %d, "exp10": %d, "admits": %s}'
            % (q(s), s, q(u), n, l, "true" if admits(s, l) else "false")
        )
    assert not admits(0.025, -1)  # the binary64 0.025 is above 1/40
    return (
        '{\n  "title": "UNC.PROP.MCM.ADAPT numerical tolerance delta = 10^l / 2 from n_dig digits of u(y)",\n'
        '  "license": "%s",\n  "cases": [\n%s\n  ],\n  "admits": [\n%s\n  ]\n}\n'
        % (LICENSE, ",\n".join(lines), ",\n".join(adm))
    )


# --------------------------------------------------------------------------- sequences


def sqrt_round(fr):
    """Nearest double to sqrt(fr) for a non-negative Fraction (60-digit decimal sqrt)."""
    if fr == 0:
        return 0.0
    d = (Decimal(fr.numerator) / Decimal(fr.denominator)).sqrt()
    x = float(d)
    # One correction step against the exact value at the half-way points.
    for _ in range(3):
        lo, hi = Fraction(next_down(x)), Fraction(next_up(x))
        xf = Fraction(x)
        if ((xf + hi) / 2) ** 2 < fr:
            x = next_up(x)
        elif ((xf + lo) / 2) ** 2 > fr:
            x = next_down(x)
        else:
            break
    return x


def step(blocks, h, u_all, n, m, cap, boundary_ok=False):
    if h == 1:
        return {"decision": "continue", "exp10": None, "delta": None, "sd": None}
    l, d = tolerance(u_all, n)
    sds = []
    ok = True
    for k in range(4):
        xs = [Fraction(b[k]) for b in blocks[:h]]
        mean = sum(xs) / h
        s2 = sum((x - mean) ** 2 for x in xs) / (h * (h - 1))
        sds.append(sqrt_round(s2))
        if l is None:
            ok = ok and s2 == 0
            continue
        lhs, rhs = 16 * s2, Fraction(10) ** (2 * l)
        if not boundary_ok and lhs != 0:
            assert abs(lhs / rhs - 1) > Fraction(1, 10 ** 9), ("too close to the boundary", h, k)
        ok = ok and lhs <= rhs
    if ok:
        dec = "stop"
    elif (h + 1) * m > cap:
        dec = "cap_reached"
    else:
        dec = "continue"
    return {"decision": dec, "exp10": l, "delta": d, "sd": sds}


def sequence_file(title, p, n, cap, blocks, u_alls, boundary_ok=False, note=""):
    m = block_size(p)
    blines = []
    for b in blocks:
        blines.append(
            '    {"y_mean": %s, "u": %s, "low": %s, "high": %s, "repr": [%s]}'
            % (q(b[0]), q(b[1]), q(b[2]), q(b[3]), ", ".join('"%r"' % x for x in b))
        )
    slines = []
    decisions = []
    for h in range(1, len(blocks) + 1):
        st = step(blocks, h, u_alls[h - 1], n, m, cap, boundary_ok)
        decisions.append(st["decision"])
        sd = "null" if st["sd"] is None else "[%s]" % ", ".join(q(s) for s in st["sd"])
        el = "null" if st["exp10"] is None else str(st["exp10"])
        de = "null" if st["delta"] is None else q(st["delta"])
        slines.append(
            '    {"h": %d, "u_all": %s, "u_all_repr": "%r", "decision": "%s", "exp10": %s, "delta": %s, "sd": %s}'
            % (h, q(u_alls[h - 1]), u_alls[h - 1], st["decision"], el, de, sd)
        )
        if st["decision"] != "continue":
            break
    text = (
        '{\n  "title": "%s",\n  "license": "%s",\n'
        '  "provenance": "hand-chosen synthetic block summaries: not measured, not simulated by the crate",\n'
        '  "note": "%s",\n'
        '  "profile": {"p": %s, "p_repr": "%r", "n_dig": %d, "cap": %d, "block_size": %d},\n'
        '  "blocks": [\n%s\n  ],\n  "steps": [\n%s\n  ]\n}\n'
        % (title, LICENSE, note, q(p), p, n, cap, m, ",\n".join(blines), ",\n".join(slines))
    )
    return text, decisions


def converge():
    # Columns: estimate, u, low, high. Endpoint spread shrinks slowly; u(y) stays near 1.0
    # (tolerance 0.05); step 3 has u(y) = 0.9951, which rounds to 1.0 at two digits (carry).
    blocks = [
        (10.012, 1.004, 8.060, 11.940),
        (9.981, 0.997, 7.930, 12.070),
        (10.004, 1.001, 8.050, 11.960),
        (9.995, 0.999, 7.980, 12.030),
        (10.003, 1.000, 8.010, 12.000),
        (10.001, 1.002, 7.990, 11.990),
        (9.999, 0.998, 8.000, 12.010),
        (10.000, 1.000, 8.000, 12.000),
    ]
    u_alls = [1.004, 1.0005, 0.9951, 1.0003, 1.0004, 1.0001, 1.0002, 1.0]
    text, dec = sequence_file(
        "UNC.PROP.MCM.ADAPT AC-1: converging sequence", 0.95, 2, 1_000_000, blocks, u_alls,
        note="stops at the first block h >= 2 where 2 s <= delta for all four quantities",
    )
    stop_at = dec.index("stop") + 1
    assert dec[: stop_at - 1] == ["continue"] * (stop_at - 1)
    assert stop_at >= 4, stop_at
    return text, stop_at


def slow():
    blocks = [
        (10.00, 1.00, 8.00, 12.00),
        (10.30, 1.06, 7.70, 12.40),
        (9.70, 0.95, 8.30, 11.60),
    ]
    u_alls = [1.0, 1.02, 1.0]
    text, dec = sequence_file(
        "UNC.PROP.MCM.ADAPT AC-2: slow sequence, cap 30 000", 0.95, 2, 30_000, blocks, u_alls,
        note="the next block would exceed the cap after block 3: cap_reached, not_converged",
    )
    assert dec == ["continue", "continue", "cap_reached"], dec
    return text


def edge():
    # u(y) = 3.0 at n_dig = 1: c = 3, l = 0, delta = 0.5. Two blocks whose estimates differ
    # by exactly 0.5 give s = 0.25 and 2 s = delta: stop. The crate computes s = 0.25
    # exactly here (deviations +-0.25, norm2 = 0.25 sqrt(2) rounded, divided by the same
    # rounded sqrt(2)).
    eq_blocks = [(10.0, 3.0, 4.0, 16.0), (10.5, 3.0, 4.0, 16.0)]
    eq, d1 = sequence_file("UNC.PROP.MCM.ADAPT edge: 2 s == delta", 0.95, 1, 1_000_000, eq_blocks, [3.0, 3.0],
                           boundary_ok=True, note="stop at h = 2 with 2 s exactly equal to delta")
    assert d1 == ["continue", "stop"], d1
    up_blocks = [(10.0, 3.0, 4.0, 16.0), (next_up(10.5), 3.0, 4.0, 16.0)]
    up, d2 = sequence_file("UNC.PROP.MCM.ADAPT edge: 2 s one ulp above delta", 0.95, 1, 1_000_000, up_blocks,
                           [3.0, 3.0], boundary_ok=True, note="continue at h = 2: 2 s exceeds delta")
    assert d2 == ["continue", "continue"], d2
    # Stop test passing on the last block the cap allows: stop, not cap_reached.
    last_blocks = [(10.0, 1.0, 8.0, 12.0), (10.01, 1.0, 8.01, 12.01)]
    last, d3 = sequence_file("UNC.PROP.MCM.ADAPT edge: converged on the last allowed block", 0.95, 2, 20_000,
                             last_blocks, [1.0, 1.0], note="cap 2 M: converged at h = 2 is stop")
    assert d3 == ["continue", "stop"], d3
    # u(y) = 0 (a constant output): delta = 0, stop only when every s is zero.
    zero_blocks = [(5.0, 0.0, 5.0, 5.0), (5.0, 0.0, 5.0, 5.0)]
    zero, d4 = sequence_file("UNC.PROP.MCM.ADAPT edge: constant output", 0.95, 2, 1_000_000, zero_blocks,
                             [0.0, 0.0], note="u(y) = 0: delta = 0 and every s = 0, stop")
    assert d4 == ["continue", "stop"], d4
    parts = [eq, up, last, zero]
    return '{\n  "cases": [\n' + ",\n".join("    " + p.strip().replace("\n", "\n    ") for p in parts) + "\n  ]\n}\n"


# --------------------------------------------------------------------------- AC-H


def irwin_hall4_cdf(s):
    c = [1, -4, 6, -4, 1]
    return sum(c[k] * (s - k) ** 4 for k in range(5) if s > k) / 24.0


def irwin_hall4_pdf(s):
    c = [1, -4, 6, -4, 1]
    return sum(c[k] * (s - k) ** 3 for k in range(5) if s > k) / 6.0


def bisect(fn, target, lo, hi):
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if fn(mid) < target:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def upper_tail(k, n, p):
    """P(X >= k) for X ~ Binomial(n, p), p in [0.5, 1), summed from i = n downwards with the
    ratio recursion (IEEE +, -, *, / only; p^n >= 2^-1000 stays normal)."""
    x = 1.0
    for _ in range(n):
        x *= p
    total = 0.0
    for i in range(n, k - 1, -1):
        total += x
        x = x * i / (n - i + 1) * (1 - p) / p
    return total


def cdf_float(k, n, p):
    return 1.0 - upper_tail(k + 1, n, p)


def bisect_p(fn, target):
    lo, hi = 0.5, 1.0
    for _ in range(60):
        mid = 0.5 * (lo + hi)
        if fn(mid) < target:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def clopper_pearson(k, n, alpha=0.05):
    """Exact (Clopper-Pearson) two-sided bounds, for k / n >= 0.6 (bisection on [0.5, 1])."""
    lo = bisect_p(lambda p: upper_tail(k, n, p), alpha / 2)
    hi = 1.0 if k == n else bisect_p(lambda p: -cdf_float(k, n, p), -alpha / 2)
    return lo, hi


def fisher_accepts(k, n, ref_k, ref_n, alpha=Decimal("0.01")):
    """Two-sided exact conditional (Fisher) test that the crate's k of n and the reference
    simulation's ref_k of ref_n come from the same pass rate. The reference rate is itself a
    simulated estimate, so this is the exact binomial comparison that accounts for its
    uncertainty; with ref_n -> infinity it becomes the one-sample exact binomial test.
    p-value = sum of the hypergeometric probabilities no larger than that of k (relative
    tie tolerance 1e-30); accept iff p > alpha. Hypergeometric weights by the ratio
    recursion in Decimal at 60 digits (correctly rounded, the same on every platform)."""
    t = k + ref_k
    lo, hi = max(0, t - ref_n), min(n, t)
    w = []
    x_w = Decimal(1)
    for x in range(lo, hi + 1):
        w.append(x_w)
        if x < hi:
            x_w = x_w * (n - x) * (t - x) / ((x + 1) * (ref_n - t + x + 1))
    wk = w[k - lo] * (1 + Decimal("1e-30"))
    total = sum(w)
    p = sum(v for v in w if v <= wk) / total
    return p > alpha


def acceptance_band(n, ref_k, ref_n):
    """The contiguous set of crate counts k that fisher_accepts, found by scanning outward
    from the reference rate; the two counts beyond each end are checked to be rejected."""
    centre = min(n, max(0, round(n * ref_k / ref_n)))
    assert fisher_accepts(centre, n, ref_k, ref_n)
    lo = centre
    while lo > 0 and fisher_accepts(lo - 1, n, ref_k, ref_n):
        lo -= 1
    hi = centre
    while hi < n and fisher_accepts(hi + 1, n, ref_k, ref_n):
        hi += 1
    for k in (lo - 1, lo - 2, hi + 1, hi + 2):
        if 0 <= k <= n:
            assert not fisher_accepts(k, n, ref_k, ref_n), k
    return lo, hi


# --------------------------------------------------------------------------- reference simulation

QUANTITIES = ["y", "u", "sym_low", "sym_high"]
SIM_FILE = "ac_h_simulated.json"
# Runs per example of the independent reference simulation (python random, not the crate).
N_REF = 10_000
SIM_SEED = {"normal4": 0x5EED_0000, "rect4": 0x5EED_1000_0000}


def sym_ranks(n, p=0.95):
    q = math.floor(Fraction(p) * n + Fraction(1, 2))
    r = (n - q + 1) // 2
    return r, r + q


def block_stats(v):
    n = len(v)
    s = sorted(v)
    mean = math.fsum(s) / n
    u = math.sqrt(math.fsum((x - mean) ** 2 for x in s) / (n - 1))
    r_lo, r_hi = sym_ranks(n)
    return mean, u, s[r_lo - 1], s[r_hi - 1]


def spread(xs):
    h = len(xs)
    mean = math.fsum(xs) / h
    return math.sqrt(math.fsum((x - mean) ** 2 for x in xs) / (h * (h - 1)))


def sim_run(example_name, run, refs, m=10_000, cap=1_000_000):
    """One run of the JCGM 101:2008 §7.9.4 procedure as restated in this file's docstring,
    with Python's Mersenne Twister as the generator. Returns (passes per quantity, h)."""
    rnd = random.Random(SIM_SEED[example_name] + run)
    if example_name == "normal4":
        g = rnd.gauss
        draw = lambda: ((g(0.0, 1.0) + g(0.0, 1.0)) + g(0.0, 1.0)) + g(0.0, 1.0)  # noqa: E731
    else:
        a = math.sqrt(3.0)
        r = rnd.random
        draw = lambda: (((2 * r() - 1) * a + (2 * r() - 1) * a) + (2 * r() - 1) * a) + (2 * r() - 1) * a  # noqa: E731
    blocks = []
    all_values = []
    while True:
        v = [draw() for _ in range(m)]
        blocks.append(block_stats(v))
        all_values.extend(v)
        h = len(blocks)
        n = len(all_values)
        mean_all = math.fsum(all_values) / n
        u_all = math.sqrt(math.fsum((x - mean_all) ** 2 for x in all_values) / (n - 1))
        if h == 1:
            continue
        l, delta = tolerance(u_all, 2)
        ok = all(admits(spread([b[k] for b in blocks]), l) for k in range(4))
        if ok or (h + 1) * m > cap:
            break
    fin = block_stats(all_values)
    passes = [abs(fin[k] - refs[k]) <= delta for k in range(4)]
    return passes, h, ok


def simulate(refs):
    out = {}
    for name in ("normal4", "rect4"):
        counts = [0, 0, 0, 0]
        h2 = 0
        caps = 0
        hs = []
        for run in range(N_REF):
            passes, h, ok = sim_run(name, run, refs[name])
            for k in range(4):
                counts[k] += passes[k]
            h2 += h == 2
            caps += not ok
            hs.append(h)
        hs.sort()
        out[name] = {"counts": counts, "stop_h2": h2, "caps": caps, "median_h": hs[N_REF // 2]}
        print(name, out[name], flush=True)
    lines = []
    for name, d in out.items():
        lines.append(
            '    "%s": {%s, "stop_at_h2": %d, "cap_reached": %d, "median_blocks": %d}'
            % (name, ", ".join('"%s": %d' % (q_, c) for q_, c in zip(QUANTITIES, d["counts"])), d["stop_h2"],
               d["caps"], d["median_h"])
        )
    return (
        '{\n  "title": "UNC.PROP.MCM.ADAPT AC-H1 reference simulation of the JCGM 101:2008 7.9.4 rule",\n'
        '  "license": "%s",\n'
        '  "provenance": "[simulated] Python random (Mersenne Twister) runs by make_fixtures.py --simulate; '
        'independent of the crate; not measured",\n'
        '  "runs": %d,\n  "counts_within_delta": {\n%s\n  }\n}\n' % (LICENSE, N_REF, ",\n".join(lines))
    )


def read_simulated():
    text = (HERE / SIM_FILE).read_text(encoding="utf-8")
    runs = int(text.split('"runs": ')[1].split(",")[0])
    res = {}
    for name in ("normal4", "rect4"):
        line = text.split('"%s": {' % name)[1].split("}")[0]
        res[name] = [int(line.split('"%s": ' % q_)[1].split(",")[0]) for q_ in QUANTITIES]
    return runs, res


def example(name, low, high, f_low, f_high, sigma, kurt, m, l, sim):
    delta = 10.0 ** l / 2
    sig = [
        sigma / math.sqrt(m),
        sigma * math.sqrt((kurt - 1) / (4 * m)),
        math.sqrt(0.025 * 0.975 / m) / f_low,
        math.sqrt(0.025 * 0.975 / m) / f_high,
    ]
    h = max(2, math.ceil(max((2 * s / delta) ** 2 for s in sig)))
    runs, counts = sim
    pred = []
    bands = []
    for q_, c in zip(QUANTITIES, counts[name]):
        lo, hi = acceptance_band(N_SEEDS, c, runs)
        pred.append('"pred_%s": "%.4f"' % (q_, c / runs))
        bands.append('"band_%s": [%d, %d]' % (q_, lo, hi))
    return (
        '    "%s": {"y": "0", "u": "%s", "sym_low": "%s", "sym_high": "%s", "sigma_block": [%s], "exp10": %d, '
        '"h_pred": %d, "trials_pred": %d, %s, %s}'
        % (name, sig12(sigma), sig12(low), sig12(high), ", ".join('"%s"' % sig12(s) for s in sig), l, h, h * m,
           ", ".join(pred), ", ".join(bands))
    )


def analytic_refs():
    z = NormalDist().inv_cdf(0.975)
    phi = math.exp(-z * z / 2) / math.sqrt(2 * math.pi)
    sc = 2 * math.sqrt(3.0)
    s_hi = bisect(irwin_hall4_cdf, 0.975, 0.0, 4.0)
    y_hi = sc * s_hi - 2 * sc
    f_hi = irwin_hall4_pdf(s_hi) / sc
    return {
        "normal4": {"low": -2 * z, "high": 2 * z, "f": phi / 2, "kurt": 3.0},
        "rect4": {"low": -y_hi, "high": y_hi, "f": f_hi, "kurt": 3.0 - 0.3},
    }


def ac_h_reference(sim):
    lo, hi = clopper_pearson(100, 100)
    assert abs(lo - 0.025 ** (1 / 100)) < 1e-9 and hi == 1.0
    m = block_size(0.95)
    assert m == 10_000
    l, _ = tolerance(2.0, 2)
    assert l == -1
    refs = analytic_refs()
    ex = []
    for name in ("normal4", "rect4"):
        r = refs[name]
        ex.append(example(name, r["low"], r["high"], r["f"], r["f"], 2.0, r["kurt"], m, l, sim))
    first = N_SEEDS * 8 // 10
    cp = ", ".join('["%.6f", "%.6f"]' % clopper_pearson(k, N_SEEDS) for k in range(first, N_SEEDS + 1))
    return (
        '{\n  "title": "UNC.PROP.MCM.ADAPT AC-H1/AC-H2 references (p = 0.95, n_dig = 2, M = 10 000)",\n'
        '  "license": "%s",\n'
        '  "provenance": "closed-form distributions and asymptotic standard errors; pred_* are [simulated] rates '
        'from %s (%d runs per example, independent of the crate); not measured",\n'
        '  "examples": {\n%s,\n%s\n  },\n'
        '  "seeds": %d,\n  "band_alpha": "0.01",\n'
        '  "clopper_pearson_95_first_k": %d,\n  "clopper_pearson_95": [%s]\n}\n'
        % (LICENSE, SIM_FILE, sim[0], ex[0], ex[1], N_SEEDS, first, cp)
    )


def main(argv):
    check = "--check" in argv[1:]
    files = {}
    if "--simulate" in argv[1:]:
        r = analytic_refs()
        refs = {n: [0.0, 2.0, r[n]["low"], r[n]["high"]] for n in r}
        files[SIM_FILE] = simulate(refs)
        if not check:
            (HERE / SIM_FILE).write_text(files[SIM_FILE], encoding="utf-8", newline="\n")
    conv, stop_at = converge()
    files.update({
        "block_size.json": block_size_file(),
        "tolerance.json": tolerance_file(),
        "summaries_converge.json": conv,
        "summaries_slow.json": slow(),
        "summaries_edge.json": edge(),
        "ac_h_reference.json": ac_h_reference(read_simulated()),
    })
    if not check:
        print(f"AC-1 sequence stops after block {stop_at}")
    bad = 0
    for name, text in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"stale: {name}")
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
