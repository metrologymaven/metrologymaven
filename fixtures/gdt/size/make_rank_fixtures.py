#!/usr/bin/env python3
"""Independent oracle for GDT.DIM.SIZEISO.RANK (fid-gps `dim::size::iso::rank`).

Writes, next to this script:

- `rank_sizes.json`: AC-1 (10 dyadic sizes with known statistics), AC-2 (even-count
  medians, including a midpoint that is a rounding tie), AC-3 (error paths) and edge cases
  (signed zeros, subnormal midpoint, midpoint and sum overflow, decimal sizes).
- `rank_seeded.json`: AC-4, one million seeded sizes (seed 0x512e): a SHA-256 of the
  generated values and the expected bits of every statistic.

Two kinds of expected value:

- `exact`: the exact statistic in rational arithmetic (`fractions.Fraction`, or integers
  for the seeded set), rounded once to the nearest binary64 (ties to even). The square root
  of a rational is rounded with an integer square root and a bracketing check, so every
  `exact` value is the correctly rounded true value, computed without any binary64
  arithmetic. Order statistics use the IEEE 754 total order on the bit patterns.
- `pinned` (mean and standard deviation only): the leaf's pinned procedure re-implemented
  here in plain binary64 from its written rule (KRN.NUM.RED fixed-order sum: 256-element
  Neumaier blocks combined by a fixed binary tree; mean = sum / n; two-pass variance;
  correctly rounded square root). The Rust result must equal it bit for bit on every CI
  platform, and must lie within `max_ulps` of `exact`; this script asserts that bound for
  its own re-implementation (1 ulp for AC-1, as the AC states; 2 ulps elsewhere).

Errors follow the leaf's order: `empty_sizes`, then the first non-finite element in index
order (`nan_input` / `infinite_input` with its index), then `insufficient_count` for the
n - 1 standard deviation of one value; an overflowing range, sum, product or deviation is
`overflow`. Nothing here is computed by the Rust code. All sizes are invented.

AC-4 stream (also implemented in the Rust test; the digest checks they agree): SplitMix64
from seed 0x512e; draw i gives k = 2**23 + (r >> 41) and the size k * 2**-33 m (about 1 mm
to 2 mm, exact in binary64). The digest is SHA-256 over the 8-byte big-endian bit patterns
in draw order.

Usage: `python3 make_rank_fixtures.py` (write) or `--check` (regenerate in memory and compare
with the committed files byte for byte). Stdlib only.
"""

import hashlib
import json
import math
import struct
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent
MASK = 0xFFFFFFFFFFFFFFFF
SEED = 0x512E
COUNT = 1_000_000
BLOCK = 256
MAXF = sys.float_info.max

KEYS = [
    "max",
    "min",
    "mean",
    "median.mean_of_middle",
    "median.lower",
    "median.upper",
    "mid_range",
    "range",
    "std_dev.n",
    "std_dev.n_minus_1",
]


class Overflow(Exception):
    pass


def bits(x: float) -> int:
    return struct.unpack(">Q", struct.pack(">d", x))[0]


def hx(x: float) -> str:
    return "0x%016x" % bits(x)


def from_bits(b: int) -> float:
    return struct.unpack(">d", struct.pack(">Q", b))[0]


def is_neg_zero(x: float) -> bool:
    return x == 0.0 and math.copysign(1.0, x) < 0


def order_key(x: float) -> int:
    """IEEE 754 totalOrder as an unsigned integer key (NaN-free input)."""
    b = bits(x)
    return (~b) & MASK if b >> 63 else b | (1 << 63)


def ulp_distance(a: float, b: float) -> int:
    return abs(order_key(a) - order_key(b))


# ---------------------------------------------------------------- exact rounding


