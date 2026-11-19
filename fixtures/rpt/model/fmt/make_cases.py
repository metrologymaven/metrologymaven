#!/usr/bin/env python3
"""Independent reference for spec node RPT.MODEL.FMT (AC-1, AC-2, AC-4, AC-6, AC-10).

Writes, next to this script:

- fmt_cases.json: the input records (characteristic, value bits, limits, decision) of
  AC-1 plus the AC-2 and AC-4 side-rule records and a tie at 17 digits;
- fmt_cases.golden.json: for each record the canonical string, the display strings for
  locales `en` and `de`, the formatter, unit, precision, `digits_added`, side and tie flag;
- locale_corpus.json / locale_corpus.golden.json: canonical decimal strings and their
  renderings under the test locales `en`, `de` and `en-IN` (grouping 3 then 2).

Everything is computed here with exact rationals (`fractions.Fraction`; a double is an exact
dyadic rational) and integer arithmetic, from the rules written in spec/rpt/model/fmt/NODE.md
and the kernel formatter rules of KRN.NUM.FMT.LEN / KRN.NUM.FMT.ANG (half-even rounding at
the requested decimals, then the side-preserving rule: add one decimal at a time until the
printed value lies strictly beyond the violated limit, capped at 17 significant digits, where
the value is rounded toward the violated side and the tie flag is set). The Rust code is
never run to produce these files. Pi for the degree conversion is bracketed between two
60-decimal rationals from Machin's formula, and every degree output is checked to be the same
at both ends of that bracket.

Licence of the generated fixtures: CC-BY-4.0. Usage: make_cases.py [--check]
"""

from __future__ import annotations

import json
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
SIG_CAP = 17

# Display value = SI value * factor (exact).
FACTOR = {"m": Fraction(1), "mm": Fraction(1000), "um": Fraction(10**6), "in": Fraction(10000, 254),
          "1": Fraction(1), "rad": Fraction(1)}
FORMATTER = {"length": "fmt.length@1", "angle": "fmt.angle@1", "scalar": "fmt.scalar@1"}
# report.default_decimals initial values (REQ-RPT.MODEL.FMT-009).
DEFAULT_DECIMALS = {"mm": 3, "um": 1, "in": 4, "deg": 4, "rad": 6, "1": 4}


