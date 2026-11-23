#!/usr/bin/env python3
"""Independent 60-digit stdlib-Python oracle and fixture generator for UNC.DEC.RISK.

Generates and checks:
  - fixtures/unc/dec/risk/specific.csv  (AC-1: outcomes and p -> specific risks)
  - fixtures/unc/dec/risk/global.csv    (AC-2 & AC-4: 50-digit global consumer/producer risks)

Uses only the Python standard library (`decimal` at 65 working digits) with no
third-party packages and no values copied from JCGM 106 examples.

Usage:
    python fixtures/unc/dec/risk/make_cases.py          # write CSV fixtures
    python fixtures/unc/dec/risk/make_cases.py --check  # verify committed fixtures match
    python fixtures/unc/dec/risk/make_cases.py --gl16   # print exact 16-pt GL f64 literals
"""

from __future__ import annotations

import argparse
import csv
import io
import struct
import sys
from decimal import Decimal, getcontext
from pathlib import Path

OUT_DIR = Path(__file__).resolve().parent
SPECIFIC_CSV = OUT_DIR / "specific.csv"
GLOBAL_CSV = OUT_DIR / "global.csv"

# 65 working digits so 50-digit published references have >12 guard digits.
getcontext().prec = 65

ZERO = Decimal(0)
HALF = Decimal("0.5")
ONE = Decimal(1)
TWO = Decimal(2)


EPS = Decimal("1e-64")


def dec_pi() -> Decimal:
    """Machin-like formula for pi to current Decimal precision."""
    def atan_inv(m: int) -> Decimal:
        m2 = Decimal(m * m)
        term = ONE / Decimal(m)
        total = term
        k = 1
        while True:
            term = -term / m2
            add = term / Decimal(2 * k + 1)
            if abs(add) < EPS:
                break
            total += add
            k += 1
        return total

    return Decimal(4) * (Decimal(4) * atan_inv(5) - atan_inv(239))


PI = dec_pi()
SQRT_2 = TWO.sqrt()
SQRT_PI = PI.sqrt()
INV_SQRT_2PI = ONE / (TWO * PI).sqrt()


def dec_erfc_nonneg(x: Decimal) -> Decimal:
    """High-precision erfc(x) for x >= 0 (accurate to >58 digits)."""
    if x == 0:
        return ONE
    # erfc(13) < 1e-74, well below 65-digit working precision relative to 1.
    if x > Decimal(13):
        return ZERO
    if x < Decimal("2.2"):
        # Power series for erf(x): 2/sqrt(pi) * exp(-x^2) * sum_{n>=0} 2^n x^(2n+1) / (2n+1)!!
        x2 = x * x
        two_x2 = TWO * x2
        term = x
        s = term
        n = 0
        while True:
            n += 1
            term = term * two_x2 / Decimal(2 * n + 1)
            if abs(term) < EPS * s:
                break
            s += term
        erf_val = TWO * ((-x2).exp()) * s / SQRT_PI
        return ONE - erf_val
    # Laplace continued fraction for erfc(x), x >= 2.2:
    # Larger x converges dramatically faster.
    if x >= Decimal(8):
        depth = 20
    elif x >= Decimal(5):
        depth = 40
    elif x >= Decimal(3):
        depth = 75
    else:
        depth = 120
    t = x
    for k in range(depth, 0, -1):
        t = x + (Decimal(k) * HALF) / t
    return ((-x * x).exp()) / (SQRT_PI * t)


def dec_upper_tail(z: Decimal) -> Decimal:
    """Standard normal upper tail P(Z > z) = 0.5 * erfc(z / sqrt(2))."""
    x = z / SQRT_2
    if x < 0:
        return HALF * (TWO - dec_erfc_nonneg(-x))
    return HALF * dec_erfc_nonneg(x)


def gl_nodes_weights(n: int) -> list[tuple[Decimal, Decimal]]:
    """Compute n-point Gauss-Legendre nodes and weights on [-1, 1] via Newton's method."""
    import math

    pairs: list[tuple[Decimal, Decimal]] = []
    m = (n + 1) // 2
    for i in range(1, m + 1):
        x = Decimal(str(math.cos(math.pi * (i - 0.25) / (n + 0.5))))
        for _ in range(30):
            p0 = ONE
            p1 = x
            for k in range(2, n + 1):
                pk = (Decimal(2 * k - 1) * x * p1 - Decimal(k - 1) * p0) / Decimal(k)
                p0, p1 = p1, pk
            dp = Decimal(n) * (x * p1 - p0) / (x * x - ONE)
            dx = p1 / dp
            x -= dx
            if abs(dx) < Decimal("1e-60"):
                break
        p0 = ONE
        p1 = x
        for k in range(2, n + 1):
            pk = (Decimal(2 * k - 1) * x * p1 - Decimal(k - 1) * p0) / Decimal(k)
            p0, p1 = p1, pk
        dp = Decimal(n) * (x * p1 - p0) / (x * x - ONE)
        w = TWO / ((ONE - x * x) * dp * dp)
        pairs.append((-x, w))
        if i != n + 1 - i:
            pairs.append((x, w))
    pairs.sort(key=lambda pair: pair[0])
    return pairs


