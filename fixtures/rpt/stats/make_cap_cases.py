#!/usr/bin/env python3
"""Independent reference for RPT.STATS.CAP.INDEX (capability index kernel), stdlib only.

Writes fixtures/rpt/stats/cap_cases.json (CC-BY-4.0 project data, invented inputs, D-038).
Nothing here is produced by running the Rust crate `fid-stats`.

Every case below is hand-chosen and hand-labelled from the leaf's rules
(spec/rpt/stats/cap/index/NODE.md); the script re-derives each label with its own reading of
the rules (`py_capability`) and refuses to write if a label disagrees. For value cases it
computes three things, none of them with the Rust code:

  exact     the index of the double inputs in exact rational arithmetic (fractions.Fraction),
            written as "p/q";
  expected  `exact` correctly rounded to the nearest double (float(Fraction) rounds once), as
            16 hex digits of the IEEE 754 bits;
  pinned    the result of the leaf's pinned operation order evaluated with Python floats, which
            are IEEE 754 binary64 with each +, -, *, / rounded to nearest-even and never fused:
            potential = (usl - lsl) / (6.0 * sigma), centred = min(usl - mean, mean - lsl) /
            (3.0 * sigma) (one-sided: the one difference), then + 0.0 on each index so a
            signed-zero input never gives -0.0 (min is IEEE totalOrder, -0.0 below +0.0). D0 means every target must give these
            bits exactly.

`tolerance_ulp` is the ulp distance between `pinned` and `expected` (0 or 1; the script refuses
a case where the pinned order is more than 1 ulp from the correctly rounded value), so AC-1's
"exactly or within 1 ulp as stated per case" is stated per case. `hand` is the value a person
computes from the decimal inputs (for example 0.2 / 0.12 = 5/3); the script checks that
`exact` is within a relative 1e-12 of it, so a typo in an input cannot hide.

`digest` is SHA-256 over one line per case, in file order, "<id> <potential> <centred>\\n",
where an index is "v:<16 hex bits of pinned>" or its status code, and an error case is
"<id> error:<reason>\\n". The Rust test recomputes it from the kernel's outputs on every CI
target (the cross-target digest test).

Run:   python fixtures/rpt/stats/make_cap_cases.py
Check: python fixtures/rpt/stats/make_cap_cases.py --check
"""

import hashlib
import json
import math
import re
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "cap_cases.json"

ALGORITHM = "stats.cap.index@1.0.0"
NA, INSUF, ZERO = "NOT_APPLICABLE", "INSUFFICIENT_DATA", "UNDEFINED_ZERO_SPREAD"


def bits(x):
    return format(struct.unpack("<Q", struct.pack("<d", x))[0], "016x")


def ulp_distance(a, b):
    ia = struct.unpack("<q", struct.pack("<d", a))[0]
    ib = struct.unpack("<q", struct.pack("<d", b))[0]
    assert (ia < 0) == (ib < 0), (a, b)
    return abs(ia - ib)


def num(s):
    return None if s is None else float(s)


def py_capability(c):
    """This script's own reading of the leaf rules. Returns ("error", reason) or
    ("ok", side, potential, centred) where an index is a status string or ("v", pinned, exact)."""
    mean, sigma, lsl, usl = num(c["mean"]), num(c["sigma"]), num(c["lsl"]), num(c["usl"])
    for name, x in (("mean", mean), ("sigma", sigma), ("lsl", lsl), ("usl", usl)):
        if x is not None and not math.isfinite(x):
            return ("error", "non_finite_" + name)
    if sigma < 0.0:  # -0.0 is not below zero: it is a zero spread
        return ("error", "negative_sigma")
    if lsl is None and usl is None:
        return ("error", "no_limits")
    if lsl is not None and usl is not None and not usl > lsl:
        return ("error", "limits_not_ordered")
    if c["min_n"] < 2:
        return ("error", "min_n_too_small")
    side = "BOTH" if (lsl is not None and usl is not None) else ("UPPER_ONLY" if usl is not None else "LOWER_ONLY")
    status = INSUF if c["n"] < c["min_n"] else (ZERO if sigma == 0.0 else None)
    if status is not None:
        return ("ok", side, status if side == "BOTH" else NA, status)
    fm, fs = Fraction(mean), Fraction(sigma)
    if side == "BOTH":
        fl, fu = Fraction(lsl), Fraction(usl)
        pot = ("v", (usl - lsl) / (6.0 * sigma), (fu - fl) / (6 * fs))
        cen = ("v", total_min(usl - mean, mean - lsl) / (3.0 * sigma), min(fu - fm, fm - fl) / (3 * fs))
    elif side == "UPPER_ONLY":
        pot = NA
        cen = ("v", (usl - mean) / (3.0 * sigma), (Fraction(usl) - fm) / (3 * fs))
    else:
        pot = NA
        cen = ("v", (mean - lsl) / (3.0 * sigma), (fm - Fraction(lsl)) / (3 * fs))
    for ix in (pot, cen):
        if ix != NA and not math.isfinite(ix[1]):
            return ("error", "result_not_finite")
    pot, cen = (ix if ix == NA else ("v", ix[1] + 0.0, ix[2]) for ix in (pot, cen))
    return ("ok", side, pot, cen)


