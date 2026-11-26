#!/usr/bin/env python3
"""Independent reference for RPT.STATS.SPC.CONST (control-chart constants), stdlib only.

Computes the Shewhart control-chart constants for subgroup sizes n = 2..25 from the normal
distribution itself, never from a published table (no table text or layout is copied, D-038)
and never by running the Rust crate `fid-stats`. Writes three files:

  crates/fid-stats/src/spc_constants_normal_v1.json    the released table spc.constants.normal@1.0.0
                                                        (canonical JSON: sorted keys, no whitespace,
                                                        every stored value a decimal string)
  fixtures/rpt/stats/spc_constants_v1.sha256           "sha256:<hex>" of that file (AC-4, REQ-007)
  crates/fid-stats/src/spc_table.rs                     the same rows as a Rust table (generated)

Base constants (for X1..Xn independent standard normal, W = max - min the range):

  d2 = E[W]   = integral over x of 1 - Phi(x)^n - (1 - Phi(x))^n
  d3 = sd(W),   E[W^2] = 2 * integral_0^inf w * (1 - F(w)) dw,
                F(w) = n * integral over x of phi(x) * (Phi(x + w) - Phi(x))^(n - 1)
  c4 = sqrt(2 / (n - 1)) * Gamma(n / 2) / Gamma((n - 1) / 2)

The integrals use composite Simpson rules on a fixed grid (step H, and 2H as an error estimate);
c4 uses the exact half-integer recursion r(2) = 1/sqrt(pi), r(n + 1) = ((n - 1) / 2) / r(n) for
r(n) = Gamma(n/2) / Gamma((n-1)/2). The script refuses to write if the two step sizes disagree by
more than 1e-8, if the n = 2 values miss their closed forms (d2 = 2/sqrt(pi), E[W^2] = 2), or if a
value lies too close to a rounding boundary of its stored digits (1e-6 for d2 and d3, 1e-12 for c4,
1e-9 for a chart factor; c4 at n = 12 is 0.975350077..., 7.7e-9 above the 0.97535 tie).

Stored rounding (the leaf's open question, decided here, stated in the table's `rounding`):
d2 and d3 to 3 decimals and c4 to 4 decimals, as in the spec's seed table; the chart factors
A2, A3, B3, B4, D3, D4 and E2 to 3 decimals, computed in 50-digit decimal arithmetic FROM THE
STORED (ROUNDED) BASE CONSTANTS with the formulas of REQ-RPT.STATS.SPC.CONST-005 and then
rounded half-even. That is what makes AC-1 ("every stored value agrees with the formulas within
half a unit in its last stored digit") hold exactly; a factor rounded from unrounded base
constants can miss it (A2 at n = 2: 3/(1.128 sqrt 2) = 1.8806..., not 1.880).

The seed values printed in spec/rpt/stats/spc/const/NODE.md (n = 2..10) are transcribed below
as SEEDS; the script refuses to write if its own rounded values differ from any of them.

Every row is `UNVERIFIED` (no expert has chosen a source, REQ-002 and the leaf's open question),
and its citation says so instead of naming a table nobody checked.

Run:   python fixtures/rpt/stats/make_spc_constants.py
Check: python fixtures/rpt/stats/make_spc_constants.py --check
"""

import hashlib
import json
import math
import os
import sys
from decimal import ROUND_HALF_EVEN, Decimal, getcontext

ROOT = os.path.normpath(os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "..", ".."))
JSON_PATH = os.path.join(ROOT, "crates", "fid-stats", "src", "spc_constants_normal_v1.json")
SHA_PATH = os.path.join(ROOT, "fixtures", "rpt", "stats", "spc_constants_v1.sha256")
RS_PATH = os.path.join(ROOT, "crates", "fid-stats", "src", "spc_table.rs")

TABLE_ID = "spc.constants.normal"
VERSION = "1.0.0"
N_MIN, N_MAX = 2, 25
BASE_PLACES = {"d2": 3, "d3": 3, "c4": 4}
FACTOR_PLACES = 3

# Seed values from the leaf's Design table: n -> (d2, d3, c4).
SEEDS = {
    2: ("1.128", "0.853", "0.7979"),
    3: ("1.693", "0.888", "0.8862"),
    4: ("2.059", "0.880", "0.9213"),
    5: ("2.326", "0.864", "0.9400"),
    6: ("2.534", "0.848", "0.9515"),
    7: ("2.704", "0.833", "0.9594"),
    8: ("2.847", "0.820", "0.9650"),
    9: ("2.970", "0.808", "0.9693"),
    10: ("3.078", "0.797", "0.9727"),
}