GL24 = gl_nodes_weights(24)


def prob_in_zone(eta: Decimal, al: Decimal | None, au: Decimal | None, u: Decimal) -> Decimal:
    if al is not None and au is not None:
        if al >= au:
            return ZERO
        if al > eta:
            return dec_upper_tail((al - eta) / u) - dec_upper_tail((au - eta) / u)
        if au < eta:
            return dec_upper_tail((eta - au) / u) - dec_upper_tail((eta - al) / u)
        return ONE - dec_upper_tail((au - eta) / u) - dec_upper_tail((eta - al) / u)
    if al is not None:
        return dec_upper_tail((al - eta) / u)
    if au is not None:
        return dec_upper_tail((eta - au) / u)
    return ONE


def prob_out_zone(eta: Decimal, al: Decimal | None, au: Decimal | None, u: Decimal) -> Decimal:
    if al is not None and au is not None:
        if al >= au:
            return ONE
        return dec_upper_tail((eta - al) / u) + dec_upper_tail((au - eta) / u)
    if al is not None:
        return dec_upper_tail((eta - al) / u)
    if au is not None:
        return dec_upper_tail((au - eta) / u)
    return ZERO


def prior_pdf(eta: Decimal, mu0: Decimal, sigma0: Decimal) -> Decimal:
    z = (eta - mu0) / sigma0
    return INV_SQRT_2PI / sigma0 * (-(HALF * z * z)).exp()


def integrate_interval(fn, a: Decimal, b: Decimal, panels: int = 2) -> Decimal:
    if b <= a:
        return ZERO
    h = (b - a) / Decimal(panels)
    total = ZERO
    for p in range(panels):
        lo = a + h * Decimal(p)
        hi = a + h * Decimal(p + 1)
        mid = HALF * (lo + hi)
        half = HALF * (hi - lo)
        s = ZERO
        for x, w in GL24:
            s += w * fn(mid + half * x)
        total += half * s
    return total


def integrate_region(
    fn,
    a: Decimal,
    b: Decimal,
    special_points: list[Decimal],
    u: Decimal,
) -> Decimal:
    if b <= a:
        return ZERO
    pts = [a, b]
    for sp in sorted(set(special_points)):
        if a < sp < b:
            pts.append(sp)
        for mult in (2, 10, 25):
            for sign in (-1, 1):
                p = sp + Decimal(sign * mult) * u
                if a < p < b:
                    pts.append(p)
    pts = sorted(set(pts))
    total = ZERO
    for lo, hi in zip(pts, pts[1:]):
        total += integrate_interval(fn, lo, hi, panels=2)
    return total


def compute_global_risk_50d(
    lsl: Decimal | None,
    usl: Decimal | None,
    guard_band: Decimal,
    mu0: Decimal,
    sigma0: Decimal,
    u: Decimal,
) -> tuple[Decimal, Decimal]:
    lo_dom = mu0 - Decimal(12) * sigma0
    hi_dom = mu0 + Decimal(12) * sigma0
    al = (lsl + guard_band) if lsl is not None else None
    au = (usl - guard_band) if usl is not None else None
    special = sorted({p for p in (lsl, usl, al, au, mu0) if p is not None})

    # Consumer risk: integral over eta NOT in [lsl, usl] of g0(eta) * P(Ym in A | eta)
    rc_fn = lambda eta: prior_pdf(eta, mu0, sigma0) * prob_in_zone(eta, al, au, u)
    rc = ZERO
    if lsl is not None:
        rc += integrate_region(rc_fn, lo_dom, min(hi_dom, lsl), special, u)
    if usl is not None:
        rc += integrate_region(rc_fn, max(lo_dom, usl), hi_dom, special, u)

    # Producer risk: integral over eta IN [lsl, usl] of g0(eta) * P(Ym not in A | eta)
    rp_fn = lambda eta: prior_pdf(eta, mu0, sigma0) * prob_out_zone(eta, al, au, u)
    in_lo = max(lo_dom, lsl) if lsl is not None else lo_dom
    in_hi = min(hi_dom, usl) if usl is not None else hi_dom
    rp = integrate_region(rp_fn, in_lo, in_hi, special, u)

    return rc, rp


def fmt_dec_50(d: Decimal) -> str:
    if d == 0:
        return "0"
    return f"{d:.50e}"


