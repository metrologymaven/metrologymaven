#!/usr/bin/env python3
"""Independent reference for UNC.DEC.EVAL (stdlib only; CC-BY-4.0 project data).

Writes, from the rules of spec/unc/dec/eval/NODE.md (Design, pinned order) and never by
running the Rust code:

cases.csv        evaluation cases (AC-1 .. AC-6): inputs, expected outcome, reason, p, the
                 conservative bounds p_lo and p_hi, U, flags or error code
samples/*.bin    Monte Carlo sample sets in format mcm-samples@1 (UNC.PROP.MCM.SAMP), built
                 from integer arithmetic and single IEEE additions only, so they are the
                 same bytes on every host
erfc.csv         x and erfc(x) (correctly rounded to binary64) for the unit test of the
                 kernel's complementary error function

Conservative normal p (review of #422). The kernel decides a conformance-probability rule
from bounds p_lo <= p <= p_hi, not from the nominal p. This script builds the same bounds by
the pinned construction (each limit's binary64 floor and ceiling brackets; z bounds by
each limit split exactly as f + delta with f its binary64 floor and delta between two adjacent
binary64 values (found here by exact rational arithmetic, in the kernel by bisection with
exact comparisons); z bounds by binary64 subtraction, addition and division, each then moved
one ulp outward, which Python floats do exactly as the kernel; tails widened by TAIL_ERR, the bounds by ARITH_ERR), but evaluates
the tails at 100 digits and combines them exactly. It checks that the exact p lies inside
the bounds and that no bound is within 1e-13 of a decision threshold, so the kernel's own
erfc error (at most 2e-15) cannot change an expected outcome.

Samples (maintainer decision 2026-11-21). Rows with a sample file are run through the
kernel's digest-checked entry point (evaluate_with_samples); `samples_digest` is empty for
the file's own SHA-256 or `wrong`. A decision needs T >= max(100, 20 / (1 - p_min))
[unverified]; with fewer trials a conformance-probability rule is INDETERMINATE with flag
samples_too_few.

Exactness. Every comparison of a value with a limit, the guard band, the capability ratio
and p against p_min is done here with fractions.Fraction (each binary64 value is an exact
dyadic rational, each decimal an exact rational). The normal-approximation probability is
computed with decimal.Decimal at 100 digits from an alternating Taylor series of erf
(|x| <= 7) and a 400-level continued fraction (|x| > 7); the kernel uses a different,
positive-term series and a 60-level continued fraction, so the two are independent.

Hand labels. Most cases carry a hand-stated expected outcome (`hand`); the computed result
must agree with it or the script stops.

Run:  python fixtures/unc/dec/eval/make_cases.py           (write)
      python fixtures/unc/dec/eval/make_cases.py --check   (fail if any file is stale)
"""

import hashlib
import json
import math
import struct
import sys
from decimal import Decimal, localcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
CASES = HERE / "cases.csv"
ERFC = HERE / "erfc.csv"
SAMPLES = HERE / "samples"

PREC = 100
PI = Decimal(
    "3.14159265358979323846264338327950288419716939937510582097494459230781640628620899"
    "8628034825342117067982148086513282306647"
)

up = lambda x: math.nextafter(x, math.inf)
down = lambda x: math.nextafter(x, -math.inf)

# ------------------------------------------------------------------ high-precision erfc


def erfc_hp(x):
    """erfc of a Decimal or Fraction, to about 90 significant digits."""
    with localcontext() as c:
        c.prec = PREC
        if isinstance(x, Fraction):
            x = Decimal(x.numerator) / Decimal(x.denominator)
        if x < 0:
            return 2 - erfc_hp(-x)
        if x <= 7:
            x2 = x * x
            s = Decimal(0)
            term = x  # (-1)^n x^(2n+1) / n!
            n = 0
            while True:
                t = term / (2 * n + 1)
                s += t
                if n > 10 and abs(t) < Decimal(10) ** -(PREC - 5):
                    break
                n += 1
                term = -term * x2 / n
            return 1 - 2 / PI.sqrt() * s
        t = x
        for k in range(400, 0, -1):
            t = x + Decimal(k) / 2 / t
        return (-(x * x)).exp() / PI.sqrt() / t


def upper_tail(z):
    """P(Z > z) for a Fraction z, as a Decimal."""
    with localcontext() as c:
        c.prec = PREC
        zd = Decimal(z.numerator) / Decimal(z.denominator)
        return erfc_hp(zd / Decimal(2).sqrt()) / 2


# ------------------------------------------------------------------ sample sets

MODEL = "sha256:" + hashlib.sha256(b"um:eval").hexdigest()
OTHER_MODEL = "sha256:" + hashlib.sha256(b"um:other").hexdigest()
INPUTS = "sha256:" + hashlib.sha256(b"inputs:eval").hexdigest()
PROFILE = "sha256:" + hashlib.sha256(b"profile:eval").hexdigest()
SEED = 0xDEC0001

