#!/usr/bin/env python3
"""Independent oracle for GDT.PROF.ZDEF.UNIF (fid-gps `prof::zdef`).

Writes, next to this script:

- `rows.json`: a DMT table of two synthetic rows derived from
  `fixtures/gdt/dmt/synthetic_table_v1.json` (the profile_of_a_surface and
  profile_of_a_line rows), with chain variants carrying the `share_side` token and a
  `zone_def_unsupported:<kind>` limitation. Synthetic: no interpretation of any standard.
- `constant.json`: AC-1, AC-3, AC-4, AC-5 cases (exact intervals, error paths, dispatch).
- `property.json`: AC-2, the 10 000 seeded (t, share, d) cases (seed 0x2def): a SHA-256
  over one line per case and the list of cases where `lower <= d <= upper` and
  `|d - shift| <= width * 0.5` disagree.

Every expected float is computed here with exact rational arithmetic (`fractions.Fraction`)
and one explicit round-to-nearest-even per IEEE-754 operation of the leaf's Design
(`t * 0.5`, `t - share`, `upper - lower`, `(lower + upper) * 0.5`, `d - shift`,
`width * 0.5`); negation and the zero sign are handled explicitly. Nothing is computed by
the Rust code. All values are invented.

The AC-2 case stream (also implemented in the Rust test; both must agree, which the digest
checks) is SplitMix64 from seed 0x2def, per case:
  r1 -> t = float(1000 + r1 % 9999001) * 1e-9            (1 um .. 10 mm)
  r2 -> kind = r2 % 5: 0 bilateral, 1 unilateral outward, 2 unilateral inward,
                       3 unequal (share_side outward), 4 unequal (share_side inward)
  r3 -> share = (t * float(r3 % 1001)) / 1000.0, clamped to t
  r4 -> mode = r4 % 4; modes 0, 1: d = ((r4 >> 11) * 2**-53 * 2.0 - 1.0) * t
        modes 2, 3: r5 -> bound = lower if r5 & 1 == 0 else upper; k = (r5 >> 1) % 7 - 3;
                    d = bound moved |k| steps up (k > 0) or down (k < 0) to the adjacent float
(generator arithmetic in plain binary64, as in Rust). The line for case i is
  "{i} {kind} {t} {share} {d} {lower} {upper} {width} {shift} {A} {B} {E}\\n"
with floats as 0x + 16 hex digits, A = lower <= d <= upper, B = |d - shift| <= width * 0.5
(both 0/1) and E = 1 when all four of upper - lower, (lower + upper) / 2, d - shift and
width / 2 are exact in binary64 (then A == B is asserted here).

Usage: `python3 make_zdef_fixtures.py` (write) or `--check` (regenerate in memory and
compare with the committed files byte for byte). Stdlib only.
"""

import hashlib
import json
import math
import struct
import sys
from fractions import Fraction as F
from pathlib import Path

HERE = Path(__file__).resolve().parent
SYNTHETIC = HERE.parent.parent / "dmt" / "synthetic_table_v1.json"
MASK = 0xFFFFFFFFFFFFFFFF
SEED = 0x2DEF
COUNT = 10000