def build_specific_csv() -> str:
    rows = [
        ("conforms_high_p", "CONFORMS", "0.99", "0.01", ""),
        ("conforms_border", "CONFORMS", "0.95", "0.05", ""),
        ("conforms_exact_one", "CONFORMS", "1.0", "0.0", ""),
        ("conforms_subhalf", "CONFORMS", "0.75", "0.25", ""),
        ("nonconforms_low_p", "NONCONFORMS", "0.02", "", "0.02"),
        ("nonconforms_zero", "NONCONFORMS", "0.0", "", "0.0"),
        ("nonconforms_near_half", "NONCONFORMS", "0.49", "", "0.49"),
        ("indeterminate_mid", "INDETERMINATE", "0.5", "0.5", "0.5"),
        ("indeterminate_skewed", "INDETERMINATE", "0.875", "0.125", "0.875"),
        ("not_decidable_u_missing", "NOT_DECIDABLE", "", "", ""),
    ]
    buf = io.StringIO()
    writer = csv.writer(buf, lineterminator="\n")
    writer.writerow(["case", "outcome", "p", "expected_consumer", "expected_producer"])
    for case, outcome, p_str, _, _ in rows:
        if p_str == "":
            writer.writerow([case, outcome, "", "", ""])
            continue
        p = float(p_str)
        c = repr(1.0 - p) if outcome in ("CONFORMS", "INDETERMINATE") else ""
        pr = repr(p) if outcome in ("NONCONFORMS", "INDETERMINATE") else ""
        writer.writerow([case, outcome, repr(p), c, pr])
    return buf.getvalue()


def build_global_csv() -> str:
    # (case, lsl, usl, guard_band, mu0, sigma0, u)
    specs = [
        ("two_sided_simple_centered", "-1.0", "1.0", "0.0", "0.0", "0.5", "0.1"),
        ("two_sided_guard_pos_centered", "-1.0", "1.0", "0.2", "0.0", "0.5", "0.1"),
        ("two_sided_guard_neg_centered", "-1.0", "1.0", "-0.1", "0.0", "0.5", "0.1"),
        ("two_sided_shifted_prior", "9.95", "10.05", "0.01", "10.02", "0.025", "0.008"),
        ("two_sided_broad_prior", "-0.5", "0.5", "0.05", "0.1", "0.8", "0.12"),
        ("two_sided_tight_process", "-1.0", "1.0", "0.0", "0.0", "0.2", "0.05"),
        ("two_sided_large_u", "-1.0", "1.0", "0.3", "0.0", "0.5", "0.4"),
        ("two_sided_empty_zone", "-1.0", "1.0", "1.2", "0.0", "0.5", "0.1"),
        ("upper_only_simple", "", "5.0", "0.0", "4.2", "0.6", "0.15"),
        ("upper_only_guarded", "", "5.0", "0.25", "4.2", "0.6", "0.15"),
        ("lower_only_simple", "2.0", "", "0.0", "2.8", "0.5", "0.1"),
        ("lower_only_guarded", "2.0", "", "0.2", "2.8", "0.5", "0.1"),
        ("tiny_u_ac4_sanity", "-1.0", "1.0", "0.0", "0.0", "0.25", "0.000002"),
    ]
    buf = io.StringIO()
    writer = csv.writer(buf, lineterminator="\n")
    writer.writerow(["case", "lsl", "usl", "guard_band", "mu0", "sigma0", "u", "global_consumer", "global_producer"])
    for case, lsl_s, usl_s, gb_s, mu0_s, sigma0_s, u_s in specs:
        lsl = Decimal(lsl_s) if lsl_s else None
        usl = Decimal(usl_s) if usl_s else None
        gb = Decimal(gb_s)
        mu0 = Decimal(mu0_s)
        sigma0 = Decimal(sigma0_s)
        u = Decimal(u_s)
        rc, rp = compute_global_risk_50d(lsl, usl, gb, mu0, sigma0, u)
        writer.writerow([case, lsl_s, usl_s, gb_s, mu0_s, sigma0_s, u_s, fmt_dec_50(rc), fmt_dec_50(rp)])
    return buf.getvalue()


def print_gl16() -> None:
    gl16 = gl_nodes_weights(16)
    print("const GL16_NODES: [f64; 16] = [")
    for x, _ in gl16:
        fx = float(x)
        bits = struct.unpack(">Q", struct.pack(">d", fx))[0]
        print(f"    {fx!r}, // 0x{bits:016x}")
    print("];")
    print("const GL16_WEIGHTS: [f64; 16] = [")
    for _, w in gl16:
        fw = float(w)
        bits = struct.unpack(">Q", struct.pack(">d", fw))[0]
        print(f"    {fw!r}, // 0x{bits:016x}")
    print("];")


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description="Generate or check UNC.DEC.RISK fixtures.")
    parser.add_argument("--check", action="store_true", help="Verify committed CSVs match generator.")
    parser.add_argument("--gl16", action="store_true", help="Print 16-point Gauss-Legendre f64 constants.")
    args = parser.parse_args(argv)

    if args.gl16:
        print_gl16()
        return 0

    spec_text = build_specific_csv()
    glob_text = build_global_csv()

    if args.check:
        ok = True
        for path, expected in ((SPECIFIC_CSV, spec_text), (GLOBAL_CSV, glob_text)):
            if not path.is_file():
                print(f"MISSING {path}", file=sys.stderr)
                ok = False
            elif path.read_text(encoding="utf-8") != expected:
                print(f"DRIFT in {path}; rerun fixtures/unc/dec/risk/make_cases.py", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    OUT_DIR.mkdir(parents=True, exist_ok=True)
    SPECIFIC_CSV.write_text(spec_text, encoding="utf-8", newline="\n")
    GLOBAL_CSV.write_text(glob_text, encoding="utf-8", newline="\n")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