LSL_S = "0.00999"
USL_S = "0.01001"


def grid(n, lo, hi):
    """n distinct binary64 values strictly inside (lo, hi): lo + (i+1) * step, IEEE ops."""
    step = (hi - lo) / (n + 1)
    return [lo + (i + 1) * step for i in range(n)]


def encode(columns, outputs, trials, failed, model=MODEL):
    header = {
        "failed": str(len(failed)),
        "inputs": INPUTS,
        "model": model,
        "outputs": outputs,
        "profile": PROFILE,
        "seed": str(SEED),
        "trials": str(trials),
        "version": "mcm-samples@1",
    }
    h = json.dumps(header, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")
    body = b"".join(struct.pack("<d", v) for col in columns for v in col)
    body += b"".join(struct.pack("<Q", i) for i in failed)
    for col in columns:
        assert len(col) == trials - len(failed)
    return struct.pack("<Q", len(h)) + h + body


def mixed(n_in, n_out, lo=0.00999, hi=0.01001):
    """n_in values inside (lo, hi) and n_out outside, interleaved deterministically."""
    inside = grid(n_in, lo, hi)
    half = n_out // 2
    below = [lo - (i + 1) * 1e-7 for i in range(half)]
    above = [hi + (i + 1) * 1e-7 for i in range(n_out - half)]
    out = below + above
    vals = []
    i = j = 0
    while i < len(inside) or j < len(out):
        if i < len(inside):
            vals.append(inside[i])
            i += 1
        if j < len(out):
            vals.append(out[j])
            j += 1
    return vals


def sample_sets():
    s = {}
    s["center"] = ([mixed(970, 30)], ["y"], 1000, [])
    s["edge"] = ([mixed(500, 500)], ["y"], 1000, [])
    s["out"] = ([mixed(10, 990)], ["y"], 1000, [])
    s["exact950"] = ([mixed(950, 50)], ["y"], 1000, [])
    s["exact50"] = ([mixed(50, 950)], ["y"], 1000, [])
    # 30 failed trials: 945 of the 970 kept values inside.
    failed = [3 + 33 * i for i in range(30)]
    s["failed"] = ([mixed(945, 25)], ["y"], 1000, failed)
    # Values at and one ulp around the binary64 neighbours of the decimal limits.
    lo, hi = float(LSL_S), float(USL_S)
    edge = [down(lo), lo, up(lo), down(hi), hi, up(hi), 0.01, 0.01, 0.01, 0.01]
    s["boundary"] = ([edge], ["y"], 10, [])
    # Two outputs: column "a" all outside, column "y" all inside.
    s["two"] = ([[0.02] * 400, grid(400, 0.00999, 0.01001)], ["a", "y"], 400, [])
    # Trial-count edges under p_min = 0.95: 400 is the minimum, 399 one short; 99 < 100.
    s["min400"] = ([grid(400, 0.00999, 0.01001)], ["y"], 400, [])
    s["min399"] = ([grid(399, 0.00999, 0.01001)], ["y"], 399, [])
    s["t99"] = ([grid(99, 0.00999, 0.01001)], ["y"], 99, [])
    s["other_model"] = ([grid(100, 0.00999, 0.01001)], ["y"], 100, [])
    # Every trial failed: no kept value, so no p.
    s["all_failed"] = ([[]], ["y"], 20, list(range(20)))
    return s


def sample_bytes():
    out = {}
    for name, (cols, outputs, trials, failed) in sample_sets().items():
        model = OTHER_MODEL if name == "other_model" else MODEL
        out[name] = encode(cols, outputs, trials, failed, model)
    return out


# ------------------------------------------------------------------ rules

BUILTIN = {
    "dr:iso14253-1:2017/default@1": ("cp", Fraction("0.95"), "mc_or_validated_normal"),
    "dr:simple-acceptance@1": ("sa", None, "mc_or_validated_normal"),
    "dr:as-reported-by-source@1": ("label", None, "none"),
    "dr:guard-band@1": ("gb_template", None, "mc_or_validated_normal"),
}


def rule_of(text):
    """(kind, param, pdf) from the rule column."""
    if "|" in text:
        _ref, kind, param, pdf = text.split("|")
        assert kind == "cp"
        return ("cp", Fraction(param), pdf)
    if text.startswith("dr:guard-band(r="):
        r = text[len("dr:guard-band(r=") : text.index(")")]
        return ("gb", Fraction(r), "mc_or_validated_normal")
    return BUILTIN[text]


# ------------------------------------------------------------------ cases

FLAG_ORDER = [
    "capability_warning",
    "capability_unchecked",
    "empty_acceptance_zone",
    "failed_trials",
    "samples_too_few",
    "provisional_uncertainty",
    "claimed_uncertainty",
    "declared_input",
    "uncharacterized_input",
]

ISO = "dr:iso14253-1:2017/default@1"
SA = "dr:simple-acceptance@1"
LABEL = "dr:as-reported-by-source@1"
TEMPLATE = "dr:guard-band@1"
STRICT = "dr:customer/acme/strict@1|cp|0.99|mc_or_validated_normal"
MC_ONLY = "dr:customer/acme/mc@1|cp|0.95|mc_only"


def gb(r):
    return f"dr:guard-band(r={r})@1"


COLUMNS = [
    "id", "rule", "lsl", "usl", "unit", "value", "class", "method", "src", "u_std", "U",
    "lpu_validated", "nlin", "selection", "samples", "samples_digest", "output", "warn",
    "outcome", "reason", "p", "p_lo", "p_hi", "U_out", "flags", "error",
]  # fmt: skip


def case(id, rule, value, lsl=LSL_S, usl=USL_S, cls="UEC-1", method="LPU", src=None, u=2e-6, U=4e-6,
         lpu="", nlin=None, sel="unc-select@1", samples="", output="y", warn="", unit="si:m", hand=None,
         digest=""):  # fmt: skip
    if cls == "UEC-0":
        method, src, u, U, lpu, nlin, sel = "", "", None, None, "", "", ""
    if src is None:
        src = {"LPU": "dcc", "MCM": "dcc", "DECLARED": "", "SPEC_DERIVED": "instrument_model"}.get(method, "")
    if nlin is None:
        nlin = "linear_adequate" if method == "LPU" else ""
    return {
        "id": id, "rule": rule, "lsl": lsl or "", "usl": usl or "", "unit": unit, "value": value,
        "class": cls, "method": method, "src": src, "u_std": u, "U": U, "lpu_validated": lpu,
        "nlin": nlin, "selection": sel, "samples": samples, "samples_digest": digest, "output": output,
        "warn": warn,
        "hand": hand,
    }  # fmt: skip


def rep_flip_values():
    """Two values y_a, y_b with p(y_a) about 0.95 + 5e-8 and p(y_b) about 0.95 - 5e-8 under a
    one-sided USL = 0.01, u = 1e-6 (AC-6)."""
    usl, u = Fraction("0.01"), Fraction(1e-6)

    def p_of(y):
        return 1 - upper_tail((usl - Fraction(y)) / u)

    def solve(target):
        lo, hi = 0.0099, 0.01  # p(lo) ~ 1, p(hi) = 0.5
        for _ in range(200):
            mid = lo + (hi - lo) / 2
            if mid in (lo, hi):
                break
            if p_of(mid) >= target:
                lo = mid
            else:
                hi = mid
        return lo

    return solve(Decimal("0.95") + Decimal("5e-8")), solve(Decimal("0.95") - Decimal("5e-8"))


def solve_y(usl, u, target, lo, hi):
    """The largest binary64 y in [lo, hi] with exact p(y) >= target, for a one-sided USL."""
    usl, u = Fraction(usl), Fraction(u)

    def p_of(y):
        with localcontext() as c:
            c.prec = PREC
            return 1 - upper_tail((usl - Fraction(y)) / u)

    for _ in range(200):
        mid = lo + (hi - lo) / 2
        if mid in (lo, hi):
            break
        if p_of(mid) >= target:
            lo = mid
        else:
            hi = mid
    return lo


def all_cases():
    c = []
    a = c.append
    # --- conformance probability, normal approximation (LPU selected by unc-select@1)
    a(case("cp_normal_center", ISO, 0.01, hand="CONFORMS"))
    a(case("cp_normal_z2", ISO, 0.010006, hand="CONFORMS"))
    a(case("cp_normal_z125", ISO, 0.0100075, hand="INDETERMINATE"))
    a(case("cp_normal_at_usl", ISO, 0.01001, hand="INDETERMINATE"))
    a(case("cp_normal_out", ISO, 0.010014, hand="NONCONFORMS"))
    a(case("cp_normal_far_out", ISO, 0.02, hand="NONCONFORMS"))
    a(case("cp_normal_below", ISO, 0.009986, hand="NONCONFORMS"))
    a(case("cp_normal_wide_u", ISO, 0.01, u=1e-5, U=2e-5, hand="INDETERMINATE"))
    a(case("cp_lpu_validated", ISO, 0.01, nlin="use_mcm", lpu="true", hand="CONFORMS"))
    a(case("cp_lpu_not_validated", ISO, 0.01, nlin="use_mcm", lpu="false", hand="NOT_DECIDABLE"))
    a(case("cp_lpu_no_selection", ISO, 0.01, sel="", hand="NOT_DECIDABLE"))
    a(case("cp_lpu_other_selection", ISO, 0.01, sel="other-select@1", hand="NOT_DECIDABLE"))
    a(case("cp_lpu_nlin_second_order", ISO, 0.01, nlin="second_order_needed", hand="NOT_DECIDABLE"))
    a(case("cp_lpu_no_nlin", ISO, 0.01, nlin="", hand="NOT_DECIDABLE"))
    a(case("cp_strict_center", STRICT, 0.01, hand="CONFORMS"))
    a(case("cp_strict_z2", STRICT, 0.010006, hand="INDETERMINATE"))
    a(case("cp_mc_only_lpu", MC_ONLY, 0.01, hand="NOT_DECIDABLE"))
    a(case("cp_one_sided_usl", ISO, 0.00003, lsl="", usl="0.00005", u=5e-6, U=1e-5, hand="CONFORMS"))
    a(case("cp_one_sided_usl_out", ISO, 0.00006, lsl="", usl="0.00005", u=5e-6, U=1e-5, hand="NONCONFORMS"))
    a(case("cp_one_sided_lsl", ISO, 0.5, lsl="0.4", usl="", u=0.02, U=0.04, hand="CONFORMS"))
    a(case("cp_one_sided_lsl_mid", ISO, 0.41, lsl="0.4", usl="", u=0.02, U=0.04, hand="INDETERMINATE"))
    a(case("cp_capability_warning", ISO, 0.01, u=3e-6, U=6e-6, hand="CONFORMS"))
    ya, yb = rep_flip_values()
    a(case("rep_flip_a", ISO, ya, lsl="", usl="0.01", u=1e-6, U=2e-6, hand="CONFORMS"))
    a(case("rep_flip_b", ISO, yb, lsl="", usl="0.01", u=1e-6, U=2e-6, hand="INDETERMINATE"))
    # --- conservative normal p near the thresholds (review of #422). USL = 1.7985560436 m
    # and u = 8e-10 m: one ulp of the limit moves p by about 3e-8.
    rv = dict(lsl="", usl="1.7985560436", u=8e-10, U=1.6e-9)
    a(case("cons_reviewer", ISO, 1.7985560422841171, **rv, hand="INDETERMINATE"))
    for name, target, hand in [("cons_pmin_plus_1e-6", "0.950001", "CONFORMS"),
                               ("cons_low_minus_1e-6", "0.049999", "NONCONFORMS")]:
        y = solve_y("1.7985560436", 8e-10, Decimal(target), 1.79855604, 1.79855605)
        a(case(name, ISO, y, **rv, hand=hand))
    # --- conformance probability from samples (MCM), digest-checked path
    m = dict(method="MCM", U=None)
    a(case("cp_mc_center", ISO, 0.01, samples="center", **m, hand="CONFORMS"))
    a(case("cp_mc_edge", ISO, 0.01, samples="edge", **m, hand="INDETERMINATE"))
    a(case("cp_mc_out", ISO, 0.01, samples="out", **m, hand="NONCONFORMS"))
    a(case("cp_mc_exact950", ISO, 0.01, samples="exact950", **m, hand="CONFORMS"))
    a(case("cp_mc_exact50", ISO, 0.01, samples="exact50", **m, hand="NONCONFORMS"))
    a(case("cp_mc_failed", ISO, 0.01, samples="failed", **m, hand="INDETERMINATE"))
    a(case("cp_mc_all_failed", ISO, 0.01, samples="all_failed", **m, hand="INDETERMINATE"))
    a(case("cp_mc_strict_center", STRICT, 0.01, samples="center", **m, hand="INDETERMINATE"))
    a(case("cp_mc_only_center", MC_ONLY, 0.01, samples="center", **m, hand="CONFORMS"))
    a(case("cp_mc_two_outputs_y", ISO, 0.01, samples="two", output="y", **m, hand="CONFORMS"))
    a(case("cp_mc_two_outputs_a", ISO, 0.01, samples="two", output="a", **m, hand="NONCONFORMS"))
    a(case("cp_mc_with_U", ISO, 0.01, samples="center", method="MCM", U=4e-6, hand="CONFORMS"))
    a(case("cp_mcm_no_samples", ISO, 0.01, method="MCM", U=None, hand="NOT_DECIDABLE"))  # AC-4
    a(case("cp_mc_min400", ISO, 0.01, samples="min400", **m, hand="CONFORMS"))
    a(case("cp_mc_min399", ISO, 0.01, samples="min399", **m, hand="INDETERMINATE"))
    a(case("cp_mc_t99", ISO, 0.01, samples="t99", **m, hand="INDETERMINATE"))
    a(case("sa_mc_t99", SA, 0.01, samples="t99", **m, hand="CONFORMS"))
    a(case("sa_mc_boundary", SA, 0.01, samples="boundary", **m, hand="CONFORMS"))
    # --- simple acceptance
    a(case("simple_just_over", SA, 0.0100500004, lsl="0.00995", usl="0.01005", hand="NONCONFORMS"))  # AC-2
    a(case("simple_at_usl_f64", SA, 0.01001, hand=None))
    a(case("simple_at_usl_up", SA, up(0.01001), hand="NONCONFORMS"))
    a(case("simple_at_usl_down", SA, down(0.01001), hand="CONFORMS"))
    a(case("simple_at_lsl_f64", SA, 0.00999, hand=None))
    a(case("simple_at_lsl_down", SA, down(0.00999), hand="NONCONFORMS"))
    a(case("simple_inside", SA, 0.01, hand="CONFORMS"))
    a(case("simple_below", SA, 0.0099, hand="NONCONFORMS"))
    a(case("simple_exact_dyadic_usl", SA, 0.5, lsl="0", usl="0.5", u=0.01, U=0.02, hand="CONFORMS"))
    a(case("simple_mcm_no_p", SA, 0.01, method="MCM", U=None, hand="CONFORMS"))
    a(case("simple_mcm_U", SA, 0.01, method="MCM", U=4e-6, hand="CONFORMS"))
    a(case("simple_cap_at_ratio", SA, 0.5, lsl="0", usl="1", u=0.125, U=0.25, hand="CONFORMS"))
    a(case("simple_cap_over_ratio", SA, 0.5, lsl="0", usl="1", u=0.125, U=up(0.25), hand="CONFORMS"))
    a(case("simple_cap_warn_custom", SA, 0.5, lsl="0", usl="1", u=0.05, U=0.1, warn="0.05", hand="CONFORMS"))
    a(case("simple_cap_warn_zero", SA, 0.5, lsl="0", usl="1", u=0.05, U=0.1, warn="0", hand="CONFORMS"))
    a(case("simple_one_sided", SA, 0.00003, lsl="", usl="0.00005", u=5e-6, U=1e-5, hand="CONFORMS"))
    a(case("simple_huge_limits", SA, 1e300, lsl="-1e400", usl="1e400", u=1e290, U=2e290, hand="CONFORMS"))
    # --- claimed and partly unsourced uncertainty (labels from the statement)
    a(case("declared_simple", SA, 0.01, method="DECLARED", u=2e-6, U=4e-6, hand="CONFORMS"))
    a(case("declared_cp", ISO, 0.01, method="DECLARED", hand="NOT_DECIDABLE"))
    a(case("declared_gb", gb("1"), 0.01, method="DECLARED", hand="CONFORMS"))
    a(case("spec_derived_simple", SA, 0.01, method="SPEC_DERIVED", hand="CONFORMS"))
    a(case("lpu_declared_input", ISO, 0.01, src="dcc;declared", hand="CONFORMS"))
    a(case("lpu_unverified_source", ISO, 0.01, src="dcc!", hand="CONFORMS"))
    a(case("lpu_instrument_model", SA, 0.0101, src="dcc;instrument_model", hand="NONCONFORMS"))
    # --- guard band
    a(case("gb_inside_zone", gb("1"), 0.010005, hand="CONFORMS"))
    a(case("gb_in_tol_out_zone", gb("1"), 0.010007, hand="NONCONFORMS"))
    a(case("gb_below_zone", gb("1"), 0.009993, hand="NONCONFORMS"))
    a(case("gb_empty_half", gb("1"), 0.01, u=5e-6, U=1e-5, hand="NONCONFORMS"))  # AC-5
    a(case("gb_empty_exact", gb("1"), 0.5, lsl="0", usl="1", u=0.25, U=0.5, hand="NONCONFORMS"))  # AC-5
    a(case("gb_empty_over", gb("1"), 0.01, u=6e-6, U=1.2e-5, hand="NONCONFORMS"))
    a(case("gb_half_r", gb("0.5"), 0.01, u=5e-6, U=1e-5, hand="CONFORMS"))
    a(case("gb_r0_equals_simple", gb("0"), 0.010009, hand="CONFORMS"))
    a(case("gb_exact_edge", gb("1"), 0.875, lsl="0", usl="1", u=0.0625, U=0.125, hand="CONFORMS"))
    a(case("gb_exact_edge_up", gb("1"), up(0.875), lsl="0", usl="1", u=0.0625, U=0.125, hand="NONCONFORMS"))
    a(case("gb_decimal_edge", gb("0.5"), 0.775, lsl="0.1", usl="0.9", u=0.125, U=0.25, hand=None))
    a(case("gb_one_sided", gb("1"), 0.375, lsl="", usl="0.5", u=0.0625, U=0.125, hand="CONFORMS"))
    a(case("gb_one_sided_out", gb("1"), up(0.375), lsl="", usl="0.5", u=0.0625, U=0.125, hand="NONCONFORMS"))
    a(case("gb_mc_reports_p", gb("1"), 0.01, samples="center", method="MCM", U=4e-6, hand="CONFORMS"))
    a(case("gb_no_U", gb("1"), 0.01, method="MCM", U=None, hand="error"))
    # --- UEC-0 and label-only (AC-3)
    a(case("uec0_iso", ISO, 0.01, cls="UEC-0", hand="NOT_DECIDABLE"))
    a(case("uec0_simple", SA, 0.01, cls="UEC-0", hand="NOT_DECIDABLE"))
    a(case("uec0_gb", gb("1"), 0.01, cls="UEC-0", hand="NOT_DECIDABLE"))
    a(case("uec0_label", LABEL, 0.01, cls="UEC-0", hand="NOT_DECIDABLE"))
    a(case("label_only_uec1", LABEL, 0.01, hand="NOT_DECIDABLE"))
    a(case("label_only_out_of_tol", LABEL, 0.5, hand="NOT_DECIDABLE"))
    # --- errors (pinned order)
    a(case("err_value_nan", ISO, float("nan"), hand="error"))
    a(case("err_value_inf", SA, float("inf"), hand="error"))
    a(case("err_nan_before_tolerance", SA, float("nan"), lsl="0.02", usl="0.01", hand="error"))
    a(case("err_no_limits", SA, 0.01, lsl="", usl="", hand="error"))
    a(case("err_lsl_above_usl", SA, 0.01, lsl="0.02", usl="0.01", hand="error"))
    a(case("err_zero_width", SA, 0.01, lsl="0.01", usl="0.01", hand="error"))
    a(case("err_warn_negative", SA, 0.01, warn="-0.1", hand="error"))
    a(case("err_unit_mismatch", SA, 0.01, unit="si:rad", hand="error"))
    a(case("err_unit_before_label", LABEL, 0.01, unit="si:1", hand="error"))
    a(case("err_template", TEMPLATE, 0.01, hand="error"))
    a(case("err_template_uec0", TEMPLATE, 0.01, cls="UEC-0", hand="error"))
    a(case("err_samples_lpu", ISO, 0.01, samples="center", hand="error"))
    a(case("err_samples_model", ISO, 0.01, samples="other_model", method="MCM", U=None, hand="error"))
    a(case("err_samples_output", ISO, 0.01, samples="two", output="b", method="MCM", U=None, hand="error"))
    a(case("err_samples_before_label", LABEL, 0.01, samples="center", hand="error"))
    a(case("err_samples_digest", ISO, 0.01, samples="center", digest="wrong", method="MCM", U=None, hand="error"))
    return c


# ------------------------------------------------------------------ independent evaluator


def frac(s):
    return Fraction(s) if s != "" else None


def evaluate(row, samples):
    """Returns (outcome, reason, p, U, flags, error) following the pinned order."""
    y = row["value"]
    if not math.isfinite(y):
        return err("invalid_number")
    lsl, usl = frac(row["lsl"]), frac(row["usl"])
    if lsl is None and usl is None:
        return err("invalid_tolerance")
    if lsl is not None and usl is not None and lsl >= usl:
        return err("invalid_tolerance")
    warn = Fraction(row["warn"] or "0.25")
    if warn < 0:
        return err("invalid_parameter")
    cls = row["class"]
    stmt_unit = None if cls == "UEC-0" else "si:m"
    if stmt_unit is not None and row["unit"] != stmt_unit:
        return err("unit_mismatch")
    kind, param, pdf = rule_of(row["rule"])
    if kind == "gb_template":
        return err("invalid_rule")
    counts = None
    if row["samples"]:
        if row["samples_digest"] == "wrong":
            return err("samples_unbound")
        if row["method"] != "MCM":
            return err("samples_unbound")
        cols, outputs, trials, failed = sample_sets()[row["samples"]]
        if row["samples"] == "other_model":
            return err("samples_unbound")
        if row["output"] not in outputs:
            return err("samples_output")
        col = cols[outputs.index(row["output"])]
        k = sum(1 for x in col if inside(Fraction(x), lsl, usl))
        counts = (k, len(col), len(failed), trials)

    flags = []
    srcs = [s for s in row["src"].split(";") if s]
    claimed = row["method"] in ("DECLARED", "SPEC_DERIVED")
    declared = any(s.rstrip("!") == "declared" for s in srcs)
    unchar = any(s.endswith("!") or s.rstrip("!") not in ("dcc", "qualification", "type_a") for s in srcs)
    if cls == "UEC-1" or claimed or declared or unchar:
        flags.append("provisional_uncertainty")
    if claimed:
        flags.append("claimed_uncertainty")
    if declared:
        flags.append("declared_input")
    if unchar:
        flags.append("uncharacterized_input")

    if kind == "label":
        return ("NOT_DECIDABLE", "label_only", None, None, flags, "", None)
    if cls == "UEC-0":
        return ("NOT_DECIDABLE", "u_missing", None, None, flags, "", None)

    U = row["U"]
    u = row["u_std"]
    p = None
    bounds = None
    p_exact_counts = None
    too_few = False
    if counts is not None:
        k, n, f, t = counts
        if f > 0:
            flags.append("failed_trials")
        if t < 100 or (kind == "cp" and t * (1 - param) < 20):
            too_few = True
            flags.append("samples_too_few")
        p = Fraction(k, n) if n > 0 else None
        bounds = (Fraction(k, t), Fraction(k + f, t))
        p_exact_counts = counts
    else:
        permitted = pdf == "mc_or_validated_normal" and u is not None and (
            row["lpu_validated"] == "true"
            or (row["method"] == "LPU" and row["selection"] == "unc-select@1" and row["nlin"] == "linear_adequate")
        )
        if permitted:
            yf, uf = Fraction(y), Fraction(u)
            above = upper_tail((usl - yf) / uf) if usl is not None else Decimal(0)
            below = upper_tail(-(lsl - yf) / uf) if lsl is not None else Decimal(0)
            with localcontext() as c:
                c.prec = PREC
                pd = 1 - above - below
            pd = min(max(pd, Decimal(0)), Decimal(1))
            p = pd
            bounds = normal_bounds(y, u, row["lsl"], row["usl"])
            assert bounds[0] <= Fraction(pd) <= bounds[1], row["id"]

    if U is not None and lsl is not None and usl is not None:
        if Fraction(U) > warn * (usl - lsl):
            flags.append("capability_warning")
    else:
        flags.append("capability_unchecked")

    if kind == "cp":
        if p is None and p_exact_counts is None:
            outcome = ("NOT_DECIDABLE", "pdf_unavailable")
        elif too_few:
            outcome = ("INDETERMINATE", "")
        elif p_exact_counts is not None:
            k, n, f, t = p_exact_counts
            conforms = Fraction(k, t) >= param
            nonconforms = Fraction(k + f, t) <= 1 - param
            outcome = (pick(conforms, nonconforms), "")
        else:
            lo, hi = bounds
            # The kernel's bounds differ from these by its erfc error (<= 2e-15) only.
            sep = Fraction(1, 10**13)
            assert abs(lo - param) > sep and abs(hi - (1 - param)) > sep, row["id"]
            outcome = (pick(lo >= param, hi <= 1 - param), "")
    elif kind == "sa":
        outcome = ("CONFORMS" if inside(Fraction(y), lsl, usl) else "NONCONFORMS", "")
    elif kind == "gb":
        if U is None:
            return err("uncertainty_missing")
        w = param * Fraction(U)
        yf = Fraction(y)
        if lsl is not None and usl is not None and usl - lsl - 2 * w <= 0:
            flags.append("empty_acceptance_zone")
            outcome = ("NONCONFORMS", "")
        else:
            ok = (lsl is None or yf >= lsl + w) and (usl is None or yf <= usl - w)
            outcome = ("CONFORMS" if ok else "NONCONFORMS", "")
    else:
        raise AssertionError(kind)
    flags = sorted(set(flags), key=FLAG_ORDER.index)
    return (outcome[0], outcome[1], p, U, flags, "", bounds)


def inside(x, lsl, usl):
    return (lsl is None or x >= lsl) and (usl is None or x <= usl)


def pick(conforms, nonconforms):
    return "CONFORMS" if conforms else ("NONCONFORMS" if nonconforms else "INDETERMINATE")


def err(code):
    return ("", "", None, None, [], code, None)


TAIL_ERR = Fraction(4e-15)
ARITH_ERR = Fraction(1e-15)


MAX = 1.7976931348623157e308


def floor_f(d):
    """Largest finite binary64 <= the rational d, or None when there is none."""
    if d > Fraction(MAX):
        return MAX
    if d < -Fraction(MAX):
        return None
    x = float(d)
    return x if Fraction(x) <= d else down(x)


def ceil_f(d):
    """Smallest finite binary64 >= the rational d, or None when there is none."""
    if d < -Fraction(MAX):
        return -MAX
    if d > Fraction(MAX):
        return None
    x = float(d)
    return x if Fraction(x) >= d else up(x)


def split(d):
    """(f, dlo, dhi): f the largest finite binary64 <= d, dlo <= d - f <= dhi adjacent
    binary64 values (equal when exact); None when no finite binary64 is <= d."""
    f = floor_f(d)
    if f is None:
        return None
    delta = d - Fraction(f)
    if delta >= Fraction(MAX):
        return f, MAX, math.inf
    dlo = floor_f(delta)
    dhi = dlo if Fraction(dlo) == delta else up(dlo)
    return f, dlo, dhi


def normal_bounds(y, u, lsl_text, usl_text):
    """(p_lo, p_hi) by the pinned construction, tails at 100 digits, combined exactly."""

    def q(z):
        if math.isinf(z):
            return Fraction(0) if z > 0 else Fraction(1)
        return Fraction(upper_tail(Fraction(z)))

    a_max = a_min = b_max = b_min = Fraction(0)
    if usl_text:
        sp = split(Fraction(usl_text))
        if sp is None:
            a_max = a_min = Fraction(1)
        else:
            f, dlo, dhi = sp
            a = f - y
            z_lo = down(down(down(a) + dlo) / u)
            z_hi = up(up(up(a) + dhi) / u)
            a_max, a_min = q(z_lo) + TAIL_ERR, q(z_hi) - TAIL_ERR
    if lsl_text:
        sp = split(Fraction(lsl_text))
        if sp is not None:
            f, dlo, dhi = sp
            a = y - f
            z_lo = down(down(down(a) - dhi) / u)
            z_hi = up(up(up(a) - dlo) / u)
            b_max, b_min = q(z_lo) + TAIL_ERR, q(z_hi) - TAIL_ERR
    clamp = lambda v: min(max(v, Fraction(0)), Fraction(1))
    return clamp(1 - a_max - b_max - ARITH_ERR), clamp(1 - a_min - b_min + ARITH_ERR)

def fmt_float(x):
    return "" if x is None else repr(x)


def fmt_p(p):
    if p is None:
        return ""
    if isinstance(p, Fraction):
        return repr(p.numerator / p.denominator)
    return repr(float(p))


def cases_text():
    samples = sample_sets()
    lines = [",".join(COLUMNS)]
    seen = set()
    for row in all_cases():
        assert row["id"] not in seen, row["id"]
        seen.add(row["id"])
        outcome, reason, p, U, flags, error, bounds = evaluate(row, samples)
        lo, hi = bounds if bounds is not None else (None, None)
        hand = row["hand"]
        if hand is not None:
            got = "error" if error else outcome
            assert got == hand, (row["id"], got, hand)
        fields = [
            row["id"], row["rule"], row["lsl"], row["usl"], row["unit"], fmt_float(row["value"]), row["class"],
            row["method"], row["src"], fmt_float(row["u_std"]), fmt_float(row["U"]), row["lpu_validated"],
            row["nlin"], row["selection"], row["samples"], row["samples_digest"], row["output"], row["warn"],
            outcome, reason, fmt_p(p), fmt_p(lo), fmt_p(hi), fmt_float(U), ";".join(flags), error,
        ]  # fmt: skip
        assert all("," not in f for f in fields), row["id"]
        lines.append(",".join(fields))
    # Spot checks of the boundary rows, labelled from the exact values directly.
    return "\n".join(lines) + "\n"


def erfc_text():
    xs = [Fraction(i, 20) for i in range(-120, 121)] + [Fraction(i, 4) for i in range(25, 121)]
    # A denser grid on [-8, 8] (step 2^-7, plus an irrational-looking offset) for the TAIL_ERR bound.
    xs += [Fraction(i, 128) for i in range(-1024, 1025)]
    xs += [Fraction(i * 0.0037 + 0.001) for i in range(-2000, 2001)]
    xs += [Fraction(1.9999999999999998), Fraction(2), Fraction(2.0000000000000004), Fraction(0.1), Fraction(1e-9)]
    lines = ["x,erfc"]
    for x in sorted(set(xs)):
        v = erfc_hp(x)
        lines.append(f"{repr(x.numerator / x.denominator)},{repr(float(v))}")
    return "\n".join(lines) + "\n"


def boundary_sanity():
    # simple_at_usl_f64: the binary64 nearest 0.01001 lies on one side of the decimal; check
    # the neighbours bracket it, so exactly one of up/down/self is on each side.
    for s in (LSL_S, USL_S, "0.01005", "0.775"):
        x = float(s)
        assert Fraction(down(x)) < Fraction(s) < Fraction(up(x))


def main():
    boundary_sanity()
    files = {CASES: cases_text().encode("ascii"), ERFC: erfc_text().encode("ascii")}
    for name, data in sample_bytes().items():
        files[SAMPLES / f"{name}.bin"] = data
    check = "--check" in sys.argv[1:]
    stale = []
    for path, data in files.items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(path.name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale:", ", ".join(stale))
        sys.exit(1)
    print(("checked " if check else "wrote ") + str(len(files)) + " files")


if __name__ == "__main__":
    main()