def total_min(a, b):
    """IEEE totalOrder minimum of two non-NaN doubles: -0.0 is below +0.0."""
    if a == b:
        return a if math.copysign(1.0, a) < 0 else b
    return a if a < b else b


def case(id_, note, mean, sigma, lsl, usl, n, label, min_n=25, family="P", estimator="overall_sd@1", hand=None):
    return dict(id=id_, note=note, mean=mean, sigma=sigma, lsl=lsl, usl=usl, n=n, min_n=min_n, family=family,
                estimator=estimator, label=label, hand=hand or {})


# Hand-chosen, hand-labelled cases. label: "value" (numbers where the rules give them),
# a (side, potential-status, centred-status) triple for typed non-results, or "error:<reason>".
CASES = [
    # AC-1: centred, off-centre, mean outside the limits, upper-only, lower-only.
    case("centred", "both limits, mean on the midpoint: 0.2/0.12 and 0.1/0.06", "10.0", "0.02", "9.9", "10.1", 50,
         "value", hand={"potential": "5/3", "centred": "5/3"}),
    case("off_centre", "both limits, mean nearer USL: 12/6 and min(5, 7)/3", "7", "1", "0", "12", 30, "value",
         hand={"potential": "2", "centred": "5/3"}),
    case("mean_outside", "mean above USL gives a negative centred index: 10/3 and min(-1, 11)/1.5", "11", "0.5", "0",
         "10", 40, "value", hand={"potential": "10/3", "centred": "-2/3"}),
    case("upper_only", "AC-2: USL 10, mu 7, sigma 1, n 30: (10 - 7)/3", "7", "1", None, "10", 30, "value",
         hand={"centred": "1"}),
    case("lower_only", "lower limit only: (5 - 2)/1.5", "5", "0.5", "2", None, 30, "value", hand={"centred": "2"}),
    # More value cases.
    case("metric_c_family", "metres, within-subgroup sigma (C family): 0.00006/0.0000486 and 0.000026/0.0000243",
         "0.025004", "0.0000081", "0.02497", "0.02503", 125, "value", family="C", estimator="rbar_d2@1",
         hand={"potential": "1.2345679012345678", "centred": "1.0699588477366255"}),
    case("mean_on_usl", "mean exactly on USL: centred index 0, potential 4/1.5", "4", "0.25", "0", "4", 30, "value",
         hand={"potential": "8/3", "centred": "0"}),
    case("n_equals_min", "n equal to the minimum gives numbers: 2/0.6 and 1/0.3", "1", "0.1", "0", "2", 25, "value",
         hand={"potential": "10/3", "centred": "10/3"}),
    case("lower_only_negative", "mean below LSL, lower limit only: (4 - 5)/3", "4", "1", "5", None, 30, "value",
         family="C", estimator="mrbar_d2@1", hand={"centred": "-1/3"}),
    case("tiny_sigma", "very small but representable spread: 2/6e-300 and 1/3e-300", "0", "1e-300", "-1", "1", 30,
         "value", hand={"potential": "1/3e-300", "centred": "1/3e-300"}),
    case("one_ulp_off_centre", "three roundings leave the centred index 1 ulp from the correctly rounded value: "
         "5.2/0.0186 and min(2.97, 2.23)/0.0093", "2.93", "0.0031", "0.7", "5.9", 60, "value",
         hand={"potential": "5.2/0.0186", "centred": "2.23/0.0093"}),
    case("signed_zero_lower", "mu -0.0 on LSL +0.0: (-0.0 - 0.0)/3 is -0.0 before the + 0.0, so the index is +0.0",
         "-0.0", "1", "0", None, 30, "value", hand={"centred": "0"}),
    case("signed_zero_upper", "USL -0.0 with mu +0.0: (-0.0 - 0.0)/3 is -0.0 before the + 0.0", "0", "1", None,
         "-0.0", 30, "value", hand={"centred": "0"}),
    case("signed_zero_both", "USL -0.0, mu +0.0, LSL -3: total_min(-0.0, 3) is -0.0 before the + 0.0", "0", "0.5",
         "-3", "-0.0", 30, "value", hand={"potential": "1", "centred": "0"}),
    case("custom_min_n", "a caller-configured minimum of 5 with n 6", "0.5", "0.125", "0", "1", 6, "value", min_n=5,
         hand={"potential": "4/3", "centred": "4/3"}),
    # AC-3 and the typed statuses.
    case("ac3_insufficient", "AC-3: n 24 below the default 25, sigma 0: insufficient data wins", "5", "0", "0", "10",
         24, ("BOTH", INSUF, INSUF)),
    case("ac3_zero_spread", "AC-3: n 30, sigma 0", "5", "0", "0", "10", 30, ("BOTH", ZERO, ZERO)),
    case("insufficient_nonzero", "n 24 with a spread: still no number", "5", "1", "0", "10", 24, ("BOTH", INSUF, INSUF)),
    case("n_zero", "no values at all", "5", "1", "0", "10", 0, ("BOTH", INSUF, INSUF)),
    case("upper_insufficient", "one-sided and too few values: potential stays not applicable", "7", "1", None, "10",
         10, ("UPPER_ONLY", NA, INSUF)),
    case("lower_zero_spread", "one-sided and zero spread", "3", "0", "2", None, 30, ("LOWER_ONLY", NA, ZERO)),
    case("negative_zero_sigma", "sigma -0.0 is a zero spread, not a negative one", "5", "-0.0", "0", "10", 30,
         ("BOTH", ZERO, ZERO)),
    # AC-4 and the error path.
    case("ac4_limits_reversed", "AC-4: USL 1, LSL 2", "1.5", "0.1", "2", "1", 30, "error:limits_not_ordered"),
    case("limits_equal", "USL equal to LSL", "2", "0.1", "2", "2", 30, "error:limits_not_ordered"),
    case("error_before_status", "a bad input fails even when n is too small", "1.5", "0", "2", "1", 3,
         "error:limits_not_ordered"),
    case("nan_mean", "non-finite mean", "nan", "1", "0", "10", 30, "error:non_finite_mean"),
    case("inf_sigma", "non-finite sigma", "5", "inf", "0", "10", 30, "error:non_finite_sigma"),
    case("neg_inf_lsl", "non-finite lower limit", "5", "1", "-inf", "10", 30, "error:non_finite_lsl"),
    case("inf_usl_upper_only", "non-finite upper limit", "5", "1", None, "inf", 30, "error:non_finite_usl"),
    case("negative_sigma", "negative spread", "5", "-0.5", "0", "10", 30, "error:negative_sigma"),
    case("no_limits", "neither limit: no index is defined", "5", "1", None, None, 30, "error:no_limits"),
    case("min_n_one", "a minimum below 2 cannot support a spread", "5", "1", "0", "10", 30, "error:min_n_too_small",
         min_n=1),
    case("overflow_tolerance", "USL - LSL overflows to infinity", "0", "1", "-1.5e308", "1.5e308", 30,
         "error:result_not_finite"),
    case("overflow_upper_only", "one-sided: USL - mu overflows to infinity", "-1.5e308", "1", None, "1.5e308", 30,
         "error:result_not_finite"),
    case("overflow_subnormal_sigma", "a subnormal sigma makes the quotient overflow", "0", "5e-324", "-1", "1", 30,
         "error:result_not_finite"),
]

