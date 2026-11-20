#!/usr/bin/env python3
"""Independent reference for UNC.PROP.MCM.INTVL fixtures (stdlib only; CC-BY-4.0 data).

Nothing here runs or imports the Rust code. Every f64 is written as its 16 hex digits of
IEEE-754 bits (the `BlockOut` JSON convention of UNC.PROP.MCM.BLOCK), with a decimal
repr next to scalars for people.

uniform_1000.json  AC-1: the values 1..1000, p = 0.95. Expected symmetric interval
                   [25th value, 975th value] and shortest interval [1st, 951st] by the rank
                   rule of REQ-UNC.PROP.MCM.INTVL-002, evaluated here in exact rationals with
                   p = 19/20. Mean and standard uncertainty are exact rationals rounded once
                   to the nearest double (sqrt by decimal.Decimal at 60 digits).
skewed.json        AC-2: 2000 exponential values from the project RNG with seed 3, stream 0,
                   indices 0..1999. The RNG is an independent Philox4x32-10 (checked against
                   the published Random123 known answers below); the uniform is the open-unit
                   map u = (2 k + 1) 2^-53 with k the top 52 bits of word1 << 32 | word0
                   (UNC.MODEL.DIST); the value is -ln(u), correctly rounded from a 60-digit decimal logarithm. The values are
                   data: the expected results are computed from them here, independently.
ac_h1_reference.json
                   AC-H1: analytic endpoints and densities of the two closed-form output
                   distributions, and the exact (Clopper-Pearson) two-sided 95 % binomial
                   bounds for k of 100 seeds, so the crate test can report a pass count with
                   its exact bounds instead of a bare point estimate.
                   - rect4: Y = X1 + X2 + X3 + X4, Xi rectangular on [-sqrt 3, sqrt 3] (unit
                     variance each), the rectangular-input additive model of JCGM 101:2008
                     [unverified: clause 9.2 variant]. Y = 2 sqrt(3) S - 4 sqrt(3) with S
                     Irwin-Hall(4); quantiles by bisection on the exact piecewise-quartic CDF.
                     Symmetric and unimodal, so the shortest 95 % interval equals the
                     probabilistically symmetric one.
                   - exp1: exponential with rate 1. Symmetric [-ln 0.975, -ln 0.025]; shortest
                     [0, -ln 0.05] (the density decreases, so the shortest interval starts at
                     the support boundary).
                   Only IEEE +, -, *, / and correctly rounded sqrt or decimal functions are
                   used for written bits, so --check gives the same bytes on every platform.

Usage: make_fixtures.py [--check]   (--check compares with the files on disk, writes nothing)
"""

import json
import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
getcontext().prec = 60

# --------------------------------------------------------------------------- RNG

M0, M1 = 0xD2511F53, 0xCD9E8D57
W0, W1 = 0x9E3779B9, 0xBB67AE85
MASK = 0xFFFFFFFF


def philox(ctr, key):
    c0, c1, c2, c3 = ctr
    k0, k1 = key
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK, (k1 + W1) & MASK
        p0, p1 = M0 * c0, M1 * c2
        hi0, lo0, hi1, lo1 = p0 >> 32, p0 & MASK, p1 >> 32, p1 & MASK
        c0, c1, c2, c3 = hi1 ^ c1 ^ k0, lo1, hi0 ^ c3 ^ k1, lo0
    return (c0, c1, c2, c3)


