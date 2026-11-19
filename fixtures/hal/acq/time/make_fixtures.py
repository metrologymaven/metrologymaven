#!/usr/bin/env python3
"""Generate the fixtures of spec node HAL.ACQ.TIME (session timebase, clock offset and drift).

Independent stdlib-only reference. Nothing here runs the Rust code.

Synthetic pairs (AC-1, AC-3)
----------------------------
Device clock samples every 1 ms from T0 = 1 000 s; the host clock reads

    t_host = t_dev + 3 000 000 ns + 50 ppm * (t_dev - T0) + round(200 ns * z)

with z standard normal (Box-Muller) drawn from the project's counter-based RNG
(Philox4x32-10, seed 7, stream 0, re-implemented here exactly as KRN.NUM.RNG addresses it,
checked against the published Random123 answers). The drift term is the exact integer
50 * i ns. Pairs are integers, so the committed JSON is the ground truth; the float maths
used to draw the jitter never enters the expected values.

Expected values (AC-1, AC-2)
----------------------------
The estimator of REQ-HAL.ACQ.TIME-002, in the exact order the leaf fixes:

    x_i = t_dev_i - t_dev_0                       (integer, exact in binary64)
    y_i = (t_host_i - t_host_0) - x_i             (integer, exact in binary64)
    sx  = sum x_i, sy = sum y_i                   (index order, one rounding per add)
    mx  = sx / n, my = sy / n
    sxx = sum (x_i - mx)^2, sxy = sum (x_i - mx)(y_i - my)    (each op rounded, no FMA)
    drift     = sxy / sxx
    intercept = my - drift * mx
    ssr       = sum (y_i - (intercept + drift * x_i))^2
    sigma     = sqrt(ssr / (n - 2))               (correctly rounded)
    offset    = float(t_host_0 - t_dev_0) + intercept

Each binary64 operation is evaluated twice: once with exact rationals (fractions.Fraction)
rounded once to the nearest double (ties to even), once with CPython floats (which never
fuse multiply-add). The script stops if the two disagree in a single bit. Square roots are
verified against an exact midpoint test. The golden bits are what every platform must
reproduce (AC-2). Exact (unrounded) OLS values are also written, computed with Fraction
and decimal at 40 digits, so the tests can bound the f64 result's distance from the truth.

Mapping golden: map_to_host(t_dev) = ref_host + dx + round_half_away(intercept + drift*dx).

CC-BY-4.0 project data. Run:  python fixtures/hal/acq/time/make_fixtures.py [--check]
"""

import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent


class CheckFailed(Exception):
    pass


def check(cond, what):
    """Explicit check that also runs under `python -O` (unlike assert)."""
    if not cond:
        raise CheckFailed(repr(what))

# ---------------------------------------------------------------- Philox4x32-10 (KRN.NUM.RNG)
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


for _ctr, _key, _want in [
    ((0, 0, 0, 0), (0, 0), (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)),
    ((MASK,) * 4, (MASK, MASK), (0x408F276D, 0x41C83B0E, 0xA20BC7C6, 0x6D5451FD)),
]:
    check(philox(_ctr, _key) == _want, 'philox(_ctr, _key) == _want')


def uniform(seed, stream, index):
    w = philox((index & MASK, index >> 32, stream & MASK, stream >> 32), (seed & MASK, seed >> 32))
    u = (w[1] << 32) | w[0]
    return (u >> 11) * 2.0 ** -53


SEED = 7
N_PAIRS = 1000
T0 = 1_000_000_000_000
STEP = 1_000_000
OFFSET = 3_000_000
SIGMA = 200.0


def make_pairs():
    pairs = []
    for i in range(N_PAIRS):
        u1 = 1.0 - uniform(SEED, 0, 2 * i)  # in (0, 1]
        u2 = uniform(SEED, 0, 2 * i + 1)
        z = math.sqrt(-2.0 * math.log(u1)) * math.cos(2.0 * math.pi * u2)
        jitter = int(round(SIGMA * z))
        t_dev = T0 + i * STEP
        t_host = t_dev + OFFSET + 50 * i + jitter
        pairs.append((t_dev, t_host))
    return pairs


# ---------------------------------------------------------------- binary64 with exact rounding
def rnd(q):
    """Nearest binary64 to the rational q, ties to even (CPython int true division is
    correctly rounded)."""
    q = Fraction(q)
    if q == 0:
        return 0.0
    return q.numerator / q.denominator


def add(a, b):
    r = rnd(Fraction(a) + Fraction(b))
    check(struct.pack("<d", r) == struct.pack("<d", a + b) or (r == 0 and a + b == 0), (a, b))
    return r


def sub(a, b):
    r = rnd(Fraction(a) - Fraction(b))
    check(r == a - b, (a, b))
    return r


def mul(a, b):
    r = rnd(Fraction(a) * Fraction(b))
    check(r == a * b, (a, b))
    return r


def div(a, b):
    r = rnd(Fraction(a) / Fraction(b))
    check(r == a / b, (a, b))
    return r


def sqrt(a):
    check(a >= 0, 'a >= 0')
    r = math.sqrt(a)
    if a == 0:
        return r
    # Correct rounding: a lies strictly between the squares of the midpoints around r.
    lo = (Fraction(math.nextafter(r, 0.0)) + Fraction(r)) / 2
    hi = (Fraction(math.nextafter(r, math.inf)) + Fraction(r)) / 2
    check(lo * lo < Fraction(a) < hi * hi, a)
    return r


