#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for UNC.RDL.GEN.HPREC (AC-1, AC-2).

Uses Python's arbitrary-precision `decimal` module (100 decimal digits, ROUND_HALF_EVEN)
independently of `fid-rdl-gen` to produce:
1. `fixtures/rdl/gen/hprec/round16.csv`: 500 values (including exact halfway ties to even/odd,
   carry-over 9.999...95 ties, negative values, powers of 2, tiny & huge exponents, zero)
   with expected 16-significant-digit scientific decimal and expected IEEE-754 `f64` hex bits.
2. `fixtures/rdl/gen/hprec/circle_mp.txt`: 20 2D points rounded with `round16`, pre-rounding
   initial guess `(cx0, cy0, r0)`, and the independent 100-digit Gauss-Newton circle-fit
   reference `(cx, cy, r)` to 60 significant digits plus final relative gradient norm.

Usage: python3 fixtures/rdl/gen/hprec/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

import decimal
import struct
import sys
from decimal import Decimal, localcontext
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent


def dec_pi() -> Decimal:
    """100-digit pi via Machin's formula in pure Decimal (zero platform libm)."""
    def atan_recip(n: int) -> Decimal:
        n2 = n * n
        term = Decimal(1) / Decimal(n)
        total = term
        k = 1
        while term != 0:
            term = term / Decimal(n2)
            add = term / Decimal(2 * k + 1)
            if add == 0:
                break
            total = total - add if (k % 2 == 1) else total + add
            k += 1
        return total

    return Decimal(4) * (Decimal(4) * atan_recip(5) - atan_recip(239))


def dec_sin_cos(x: Decimal) -> tuple[Decimal, Decimal]:
    """Pure-Decimal Taylor series for (sin(x), cos(x)) for |x| <= 10 (zero platform libm)."""
    x2 = x * x
    s_term = x
    s_sum = x
    c_term = Decimal(1)
    c_sum = Decimal(1)
    k = 1
    while k < 120:
        s_term = -s_term * x2 / Decimal((2 * k) * (2 * k + 1))
        c_term = -c_term * x2 / Decimal((2 * k - 1) * (2 * k))
        if s_term == 0 and c_term == 0:
            break
        s_sum += s_term
        c_sum += c_term
        k += 1
    return s_sum, c_sum


def round16_dec(d: Decimal) -> tuple[str, str]:
    """Exact 16-significant-digit ROUND_HALF_EVEN decimal + exact nearest f64 hex."""
    with localcontext() as ctx:
        ctx.prec = 16
        ctx.rounding = decimal.ROUND_HALF_EVEN
        r = +d
    if r == 0:
        dec_str = "0.000000000000000e+00"
        return dec_str, "0x0000000000000000"
    neg = r.is_signed()
    r_abs = abs(r)
    tup = r_abs.as_tuple()
    digits = list(tup.digits)
    while len(digits) < 16:
        digits.append(0)
    digits = digits[:16]
    e10 = tup.exponent + len(tup.digits) - 1
    mant = f"{digits[0]}.{''.join(str(x) for x in digits[1:])}"
    sign = "-" if neg else ""
    exp_str = f"e+{e10:02d}" if e10 >= 0 else f"e-{(-e10):02d}"
    dec_str = f"{sign}{mant}{exp_str}"
    fval = float(dec_str)
    if fval == 0.0:
        fval = 0.0
    hex_str = "0x" + struct.pack(">d", fval).hex()
    return dec_str, hex_str