def machin_pi(digits: int) -> Fraction:
    """floor(pi * 10^digits) / 10^digits, from Machin's formula with 20 guard digits."""
    scale = 10 ** (digits + 20)

    def arctan_inv(n: int) -> int:
        total, term, k, sign = 0, scale // n, 1, 1
        n2 = n * n
        while term:
            total += sign * (term // k)
            term //= n2
            k += 2
            sign = -sign
        return total

    pi_scaled = 16 * arctan_inv(5) - 4 * arctan_inv(239)
    return Fraction(pi_scaled // 10**20, 10**digits)


PI_LO = machin_pi(60)
PI_HI = PI_LO + Fraction(1, 10**60)


def bits_of(x: float) -> str:
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


def value_of(bits: str) -> Fraction:
    return Fraction(struct.unpack(">d", struct.pack(">Q", int(bits, 16)))[0])


def next_up(x: float) -> float:
    b = struct.unpack(">Q", struct.pack(">d", x))[0]
    return struct.unpack(">d", struct.pack(">Q", b + 1 if x >= 0 else b - 1))[0]


def floor_log10(v: Fraction) -> int:
    v = abs(v)
    k = len(str(v.numerator)) - len(str(v.denominator))
    while Fraction(10) ** k > v:
        k -= 1
    while Fraction(10) ** (k + 1) <= v:
        k += 1
    return k


def floor_frac(v: Fraction) -> int:
    return v.numerator // v.denominator


def round_half_even(v: Fraction, p: int) -> int:
    t = abs(v) * 10**p
    fl = floor_frac(t)
    r = t - fl
    if r > Fraction(1, 2) or (r == Fraction(1, 2) and fl % 2 == 1):
        fl += 1
    return fl


def render(neg: bool, q: int, p: int) -> str:
    s = str(q)
    if p > 0:
        s = s.rjust(p + 1, "0")
        s = s[:-p] + "." + s[-p:]
    return ("-" if neg and q != 0 else "") + s


def parse_dec(s: str) -> Fraction:
    return Fraction(s)


def render_dms(neg: bool, q: int, p: int) -> str:
    """q counts 10^-p seconds: split exactly into D:MM:SS[.s...]."""
    unit = 10**p
    d, rest = divmod(q, 3600 * unit)
    m, sec = divmod(rest, 60 * unit)
    whole, frac = divmod(sec, unit)
    out = f"{d}:{m:02d}:{whole:02d}"
    if p > 0:
        out += "." + str(frac).rjust(p, "0")
    return ("-" if neg and q != 0 else "") + out


def fmt_exact(v: Fraction, p: int, upper: Fraction | None, lower: Fraction | None, dms: bool = False):
    """Display value v (exact; seconds for dms), decimals p, limits in the same unit.
    Returns (canonical, digits_added, tie, side)."""
    show = render_dms if dms else render
    neg = v < 0
    if upper is not None and v > upper:
        violated, side = "upper", "above"
    elif lower is not None and v < lower:
        violated, side = "lower", "below"
    else:
        violated = None
        side = "none" if upper is None and lower is None else "within"
    if violated is None:
        return show(neg, round_half_even(v, p), p), 0, False, side
    k = floor_log10(v) if v != 0 else 0
    added = 0
    while True:
        q = round_half_even(v, p)
        out = Fraction(-q if neg else q, 10**p)
        if (violated == "upper" and out > upper) or (violated == "lower" and out < lower):
            return show(neg, q, p), added, False, side
        if k + 1 + p >= SIG_CAP:
            t = v * 10**p
            w = -floor_frac(-t) if violated == "upper" else floor_frac(t)
            return show(w < 0, abs(w), p), added, True, side
        p += 1
        added += 1


def fmt_angle(x: Fraction, unit: str, p: int, upper: Fraction | None, lower: Fraction | None):
    """deg or dms with a pi bracket: both ends must give the same result (limits in degrees,
    compared in seconds for dms)."""
    c, scale = (180, 1) if unit == "deg" else (648000, 3600)
    up = upper * scale if upper is not None else None
    lo = lower * scale if lower is not None else None
    ends = [fmt_exact(x * c / pi, p, up, lo, unit == "dms") for pi in (PI_HI, PI_LO)]
    if ends[0] != ends[1]:
        raise SystemExit(f"angle case not decided by the 60-digit bracket: {ends}")
    return ends[0]


def next_down(x: float) -> float:
    return -next_up(-x)


def first_above(x: float, deg_limit: Fraction) -> float:
    """The first double at or after x whose degree value lies strictly above deg_limit,
    decided with the pi bracket at both ends."""
    while not Fraction(x) * 180 / PI_HI > deg_limit:
        x = next_up(x)
    return x


# --------------------------------------------------------------------------- locales

LOCALES = {
    "en": {"decimal": ".", "group": ",", "primary": 3, "secondary": 3, "minus": "-"},
    "de": {"decimal": ",", "group": ".", "primary": 3, "secondary": 3, "minus": "-"},
    "en-IN": {"decimal": ".", "group": ",", "primary": 3, "secondary": 2, "minus": "-"},
    # Arabic-style right-to-left presentation: ARABIC DECIMAL SEPARATOR, ARABIC THOUSANDS
    # SEPARATOR, and ARABIC LETTER MARK before the hyphen-minus.
    "ar": {"decimal": "\u066b", "group": "\u066c", "primary": 3, "secondary": 3, "minus": "\u061c-"},
}


def locale_render(canonical: str, loc: dict) -> str:
    neg = canonical.startswith("-")
    body = canonical[1:] if neg else canonical
    int_part, frac = body.split(".", 1) if "." in body else (body, None)
    if ":" in body:  # dms: no grouping, only the seconds' decimal point changes
        out = int_part
    else:
        groups = []
        rest = int_part
        size = loc["primary"]
        while len(rest) > size:
            groups.insert(0, rest[-size:])
            rest = rest[:-size]
            size = loc["secondary"]
        groups.insert(0, rest)
        out = loc["group"].join(groups)
    if frac is not None:
        out += loc["decimal"] + frac
    return (loc["minus"] if neg else "") + out


CORPUS = [
    "0", "1", "-1", "0.5", "-0.5", "12", "123", "1234", "-1234", "12345", "123456", "1234567",
    "12345678", "1000000", "1000.000", "1234.5", "1234567.890", "-1234567.000", "0.001",
    "0.0010", "0.00100", "100.000", "-0.010", "999.999", "9999.9999", "12.345", "1.2345",
    "0.30000000000000005", "0.05000000000000001", "-0.012", "123456789012345678901",
    "1:05:03.25", "-12:30:15.5", "0:00:00", "57.3", "45.0000",
]


# --------------------------------------------------------------------------- cases


def lim(lower: str | None, upper: str | None, unit: str = "si") -> dict:
    return {"lower": lower, "upper": upper, "unit": unit}


CASES = [
    dict(name="len_mm_auth3_conforms", kind="length", unit="mm", decimals=3, value=0.0123456,
         limits=lim("0.012", "0.013"), acceptance=None, outcome="CONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="len_mm_default_nolimits", kind="length", unit="mm", decimals=None, value=0.025,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="len_in_auth4_conforms", kind="length", unit="in", decimals=4, value=0.0313578,
         limits=lim("0.03", "0.032"), acceptance=None, outcome="CONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="len_um_default", kind="length", unit="um", decimals=None, value=1.23456e-5,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="len_mm_grouping", kind="length", unit="mm", decimals=3, value=1234.5678,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="len_m_auth4", kind="length", unit="m", decimals=4, value=1.23456,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="len_mm_negative_below", kind="length", unit="mm", decimals=3, value=-1.23e-5,
         limits=lim("-0.00001", "0.00001"), acceptance=None, outcome="NONCONFORMS",
         rule="dr:simple-acceptance@1"),
    # AC-2: the next double above 5e-5 m against an upper limit of exactly 0.050 mm.
    dict(name="ac2_side_rule", kind="length", unit="mm", decimals=3, value=next_up(5e-5),
         limits=lim(None, "0.00005"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    # AC-4: guard-banded rule; acceptance upper limit 0.048 mm inside the spec limit 0.050 mm.
    dict(name="ac4_guard_band", kind="length", unit="mm", decimals=3, value=4.800001e-5,
         limits=lim(None, "0.00005"), acceptance=lim(None, "0.000048"), outcome="NONCONFORMS",
         rule="dr:guard-band(r=1)@1"),
    dict(name="ang_deg_auth4", kind="angle", unit="deg", decimals=4, value=0.5,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="ang_rad_default", kind="angle", unit="rad", decimals=None, value=0.123456789,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="ang_deg_default_within", kind="angle", unit="deg", decimals=None, value=0.7853981633974483,
         limits=lim("44.9", "45.1", "deg"), acceptance=None, outcome="CONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="scalar_auth2", kind="scalar", unit="1", decimals=2, value=0.987654,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="scalar_default_conforms", kind="scalar", unit="1", decimals=None, value=3.14159,
         limits=lim("3", "4"), acceptance=None, outcome="CONFORMS", rule="dr:simple-acceptance@1"),
    # Tie at 17 digits: 0.1 + 0.2 lies above 0.30000000000000004, which is also its 17-digit
    # half-even rounding, so the rule rounds toward the violated side and flags the tie.
    dict(name="scalar_tie_at_cap", kind="scalar", unit="1", decimals=2, value=0.1 + 0.2,
         limits=lim(None, "0.30000000000000004"), acceptance=None, outcome="NONCONFORMS",
         rule="dr:simple-acceptance@1"),

    # Just outside a limit in every unit: the side rule must print strictly beyond it.
    dict(name="nc_in_just_above", kind="length", unit="in", decimals=4, value=next_up(0.0254),
         limits=lim(None, "0.0254"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_um_just_above", kind="length", unit="um", decimals=1, value=next_up(1e-5),
         limits=lim(None, "0.00001"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_m_just_above", kind="length", unit="m", decimals=3, value=next_up(0.5),
         limits=lim(None, "0.5"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_rad_just_above", kind="angle", unit="rad", decimals=None, value=0.1,
         limits=lim(None, "0.1"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_deg_just_above", kind="angle", unit="deg", decimals=4, value=first_above(0.5235987755982988, Fraction(30)),
         limits=lim(None, "30", "deg"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_dms_just_above", kind="angle", unit="dms", decimals=0, value=first_above(0.5235987755982988, Fraction(30)),
         limits=lim(None, "30", "deg"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_one_just_above", kind="scalar", unit="1", decimals=2, value=next_up(1.0),
         limits=lim(None, "1"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    # Below the lower limit by one ulp: digits are added on the low side.
    dict(name="nc_mm_just_below", kind="length", unit="mm", decimals=3, value=next_down(1e-5),
         limits=lim("0.00001", None), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    # Angles near the 0/360 wrap: the formatter never wraps, and rounding carries into 360.
    dict(name="wrap_deg_360", kind="angle", unit="deg", decimals=4, value=6.283185307179586,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="wrap_dms_360", kind="angle", unit="dms", decimals=0, value=6.283185307179586,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="wrap_deg_below_zero", kind="angle", unit="deg", decimals=4, value=-1e-9,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    dict(name="wrap_dms_below_zero", kind="angle", unit="dms", decimals=1, value=-1e-9,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None),
    # NONCONFORMS at the wrap: just below 0 rad against a lower limit of 0 deg, and just
    # below 2 pi against an upper limit of 359.99 deg (the formatter never wraps to 0).
    dict(name="nc_wrap_below_zero", kind="angle", unit="deg", decimals=4, value=-1e-12,
         limits=lim("0", None, "deg"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_wrap_below_2pi_deg", kind="angle", unit="deg", decimals=4, value=6.283185307179586,
         limits=lim(None, "359.99", "deg"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
    dict(name="nc_wrap_below_2pi_dms", kind="angle", unit="dms", decimals=0, value=6.283185307179586,
         limits=lim(None, "359.99", "deg"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1"),
] + [
    # Precision sweep 0..9: plain rounding, and the side rule from every starting precision.
    dict(name=f"sweep_mm_d{d}", kind="length", unit="mm", decimals=d, value=0.0123456789012,
         limits=None, acceptance=None, outcome="NOT_DECIDABLE", rule=None)
    for d in range(10)
] + [
    dict(name=f"sweep_nc_mm_d{d}", kind="length", unit="mm", decimals=d, value=next_up(1.25e-5),
         limits=lim(None, "0.0000125"), acceptance=None, outcome="NONCONFORMS", rule="dr:simple-acceptance@1")
    for d in range(10)
]


def expected(case: dict) -> dict:
    x = value_of(case["value_bits"])
    unit = case["unit"]
    p = case["decimals"] if case["decimals"] is not None else DEFAULT_DECIMALS[unit]
    lims = case["acceptance"] if case["rule"] and case["rule"].startswith("dr:guard-band(") else case["limits"]
    lower = upper = None
    if lims is not None:
        lower = parse_dec(lims["lower"]) if lims["lower"] is not None else None
        upper = parse_dec(lims["upper"]) if lims["upper"] is not None else None
    if unit in ("deg", "dms"):
        canonical, added, tie, side = fmt_angle(x, unit, p, upper, lower)
    else:
        f = FACTOR[unit]
        canonical, added, tie, side = fmt_exact(
            x * f, p, upper * f if upper is not None else None, lower * f if lower is not None else None
        )
    if case["outcome"] == "NONCONFORMS":
        assert side in ("above", "below"), case["name"]
    if case["outcome"] == "CONFORMS":
        assert side == "within", case["name"]
    side_name = {"none": "not_applicable"}.get(side, side)
    return {
        "canonical": canonical,
        "display_en": locale_render(canonical, LOCALES["en"]),
        "display_de": locale_render(canonical, LOCALES["de"]),
        "formatter": FORMATTER[case["kind"]],
        "unit": unit,
        "decimals": p,
        "digits_added": added,
        "tie": tie,
        "side": side_name,
    }


def build() -> dict[str, bytes]:
    cases = []
    for c in CASES:
        c = dict(c)
        c["value_bits"] = bits_of(c.pop("value"))
        c["statement"] = "uec1_lpu.json"
        cases.append(c)
    golden = {c["name"]: expected(c) for c in cases}
    corpus_golden = {name: [locale_render(s, loc) for s in CORPUS] for name, loc in LOCALES.items()}

    def dump(obj) -> bytes:
        return (json.dumps(obj, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("utf-8")

    return {
        "fmt_cases.json": dump({"schema": "rpt-fmt-cases@1", "cases": cases}),
        "fmt_cases.golden.json": dump(golden),
        "locale_corpus.json": dump({"schema": "rpt-locale-corpus@1", "locales": LOCALES, "canonical": CORPUS}),
        "locale_corpus.golden.json": dump(corpus_golden),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv
    bad = 0
    for name, data in build().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"{path}: out of date; run make_cases.py", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(data)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
