#!/usr/bin/env python3
"""Independent reference for UNC.RDL.REP (CC-BY-4.0 project data).

Writes:
- crates/fid-rep/src/table/rep-1@1.0.0.json: the canonical JSON of table rep-1@1.0.0, built
  here from the bounds in REQ-UNC.RDL.REP-001 (not from the Rust code);
- fixtures/rdl/rep/rep-1@1.0.0.sha256: `sha256:<hex>` of that file;
- fixtures/rdl/rep/cases.csv: classification cases (AC-1 .. AC-5).

Every case carries a hand-stated expected outcome and annotation taken from the spec's rules.
Before anything is written, each label is cross-checked against a second, independent
statement of the rules below that uses exact rational arithmetic (fractions.Fraction), so a
wrong hand label or a wrong boundary construction stops the script.

Run:  python fixtures/rdl/rep/make_cases.py           (write)
      python fixtures/rdl/rep/make_cases.py --check   (fail if any file is stale)
"""

import hashlib
import json
import math
import struct
import sys
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TABLE_PATH = ROOT / "crates" / "fid-rep" / "src" / "table" / "rep-1@1.0.0.json"
DIGEST_PATH = ROOT / "fixtures" / "rdl" / "rep" / "rep-1@1.0.0.sha256"
CASES_PATH = ROOT / "fixtures" / "rdl" / "rep" / "cases.csv"