def round_half_away(x):
    q = Fraction(x)
    m = abs(q)
    k = m.numerator // m.denominator
    if m - k >= Fraction(1, 2):
        k += 1
    return -k if q < 0 else k


def estimate(pairs):
    n = len(pairs)
    d0, h0 = pairs[0]
    xs = [d - d0 for d, _ in pairs]
    ys = [(h - h0) - (d - d0) for d, h in pairs]
    for v in xs + ys:
        check(abs(v) <= 1 << 53, 'abs(v) <= 1 << 53')
    xs = [float(v) for v in xs]
    ys = [float(v) for v in ys]
    nf = float(n)
    sx = 0.0
    sy = 0.0
    for i in range(n):
        sx = add(sx, xs[i])
        sy = add(sy, ys[i])
    mx = div(sx, nf)
    my = div(sy, nf)
    sxx = 0.0
    sxy = 0.0
    for i in range(n):
        dx = sub(xs[i], mx)
        dy = sub(ys[i], my)
        sxx = add(sxx, mul(dx, dx))
        sxy = add(sxy, mul(dx, dy))
    drift = div(sxy, sxx)
    intercept = sub(my, mul(drift, mx))
    ssr = 0.0
    for i in range(n):
        r = sub(ys[i], add(intercept, mul(drift, xs[i])))
        ssr = add(ssr, mul(r, r))
    sigma = sqrt(div(ssr, sub(nf, 2.0)))
    offset = add(float(h0 - d0), intercept)
    return {"offset": offset, "drift": drift, "sigma": sigma, "intercept": intercept, "ref_dev": d0, "ref_host": h0}


def exact_ols(pairs):
    n = len(pairs)
    d0, h0 = pairs[0]
    xs = [Fraction(d - d0) for d, _ in pairs]
    ys = [Fraction((h - h0) - (d - d0)) for d, h in pairs]
    mx = sum(xs) / n
    my = sum(ys) / n
    sxx = sum((x - mx) ** 2 for x in xs)
    sxy = sum((x - mx) * (y - my) for x, y in zip(xs, ys))
    b = sxy / sxx
    a = my - b * mx
    ssr = sum((y - a - b * x) ** 2 for x, y in zip(xs, ys))
    getcontext().prec = 40
    var = ssr / (n - 2)
    sigma = (Decimal(var.numerator) / Decimal(var.denominator)).sqrt()
    offset = Fraction(h0 - d0) + a

    def dec(q):
        return Decimal(q.numerator) / Decimal(q.denominator)

    return {"offset": dec(offset), "drift": dec(b), "sigma": sigma}


def map_to_host(m, t_dev):
    dx = t_dev - m["ref_dev"]
    check(abs(dx) <= 1 << 53, 'abs(dx) <= 1 << 53')
    corr = add(m["intercept"], mul(m["drift"], float(dx)))
    return m["ref_host"] + dx + round_half_away(corr)


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def pairs_json(pairs, comment):
    lines = ["{", f'  "comment": "{comment}",', '  "pairs": [']
    for i, (d, h) in enumerate(pairs):
        sep = "," if i + 1 < len(pairs) else ""
        lines.append(f"    [{d}, {h}]{sep}")
    lines += ["  ]", "}", ""]
    return "\n".join(lines).encode()


MAP_PROBES = [T0, T0 + 1, T0 + 500_000_000 + 7, T0 + 999 * STEP, T0 + 3_600_000_000_000, T0 - 1_000_000_000]


def outputs():
    pairs = make_pairs()
    m = estimate(pairs)
    ex = exact_ols(pairs)
    # AC-1 tolerances hold for the reference itself.
    check(abs(m["offset"] - 3_000_000.0) <= 1_000.0, 'abs(m["offset"] - 3_000_000.0) <= 1_000.0')
    check(abs(m["drift"] - 50e-6) <= 0.5e-6, 'abs(m["drift"] - 50e-6) <= 0.5e-6')
    check(abs(m["sigma"] - 200.0) <= 30.0, 'abs(m["sigma"] - 200.0) <= 30.0')
    lines = [
        "# HAL.ACQ.TIME AC-1/AC-2 golden values for drift_50ppm.json, from make_fixtures.py.",
        "# <name> <binary64 bits, hex> <decimal>; exact_* are unrounded OLS values (40 digits).",
        f"n {len(pairs)}",
        f"ref_dev_ns {m['ref_dev']}",
        f"ref_host_ns {m['ref_host']}",
    ]
    for k in ["offset", "drift", "sigma", "intercept"]:
        name = {"offset": "offset_ns", "drift": "drift", "sigma": "sigma_ns", "intercept": "intercept_ns"}[k]
        lines.append(f"{name} {bits(m[k]):016x} {m[k]!r}")
    for k, name in [("offset", "exact_offset_ns"), ("drift", "exact_drift"), ("sigma", "exact_sigma_ns")]:
        lines.append(f"{name} {ex[k]}")
    for t in MAP_PROBES:
        lines.append(f"map {t} {map_to_host(m, t)}")
    return {
        "drift_50ppm.json": pairs_json(pairs, "HAL.ACQ.TIME AC-1: offset 3 ms, drift 50 ppm, jitter 200 ns, seed 7"),
        "ten_pairs.json": pairs_json(pairs[:10], "HAL.ACQ.TIME AC-3: ten pairs, below the 16-pair minimum"),
        "expected-v1.txt": ("\n".join(lines) + "\n").encode(),
    }


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale:", ", ".join(stale))
        sys.exit(1)
    print("ok" if check else "written")


if __name__ == "__main__":
    main()
