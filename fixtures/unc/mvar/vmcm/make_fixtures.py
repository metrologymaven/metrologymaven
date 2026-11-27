#!/usr/bin/env python3
"""Generate the UNC.PROP.MVAR.VMCM fixtures (CC-BY-4.0 project data, hand-built values).

Independent reference for `fid_kunc::prop::mvar::vmcm` (D-038): every expected value comes
from the Python standard library only (fractions, decimal, hashlib, json, struct) and from
the rules written in spec/unc/prop/mvar/vmcm/NODE.md and restated below, never from the
Rust code.

Rules this reference implements on its own, in exact rational arithmetic:

- The joint sample of a group is the group's output columns of an `mcm-samples@1` set: one
  value per successful trial, so a trial that failed is excluded from every output
  (REQ-UNC.PROP.MVAR.VMCM-001). M is the number of successful trials.
- Sample mean per output: sum / M. Sample covariance: sum((x_i - mean_i)(x_j - mean_j)) /
  (M - 1), JCGM 102:2011 [unverified: clause 7.6 numbering].
- Squared Mahalanobis distance of trial r: (x_r - mean)^T S^-1 (x_r - mean), with S^-1 by
  exact Gauss-Jordan elimination.
- Region (REQ-002): rank n_p = ceil(p M) computed exactly from the binary64 value of p; the
  radius of record r2_p is the n_p-th smallest squared distance; `contained` counts the
  trials with distance <= r2_p and `at_radius` those equal to it.
- The `mcm-samples@1` bytes (UNC.PROP.MCM.SAMP): u64 LE header length, the canonical JSON
  header (sorted keys, no whitespace, integers as decimal strings), every output's column as
  LE doubles in header order, then the failed indices as LE u64; `samples` is the SHA-256
  of those bytes.

Values in the hand cases are small multiples of 1/8 from a fixed integer LCG, so means are
correctly rounded quotients of exact sums. The cases avoid near ties in the squared
distances (relative gap at least 1e-6), so the expected rank statistics do not depend on
rounding.

Files written next to this script:

- bivariate.json  AC-1: a linear model y1 = x1 + x2, y2 = x2 + x3 with independent normal
                  inputs, seed 11 and 10^6 trials, the exact output covariance A diag(s^2)
                  A^T, the 1 % tolerance, the exact means with a 5-standard-error tolerance
                  and the chi-square(2) quantile -2 ln(1 - p) of the Mahalanobis region
- cases.json      AC-2 and the rule checks: hand-built sample sets (columns, failed
                  indices), a group, p, and the expected M, mean, covariance, rank, radius,
                  counts and samples digest

Every f64 is written as its IEEE-754 bit pattern in 16 lowercase hex digits.

Run:    python fixtures/unc/mvar/vmcm/make_fixtures.py
Check:  python fixtures/unc/mvar/vmcm/make_fixtures.py --check
"""

import hashlib
import json
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
getcontext().prec = 60


def hex64(x):
    """IEEE-754 bit pattern of a finite f64 as 16 lowercase hex digits."""
    x = float(x)
    assert x == x and abs(x) != float("inf")
    return struct.pack(">d", x).hex()


def round_frac(q):
    """Fraction to the nearest double, ties to even (Python's Fraction.__float__ is
    correctly rounded)."""
    return float(Fraction(q))