# REQ-UNC.RDL.REP-001. (kind, mode, bound text, unit of the bound).
BOUNDS = [
    ("length", "abs", "1e-9", "m"),
    ("angle", "abs", "1e-9", "rad"),
    ("dimensionless", "rel", "1e-9", "1"),
    ("expanded_uncertainty", "rel", "1e-6", "1"),
    ("p_conformance", "abs", "1e-6", "1"),
]
TABLE = {
    "algorithm": "rdl.rep.classify@1.0.0",
    "bounds": [{"bound": b, "kind": k, "mode": m, "unit": u} for k, m, b, u in BOUNDS] + [
        {"kind": "exact", "mode": "exact"}
    ],
    "schema": "rep-table@1",
    "version": "rep-1@1.0.0",
}
# RFC 8785 for this ASCII-only, integer-free document: sorted keys, no whitespace.
TABLE_TEXT = json.dumps(TABLE, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
BOUND = {k: (m, Fraction(b)) for k, m, b, _ in BOUNDS}

up = lambda x: math.nextafter(x, math.inf)
down = lambda x: math.nextafter(x, -math.inf)


def hx(x):
    return "absent" if x is None else struct.pack(">d", x).hex()


def bits(x):
    return struct.unpack(">Q", struct.pack(">d", x))[0]


# ---------------------------------------------------------------- independent reference rules


def ref_within(a, b, kind):
    mode, d = BOUND[kind]
    delta = abs(Fraction(a) - Fraction(b))
    scale = Fraction(1) if mode == "abs" else max(abs(Fraction(a)), abs(Fraction(b)))
    return delta <= d * scale


def ref_marginal(kind, rec, rep, limit):
    if limit is None or kind not in BOUND:
        return False
    vals = [v for v in (rec, rep) if v is not None and math.isfinite(v)]
    if not vals:
        return False
    mode, d = BOUND[kind]
    scale = Fraction(1) if mode == "abs" else max(abs(Fraction(v)) for v in vals)
    lim = Fraction(limit)
    return any(abs(Fraction(v) - lim) <= d * scale for v in vals)


def ref_classify(kind, rec, rep, same_manifest, det, same_platform, limit):
    if kind not in BOUND and kind != "exact":
        return "ERR:kind_unknown", ""
    if any(v is not None and math.isnan(v) for v in (rec, rep)):
        return "ERR:nan_input", ""
    ann = "MARGINAL" if ref_marginal(kind, rec, rep, limit) else ""
    if rec is None and rep is None:
        return "IDENTICAL", ann
    if rec is None or rep is None:
        return "DIVERGENT", ann
    if bits(rec) == bits(rep):
        return "IDENTICAL", ann
    if kind == "exact":
        return "DIVERGENT", ann
    if same_manifest and (det == "D0" or (det in ("D1", "D2") and same_platform)):
        return "DIVERGENT", ann
    if not (math.isfinite(rec) and math.isfinite(rep)):
        return "DIVERGENT", ann
    return ("EQUIVALENT" if ref_within(rec, rep, kind) else "DIVERGENT"), ann


# ---------------------------------------------------------------- hand-labelled cases

rows = []


def case(name, kind, rec, rep, expect, ann="", same_manifest=False, det="D3", same_platform=True, limit=None):
    got = ref_classify(kind, rec, rep, same_manifest, det, same_platform, limit)
    assert got == (expect, ann), (name, got, (expect, ann))
    assert all(name != r[0] for r in rows), name
    rows.append([name, kind, hx(rec), hx(rep), "1" if same_manifest else "0", det, "1" if same_platform else "0",
                 "" if limit is None else limit, expect, ann])


def first_above(base, kind):
    """Smallest double r >= base whose exact distance to base is beyond the bound."""
    mode, d = BOUND[kind]
    target = Fraction(base) + d * (abs(Fraction(base)) if mode == "rel" else 1)
    r = float(target)
    if ref_within(base, r, kind):
        while ref_within(base, r, kind):
            r = up(r)
    else:
        while not ref_within(base, down(r), kind):
            r = down(r)
    assert not ref_within(base, r, kind) and ref_within(base, down(r), kind)
    return r


def last_within(base, kind):
    r = first_above(base, kind)
    r = down(r)
    assert ref_within(base, r, kind)
    return r


KINDS = ["length", "angle", "dimensionless", "expanded_uncertainty", "p_conformance", "exact"]
CLASSES = ["D0", "D1", "D2", "D3", "N"]

# A. Bit-equal values are IDENTICAL in every context (REQ-003, REQ-004).
for k in KINDS:
    case(f"identical_{k}", k, 0.125, 0.125, "IDENTICAL")
for c in CLASSES:
    case(f"identical_same_manifest_{c}", "length", 12.5, 12.5, "IDENTICAL", same_manifest=True, det=c)
case("identical_negative", "length", -3.75, -3.75, "IDENTICAL")
case("identical_subnormal", "length", 5e-324, 5e-324, "IDENTICAL")
case("identical_inf", "length", math.inf, math.inf, "IDENTICAL")

# B. Same manifest, D0 or D1/D2 on the same platform class: bit identity only (REQ-003, AC-5).
for c in ["D0", "D1", "D2"]:
    case(f"ulp_same_manifest_{c}", "length", 7.25, up(7.25), "DIVERGENT", same_manifest=True, det=c)
case("ulp_same_manifest_D0_other_platform", "length", 7.25, up(7.25), "DIVERGENT", same_manifest=True, det="D0",
     same_platform=False)
for c in ["D1", "D2"]:
    case(f"ulp_same_manifest_{c}_other_platform", "length", 7.25, up(7.25), "EQUIVALENT", same_manifest=True, det=c,
         same_platform=False)
case("ulp_same_manifest_D3", "length", 7.25, up(7.25), "EQUIVALENT", same_manifest=True, det="D3")
case("ulp_same_manifest_N", "length", 7.25, up(7.25), "EQUIVALENT", same_manifest=True, det="N")
# Manifests differ: the table applies to every class (REQ-004).
for c in CLASSES:
    case(f"ulp_other_manifest_{c}", "length", 7.25, up(7.25), "EQUIVALENT", det=c)
case("ulp_same_manifest_D0_angle", "angle", 0.5, up(0.5), "DIVERGENT", same_manifest=True, det="D0")
case("ulp_same_manifest_D0_p", "p_conformance", 0.5, up(0.5), "DIVERGENT", same_manifest=True, det="D0")

# C. Table bounds, decided exactly at the boundary (REQ-001, REQ-004, REQ-006, AC-2).
case("len_just_under", "length", 1.0, last_within(1.0, "length"), "EQUIVALENT")
case("len_just_over", "length", 1.0, first_above(1.0, "length"), "DIVERGENT")
case("len_just_under_swapped", "length", last_within(1.0, "length"), 1.0, "EQUIVALENT")
case("len_just_over_swapped", "length", first_above(1.0, "length"), 1.0, "DIVERGENT")
case("len_just_under_neg", "length", -1.0, -last_within(1.0, "length"), "EQUIVALENT")
case("len_just_over_neg", "length", -1.0, -first_above(1.0, "length"), "DIVERGENT")
case("len_just_under_big", "length", 250.0, last_within(250.0, "length"), "EQUIVALENT")
case("len_just_over_big", "length", 250.0, first_above(250.0, "length"), "DIVERGENT")
# 1e-9 is not a double: fl(1e-9) lies above it, so a rounded f64 comparison would accept it.
F1E9 = 1e-9
assert Fraction(F1E9) > Fraction(1, 10**9)
case("len_decimal_not_f64_over", "length", 0.0, F1E9, "DIVERGENT")
case("len_decimal_not_f64_under", "length", 0.0, down(F1E9), "EQUIVALENT")
case("len_decimal_not_f64_neg", "length", 0.0, -F1E9, "DIVERGENT")
case("len_cross_zero_within", "length", -4e-10, 5e-10, "EQUIVALENT")
case("len_cross_zero_beyond", "length", -5e-10, 5.1e-10, "DIVERGENT")
case("len_far", "length", 10.0, 10.001, "DIVERGENT")
case("len_signed_zero_table", "length", 0.0, -0.0, "EQUIVALENT")
case("len_signed_zero_same_manifest_D0", "length", 0.0, -0.0, "DIVERGENT", same_manifest=True, det="D0")
case("len_subnormals", "length", 5e-324, 1e-320, "EQUIVALENT")
case("ang_just_under", "angle", 1.5, last_within(1.5, "angle"), "EQUIVALENT")
case("ang_just_over", "angle", 1.5, first_above(1.5, "angle"), "DIVERGENT")
case("ang_decimal_not_f64_over", "angle", 0.0, F1E9, "DIVERGENT")
case("ang_negative_within", "angle", -3.0, -3.0000000005, "EQUIVALENT")
case("dimless_just_under", "dimensionless", 1000.0, last_within(1000.0, "dimensionless"), "EQUIVALENT")
case("dimless_just_over", "dimensionless", 1000.0, first_above(1000.0, "dimensionless"), "DIVERGENT")
case("dimless_small_within", "dimensionless", 1e-3, 1e-3 * (1 + 5e-10), "EQUIVALENT")
case("dimless_small_beyond", "dimensionless", 1e-3, 1.000002e-3, "DIVERGENT")
case("dimless_zero_vs_tiny", "dimensionless", 0.0, 5e-324, "DIVERGENT")
case("dimless_one_within", "dimensionless", 1.0, 1.0000000009, "EQUIVALENT")
case("dimless_huge_within", "dimensionless", 1e300, 1e300 * (1 + 1e-10), "EQUIVALENT")
case("dimless_huge_opposite_sign", "dimensionless", 1.7e308, -1.7e308, "DIVERGENT")
case("u_just_under", "expanded_uncertainty", 0.002, last_within(0.002, "expanded_uncertainty"), "EQUIVALENT")
case("u_just_over", "expanded_uncertainty", 0.002, first_above(0.002, "expanded_uncertainty"), "DIVERGENT")
case("u_within_relative", "expanded_uncertainty", 1.5e-6, 1.5000001e-6, "EQUIVALENT")
case("u_beyond_relative", "expanded_uncertainty", 1.5e-6, 1.501e-6, "DIVERGENT")
case("p_just_under", "p_conformance", 0.5, last_within(0.5, "p_conformance"), "EQUIVALENT")
case("p_just_over", "p_conformance", 0.5, first_above(0.5, "p_conformance"), "DIVERGENT")
case("p_diff_1e-7", "p_conformance", 0.95, 0.9500001, "EQUIVALENT")
case("p_diff_1e-5", "p_conformance", 0.95, 0.95001, "DIVERGENT")
case("p_zero_vs_tiny", "p_conformance", 0.0, 1e-7, "EQUIVALENT")
case("len_inf_vs_finite", "length", math.inf, 1.0, "DIVERGENT")
case("len_inf_vs_neg_inf", "length", math.inf, -math.inf, "DIVERGENT")
case("len_max_vs_neg_max", "length", 1.7976931348623157e308, -1.7976931348623157e308, "DIVERGENT")

# D. Exact kinds: any difference is DIVERGENT, whatever the context (REQ-005, AC-4).
for c in CLASSES:
    case(f"exact_ulp_{c}", "exact", 3.0, up(3.0), "DIVERGENT", det=c)
case("exact_count_differs", "exact", 4.0, 5.0, "DIVERGENT")
case("exact_signed_zero", "exact", 0.0, -0.0, "DIVERGENT")
case("exact_identical_count", "exact", 17.0, 17.0, "IDENTICAL", same_manifest=True, det="D0")
case("exact_limit_ignored", "exact", 2.0, 2.0, "IDENTICAL", limit="2")
case("exact_absent_one", "exact", 1.0, None, "DIVERGENT")

# E. Absent values, UEC-0 (REQ-002, AC-3).
case("uec0_both_absent", "expanded_uncertainty", None, None, "IDENTICAL")
case("uec0_recorded_absent", "expanded_uncertainty", None, 0.004, "DIVERGENT")
case("uec0_replayed_absent", "expanded_uncertainty", 0.004, None, "DIVERGENT")
case("uec0_both_absent_same_manifest", "expanded_uncertainty", None, None, "IDENTICAL", same_manifest=True, det="D0")
case("uec0_one_absent_zero", "expanded_uncertainty", 0.0, None, "DIVERGENT")
case("absent_both_length", "length", None, None, "IDENTICAL")

# F. MARGINAL: a value within its bound of a caller-supplied limit; outcome unchanged (REQ-007).
case("marginal_identical", "length", 0.01, 0.01, "IDENTICAL", "MARGINAL", limit="0.0100000000005")
case("marginal_exact_on_limit", "length", 0.5, 0.5, "IDENTICAL", "MARGINAL", limit="0.5")
case("marginal_not_near", "length", 0.01, 0.01, "IDENTICAL", limit="0.010000002")
case("marginal_limit_exactly_bound_away", "length", 0.5, 0.5, "IDENTICAL", "MARGINAL", limit="0.500000001")
case("marginal_limit_just_beyond_bound", "length", 0.5, 0.5, "IDENTICAL", limit="0.5000000010000000000001")
case("marginal_replayed_only", "length", 0.4, 0.5, "DIVERGENT", "MARGINAL", limit="0.5000000002")
case("marginal_recorded_only", "length", 0.5, 0.4, "DIVERGENT", "MARGINAL", limit="0.4999999999")
case("marginal_equivalent", "length", 2.0, 2.0000000004, "EQUIVALENT", "MARGINAL", limit="2.0000000008")
case("marginal_same_manifest_divergent", "length", 2.0, up(2.0), "DIVERGENT", "MARGINAL", same_manifest=True,
     det="D0", limit="2")
case("marginal_p", "p_conformance", 0.9500004, 0.9500004, "IDENTICAL", "MARGINAL", limit="0.95")
case("marginal_p_not", "p_conformance", 0.951, 0.951, "IDENTICAL", limit="0.95")
case("marginal_relative", "dimensionless", 1000.0, 1000.0, "IDENTICAL", "MARGINAL", limit="1000.0000009")
case("marginal_relative_not", "dimensionless", 1000.0, 1000.0, "IDENTICAL", limit="1000.0000011")
case("marginal_negative_limit", "angle", -0.25, -0.25, "IDENTICAL", "MARGINAL", limit="-0.2500000005")
case("marginal_absent_values", "expanded_uncertainty", None, None, "IDENTICAL", limit="0.001")
case("marginal_one_absent", "length", 0.3, None, "DIVERGENT", "MARGINAL", limit="0.3")
case("marginal_inf_ignored", "length", math.inf, math.inf, "IDENTICAL", limit="1e300")

# G. Errors.
case("nan_recorded", "length", math.nan, 1.0, "ERR:nan_input")
case("nan_replayed", "angle", 1.0, math.nan, "ERR:nan_input")
case("nan_both_exact", "exact", math.nan, math.nan, "ERR:nan_input")
case("nan_with_absent", "expanded_uncertainty", None, math.nan, "ERR:nan_input")
case("kind_unknown", "roughness", 1.0, 1.0, "ERR:kind_unknown")
case("kind_unknown_case", "Length", 1.0, 1.0, "ERR:kind_unknown")

HEADER = ["name", "kind", "recorded", "replayed", "same_manifest", "determinism", "same_platform_class", "limit",
          "expected", "annotation"]
for r in rows:
    assert all("," not in f for f in r), r
CASES_TEXT = "".join(",".join(r) + "\n" for r in [HEADER] + rows)
DIGEST_TEXT = "sha256:" + hashlib.sha256(TABLE_TEXT.encode("ascii")).hexdigest() + "\n"

OUTPUTS = [(TABLE_PATH, TABLE_TEXT), (DIGEST_PATH, DIGEST_TEXT), (CASES_PATH, CASES_TEXT)]


def main():
    if "--check" in sys.argv[1:]:
        stale = [str(p) for p, t in OUTPUTS if not p.exists() or p.read_bytes() != t.encode("ascii")]
        if stale:
            print("stale:", *stale)
            sys.exit(1)
        print(f"ok: {len(rows)} cases, table digest {DIGEST_TEXT.strip()}")
        return
    for p, t in OUTPUTS:
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(t.encode("ascii"))
    print(f"wrote {len(rows)} cases")


main()
