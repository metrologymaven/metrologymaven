#!/usr/bin/env python3
"""Independent reference for KRN.NUM.RED fixed-order reductions (CC-BY-4.0 project data).

Writes
  fixtures/num/red-illcond-v1.txt  seeded ill-conditioned sums (condition 1e3 .. 1e20)
  fixtures/num/red-edge-v1.txt     hand-chosen adversarial cases for sum, dot, sum_sq,
                                   norm2 and mean, plus one 10^6-element generated sum

Two independent parts, neither of which runs the Rust code (stdlib only):

1. An exact oracle with `fractions.Fraction`: every sum is exact, then rounded once to
   binary64 (`float(Fraction)` is correctly rounded, subnormals included). The norm oracle
   is the correctly rounded square root of the exact sum of squares (integer `isqrt` with a
   sticky bit). The oracle knows nothing about blocks, trees or compensation.
2. A bit-exact Python model of the documented rule (256-element blocks, Neumaier in index
   order, pairwise tree of recursive halves, `s if c == 0 else s + c`). CPython floats are
   binary64 with round-to-nearest-even and never fuse a multiply-add, so the model's bit
   patterns are the expected Rust results on every target.

Before anything is written the script asserts that the model is within the stated ulp bound
of the oracle for every case (2 ulp for sums with condition number <= 1e16, AC-1).

Run:  python fixtures/num/make_red_fixtures.py          (writes both fixtures)
      python fixtures/num/make_red_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
BLOCK = 256
MAX = sys.float_info.max
TINY = 5e-324
INF = math.inf
NAN = math.nan
MASK64 = (1 << 64) - 1


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def fb(u):
    return struct.unpack("<d", struct.pack("<Q", u & MASK64))[0]


def hx(x):
    return f"{bits(x):016x}"


def ordered(x):
    """Maps a double to an integer so that adjacent doubles differ by 1 (+0 and -0 both 0)."""
    b = bits(x)
    return -(b & ~(1 << 63)) if b >> 63 else b


def ulp_dist(a, b):
    return abs(ordered(a) - ordered(b))


# ---------------------------------------------------------------- 1. exact oracle


def exact_round(q):
    """Correctly rounded binary64 of an exact rational, or None on overflow."""
    try:
        return float(q)
    except OverflowError:
        return None


def all_neg_zero(xs):
    return len(xs) > 0 and all(bits(x) == bits(-0.0) for x in xs)


def oracle_sum(terms):
    """Exact sum of the (already rounded) terms; IEEE sign rule for a zero result."""
    s = sum((Fraction(t) for t in terms), Fraction(0))
    if s == 0:
        return -0.0 if all_neg_zero(terms) else 0.0
    return exact_round(s)


def sqrt_round(q):
    """Correctly rounded binary64 square root of a non-negative rational."""
    if q == 0:
        return 0.0
    k = 80 - (q.numerator.bit_length() - q.denominator.bit_length()) // 2
    scaled = q * Fraction(4) ** k
    n = scaled.numerator // scaled.denominator
    r = math.isqrt(n)
    exact = r * r == n and scaled.denominator == 1
    assert r.bit_length() > 60
    return exact_round(Fraction(2 * r + (0 if exact else 1)) / Fraction(2) ** (k + 1))


def oracle_norm2(xs):
    return sqrt_round(sum((Fraction(x) ** 2 for x in xs), Fraction(0)))


# ---------------------------------------------------------------- 2. model of the rule


def block_sum(terms):
    s, c = -0.0, -0.0
    for x in terms:
        t = s + x
        if abs(s) >= abs(x):
            c = c + ((s - t) + x)
        else:
            c = c + ((x - t) + s)
        s = t
    return s, c


def combine(p, q):
    s1, c1 = p
    s2, c2 = q
    s = s1 + s2
    e = (s1 - s) + s2 if abs(s1) >= abs(s2) else (s2 - s) + s1
    return s, (c1 + c2) + e


def tree(pairs):
    if len(pairs) == 1:
        return pairs[0]
    left = len(pairs) // 2
    return combine(tree(pairs[:left]), tree(pairs[left:]))


def finite(x):
    return not (math.isnan(x) or math.isinf(x))


def model_fixed_sum(terms):
    if not terms:
        return ("ok", 0.0)
    pairs = [block_sum(terms[i:i + BLOCK]) for i in range(0, len(terms), BLOCK)]
    s, c = tree(pairs)
    if not (finite(s) and finite(c)):
        return ("err", "Infinite", None)
    r = s if c == 0 else s + c
    return ("ok", r) if finite(r) else ("err", "Infinite", None)


def scan(xs):
    for i, x in enumerate(xs):
        if math.isnan(x):
            return ("err", "NotANumber", i)
        if math.isinf(x):
            return ("err", "Infinite", i)
    return None


def scan_products(ps):
    for i, p in enumerate(ps):
        if not finite(p):
            return ("err", "Infinite", i)
    return None


def model(op, a, b):
    if op == "sum":
        return scan(a) or model_fixed_sum(a)
    if op == "dot":
        if len(a) != len(b):
            return ("err", "LengthMismatch", (len(a), len(b)))
        ps = [x * y for x, y in zip(a, b)]
        return scan(a) or scan(b) or scan_products(ps) or model_fixed_sum(ps)
    if op == "sum_sq":
        ps = [x * x for x in a]
        return scan(a) or scan_products(ps) or model_fixed_sum(ps)
    if op == "norm2":
        e = scan(a)
        if e:
            return e
        m = max((abs(x) for x in a), default=0.0)
        if m == 0:
            return ("ok", 0.0)
        r = model_fixed_sum([(x / m) * (x / m) for x in a])
        if r[0] == "err":
            return r
        y = m * math.sqrt(r[1])
        return ("ok", y) if finite(y) else ("err", "Infinite", None)
    if op == "mean":
        if not a:
            return ("err", "EmptyInput", None)
        r = scan(a) or model_fixed_sum(a)
        return r if r[0] == "err" else ("ok", r[1] / len(a))
    raise ValueError(op)


def oracle(op, a, b):
    """Exact result of what each reduction is defined to compute (None if it overflows)."""
    if op == "sum":
        return oracle_sum(a)
    if op == "dot":
        return oracle_sum([x * y for x, y in zip(a, b)])
    if op == "sum_sq":
        return oracle_sum([x * x for x in a])
    if op == "norm2":
        return oracle_norm2(a)
    if op == "mean":
        s = sum((Fraction(x) for x in a), Fraction(0))
        if s == 0:
            return -0.0 if all_neg_zero(a) else 0.0
        return exact_round(s / len(a))
    raise ValueError(op)


# Accuracy bounds of the model against the oracle, in ulps (adjacent-double steps).
BOUND = {"sum": 2, "dot": 2, "sum_sq": 2, "mean": 2, "norm2": 3}

# ---------------------------------------------------------------- ill-conditioned sums


def gen_sum(n, cond, rng):
    """Ogita-Rump-Oishi style generator: about half the terms spread over exponents 0..b
    (b = log2 cond), the rest chosen to cancel the running exact sum with decreasing
    exponents, then shuffled. The exact sum is known because it is tracked as a Fraction."""
    b = math.log2(cond)
    n1 = n // 2
    exps = [round(rng.random() * b) for _ in range(n1)]
    exps[0], exps[-1] = round(b), 0
    xs = [(2 * rng.random() - 1) * 2.0 ** e for e in exps]
    s = sum((Fraction(x) for x in xs), Fraction(0))
    rest = n - n1
    for i in range(rest):
        e = round(b * (rest - 1 - i) / max(rest - 1, 1))
        x = (2 * rng.random() - 1) * 2.0 ** e - float(s)
        xs.append(x)
        s += Fraction(x)
    rng.shuffle(xs)
    return xs


def exact_ulp_error(r, xs):
    """|r - S| / ulp(S) with S the exact sum (ulp of the binade of |S|, normal range)."""
    s = sum((Fraction(x) for x in xs), Fraction(0))
    if s == 0:
        return 0.0 if r == 0 else math.inf
    a = abs(s)
    e = a.numerator.bit_length() - a.denominator.bit_length()
    if Fraction(2) ** e > a:
        e -= 1
    ulp = Fraction(2) ** max(e - 52, -1074)
    return float(abs(Fraction(r) - s) / ulp)


def make_illcond():
    rng = random.Random(20261118)
    out = [
        "# KRN.NUM.RED AC-1, AC-2: seeded ill-conditioned sums (Ogita-Rump-Oishi style), CC-BY-4.0.",
        "# Written by make_red_fixtures.py; do not edit. One `case` line per sum, followed by its",
        "# terms as binary64 bit patterns (hex, 8 per line). cond = sum|x| / |sum x| (measured);",
        "# exact = correctly rounded exact sum; ref = result of the documented block-and-tree rule;",
        "# err_ulp = |ref - exact sum| / ulp(exact sum). `digest` = SHA-256 of the little-endian",
        "# bit patterns of every ref, in case order.",
    ]
    body = []
    h = hashlib.sha256()
    for target in [1e3, 1e6, 1e9, 1e12, 1e13, 1e14, 3e14, 5e14, 7e14, 1e15, 3e15, 1e16, 1e18, 1e20]:
        for n in [100, 600, 1100]:
            xs = gen_sum(n, target, rng)
            s_exact = sum((Fraction(x) for x in xs), Fraction(0))
            cond = float(sum((abs(Fraction(x)) for x in xs), Fraction(0)) / abs(s_exact))
            kind, ref = model_fixed_sum(xs)
            assert kind == "ok"
            err = exact_ulp_error(ref, xs)
            if cond <= 1e16:
                assert err <= 2.0, (target, n, cond, err)
            h.update(struct.pack("<Q", bits(ref)))
            body.append(
                f"case c{target:.0e}-n{n} cond {cond:.3e} n {n} exact {hx(float(s_exact))} "
                f"ref {hx(ref)} err_ulp {err:.3f}".replace("+", "")
            )
            for i in range(0, n, 8):
                body.append(" ".join(hx(x) for x in xs[i:i + 8]))
    out.append(f"digest sha256:{h.hexdigest()}")
    return "\n".join(out + body) + "\n"


# ---------------------------------------------------------------- adversarial cases


def splitmix64(state):
    state = (state + 0x9E3779B97F4A7C15) & MASK64
    z = state
    z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
    z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
    return state, z ^ (z >> 31)


def gen_values(seed, n):
    """Finite doubles with random sign, exponent in [2^-30, 2^34) and random mantissa."""
    st, xs = seed, []
    for _ in range(n):
        st, u = splitmix64(st)
        e = 993 + ((u >> 52) & 63)
        xs.append(fb(((u >> 63) << 63) | (e << 52) | (u & ((1 << 52) - 1))))
    return xs


def rand_list(rng, n, lo=-40, hi=40):
    return [(2 * rng.random() - 1) * 2.0 ** rng.randint(lo, hi) for _ in range(n)]


def edge_cases():
    rng = random.Random(7)
    one_eps = 1.0 + 2.0 ** -52
    r1000 = rand_list(rng, 1000)
    r1000b = rand_list(rng, 1000)
    c = []

    def add(op, name, a, b=None):
        c.append((op, name, a, b))

    # sum: signed zeros
    add("sum", "empty", [])
    add("sum", "neg_zero", [-0.0])
    add("sum", "neg_zero_300", [-0.0] * 300)
    add("sum", "neg_zeros_then_pos_zero", [-0.0] * 300 + [0.0])
    add("sum", "pos_zero_then_neg_zeros", [0.0] + [-0.0] * 10)
    add("sum", "cancel_to_zero", [1.5, -1.5])
    add("sum", "cancel_to_zero_blocks", [3.0] * 256 + [-3.0] * 256)
    # sum: catastrophic cancellation
    add("sum", "cancel_basic", [1e100, 1.0, -1e100])
    add("sum", "cancel_two", [1.0, 1e100, 1.0, -1e100])
    add("sum", "cancel_cross_block", [1e16] * 256 + [1.0] + [-1e16] * 256)
    add("sum", "cancel_ulps", [one_eps, -1.0] * 300)
    add("sum", "rump_like", [2.0 ** 53, 1.0, -(2.0 ** 53), 1.0])
    add("sum", "tenths", [0.1] * 10)
    add("sum", "harmonic_2000", [1.0 / k for k in range(1, 2001)])
    # sum: huge/tiny mixes and subnormals
    add("sum", "huge_tiny", [1e308, 1e-308, -1e308])
    add("sum", "huge_min_subnormal", [2.0 ** 1023, TINY, -(2.0 ** 1023)])
    add("sum", "subnormals_1000", [TINY] * 1000)
    add("sum", "normal_minus_subnormal", [sys.float_info.min, -TINY])
    add("sum", "subnormal_cancel", [TINY * 3, -TINY * 3, TINY])
    add("sum", "max_cancel", [MAX, -MAX, MAX])
    add("sum", "tiny_then_huge_blocks", [TINY] * 300 + [1e300, -1e300])
    # sum: block boundaries
    for n in [255, 256, 257, 511, 512, 513, 1025]:
        add("sum", f"random_{n}", rand_list(rng, n))
    # sum: errors
    add("sum", "overflow", [MAX, MAX])
    add("sum", "overflow_then_cancel", [MAX, MAX, -MAX])
    add("sum", "inf_at_9", [1.0] * 9 + [INF] + [1.0] * 5)
    add("sum", "neg_inf_at_0", [-INF, 1.0])
    add("sum", "nan_before_inf", [0.0, 0.0, 0.0, NAN, INF])
    add("sum", "inf_before_nan", [INF, NAN])
    add("sum", "nan_at_300", [1.0] * 300 + [NAN])
    # dot
    add("dot", "empty", [], [])
    add("dot", "len_3_4", [1.0, 2.0, 3.0], [1.0, 2.0, 3.0, 4.0])
    add("dot", "len_0_1", [], [1.0])
    add("dot", "basic", [1.0, 2.0, 3.0], [4.0, 5.0, 6.0])
    add("dot", "cancel", [1e100, 1.0, -1e100], [1.0, 1.0, 1.0])
    add("dot", "cancel_products", [1e150, 3.0, 1e150], [1e150, 1.0, -1e150])
    add("dot", "neg_zero", [-0.0], [1.0])
    add("dot", "neg_zero_product", [-1.0, 2.0], [0.0, -0.0])
    add("dot", "product_overflow", [1.0, 1e200], [1.0, 1e200])
    add("dot", "product_underflow", [1e-200, 1.0], [1e-200, 0.0])
    add("dot", "subnormal_products", [2.0 ** -1000] * 4, [2.0 ** -70] * 4)
    add("dot", "sum_overflow", [MAX, MAX], [1.0, 1.0])
    add("dot", "nan_in_b", [1.0, 2.0], [1.0, NAN])
    add("dot", "a_scanned_first", [1.0, INF], [NAN, 1.0])
    add("dot", "random_1000", r1000, r1000b)
    # sum_sq
    add("sum_sq", "empty", [])
    add("sum_sq", "neg_zero", [-0.0])
    add("sum_sq", "three_four", [3.0, -4.0])
    add("sum_sq", "overflow", [1e200])
    add("sum_sq", "underflow", [1e-200, 1e-200])
    add("sum_sq", "subnormal_squares", [2.0 ** -530] * 300)
    add("sum_sq", "nan", [1.0, NAN])
    add("sum_sq", "random_1000", r1000)
    # norm2
    add("norm2", "empty", [])
    add("norm2", "zeros", [-0.0, 0.0, -0.0])
    add("norm2", "three_four", [3.0, -4.0])
    add("norm2", "no_overflow", [1e200, 1e200])
    add("norm2", "no_underflow", [1e-200, -1e-200])
    add("norm2", "subnormals", [TINY] * 4)
    add("norm2", "subnormal_mix", [TINY, 3 * TINY, -7 * TINY])
    add("norm2", "huge_and_tiny", [1e300, 1e-300])
    add("norm2", "max_single", [-MAX])
    add("norm2", "overflow", [MAX, MAX])
    add("norm2", "inf", [1.0, -INF])
    add("norm2", "nan", [NAN])
    add("norm2", "random_1000", r1000)
    add("norm2", "ones_1000", [1.0] * 1000)
    # mean
    add("mean", "empty", [])
    add("mean", "neg_zero", [-0.0, -0.0])
    add("mean", "basic", [1.0, 2.0, 3.0, 4.0])
    add("mean", "thirds", [MAX, -MAX, 1.0])
    add("mean", "subnormals", [TINY] * 3)
    add("mean", "overflow", [MAX, MAX])
    add("mean", "nan", [1.0, NAN])
    add("mean", "random_1000", r1000)
    return c


def tokens(xs):
    """Run-length encoded bit patterns: `hex` or `hex*count`."""
    out, i = [], 0
    while i < len(xs):
        j = i
        while j < len(xs) and bits(xs[j]) == bits(xs[i]):
            j += 1
        out.append(hx(xs[i]) if j - i == 1 else f"{hx(xs[i])}*{j - i}")
        i = j
    return out


def expect_str(r):
    if r[0] == "ok":
        return f"ok:{hx(r[1])}"
    _, kind, idx = r
    if kind == "LengthMismatch":
        return f"err:LengthMismatch:{idx[0]}:{idx[1]}"
    if kind == "EmptyInput":
        return "err:EmptyInput"
    return f"err:{kind}:{'none' if idx is None else idx}"


GEN = [(0x5EED_0001, 1_000_000)]


def make_edge():
    out = [
        "# KRN.NUM.RED adversarial cases, CC-BY-4.0. Written by make_red_fixtures.py; do not edit.",
        "# <op> <name> <expect> exact <hex|-> maxulp <k|-> a <values> [b <values>]",
        "# expect: ok:<hex> (result of the documented rule) or err:<kind>[:<index|none>][:<len a>:<len b>].",
        "# exact: the oracle (exact rational, rounded once); maxulp: bound on |expect - exact| in",
        "# adjacent-double steps, asserted by the script and the Rust tests. values: hex bit",
        "# patterns, `hex*count` repeats one value.",
        "# gen lines: gen sum splitmix64 <seed> <n> <expect> exact <hex> (values from gen_values).",
    ]
    for op, name, a, b in edge_cases():
        r = model(op, a, b)
        ex = None
        bound = None
        if r[0] == "ok":
            ex = oracle(op, a, b)
            assert ex is not None, (op, name)
            bound = BOUND[op]
            d = ulp_dist(r[1], ex)
            assert d <= bound, (op, name, d)
            assert ex != 0 or bits(r[1]) == bits(ex), (op, name, "zero sign")
        line = [op, name, expect_str(r), "exact", "-" if ex is None else hx(ex), "maxulp", "-" if bound is None else str(bound), "a"]
        line += tokens(a)
        if b is not None:
            line += ["b"] + tokens(b)
        out.append(" ".join(line))
    for seed, n in GEN:
        xs = gen_values(seed, n)
        r = model_fixed_sum(xs)
        ex = oracle_sum(xs)
        assert r[0] == "ok" and ulp_dist(r[1], ex) <= BOUND["sum"]
        out.append(f"gen sum splitmix64 {seed:#x} {n} {expect_str(r)} exact {hx(ex)}")
    return "\n".join(out) + "\n"


def self_test():
    # The model and the oracle agree on trivially exact cases and on the sign rule.
    assert model("sum", [1.0, 2.0], None) == ("ok", 3.0)
    assert bits(model("sum", [-0.0], None)[1]) == bits(-0.0)
    assert bits(model("sum", [], None)[1]) == bits(0.0)
    assert model("norm2", [3.0, 4.0], None) == ("ok", 5.0) and oracle_norm2([3.0, 4.0]) == 5.0
    assert sqrt_round(Fraction(2)) == math.sqrt(2.0)
    assert ulp_dist(1.0, 1.0 + 2.0 ** -52) == 1 and ulp_dist(-0.0, 0.0) == 0 and ulp_dist(-TINY, TINY) == 2


def main():
    self_test()
    files = {"red-illcond-v1.txt": make_illcond(), "red-edge-v1.txt": make_edge()}
    check = "--check" in sys.argv[1:]
    stale = False
    for name, text in files.items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: fixtures/num/{name}")
                stale = True
        else:
            path.write_bytes(data)
            print(f"wrote fixtures/num/{name} ({len(data)} bytes)")
    if stale:
        sys.exit(1)


if __name__ == "__main__":
    main()