def build_round16_csv() -> bytes:
    """500 exact-binary and general decimal test inputs covering ties-to-even and wide exponents."""
    cases: list[str] = [
        "0",
        "1",
        "-1",
        "10000000000000005",  # exact integer tie -> even (0 stays 0)
        "10000000000000015",  # exact integer tie -> even (1 -> 2)
        "10000000000000025",  # exact integer tie -> even (2 stays 2)
        "10000000000000035",  # exact integer tie -> even (3 -> 4)
        "-10000000000000005",
        "-10000000000000015",
        "99999999999999995",  # exact integer tie with carry -> 1.000000000000000e+17
        "-99999999999999995",
        "99999999999999985",  # exact integer tie -> even (8 stays 8)
        "99999999999999994",
        "99999999999999996",
        "50000000000000005",
        "50000000000000015",
        "0.5",
        "0.25",
        "0.125",
        "0.0625",
        "0.1",
        "0.3",
        "3.14159265358979323846264338327950288419716939937510",
        "2.71828182845904523536028747135266249775724709369995",
        "1.41421356237309504880168872420969807856967187537694",
        "1e-200",
        "1e+200",
        "1.7976931348623157e+308",
        "2.2250738585072014e-308",
    ]
    # Exact integer halfway ties (p=0) and exact binary-scaled multiples across powers of 2.
    for d in range(10):
        for p in (0, 1, 2, 3, 4, 8, 16, 32):
            base = (1234567890123450 + d) * 10 + 5
            val = Decimal(base) * (Decimal(2) ** p)
            cases.append(str(val))
            cases.append(str(-val))
    # Exact dyadic fractions k / 2^p across negative exponents.
    for p in range(1, 65):
        val = Decimal(3 * p + 1) / (Decimal(2) ** p)
        cases.append(f"{val:.60g}")
    # General non-tie high-precision decimals across wide exponent range until 500 total.
    x = 123456789
    while len(cases) < 500:
        x = (x * 6364136223846793005 + 1442695040888963407) & ((1 << 64) - 1)
        sign = "-" if (x & 1) else ""
        # 25-digit mantissa avoiding exact ...500000000 tie
        mant = 10**24 + (x >> 4) % (9 * 10**24)
        if mant % 10 == 0:
            mant += 3
        exp = ((x >> 40) % 401) - 200
        s = f"{sign}{str(mant)[0]}.{str(mant)[1:]}e{exp:+03d}"
        cases.append(s)

    assert len(cases) == 500
    lines = ["input,expected_decimal,expected_hex"]
    for c in cases:
        d = Decimal(c)
        dec_str, hex_str = round16_dec(d)
        lines.append(f"{c},{dec_str},{hex_str}")
    return ("\n".join(lines) + "\n").encode("utf-8")


def solve_linear_3x3(a: list[list[Decimal]], b: list[Decimal]) -> list[Decimal]:
    """Gaussian elimination with partial pivoting in high-precision Decimal."""
    n = 3
    m = [row[:] + [b[i]] for i, row in enumerate(a)]
    for col in range(n):
        piv = max(range(col, n), key=lambda r: abs(m[r][col]))
        m[col], m[piv] = m[piv], m[col]
        for r in range(col + 1, n):
            f = m[r][col] / m[col][col]
            for c in range(col, n + 1):
                m[r][c] -= f * m[col][c]
    x = [Decimal(0)] * n
    for i in range(n - 1, -1, -1):
        s = m[i][n] - sum(m[i][j] * x[j] for j in range(i + 1, n))
        x[i] = s / m[i][i]
    return x


