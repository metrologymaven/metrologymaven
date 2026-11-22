#!/usr/bin/env python3
"""Independent reference generator for UNC.PROP.VALID fixtures (stdlib only; CC-BY-4.0).

Nothing here imports or runs the Rust crate. Every f64 is recorded alongside its exact
16-hex-digit IEEE-754 bit representation so tests verify cross-target bit-identity.

Generates:
  fixtures/unc/valid/linear_sum.json      AC-1: Y = X1 + X2, normal inputs, seed 0xA11CE -> validated = true
  fixtures/unc/valid/square_near_zero.json AC-2: Y = X^2, X ~ N(0.1, 1) near zero, seed 0xA11CE -> validated = false
  fixtures/unc/valid/delta_cases.csv      AC-3: exact delta = 0.5 * 10^l for u(y) and n_dig in {1, 2, 3}
  fixtures/unc/valid/README.md            Provenance and license (CC-BY-4.0)

Usage:
  python3 fixtures/unc/valid/make_fixtures.py          # write files
  python3 fixtures/unc/valid/make_fixtures.py --check  # verify files on disk match byte-for-byte
"""

import json
import math
import random
import statistics
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
LICENSE = "CC-BY-4.0"
SEED = 0xA11CE
TRIALS = 200_000
P_COVERAGE = 0.95
# Exact inverse normal quantile Phi^{-1}((1 + 0.95) / 2) via stdlib NormalDist.
Z_95 = statistics.NormalDist(0.0, 1.0).inv_cdf(0.5 * (1.0 + P_COVERAGE))


def hexbits(x: float) -> str:
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def exact_tolerance(u: float, n_dig: int):
    """Exact (exp10, delta_f64, delta_frac) for finite u >= 0 and 1 <= n_dig <= 17.

    Expresses u rounded half-up to n_dig significant decimal digits as c * 10^l,
    raising l by 1 when rounding carries to 10^n_dig. delta = 0.5 * 10^l.
    """
    uf = Fraction(u)
    assert uf >= 0
    if uf == 0:
        return None, 0.0, Fraction(0)
    # Exact decimal exponent E with 10^E <= u < 10^(E + 1).
    # Start from float approximation and adjust by exact rational comparisons.
    e = math.floor(math.log10(u))
    while Fraction(10) ** e > uf:
        e -= 1
    while Fraction(10) ** (e + 1) <= uf:
        e += 1
    l = e - n_dig + 1
    ten_n = 10 ** n_dig
    # Carry iff u >= (10^n_dig - 1/2) * 10^l
    threshold = Fraction(2 * ten_n - 1, 2) * (Fraction(10) ** l)
    if uf >= threshold:
        l += 1
    delta_frac = Fraction(1, 2) * (Fraction(10) ** l)
    delta_f64 = float(f"5e{l - 1}")
    return l, delta_f64, delta_frac


def validate_exact(y: float, u_p: float, y_low: float, y_high: float, u_mcm: float, n_dig: int):
    lpu_low = y - u_p
    lpu_high = y + u_p
    d_low = abs(lpu_low - y_low)
    d_high = abs(lpu_high - y_high)
    exp10, delta_f64, delta_frac = exact_tolerance(u_mcm, n_dig)
    validated = Fraction(d_low) <= delta_frac and Fraction(d_high) <= delta_frac
    return {
        "validated": validated,
        "d_low": d_low,
        "d_low_hex": hexbits(d_low),
        "d_high": d_high,
        "d_high_hex": hexbits(d_high),
        "delta": delta_f64,
        "delta_hex": hexbits(delta_f64),
        "exp10": exp10,
    }


def mcm_summary(samples: list[float], p: float):
    """Symmetric coverage interval and sample standard uncertainty (JCGM 101 §7.7)."""
    m = len(samples)
    mean = math.fsum(samples) / m
    var = math.fsum((x - mean) ** 2 for x in samples) / (m - 1)
    u = math.sqrt(var)
    samples.sort()
    q = round(p * m)
    r = (m - q) // 2
    y_low = samples[r]
    y_high = samples[r + q]
    return mean, u, y_low, y_high