# Published Random123 known answers for philox4x32_10 (kat_vectors).
for _ctr, _key, _want in [
    ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
    ((MASK,) * 4, (MASK, MASK), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
    ((0x243F6A88, 0x85A308D3, 0x13198A2E, 0x03707344), (0xA4093822, 0x299F31D0),
     (0xD16CFE09, 0x94FDCCEB, 0x5001E420, 0x24126EA1)),
]:
    assert philox(_ctr, _key) == _want


def open_unit(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    x = (w[1] << 32) | w[0]
    k = x >> 12
    return (2 * k + 1) * 2.0 ** -53


# --------------------------------------------------------------------------- helpers


def ipow(x, n):
    """x**n by repeated multiplication: plain IEEE arithmetic, no platform libm pow."""
    r = 1.0
    for _ in range(n):
        r *= x
    return r


def neg_ln(u):
    """-ln(u) correctly rounded (60-digit decimal), so the output is the same on every platform."""
    return float(-Decimal(u).ln())


def hexbits(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


def round_half_away(fr):
    """Round a non-negative Fraction to the nearest integer, halves away from zero."""
    assert fr >= 0
    return int(fr + Fraction(1, 2))


def exact_mean_u(values):
    m = len(values)
    fr = [Fraction(v) for v in values]
    mean = sum(fr) / m
    var = sum((x - mean) ** 2 for x in fr) / (m - 1)
    u = (Decimal(var.numerator) / Decimal(var.denominator)).sqrt()
    return float(mean), float(u), mean


def intervals(values, p):
    """Rank rule of REQ-UNC.PROP.MCM.INTVL-002 with 1-based ranks, in exact rationals."""
    xs = sorted(values)
    m = len(xs)
    r = round_half_away(m * (1 - p) / 2)
    q = round_half_away(p * m)
    # Guard: neither rounding may sit near a half, so float evaluation agrees.
    assert abs(m * (1 - p) / 2 - r) < Fraction(2, 5) or abs(m * (1 - p) / 2 - r) > Fraction(3, 5)
    lo_rank = 1 if r < 1 else (m if r > m else r)
    hi_rank = r + q
    hi_rank = 1 if hi_rank < 1 else (m if hi_rank > m else hi_rank)
    sym = (xs[lo_rank - 1], xs[hi_rank - 1])
    best = None
    for s in range(1, m - q + 1):
        length = Fraction(xs[s + q - 1]) - Fraction(xs[s - 1])
        if best is None or length < best[0]:
            best = (length, s)
    s = best[1]
    return sym, (xs[s - 1], xs[s + q - 1]), (lo_rank, hi_rank), (s, s + q)


def case(values, p_num, p_den, fx, note):
    p = Fraction(p_num, p_den)
    pf = p_num / p_den
    mean_f, u_f, mean_exact = exact_mean_u(values)
    sym, sh, sym_ranks, sh_ranks = intervals(values, p)
    sym_len = Fraction(sym[1]) - Fraction(sym[0])
    sh_len = Fraction(sh[1]) - Fraction(sh[0])
    assert sh_len <= sym_len
    exp = {
        "mean": hexbits(mean_f),
        "mean_dec": repr(mean_f),
        "u": hexbits(u_f),
        "u_dec": repr(u_f),
        "sym_low": hexbits(sym[0]),
        "sym_high": hexbits(sym[1]),
        "sym_ranks": list(sym_ranks),
        "shortest_low": hexbits(sh[0]),
        "shortest_high": hexbits(sh[1]),
        "shortest_ranks": list(sh_ranks),
        "shortest_strictly_shorter": sh_len < sym_len,
        "mean_minus_fx": hexbits(float(mean_exact - Fraction(fx))),
    }
    lines = [
        "{",
        f'  "note": {json.dumps(note)},',
        f'  "p": "{hexbits(pf)}",',
        f'  "p_dec": "{pf!r}",',
        f'  "fx": "{hexbits(fx)}",',
        f'  "fx_dec": "{fx!r}",',
        f'  "m": {len(values)},',
        f'  "expected": {json.dumps(exp, separators=(", ", ": "))},',
        '  "values": [' + ", ".join(f'"{hexbits(v)}"' for v in values) + "]",
        "}",
    ]
    return "\n".join(lines) + "\n", exp


# --------------------------------------------------------------------------- AC-H1


def irwin_hall4_cdf(s):
    if s <= 0:
        return 0.0
    if s >= 4:
        return 1.0
    return sum((-1) ** k * math.comb(4, k) * ipow(s - k, 4) for k in range(int(s) + 1)) / 24.0


def irwin_hall4_pdf(s):
    if s <= 0 or s >= 4:
        return 0.0
    return sum((-1) ** k * math.comb(4, k) * ipow(s - k, 3) for k in range(int(s) + 1)) / 6.0


def bisect(fn, target, lo, hi):
    for _ in range(200):
        mid = 0.5 * (lo + hi)
        if fn(mid) < target:
            lo = mid
        else:
            hi = mid
    return 0.5 * (lo + hi)


def rect4_endpoint(q):
    s = bisect(irwin_hall4_cdf, q, 0.0, 4.0)
    scale = 2.0 * math.sqrt(3.0)
    return {"q": q, "x": s * scale - 4.0 * math.sqrt(3.0), "density": irwin_hall4_pdf(s) / scale}


def exp_endpoint(q):
    tail = Decimal(1) - Decimal(q)
    return {"q": q, "x": float(-tail.ln()), "density": float(tail)}


def binom_cdf(k, n, p):
    return sum(math.comb(n, i) * ipow(p, i) * ipow(1 - p, n - i) for i in range(k + 1))


def clopper_pearson(k, n, alpha=0.05):
    # P(X >= k) increases with p; P(X <= k) decreases, so bisect on its negative.
    lo = 0.0 if k == 0 else bisect(lambda p: 1 - binom_cdf(k - 1, n, p), alpha / 2, 0.0, 1.0)
    hi = 1.0 if k == n else bisect(lambda p: -binom_cdf(k, n, p), -alpha / 2, 0.0, 1.0)
    return lo, hi


def ac_h1():
    # Known closed forms for the boundary counts.
    lo, hi = clopper_pearson(100, 100)
    assert abs(lo - 0.025 ** (1 / 100)) < 1e-12 and hi == 1.0
    lo, hi = clopper_pearson(0, 100)
    assert lo == 0.0 and abs(hi - (1 - 0.025 ** (1 / 100))) < 1e-12
    rect = {
        "sym_low": rect4_endpoint(0.025),
        "sym_high": rect4_endpoint(0.975),
        "shortest_low": rect4_endpoint(0.025),
        "shortest_high": rect4_endpoint(0.975),
    }
    # Symmetry check of the reference itself.
    assert abs(rect["sym_low"]["x"] + rect["sym_high"]["x"]) < 1e-12
    exp1 = {
        "sym_low": exp_endpoint(0.025),
        "sym_high": exp_endpoint(0.975),
        "shortest_low": {"q": 0.0, "x": 0.0, "density": 1.0},
        "shortest_high": exp_endpoint(0.95),
    }
    out = ["{", '  "p": 0.95,']
    for name, d in (("rect4", rect), ("exp1", exp1)):
        out.append(f'  "{name}": {{')
        items = []
        for key in ("sym_low", "sym_high", "shortest_low", "shortest_high"):
            e = d[key]
            items.append(
                f'    "{key}": {{"q": {e["q"]!r}, "x": "{hexbits(e["x"])}", "x_dec": {e["x"]!r}, '
                f'"density": "{hexbits(e["density"])}", "density_dec": {e["density"]!r}}}'
            )
        out.append(",\n".join(items))
        out.append("  },")
    cp = []
    for k in range(101):
        lo, hi = clopper_pearson(k, 100)
        cp.append(f'["{lo:.6f}", "{hi:.6f}"]')
    out.append('  "clopper_pearson_95_n100": [' + ", ".join(cp) + "]")
    out.append("}")
    return "\n".join(out) + "\n"


# --------------------------------------------------------------------------- main


def main(argv):
    check = "--check" in argv[1:]
    uni_text, uni = case([float(i) for i in range(1, 1001)], 19, 20, 500.5, "AC-1: the values 1..1000")
    assert uni["sym_ranks"] == [25, 975] and uni["shortest_ranks"] == [1, 951]
    assert not uni["shortest_strictly_shorter"]
    skew_values = [neg_ln(open_unit(3, 0, i)) for i in range(2000)]
    skew_text, skew = case(skew_values, 19, 20, 1.0, "AC-2: exponential(1), seed 3, stream 0, indices 0..1999")
    assert skew["shortest_strictly_shorter"]
    files = {
        "uniform_1000.json": uni_text,
        "skewed.json": skew_text,
        "ac_h1_reference.json": ac_h1(),
    }
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