def digest_of(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def ceil_count(p, m):
    """ceil(p m) exactly, from the binary64 value of p."""
    q = Fraction(p) * m
    n = q.numerator // q.denominator
    return n if n * q.denominator == q.numerator else n + 1


def min_trials(p, k):
    """The smallest M with ceil(p M) < M, and at least k + 1 (module rule)."""
    one_minus = 1 - Fraction(p)
    q = 1 / one_minus
    m = q.numerator // q.denominator
    if m * q.denominator != q.numerator:
        m += 1
    return max(m, k + 1, 2)


def inverse(a):
    """Exact Gauss-Jordan inverse of a square Fraction matrix."""
    n = len(a)
    m = [row[:] + [Fraction(int(i == j)) for j in range(n)] for i, row in enumerate(a)]
    for c in range(n):
        piv = next(r for r in range(c, n) if m[r][c] != 0)
        m[c], m[piv] = m[piv], m[c]
        f = m[c][c]
        m[c] = [x / f for x in m[c]]
        for r in range(n):
            if r != c and m[r][c] != 0:
                g = m[r][c]
                m[r] = [x - g * y for x, y in zip(m[r], m[c])]
    return [row[n:] for row in m]


def lcg(seed):
    """A fixed 32-bit LCG (Numerical Recipes constants), as a value source only."""
    s = seed
    while True:
        s = (1664525 * s + 1013904223) % (1 << 32)
        yield s


def samples_bytes(outputs, trials, failed, columns, seed):
    header = {
        "failed": str(len(failed)),
        "inputs": digest_of("vmcm-fixture-inputs"),
        "model": digest_of("vmcm-fixture-model"),
        "outputs": outputs,
        "profile": digest_of("vmcm-fixture-profile"),
        "seed": str(seed),
        "trials": str(trials),
        "version": "mcm-samples@1",
    }
    h = json.dumps(header, sort_keys=True, separators=(",", ":")).encode("ascii")
    out = struct.pack("<Q", len(h)) + h
    for name in outputs:
        for x in columns[name]:
            out += struct.pack("<d", float(x))
    for t in failed:
        out += struct.pack("<Q", t)
    return out


def stats(cols, p):
    """Exact mean, covariance, squared distances and region of the joint columns."""
    k = len(cols)
    m = len(cols[0])
    mean = [sum(c, Fraction(0)) / m for c in cols]
    dev = [[x - mu for x in c] for c, mu in zip(cols, mean)]
    cov = [[sum((a * b for a, b in zip(dev[i], dev[j])), Fraction(0)) / (m - 1) for j in range(k)] for i in range(k)]
    inv = inverse(cov)
    d2 = []
    for r in range(m):
        y = [dev[i][r] for i in range(k)]
        d2.append(sum(y[i] * inv[i][j] * y[j] for i in range(k) for j in range(k)))
    order = sorted(d2)
    for a, b in zip(order, order[1:]):
        assert b > a and (b - a) / b > Fraction(1, 10**6), "near tie in the squared distances"
    rank = ceil_count(p, m)
    r2 = order[rank - 1]
    return {
        "m": m,
        "mean": mean,
        "cov": cov,
        "rank": rank,
        "r2_p": r2,
        "contained": sum(1 for x in d2 if x <= r2),
        "at_radius": sum(1 for x in d2 if x == r2),
    }


def hand_case(name, outputs, trials, block_size, failed, gen, group, p, seed, note):
    """`gen(t, src)` gives the dict of output values (multiples of 1/8) of trial t."""
    src = lcg(seed)
    columns = {o: [] for o in outputs}
    would_be = {}
    for t in range(trials):
        vals = gen(t, src)
        if t in failed:
            would_be[str(t)] = {o: hex64(vals[o]) for o in outputs}
            continue
        for o in outputs:
            columns[o].append(Fraction(vals[o]))
    cols = [columns[g] for g in group]
    s = stats(cols, p)
    data = samples_bytes(outputs, trials, sorted(failed), columns, seed)
    assert s["m"] >= min_trials(p, len(group))
    return {
        "name": name,
        "note": note,
        "outputs": outputs,
        "trials": str(trials),
        "block_size": str(block_size),
        "seed": str(seed),
        "failed": [str(t) for t in sorted(failed)],
        "failed_would_be": would_be,
        "columns": {o: [hex64(x) for x in columns[o]] for o in outputs},
        "group": group,
        "p": hex64(p),
        "expected": {
            "m": str(s["m"]),
            "mean": [hex64(round_frac(x)) for x in s["mean"]],
            "cov": [[hex64(round_frac(x)) for x in row] for row in s["cov"]],
            "rank": str(s["rank"]),
            "r2_p": hex64(round_frac(s["r2_p"])),
            "contained": str(s["contained"]),
            "at_radius": str(s["at_radius"]),
            "min_trials": str(min_trials(p, len(group))),
            "samples": "sha256:" + hashlib.sha256(data).hexdigest(),
        },
    }


def eighths(src, lo, hi):
    """A multiple of 1/8 in [lo, hi) from the LCG."""
    span = (hi - lo) * 8
    return Fraction(lo) + Fraction(next(src) % span, 8)


def gen_pair(t, src):
    a = eighths(src, -20, 20)
    e = eighths(src, -4, 4)
    return {"a": a, "b": a / 2 + e}


def gen_triple(t, src):
    u = eighths(src, -40, 40)
    v = eighths(src, -10, 10)
    e = eighths(src, -3, 3)
    w = u - v + e
    return {"u": u, "v": v, "w": w, "z": eighths(src, 0, 5)}


def build_cases():
    cases = [
        hand_case(
            "joint_fail",
            ["a", "b"],
            12,
            6,
            {5},
            gen_pair,
            ["a", "b"],
            0.75,
            5,
            "AC-2: trial 5 failed (the operator of output a only); it is absent from both "
            "columns and excluded from the statistics of the group (a, b). failed_would_be "
            "shows the values the two outputs would have had.",
        ),
        hand_case(
            "triple_reordered",
            ["u", "v", "w", "z"],
            25,
            5,
            {0, 13, 24},
            gen_triple,
            ["w", "u", "v"],
            0.9,
            7,
            "REQ-001/-002: a three-output group in non-sorted order (w, u, v) out of four "
            "retained outputs, three failed trials including the first and the last.",
        ),
    ]
    return cases


def build_bivariate():
    m = 10**6
    p = 0.95
    inputs = [("x1", 10.0, 0.5), ("x2", -3.0, 1.0), ("x3", 0.5, 0.25)]
    a = [[1, 1, 0], [0, 1, 1]]
    var = [Fraction(s) ** 2 for _, _, s in inputs]
    mu = [Fraction(x) for _, x, _ in inputs]
    cov = [[sum(a[i][k] * var[k] * a[j][k] for k in range(3)) for j in range(2)] for i in range(2)]
    mean = [sum(a[i][k] * mu[k] for k in range(3)) for i in range(2)]
    # 5 standard errors of the mean: 5 sqrt(var / M), in 60-digit decimal arithmetic.
    mean_tol = [Decimal(5) * (Decimal(c[i].numerator) / Decimal(c[i].denominator) / Decimal(m)).sqrt() for i, c in enumerate(cov)]
    # Mahalanobis region of a bivariate normal: d^2 ~ chi-square(2), quantile -2 ln(1 - p).
    q = -2 * (Decimal(1) - Decimal(Fraction(p).numerator) / Decimal(Fraction(p).denominator)).ln()
    for row in cov:
        for x in row:
            assert float(x) == x
    return {
        "note": "AC-1: linear model y1 = x1 + x2, y2 = x2 + x3 (one binary64 addition each) "
        "with independent normal inputs drawn at the UNC.PROP.MCM.BLOCK address (seed, stream, "
        "t) for global trials t = 0 .. trials - 1. known_cov = A diag(sigma^2) A^T exactly; "
        "each sample covariance entry must be within rel_tol of it. mean_tol is 5 standard "
        "errors; chi2_quantile is -2 ln(1 - p), checked with quantile_rel_tol.",
        "seed": "11",
        "trials": str(m),
        "block_size": "100000",
        "p": hex64(p),
        "inputs": [{"id": i, "mu": hex64(x), "sigma": hex64(s), "stream": str(n)} for n, (i, x, s) in enumerate(inputs)],
        "outputs": [{"id": "y1", "terms": ["x1", "x2"]}, {"id": "y2", "terms": ["x2", "x3"]}],
        "known_cov": [[hex64(x) for x in row] for row in cov],
        "known_cov_decimal": [[str(Decimal(x.numerator) / Decimal(x.denominator)) for x in row] for row in cov],
        "rel_tol": hex64(0.01),
        "known_mean": [hex64(x) for x in mean],
        "mean_tol": [hex64(float(t)) for t in mean_tol],
        "chi2_quantile": hex64(float(q)),
        "quantile_rel_tol": hex64(0.01),
    }


def dump(obj):
    return (json.dumps(obj, indent=1, sort_keys=True) + "\n").encode("ascii")


def build():
    return {
        "bivariate.json": dump(build_bivariate()),
        "cases.json": dump({"cases": build_cases()}),
    }


def main():
    check = "--check" in sys.argv[1:]
    out = build()
    bad = 0
    for name, data in out.items():
        target = HERE / name
        if check:
            if not target.exists() or target.read_bytes() != data:
                print(f"stale: {target}")
                bad += 1
        else:
            target.write_bytes(data)
            print(f"wrote {target}")
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