def round_q(q: F) -> float:
    """Round a non-zero rational to the nearest binary64, ties to even; Overflow if it
    rounds to infinity. A result that underflows to zero keeps the sign of q."""
    neg = q < 0
    a = -q if neg else q
    n, d = a.numerator, a.denominator
    # Find e with 2^e <= a < 2^(e+1).
    e = n.bit_length() - d.bit_length()
    if F(n, d) < F(2) ** e:
        e -= 1
    # Quantum: 2^(e-52) for normals, 2^-1074 for subnormals.
    qexp = max(e - 52, -1074)
    scaled = a / (F(2) ** qexp)
    m = scaled.numerator // scaled.denominator
    rem = scaled - m
    if rem > F(1, 2) or (rem == F(1, 2) and m % 2 == 1):
        m += 1
    value = F(m) * (F(2) ** qexp)
    if value > F(MAXF):
        raise Overflow()
    x = float(value)  # exact: value is representable
    assert F(x) == value
    if x == 0.0:
        x = 0.0
    return -x if neg else x


def sqrt_q(q: F) -> float:
    """Correctly rounded sqrt of a non-negative rational (via an integer square root and a
    check that both ends of the bracketing interval round to the same binary64)."""
    if q == 0:
        return 0.0
    # 2^-1300 resolution: far below the 2^-1075 spacing of the smallest rounding boundaries.
    p = 1300
    # s = floor(sqrt(q) * 2^p) = isqrt(floor(q * 4^p)).
    t = (q.numerator << (2 * p)) // q.denominator
    s = math.isqrt(t)
    lo = F(s, 1 << p)
    hi = F(s + 1, 1 << p)
    assert lo * lo <= q < hi * hi
    if lo * lo == q:
        return round_q(lo)  # exact root (possibly a tie point): round it directly
    a, b = round_q(lo), round_q(hi)
    if a != b:
        # sqrt(q) lies within 2^-1300 of a rounding boundary: refine (never expected).
        raise AssertionError("sqrt rounding not resolved at 1300 bits")
    return a


def exact_add_sign(xs) -> float:
    """IEEE sign of an exact-zero sum of a non-empty list starting from -0.0: -0.0 iff
    every element is -0.0 (the KRN.NUM.RED rule)."""
    return -0.0 if all(is_neg_zero(x) for x in xs) else 0.0


# ---------------------------------------------------------------- pinned re-implementation


def block_sum(terms, lo, hi):
    s, c = -0.0, -0.0
    for i in range(lo, hi):
        x = terms[i]
        if not math.isfinite(x):
            raise Overflow()
        t = s + x
        if abs(s) >= abs(x):
            c += (s - t) + x
        else:
            c += (x - t) + s
        s = t
    return s, c


def combine(a, b):
    s = a[0] + b[0]
    if abs(a[0]) >= abs(b[0]):
        e = (a[0] - s) + b[0]
    else:
        e = (b[0] - s) + a[0]
    return s, (a[1] + b[1]) + e


def tree(terms, n, first, count):
    if count == 1:
        lo = first * BLOCK
        hi = n if n - lo < BLOCK else lo + BLOCK
        return block_sum(terms, lo, hi)
    left = count // 2
    a = tree(terms, n, first, left)
    b = tree(terms, n, first + left, count - left)
    return combine(a, b)