def build_linear_sum() -> str:
    """AC-1: Y = X1 + X2, X1 ~ N(10.0, 0.3), X2 ~ N(5.0, 0.4), seed 0xA11CE."""
    rng = random.Random(SEED)
    samples = [rng.gauss(10.0, 0.3) + rng.gauss(5.0, 0.4) for _ in range(TRIALS)]
    y_mcm, u_mcm, y_low, y_high = mcm_summary(samples, P_COVERAGE)

    # Analytical LPU: y = 15.0, u(y) = sqrt(0.3^2 + 0.4^2) = 0.5, U_0.95 = z_0.95 * 0.5
    y_lpu = 15.0
    u_lpu = 0.5
    u_p = Z_95 * u_lpu
    n_dig = 2
    expected = validate_exact(y_lpu, u_p, y_low, y_high, u_mcm, n_dig)
    assert expected["validated"] is True, expected

    doc = {
        "title": "UNC.PROP.VALID AC-1: linear additive model Y = X1 + X2 (both normal, seed 0xA11CE)",
        "license": LICENSE,
        "model": "Y = X1 + X2; X1 ~ N(10.0, 0.3), X2 ~ N(5.0, 0.4)",
        "seed": "0xA11CE",
        "trials": TRIALS,
        "input": {
            "y": y_lpu,
            "y_hex": hexbits(y_lpu),
            "u_p": u_p,
            "u_p_hex": hexbits(u_p),
            "p_lpu": P_COVERAGE,
            "p_lpu_hex": hexbits(P_COVERAGE),
            "y_low": y_low,
            "y_low_hex": hexbits(y_low),
            "y_high": y_high,
            "y_high_hex": hexbits(y_high),
            "p_mcm": P_COVERAGE,
            "p_mcm_hex": hexbits(P_COVERAGE),
            "u_mcm": u_mcm,
            "u_mcm_hex": hexbits(u_mcm),
            "y_mcm_mean": y_mcm,
            "n_dig": n_dig,
        },
        "expected": expected,
    }
    return json.dumps(doc, indent=2) + "\n"


def build_square_near_zero() -> str:
    """AC-2: Y = X^2, X ~ N(0.1, 1.0) near zero (non-empty LPU interval), seed 0xA11CE."""
    rng = random.Random(SEED)
    mu_x = 0.1
    sigma_x = 1.0
    samples = []
    for _ in range(TRIALS):
        x = rng.gauss(mu_x, sigma_x)
        samples.append(x * x)
    y_mcm, u_mcm, y_low, y_high = mcm_summary(samples, P_COVERAGE)

    # First-order LPU at x0 = 0.1: y = x0^2 = 0.01, dy/dx = 2*x0 = 0.2, u_lpu = 0.2 * 1.0 = 0.2
    y_lpu = mu_x * mu_x
    u_lpu = abs(2.0 * mu_x) * sigma_x
    u_p = Z_95 * u_lpu
    n_dig = 2
    expected = validate_exact(y_lpu, u_p, y_low, y_high, u_mcm, n_dig)
    assert expected["validated"] is False, expected

    doc = {
        "title": "UNC.PROP.VALID AC-2: strongly nonlinear model Y = X^2 near zero (X ~ N(0.1, 1.0), seed 0xA11CE)",
        "license": LICENSE,
        "model": "Y = X^2; X ~ N(0.1, 1.0) (near zero so first-order LPU has positive U_p while failing validation)",
        "seed": "0xA11CE",
        "trials": TRIALS,
        "input": {
            "y": y_lpu,
            "y_hex": hexbits(y_lpu),
            "u_p": u_p,
            "u_p_hex": hexbits(u_p),
            "p_lpu": P_COVERAGE,
            "p_lpu_hex": hexbits(P_COVERAGE),
            "y_low": y_low,
            "y_low_hex": hexbits(y_low),
            "y_high": y_high,
            "y_high_hex": hexbits(y_high),
            "p_mcm": P_COVERAGE,
            "p_mcm_hex": hexbits(P_COVERAGE),
            "u_mcm": u_mcm,
            "u_mcm_hex": hexbits(u_mcm),
            "y_mcm_mean": y_mcm,
            "n_dig": n_dig,
        },
        "expected": expected,
    }
    return json.dumps(doc, indent=2) + "\n"