# Estimator ID syntax (hand-labelled): <name>@<major>, name [a-z][a-z0-9_.]*, major 1-9999 with
# no leading zero, at most 64 bytes in all.
ESTIMATORS = [
    ("rbar_d2@1", True), ("mrbar_d2@1", True), ("overall_sd@1", True), ("sbar_c4@12", True),
    ("stats.sd.pooled@3", True), ("a" * 62 + "@1", True),
    ("", False), ("RBAR@1", False), ("rbar", False), ("rbar@", False), ("rbar@0", False), ("rbar@01", False),
    ("rbar@1 ", False), ("rbär@1", False), ("_rbar@1", False), ("1rbar@1", False), ("rbar@12345", False),
    ("a" * 63 + "@1", False), ("rbar@1@2", False),
]
EST_RE = re.compile(r"[a-z][a-z0-9_.]*@[1-9][0-9]{0,3}")


def index_json(ix, hand):
    if isinstance(ix, str):
        return {"status": ix}
    _, pinned, exact = ix
    expected = float(exact)  # correctly rounded once
    tol = ulp_distance(pinned, expected)
    assert tol <= 1, ("pinned order more than 1 ulp from the correctly rounded value", pinned, expected)
    if hand is not None:
        hv = hand_value(hand)
        assert abs(float(exact) - hv) <= 1e-12 * abs(hv) or (hv == 0 and exact == 0), (hand, float(exact))
    return {"value": {"expected": bits(expected), "pinned": bits(pinned), "tolerance_ulp": tol,
                      "exact": f"{exact.numerator}/{exact.denominator}", "decimal": repr(pinned),
                      "hand": hand}}