CITATION = {
    "source": "none chosen (expert choice open); computed from the normal distribution by "
    "fixtures/rpt/stats/make_spc_constants.py",
    "edition": "none",
    "table": "none",
}
REVIEW = "UNVERIFIED"
ROUNDING = {
    "c4": "4 decimals, half-even",
    "d2": "3 decimals, half-even",
    "d3": "3 decimals, half-even",
    "factors": "3 decimals, half-even, computed from the stored d2, d3 and c4",
}

# Integration grid: x in [-X_HALF, X_HALF], w in [0, W_MAX], step H (Simpson needs even counts).
H = 0.01
X_HALF = 8.0
W_MAX = 14.0
# How far a computed base constant must lie from a rounding boundary of its stored digits: the
# integrals are good to about 1e-9 (step-halving check), c4 to a few ulp. A factor must lie at
# least 1e-9 (1e-6 of its last stored unit) from a tie. The Rust consistency test compares with
# a slack of 1e-12, far inside both margins.
INTEGRAL_MARGIN = 1e-6
C4_MARGIN = 1e-12


def phi_cdf(x):
    return 0.5 * math.erfc(-x / math.sqrt(2.0))


def simpson_weights(m):
    """Composite Simpson weights (without the h/3 factor) for m + 1 points, m even."""
    if m % 2:
        raise SystemExit("odd Simpson interval count")
    w = [2.0 if i % 2 == 0 else 4.0 for i in range(m + 1)]
    w[0] = w[m] = 1.0
    return w


def base_constants(h):
    """d2[n], e_w2[n] (E[W^2]) for n in N_MIN..N_MAX on step h."""
    mx = int(round(2 * X_HALF / h))
    mw = int(round(W_MAX / h))
    xs = [-X_HALF + i * h for i in range(mx + 1)]
    # Phi on the extended grid x_0 .. x_mx + mw, so Phi(x_i + w_j) = big[i + j].
    big = [phi_cdf(-X_HALF + k * h) for k in range(mx + mw + 1)]
    upper = [0.5 * math.erfc(x / math.sqrt(2.0)) for x in xs]  # 1 - Phi(x), accurate in the tail
    dens = [math.exp(-0.5 * x * x) / math.sqrt(2.0 * math.pi) for x in xs]
    wx = simpson_weights(mx)
    ww = simpson_weights(mw)
    lower = big[: mx + 1]
    d2 = {}
    ew2 = {}
    for n in range(N_MIN, N_MAX + 1):
        s = 0.0
        for i in range(mx + 1):
            s += wx[i] * (1.0 - lower[i] ** n - upper[i] ** n)
        d2[n] = s * h / 3.0
        cw = [wx[i] * dens[i] for i in range(mx + 1)]
        tot = 0.0
        for j in range(mw + 1):
            shifted = big[j : j + mx + 1]
            inner = 0.0
            for c, p_hi, p_lo in zip(cw, shifted, lower):
                inner += c * (p_hi - p_lo) ** (n - 1)
            f = n * inner * h / 3.0
            tot += ww[j] * (j * h) * (1.0 - f)
        ew2[n] = 2.0 * tot * h / 3.0
    return d2, ew2


def c4_values():
    out = {}
    r = 1.0 / math.sqrt(math.pi)  # r(2) = Gamma(1) / Gamma(1/2)
    for n in range(N_MIN, N_MAX + 1):
        out[n] = math.sqrt(2.0 / (n - 1)) * r
        r = ((n - 1) / 2.0) / r  # r(n + 1)
    return out


def rounded(x, places, margin):
    """x (a float) to `places` decimals, refusing a value within `margin` of a rounding boundary."""
    scaled = x * 10**places
    frac = scaled - math.floor(scaled)
    if abs(frac - 0.5) < margin * 10**places:
        raise SystemExit(f"{x!r} is within {margin} of a rounding boundary at {places} places")
    q = Decimal(1).scaleb(-places)
    return Decimal(repr(x)).quantize(q, rounding=ROUND_HALF_EVEN)


def factor(value, name, n):
    q = Decimal(1).scaleb(-FACTOR_PLACES)
    # A tie at the stored digit would make the half-unit check ambiguous; refuse it.
    frac = (value * 10**FACTOR_PLACES) % 1
    if abs(frac - Decimal("0.5")) < Decimal("1e-6"):
        raise SystemExit(f"{name} at n = {n} is a rounding tie: {value}")
    return value.quantize(q, rounding=ROUND_HALF_EVEN)