def fixed_sum(terms) -> float:
    n = len(terms)
    if n == 0:
        return 0.0
    s, c = tree(terms, n, 0, -(-n // BLOCK))
    if not (math.isfinite(s) and math.isfinite(c)):
        raise Overflow()
    r = s if c == 0.0 else s + c
    if not math.isfinite(r):
        raise Overflow()
    return r


def pinned_mean(xs) -> float:
    return fixed_sum(xs) / float(len(xs))


def pinned_std(xs, denom: int) -> float:
    m = pinned_mean(xs)
    dev = []
    for x in xs:
        d = x - m
        if not math.isfinite(d):
            raise Overflow()
        dev.append(d)
    sq = [d * d for d in dev]
    s = fixed_sum(sq)
    return math.sqrt(s / float(denom))


# ---------------------------------------------------------------- one case


def statistics(xs, max_ulps):
    """Expected result of every statistic for the list `xs` (floats)."""
    out = {}
    n = len(xs)
    if n == 0:
        return {k: {"error": "empty_sizes"} for k in KEYS}
    for i, x in enumerate(xs):
        if math.isnan(x):
            return {k: {"error": "nan_input", "index": i} for k in KEYS}
        if math.isinf(x):
            return {k: {"error": "infinite_input", "index": i} for k in KEYS}
    srt = sorted(xs, key=order_key)
    hi, lo = srt[-1], srt[0]
    out["max"] = {"exact": hx(hi)}
    out["min"] = {"exact": hx(lo)}

    def midpoint(a, b):
        q = (F(a) + F(b)) / 2
        if q == 0:
            return -0.0 if (is_neg_zero(a) and is_neg_zero(b)) else 0.0
        return round_q(q)

    def value_or_overflow(fn):
        try:
            return {"exact": hx(fn())}
        except Overflow:
            return {"error": "overflow"}

    def rng():
        q = F(hi) - F(lo)
        return 0.0 if q == 0 else round_q(q)

    out["range"] = value_or_overflow(rng)
    out["mid_range"] = value_or_overflow(lambda: midpoint(hi, lo))
    if n % 2 == 1:
        med = srt[n // 2]
        for v in ("mean_of_middle", "lower", "upper"):
            out["median." + v] = {"exact": hx(med)}
    else:
        a, b = srt[n // 2 - 1], srt[n // 2]
        out["median.lower"] = {"exact": hx(a)}
        out["median.upper"] = {"exact": hx(b)}
        out["median.mean_of_middle"] = value_or_overflow(lambda: midpoint(a, b))

    total = sum((F(x) for x in xs), F(0))
    mean_q = total / n
    try:
        mean_x = exact_add_sign(xs) if mean_q == 0 else round_q(mean_q)
    except Overflow:
        mean_x = None  # only used if the pinned mean is finite (asserted below)

    def pinned(key, exact_fn, fn):
        try:
            p = fn()
        except Overflow:
            out[key] = {"error": "overflow"}
            return
        exact = exact_fn()
        d = ulp_distance(p, exact)
        assert d <= max_ulps, (key, xs, d)
        out[key] = {"exact": hx(exact), "pinned": hx(p), "max_ulps": max_ulps}

    pinned("mean", lambda: mean_x, lambda: pinned_mean(xs))
    ss = sum(((F(x) - mean_q) ** 2 for x in xs), F(0))
    pinned("std_dev.n", lambda: sqrt_q(ss / n), lambda: pinned_std(xs, n))
    if n < 2:
        out["std_dev.n_minus_1"] = {"error": "insufficient_count", "needed": 2, "got": n}
    else:
        pinned("std_dev.n_minus_1", lambda: sqrt_q(ss / (n - 1)), lambda: pinned_std(xs, n - 1))
    return {k: out[k] for k in KEYS}


def case(name, ac, xs, max_ulps=2, note=""):
    return {
        "name": name,
        "ac": ac,
        "note": note,
        "sizes": [hx(x) for x in xs],
        "results": statistics(xs, max_ulps),
    }


def sizes_doc():
    u20 = 2.0**-20
    ac1 = [(10240 + k) * u20 for k in (7, -3, 12, 0, -8, 5, 5, 2, -1, 11)]
    a = 0.01
    b = math.nextafter(a, 1.0)
    tie = [0.0102, a, b, 0.0099, 0.0101, 0.0098]
    decimals = [0.010003, 0.009998, 0.010001, 0.010000, 0.009999, 0.010002, 0.009997]
    big = MAXF
    big_prev = math.nextafter(MAXF, 0.0)
    tiny = 5e-324
    cases = [
        case(
            "ac1_dyadic_ten",
            "AC-1",
            ac1,
            max_ulps=1,
            note="(10240 + k) * 2^-20 m; mean exact, std_dev within 1 ulp",
        ),
        case("ac2_even_tie", "AC-2", tie, note="middle pair 0.01 and its successor: the midpoint is a tie"),
        case("ac2_odd_seven", "AC-2", decimals, note="odd count: every median variant is the middle value"),
        case("ac3_empty", "AC-3", []),
        case("ac3_one_value", "AC-3", [0.0125], note="std_dev.n_minus_1 needs two values"),
        case("ac3_nan", "AC-3", [0.01, 0.02, float("nan"), 0.03]),
        case("ac3_nan_one_value", "AC-3", [float("nan")], note="nan_input before insufficient_count"),
        case("infinite_input", "AC-3", [0.01, float("inf"), float("nan")], note="first non-finite wins"),
        case("negative_infinite_input", "AC-3", [-math.inf]),
        case("signed_zeros", "edge", [0.0, -0.0, -0.0]),
        case("negative_zero_alone", "edge", [-0.0]),
        case("subnormal_midpoint_tie", "edge", [tiny, 0.0], note="(2^-1074 + 0) / 2 ties to +0.0"),
        case("midpoint_overflow_fallback", "edge", [big, big_prev], note="max + min overflows; halves are exact"),
        case("range_overflow", "edge", [big, -big]),
        case("mean_overflow", "edge", [big, big]),
    ]
    return {"schema": "fid.test.gdt.dim.size.rank.cases/1", "keys": KEYS, "cases": cases}


# ---------------------------------------------------------------- AC-4


def splitmix(seed):
    state = seed
    while True:
        state = (state + 0x9E3779B97F4A7C15) & MASK
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        yield z ^ (z >> 31)


def seeded_doc():
    gen = splitmix(SEED)
    ks = [(1 << 23) + (next(gen) >> 41) for _ in range(COUNT)]
    scale = 2.0**-33
    xs = [float(k) * scale for k in ks]
    h = hashlib.sha256()
    for x in xs:
        h.update(struct.pack(">d", x))
    n = COUNT
    sk = sorted(ks)
    s1 = sum(ks)
    s2 = sum(k * k for k in ks)
    unit = F(1, 1 << 33)

    def q(k):
        return F(k) * unit

    def r(v: F) -> str:
        return hx(round_q(v))

    exact = {
        "max": r(q(sk[-1])),
        "min": r(q(sk[0])),
        "median.lower": r(q(sk[n // 2 - 1])),
        "median.upper": r(q(sk[n // 2])),
        "median.mean_of_middle": r((q(sk[n // 2 - 1]) + q(sk[n // 2])) / 2),
        "mid_range": r((q(sk[-1]) + q(sk[0])) / 2),
        "range": r(q(sk[-1]) - q(sk[0])),
        "mean": r(F(s1, n) * unit),
    }
    # sum (x - mean)^2 = (n * s2 - s1^2) / n * unit^2
    ss = F(n * s2 - s1 * s1, n) * unit * unit
    exact["std_dev.n"] = hx(sqrt_q(ss / n))
    exact["std_dev.n_minus_1"] = hx(sqrt_q(ss / (n - 1)))
    # Integer cross-checks of the binary64 values against the exact statistics.
    assert F(xs[0]) == q(ks[0]) and F(max(xs)) == q(sk[-1])
    pinned = {
        "mean": pinned_mean(xs),
        "std_dev.n": pinned_std(xs, n),
        "std_dev.n_minus_1": pinned_std(xs, n - 1),
    }
    results = {}
    for k in KEYS:
        if k in pinned:
            d = ulp_distance(pinned[k], from_bits(int(exact[k], 16)))
            # The sum of the seeded sizes is exact, so the mean is correctly rounded.
            assert d <= (0 if k == "mean" else 2), (k, d)
            results[k] = {"exact": exact[k], "pinned": hx(pinned[k]), "max_ulps": 0 if k == "mean" else 2}
        else:
            results[k] = {"exact": exact[k]}
    return {
        "schema": "fid.test.gdt.dim.size.rank.seeded/1",
        "seed": "0x512e",
        "count": COUNT,
        "generator": "splitmix64; k = 2^23 + (r >> 41); size = k * 2^-33 m",
        "sha256": h.hexdigest(),
        "middle_pair_equal": sk[n // 2 - 1] == sk[n // 2],
        "results": results,
    }


def dump(obj) -> str:
    return json.dumps(obj, indent=1, sort_keys=False) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    outputs = {
        "rank_sizes.json": dump(sizes_doc()),
        "rank_seeded.json": dump(seeded_doc()),
    }
    bad = False
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"{path}: out of date; run make_rank_fixtures.py", file=sys.stderr)
                bad = True
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {path}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