def build_circle_mp() -> bytes:
    """20 rounded points near a circle + independent 100-digit Gauss-Newton reference."""
    with localcontext() as ctx:
        ctx.prec = 100
        pi = dec_pi()
        cx0 = Decimal("12.345678901234567")
        cy0 = Decimal("-4.567890123456789")
        r0 = Decimal("7.890123456789012")

        pts: list[tuple[str, str]] = []
        pts_d: list[tuple[Decimal, Decimal]] = []
        for i in range(20):
            # Pure-Decimal deterministic angle + small radial perturbation (OS-independent).
            s_pert, _ = dec_sin_cos(Decimal(i) * Decimal("1.7"))
            theta = Decimal(2) * pi * Decimal(i) / Decimal(20) + Decimal("0.03") * s_pert
            sin_t, cos_t = dec_sin_cos(theta)
            dr = Decimal(i % 5 - 2) * Decimal("0.0004")
            px_raw = cx0 + (r0 + dr) * cos_t
            py_raw = cy0 + (r0 + dr) * sin_t
            px_s, _ = round16_dec(px_raw)
            py_s, _ = round16_dec(py_raw)
            pts.append((px_s, py_s))
            pts_d.append((Decimal(px_s), Decimal(py_s)))

        # Independent 100-digit Gauss-Newton on geometric circle residual r_i = dist(p_i, c) - R.
        cx, cy, rad = cx0, cy0, r0
        grad_norm = Decimal(1)
        for _ in range(50):
            jtj = [[Decimal(0)] * 3 for _ in range(3)]
            jtr = [Decimal(0)] * 3
            r_sq = Decimal(0)
            j_sq = Decimal(0)
            for px, py in pts_d:
                dx = px - cx
                dy = py - cy
                d = (dx * dx + dy * dy).sqrt()
                ri = d - rad
                jrow = [-dx / d, -dy / d, Decimal(-1)]
                r_sq += ri * ri
                for a_idx in range(3):
                    j_sq += jrow[a_idx] * jrow[a_idx]
                    jtr[a_idx] += jrow[a_idx] * ri
                    for b_idx in range(3):
                        jtj[a_idx][b_idx] += jrow[a_idx] * jrow[b_idx]
            g_norm = (jtr[0] ** 2 + jtr[1] ** 2 + jtr[2] ** 2).sqrt()
            scale = max(Decimal(1), j_sq.sqrt() * r_sq.sqrt())
            grad_norm = g_norm / scale
            if grad_norm < Decimal("1e-75"):
                break
            dp = solve_linear_3x3(jtj, [-v for v in jtr])
            cx += dp[0]
            cy += dp[1]
            rad += dp[2]

        assert grad_norm < Decimal("1e-75"), f"did not converge: {grad_norm}"

        def fmt60(v: Decimal) -> str:
            with localcontext() as c2:
                c2.prec = 60
                c2.rounding = decimal.ROUND_HALF_EVEN
                r = +v
            neg = r.is_signed()
            tup = abs(r).as_tuple()
            digs = list(tup.digits)
            while len(digs) < 60:
                digs.append(0)
            digs = digs[:60]
            e10 = tup.exponent + len(tup.digits) - 1
            sign = "-" if neg else ""
            exp_s = f"e+{e10:02d}" if e10 >= 0 else f"e-{(-e10):02d}"
            return f"{sign}{digs[0]}.{''.join(str(x) for x in digs[1:])}{exp_s}"

        lines = [
            "# Independent 100-digit Python Decimal reference for UNC.RDL.GEN.HPREC AC-2",
            f"p0_cx={cx0}",
            f"p0_cy={cy0}",
            f"p0_r={r0}",
            f"ref_cx={fmt60(cx)}",
            f"ref_cy={fmt60(cy)}",
            f"ref_r={fmt60(rad)}",
            f"ref_grad_norm={grad_norm:.6e}",
        ]
        for idx, (px_s, py_s) in enumerate(pts):
            lines.append(f"pt_{idx}={px_s},{py_s}")
        return ("\n".join(lines) + "\n").encode("utf-8")


def main() -> int:
    targets = {
        HERE / "round16.csv": build_round16_csv(),
        HERE / "circle_mp.txt": build_circle_mp(),
    }
    if "--check" in sys.argv[1:]:
        stale = []
        for path, want in sorted(targets.items()):
            if not path.is_file() or path.read_bytes() != want:
                stale.append(str(path.relative_to(ROOT)))
        if stale:
            print(f"FAIL: stale hprec fixtures: {', '.join(stale)}")
            return 1
        print("fixtures/rdl/gen/hprec: ok")
        return 0
    for path, want in sorted(targets.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(want)
    print(f"wrote {len(targets)} fixture files under {HERE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