def hand_value(h):
    if "/" in h:
        p, q = h.split("/")
        return float(p) / float(q)
    return float(h)


def digest_token(ix):
    return ix if isinstance(ix, str) else "v:" + bits(ix[1])


def build():
    cases, lines = [], []
    ids = set()
    for c in CASES:
        assert c["id"] not in ids, c["id"]
        ids.add(c["id"])
        got = py_capability(c)
        lab = c["label"]
        out = {k: c[k] for k in ("id", "note", "mean", "sigma", "lsl", "usl", "n", "min_n", "family", "estimator")}
        if isinstance(lab, str) and lab.startswith("error:"):
            assert got == ("error", lab[6:]), (c["id"], got)
            out["expect"] = {"error": lab[6:]}
            lines.append(f"{c['id']} error:{lab[6:]}\n")
        else:
            assert got[0] == "ok", (c["id"], got)
            _, side, pot, cen = got
            if lab == "value":
                assert not isinstance(cen, str), (c["id"], got)
                assert side == "BOTH" or pot == NA, (c["id"], got)
            else:
                assert (side, pot, cen) == lab, (c["id"], got)
            out["expect"] = {"side": side, "potential": index_json(pot, c["hand"].get("potential")),
                             "centred": index_json(cen, c["hand"].get("centred"))}
            lines.append(f"{c['id']} {digest_token(pot)} {digest_token(cen)}\n")
        cases.append(out)
    est = []
    for s, ok in ESTIMATORS:
        mine = len(s.encode("utf-8")) <= 64 and EST_RE.fullmatch(s) is not None
        assert mine == ok, (s, ok)
        est.append({"id": s, "valid": ok})
    doc = {
        "fixture": "RPT.STATS.CAP.INDEX capability index cases",
        "licence": "CC-BY-4.0",
        "generator": "fixtures/rpt/stats/make_cap_cases.py",
        "algorithm": ALGORITHM,
        "default_min_n": 25,
        "cases": cases,
        "estimator_ids": est,
        "digest": hashlib.sha256("".join(lines).encode("ascii")).hexdigest(),
    }
    return json.dumps(doc, indent=1, ensure_ascii=True) + "\n"


def main():
    text = build()
    if "--check" in sys.argv[1:]:
        have = OUT.read_text(encoding="utf-8") if OUT.exists() else ""
        if have != text:
            print(f"{OUT} is out of date; run python fixtures/rpt/stats/make_cap_cases.py")
            return 1
        print("cap_cases.json up to date")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {OUT}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