def build_rows():
    d2a, ew2a = base_constants(H)
    d2b, ew2b = base_constants(2 * H)
    c4 = c4_values()
    worst = 0.0
    for n in range(N_MIN, N_MAX + 1):
        worst = max(worst, abs(d2a[n] - d2b[n]), abs(ew2a[n] - ew2b[n]))
    if worst > 1e-8:
        raise SystemExit(f"step H and 2H disagree by {worst:.3e}")
    if abs(d2a[2] - 2.0 / math.sqrt(math.pi)) > 1e-10 or abs(ew2a[2] - 2.0) > 1e-9:
        raise SystemExit("n = 2 closed forms missed")
    getcontext().prec = 50
    three = Decimal(3)
    one = Decimal(1)
    rows = []
    for n in range(N_MIN, N_MAX + 1):
        d3 = math.sqrt(ew2a[n] - d2a[n] ** 2)
        b_d2 = rounded(d2a[n], BASE_PLACES["d2"], INTEGRAL_MARGIN)
        b_d3 = rounded(d3, BASE_PLACES["d3"], INTEGRAL_MARGIN)
        b_c4 = rounded(c4[n], BASE_PLACES["c4"], C4_MARGIN)
        if n in SEEDS and (str(b_d2), str(b_d3), str(b_c4)) != SEEDS[n]:
            raise SystemExit(f"n = {n}: computed {(str(b_d2), str(b_d3), str(b_c4))} != seed {SEEDS[n]}")
        sn = Decimal(n).sqrt()
        k = (one - b_c4 * b_c4).sqrt() / b_c4
        zero = Decimal(0)
        row = {
            "n": n,
            "d2": str(b_d2),
            "d3": str(b_d3),
            "c4": str(b_c4),
            "a2": str(factor(three / (b_d2 * sn), "A2", n)),
            "a3": str(factor(three / (b_c4 * sn), "A3", n)),
            "b3": str(factor(max(zero, one - three * k), "B3", n)),
            "b4": str(factor(one + three * k, "B4", n)),
            "d3_factor": str(factor(max(zero, one - three * b_d3 / b_d2), "D3", n)),
            "d4": str(factor(one + three * b_d3 / b_d2, "D4", n)),
            "citation": dict(CITATION),
            "review": REVIEW,
        }
        if n == 2:
            row["e2"] = str(factor(three / b_d2, "E2", n))
        rows.append(row)
    return rows, worst


def table_bytes(rows):
    doc = {"table_id": TABLE_ID, "version": VERSION, "rounding": ROUNDING, "rows": rows}
    return json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def rust_value(text):
    return f'sv("{text}", {text})'


def rust_source(rows):
    out = [
        "// @generated by fixtures/rpt/stats/make_spc_constants.py from its own normal-distribution",
        "// integrals; do not edit. The test `spc_constants` checks that these rows serialise to the",
        "// embedded `src/spc_constants_normal_v1.json` byte for byte.",
        "",
        "use crate::spc::{sv, Citation, ReviewStatus, SpcRow};",
        "",
        "const CITATION: Citation = Citation {",
        f'    source: "{CITATION["source"]}",',
        f'    edition: "{CITATION["edition"]}",',
        f'    table: "{CITATION["table"]}",',
        "};",
        "",
        "/// The rows of `spc.constants.normal@1.0.0`, n = 2..25 in order.",
        "#[rustfmt::skip]",
        f"pub(crate) static ROWS: [SpcRow; {len(rows)}] = [",
    ]
    for r in rows:
        e2 = f"Some({rust_value(r['e2'])})" if "e2" in r else "None"
        out.append(
            f"    SpcRow {{ n: {r['n']}, d2: {rust_value(r['d2'])}, d3: {rust_value(r['d3'])}, "
            f"c4: {rust_value(r['c4'])}, a2: {rust_value(r['a2'])}, a3: {rust_value(r['a3'])}, "
            f"b3: {rust_value(r['b3'])}, b4: {rust_value(r['b4'])}, d3_factor: {rust_value(r['d3_factor'])}, "
            f"d4: {rust_value(r['d4'])}, e2: {e2}, citation: CITATION, review: ReviewStatus::Unverified }},"
        )
    out.append("];")
    out.append("")
    return "\n".join(out).encode("ascii")


def main():
    check = "--check" in sys.argv[1:]
    rows, worst = build_rows()
    data = table_bytes(rows)
    sha = ("sha256:" + hashlib.sha256(data).hexdigest() + "\n").encode("ascii")
    rs = rust_source(rows)
    outputs = [(JSON_PATH, data), (SHA_PATH, sha), (RS_PATH, rs)]
    bad = False
    for path, content in outputs:
        if check:
            try:
                with open(path, "rb") as f:
                    have = f.read()
            except OSError:
                have = None
            if have != content:
                print(f"STALE {os.path.relpath(path, ROOT)}")
                bad = True
        else:
            os.makedirs(os.path.dirname(path), exist_ok=True)
            with open(path, "wb") as f:
                f.write(content)
    if bad:
        return 1
    print(f"{'checked' if check else 'wrote'} {len(rows)} rows; step-halving difference {worst:.2e}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