def build_delta_cases() -> str:
    """AC-3: hand-verified exact delta values for u(y) and n_dig in {1, 2, 3}."""
    u_values = [
        "0.0123",
        "1.0e-6",
        "9.96e-7",
        "9.95e-7",
        "9.94e-7",
        "0.0995",
        "0.0994",
        "0.005",
        "0.5",
        "1.0",
        "9.5",
        "9.4",
        "99.5",
        "123.45",
        "1.0e-300",
        "1.0e300",
        "0.0",
    ]
    lines = ["u_mcm,u_hex,n_dig,exp10,delta,delta_hex"]
    for u_str in u_values:
        u_val = float(u_str)
        for n_dig in (1, 2, 3):
            exp10, delta_f64, _ = exact_tolerance(u_val, n_dig)
            exp_str = "" if exp10 is None else str(exp10)
            delta_str = "0.0" if exp10 is None else f"5e{exp10 - 1}"
            lines.append(
                f"{u_str},{hexbits(u_val)},{n_dig},{exp_str},{delta_str},{hexbits(delta_f64)}"
            )
    return "\n".join(lines) + "\n"


def build_readme() -> str:
    return (
        "# UNC.PROP.VALID test fixtures\n"
        "\n"
        "- **Spec node:** `UNC.PROP.VALID` (`unc.prop.validate_lpu@1.0.0`)\n"
        "- **License:** CC-BY-4.0 (synthetic reference models and exact decimal tolerance tables)\n"
        "- **Generator:** `python3 fixtures/unc/valid/make_fixtures.py` (stdlib Python 3 only; verify with `--check`)\n"
        "\n"
        "## Files\n"
        "\n"
        "- `linear_sum.json` (AC-1): `Y = X1 + X2` with `X1 ~ N(10.0, 0.3)` and `X2 ~ N(5.0, 0.4)`,\n"
        "  200 000 MCM trials seeded with `0xA11CE`, compared against analytical LPU at `p = 0.95` and\n"
        "  `n_dig = 2`. Expected outcome: `validated = true`.\n"
        "- `square_near_zero.json` (AC-2): `Y = X^2` with `X ~ N(0.1, 1.0)` (mean near zero so\n"
        "  first-order LPU has a strictly non-empty interval `U_p > 0` per REQ-UNC.PROP.VALID-003,\n"
        "  while strong nonlinearity makes LPU disagree sharply with MCM), 200 000 MCM trials seeded\n"
        "  with `0xA11CE` at `p = 0.95` and `n_dig = 2`. Expected outcome: `validated = false`.\n"
        "- `delta_cases.csv` (AC-3): exact decimal tolerances `delta = 0.5 * 10^l` computed with\n"
        "  Python `fractions.Fraction` across `u(y)` including `0.0123`, `1.0e-6`, `9.96e-7` and\n"
        "  rounding-carry boundaries for `n_dig` in `{1, 2, 3}`.\n"
    )


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = {
        HERE / "linear_sum.json": build_linear_sum(),
        HERE / "square_near_zero.json": build_square_near_zero(),
        HERE / "delta_cases.csv": build_delta_cases(),
        HERE / "README.md": build_readme(),
    }
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            actual = path.read_text(encoding="utf-8")
            if actual != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    HERE.mkdir(parents=True, exist_ok=True)
    for path, content in files.items():
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