def hx(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def is_neg_zero(x: float) -> bool:
    return x == 0.0 and math.copysign(1.0, x) < 0


def rnd(q: F) -> float:
    """Round an exact rational to the nearest binary64 (ties to even); 0 -> +0.0."""
    if q == 0:
        return 0.0
    # int / int true division in CPython is correctly rounded.
    return q.numerator / q.denominator


def r_add(a: float, b: float) -> float:
    q = F(a) + F(b)
    if q == 0:
        return -0.0 if (is_neg_zero(a) and is_neg_zero(b)) else 0.0
    return rnd(q)


def r_sub(a: float, b: float) -> float:
    q = F(a) - F(b)
    if q == 0:
        return -0.0 if (is_neg_zero(a) and b == 0.0 and not is_neg_zero(b)) else 0.0
    return rnd(q)


def r_half(a: float) -> float:
    q = F(a) / 2
    if q == 0:
        return -0.0 if is_neg_zero(a) else 0.0
    return rnd(q)


def neg(a: float) -> float:
    return -a


def zone(kind: str, t: float, share, side):
    """(lower, upper, width, shift) per the leaf's REQ-001..004 table."""
    if kind == "bilateral":
        h = r_half(t)
        lower, upper = neg(h), h
    elif kind == "unilateral":
        lower, upper = (0.0, t) if side == "outward" else (neg(t), 0.0)
    elif kind == "unequal":
        rest = r_sub(t, share)
        lower, upper = (neg(rest), share) if side == "outward" else (neg(share), rest)
    else:
        raise ValueError(kind)
    width = r_sub(upper, lower)
    shift = r_half(r_add(lower, upper))
    # Exact cross-check of the closed forms against the rational definition.
    tq = F(t)
    if kind == "bilateral":
        exp = (-tq / 2, tq / 2)
    elif kind == "unilateral":
        exp = (F(0), tq) if side == "outward" else (-tq, F(0))
    else:
        sq = F(share)
        exp = (-(tq - sq), sq) if side == "outward" else (-sq, tq - sq)
    assert abs(F(lower) - exp[0]) <= F(math.ulp(t)) and abs(F(upper) - exp[1]) <= F(math.ulp(t))
    return lower, upper, width, shift


# ------------------------------------------------------------------ rows.json


def rows_table():
    src = json.loads(SYNTHETIC.read_text(encoding="utf-8"))
    by_key = {r["key"]: r for r in src["rows"]}
    surf = json.loads(json.dumps(by_key["dmt:asme/profile_of_a_surface/none@1"]))
    line = json.loads(json.dumps(by_key["dmt:asme/profile_of_a_line/none@1"]))
    base_steps = surf["variants"][0]["steps"]

    def with_side(values):
        """Copy of the base steps; `values` maps step index -> share_side param value."""
        steps = json.loads(json.dumps(base_steps))
        for i, v in values.items():
            steps[i]["params"]["share_side"] = v
        return steps

    construction = 3  # the construction step of the base chain
    evaluation = 4
    variants = {
        "share_bad_kind": with_side({construction: True}),
        "share_bad_token": with_side({construction: "sideways"}),
        "share_conflict": with_side({construction: "outward", evaluation: "inward"}),
        "share_inward": with_side({construction: "inward"}),
        "share_missing": with_side({}),
        "share_outward": with_side({construction: "outward", evaluation: "outward"}),
    }
    note = (
        "Synthetic test row derived from synthetic_table_v1.json for GDT.PROF.ZDEF.UNIF tests; "
        "not an interpretation of any standard."
    )
    surf["variants"] = [{"variant": k, "steps": variants[k]} for k in sorted(variants)]
    surf["default_variant"] = "share_outward"
    surf["notes"] = note
    line["variants"] = [{"variant": k, "steps": variants[k]} for k in ("share_inward", "share_outward")]
    line["default_variant"] = "share_outward"
    line["status"] = "partial"
    line["limitations"] = ["zone_def_unsupported:nonuniform", "zone_def_unsupported:unequal"]
    line["notes"] = note
    return {"schema": src["schema"], "version": src["version"], "rows": [line, surf]}


SURF = "dmt:asme/profile_of_a_surface/none@1"
LINE = "dmt:asme/profile_of_a_line/none@1"


# ------------------------------------------------------------------ constant.json


def mm(x: float) -> str:
    """Readable annotation only (not compared): the value in mm, 6 decimals."""
    return "%.6f" % (x * 1000.0)


def interval_case(name, kind, t, share, side, row, variant):
    lower, upper, width, shift = zone(kind, t, share, side)
    d = {"kind": kind, "t": hx(t)}
    if kind == "unilateral":
        d["side"] = side
    if kind == "unequal":
        d["share"] = hx(share)
    rec = {
        "kind": kind,
        "lower": hx(lower),
        "share": hx(share) if kind == "unequal" else None,
        "share_side": side if kind == "unequal" else None,
        "shift": hx(shift),
        "side": side if kind == "unilateral" else None,
        "t": hx(t),
        "upper": hx(upper),
    }
    return {
        "name": name,
        "def": d,
        "row": row,
        "variant": variant,
        "expect": {
            "lower": hx(lower),
            "upper": hx(upper),
            "width": hx(width),
            "shift": hx(shift),
            "mm": [mm(lower), mm(upper)],
            "record": rec,
        },
    }


def error_case(name, d, row, variant, error):
    return {"name": name, "def": d, "row": row, "variant": variant, "expect": {"error": error}}


def constant_doc():
    cases = []
    # AC-1 decimal: t = 0.4 mm, share = 0.1 mm as the nearest binary64 values in metres.
    t, s = 0.4e-3, 0.1e-3
    # AC-1 dyadic: t = 105 * 2**-18 m (about 0.4005 mm), share = 26 * 2**-18 m (about
    # 0.0992 mm): every operation is exact, so the intervals are the exact rationals.
    td, sd = 105.0 * 2.0**-18, 26.0 * 2.0**-18
    for tag, tt, ss in (("decimal", t, s), ("dyadic", td, sd)):
        cases.append(interval_case(f"ac1_{tag}_bilateral", "bilateral", tt, None, None, SURF, "share_outward"))
        cases.append(interval_case(f"ac1_{tag}_unilateral_outward", "unilateral", tt, None, "outward", SURF, "share_outward"))
        cases.append(interval_case(f"ac1_{tag}_unilateral_inward", "unilateral", tt, None, "inward", SURF, "share_outward"))
        cases.append(interval_case(f"ac1_{tag}_unequal_outward", "unequal", tt, ss, "outward", SURF, "share_outward"))
        cases.append(interval_case(f"ac1_{tag}_unequal_inward", "unequal", tt, ss, "inward", SURF, "share_inward"))
    # Dyadic exactness: the intervals equal the exact rationals of REQ-001..003.
    tq, sq = F(td), F(sd)
    exact = {
        "bilateral": (-tq / 2, tq / 2),
        "unilateral_outward": (F(0), tq),
        "unilateral_inward": (-tq, F(0)),
        "unequal_outward": (-(tq - sq), sq),
        "unequal_inward": (-sq, tq - sq),
    }
    for c in cases:
        if c["name"].startswith("ac1_dyadic_"):
            lo, hi = exact[c["name"][len("ac1_dyadic_"):]]
            assert F(struct.unpack(">d", bytes.fromhex(c["expect"]["lower"][2:]))[0]) == lo
            assert F(struct.unpack(">d", bytes.fromhex(c["expect"]["upper"][2:]))[0]) == hi
    # Edges: share = 0 and share = t on both sides (lower of share 0 inward is -0.0).
    for ss, tag in ((0.0, "zero"), (t, "full")):
        cases.append(interval_case(f"edge_share_{tag}_outward", "unequal", t, ss, "outward", SURF, "share_outward"))
        cases.append(interval_case(f"edge_share_{tag}_inward", "unequal", t, ss, "inward", SURF, "share_inward"))
    # A kind the limited row does not list is still supported there.
    cases.append(interval_case("line_row_bilateral_supported", "bilateral", t, None, None, LINE, "share_outward"))
    # AC-3 and the other bad_zone_parameter paths.
    nan = "0x7ff8000000000000"
    bad = [
        ("ac3_share_above_t", {"kind": "unequal", "t": hx(t), "share": hx(0.5e-3)}, "share"),
        ("ac3_t_nan", {"kind": "bilateral", "t": nan}, "t"),
        ("t_nan_unequal", {"kind": "unequal", "t": nan, "share": hx(s)}, "t"),
        ("t_zero", {"kind": "bilateral", "t": hx(0.0)}, "t"),
        ("t_negative_zero", {"kind": "unilateral", "t": hx(-0.0), "side": "outward"}, "t"),
        ("t_negative", {"kind": "unilateral", "t": hx(-t), "side": "inward"}, "t"),
        ("t_infinite", {"kind": "bilateral", "t": "0x7ff0000000000000"}, "t"),
        ("t_subnormal", {"kind": "bilateral", "t": "0x0000000000000001"}, "t"),
        ("share_negative", {"kind": "unequal", "t": hx(t), "share": hx(-s)}, "share"),
        ("share_nan", {"kind": "unequal", "t": hx(t), "share": nan}, "share"),
        ("share_next_above_t", {"kind": "unequal", "t": hx(t), "share": hx(math.nextafter(t, 1.0))}, "share"),
    ]
    for name, d, p in bad:
        cases.append(error_case(name, d, SURF, "share_outward", {"code": "bad_zone_parameter", "name": p}))
    good_unequal = {"kind": "unequal", "t": hx(t), "share": hx(s)}
    for v in ("share_bad_kind", "share_bad_token", "share_conflict", "share_missing"):
        cases.append(error_case(f"chain_{v}", good_unequal, SURF, v, {"code": "bad_zone_parameter", "name": "share_side"}))
    # Non-unequal kinds never read the chain's share_side.
    cases.append(interval_case("chain_share_missing_bilateral", "bilateral", t, None, None, SURF, "share_missing"))
    # AC-4: the row lists unequal as unsupported (dispatch before parameter checks).
    cases.append(error_case("ac4_unequal_unsupported", good_unequal, LINE, "share_outward", {"code": "UNSUPPORTED", "kind": "unequal"}))
    cases.append(
        error_case(
            "ac4_unsupported_before_params",
            {"kind": "unequal", "t": hx(t), "share": hx(0.5e-3)},
            LINE,
            "share_outward",
            {"code": "UNSUPPORTED", "kind": "unequal"},
        )
    )
    # AC-5: non-uniform without the non-uniform module, on a row with and without the
    # (synthetic-table spelling `nonuniform`) limitation.
    nu = {"kind": "non_uniform", "widths": [hx(t), hx(2 * t)]}
    cases.append(error_case("ac5_non_uniform_unlinked", nu, SURF, "share_outward", {"code": "UNSUPPORTED", "kind": "non_uniform"}))
    cases.append(error_case("ac5_non_uniform_row_limited", nu, LINE, "share_outward", {"code": "UNSUPPORTED", "kind": "non_uniform"}))
    return {
        "schema": "fid.test.gdt.prof.zdef.constant/1",
        "table": "rows.json",
        "note": "Invented values, metres as IEEE-754 hex bits; mm annotations are for readers only.",
        "cases": cases,
    }


# ------------------------------------------------------------------ property.json


class SplitMix:
    def __init__(self, seed):
        self.s = seed & MASK

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK
        return z ^ (z >> 31)


KINDS = [
    ("bilateral", None),
    ("unilateral", "outward"),
    ("unilateral", "inward"),
    ("unequal", "outward"),
    ("unequal", "inward"),
]


def step(x: float, k: int) -> float:
    for _ in range(abs(k)):
        x = math.nextafter(x, math.inf if k > 0 else -math.inf)
    return x


def property_doc():
    rng = SplitMix(SEED)
    h = hashlib.sha256()
    disagreements = []
    exact_count = 0
    near_bound = 0
    for i in range(COUNT):
        r1 = rng.next()
        t = float(1000 + r1 % 9999001) * 1e-9
        r2 = rng.next()
        kc = r2 % 5
        kind, side = KINDS[kc]
        r3 = rng.next()
        share = (t * float(r3 % 1001)) / 1000.0
        if share > t:
            share = t
        r4 = rng.next()
        lower, upper, width, shift = zone(kind, t, share if kind == "unequal" else None, side)
        if r4 % 4 < 2:
            u = float(r4 >> 11) * 2.0**-53
            d = (u * 2.0 - 1.0) * t
        else:
            near_bound += 1
            r5 = rng.next()
            bound = lower if r5 & 1 == 0 else upper
            d = step(bound, (r5 >> 1) % 7 - 3)
        a = F(lower) <= F(d) <= F(upper)
        diff = r_sub(d, shift)
        hw = r_half(width)
        b = abs(diff) <= hw
        e = (
            F(width) == F(upper) - F(lower)
            and F(shift) == (F(lower) + F(upper)) / 2
            and F(diff) == F(d) - F(shift)
            and F(hw) == F(width) / 2
        )
        if e:
            exact_count += 1
            assert a == b, i
        elif a != b:
            dist = min(abs(F(d) - F(lower)), abs(F(d) - F(upper)))
            ulp_t = F(math.ulp(t))
            assert dist <= ulp_t, (i, dist / ulp_t)
            disagreements.append(
                {
                    "index": i,
                    "d": hx(d),
                    "lower": hx(lower),
                    "upper": hx(upper),
                    "in_interval": a,
                    "dist_over_ulp_t": "%.6f" % float(dist / ulp_t),
                }
            )
        line = "%d %d %s %s %s %s %s %s %s %d %d %d\n" % (
            i, kc, hx(t), hx(share), hx(d), hx(lower), hx(upper), hx(width), hx(shift), a, b, e,
        )
        h.update(line.encode("ascii"))
    return {
        "schema": "fid.test.gdt.prof.zdef.property/1",
        "seed": "0x2def",
        "count": COUNT,
        "near_bound_cases": near_bound,
        "exact_cases": exact_count,
        "sha256": h.hexdigest(),
        "disagreements": disagreements,
    }


def dump(obj) -> str:
    return json.dumps(obj, indent=1, sort_keys=False) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    outputs = {
        "rows.json": dump(rows_table()),
        "constant.json": dump(constant_doc()),
        "property.json": dump(property_doc()),
    }
    bad = False
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"{path}: out of date; run make_zdef_fixtures.py", file=sys.stderr)
                bad = True
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {path}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
