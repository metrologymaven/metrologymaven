#!/usr/bin/env python3
"""Independent reference for GEO.FRAME.UNITS; writes fixtures/geo/frame/units_cases.json.

Run:  python tools/fixtures/geo_units_cases.py [--check]
With --check nothing is written; the script exits non-zero if the file differs.

Stdlib only (fractions, random, math, re, json). Shares no code with the Rust crate
fid-geo-frame: decimals are parsed with a regular expression into `fractions.Fraction`, every
conversion is an exact rational product, and rounding to binary64 is CPython's correctly
rounded `int / int` true division (`float(Fraction)`), cross-checked case by case against both
neighbouring doubles (`math.nextafter`) with ties to even. The leaf names `mpmath` at 300
digits; CI's Python has only the stdlib, so pi comes from Machin's formula in 1400-bit integer
fixed point with a rigorous error bound instead (see `pi_interval`).

Rules (pinned in spec/geo/frame/units/NODE.md, Design):

* factors to SI: m 1, cm 1/100, mm 1/1000, um 1/10^6, nm 1/10^9, in 127/5000, ft 381/1250,
  thou 127/5000000; rad 1, deg PI/180, arcmin PI/10800, arcsec PI/648000, gon PI/200 with
  PI = round(pi * 2^254) / 2^254 (the 256-bit constant, written to the fixture as pi_256_hex);
* grammar `[+-]digits[.digits][(e|E)[+-]digits]`; checks in order: > 1024 bytes
  too_many_digits; nan/inf/infinity (any case, after a sign) non_finite; grammar not_decimal;
  > 40 significant digits (first non-zero digit to last digit written) too_many_digits;
  |value| >= 2^1024 - 2^970 overflow;
* one rounding to the nearest double, ties to even; the sign of the result (zero included) is
  the written sign;
* display: q = round_half_even(|x| / factor * 10^n), n digits after the point, '-' only when
  x is negative and q != 0.

Every angle case is also checked against the true pi: the result must be the same double for
the lower and the upper end of pi's interval, so the 256-bit constant never changes a fixture
answer (the leaf's [unverified] "256 bits suffice" claim, checked on these cases).

Cases (one `random.Random(13)` stream, seed 13 per NODE.md):
* parse: hand-picked cases first (AC-2 and AC-3 values, halfway values for every length unit,
  near-halfway values, subnormal and overflow boundaries, 40-digit inputs, angle landmarks),
  then random decimals up to 2000 cases: unit, 1..40 digits, point position, optional
  leading zeros, sign and exponent all drawn from the stream;
* display: hand-picked cases (AC-4's 0.0254 m in inches at 6 digits, half-even ties, signed
  zero, extremes), then random doubles up to 500 cases.
"""

import json
import math
import random
import re
import sys
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT = ROOT / "fixtures" / "geo" / "frame" / "units_cases.json"

N_PARSE = 2000
N_DISPLAY = 500
MAX_TEXT_LEN = 1024
MAX_SIG = 40
MAX_DIGITS = 20
OVERFLOW_AT = Fraction(2**1024 - 2**970)

GRAMMAR = re.compile(r"\A([+-]?)([0-9]+)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?\Z")


def atan_inv(x, one):
    """arctan(1/x) * one, truncating each term; error at most one unit per term."""
    total = term = one // x
    x2 = x * x
    n = 1
    sign = -1
    terms = 1
    while term:
        term //= x2
        total += sign * (term // (2 * n + 1))
        sign = -sign
        n += 1
        terms += 1
    return total, terms


def pi_interval(bits=1400):
    """(lo, hi) Fractions with lo < pi < hi, from Machin: pi = 16 atan(1/5) - 4 atan(1/239)."""
    one = 1 << bits
    a, ta = atan_inv(5, one)
    b, tb = atan_inv(239, one)
    approx = 16 * a - 4 * b
    # Each truncated term is off by < 1 unit, and the series tail is below the last term.
    err = 16 * (ta + 2) + 4 * (tb + 2)
    return Fraction(approx - err, one), Fraction(approx + err, one)


PI_LO, PI_HI = pi_interval()
_scaled_lo, _scaled_hi = PI_LO * 2**254, PI_HI * 2**254
PI_256 = round(_scaled_lo)
assert PI_256 == round(_scaled_hi), "pi * 2^254 too close to a half integer"
assert 2**255 <= PI_256 < 2**256
PI_Q = Fraction(PI_256, 2**254)

LENGTH = {
    "m": Fraction(1),
    "cm": Fraction(1, 100),
    "mm": Fraction(1, 1000),
    "um": Fraction(1, 10**6),
    "nm": Fraction(1, 10**9),
    "in": Fraction(127, 5000),
    "ft": Fraction(381, 1250),
    "thou": Fraction(127, 5000000),
}
ANGLE_PER_HALF_TURN = {"rad": None, "deg": 180, "arcmin": 10800, "arcsec": 648000, "gon": 200}
UNITS = list(LENGTH) + list(ANGLE_PER_HALF_TURN)


def factor(unit, pi=PI_Q):
    if unit in LENGTH:
        return LENGTH[unit]
    k = ANGLE_PER_HALF_TURN[unit]
    return Fraction(1) if k is None else pi / k


class Err(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def parse_decimal(text):
    """(negative, exact |value|) or Err, checks in the pinned order."""
    if len(text.encode("utf-8")) > MAX_TEXT_LEN:
        raise Err("too_many_digits")
    body = text[1:] if text[:1] in ("+", "-") else text
    if body.lower() in ("nan", "inf", "infinity") and body.isascii():
        raise Err("non_finite")
    m = GRAMMAR.match(text)
    if not m:
        raise Err("not_decimal")
    sign, ip, fp, ex = m.groups()
    fp = fp or ""
    digits = (ip + fp).lstrip("0")
    if len(digits) > MAX_SIG:
        raise Err("too_many_digits")
    e = int(ex or "0") - len(fp)
    c = int(ip + fp)
    # Far outside the double range the power of ten is not built: a non-zero c * 10^e with
    # e > 1000 exceeds 2^1024 for every factor (all >= 1e-9), and one below 10^-1000 is
    # under half the smallest subnormal for every factor (all <= 1).
    if c != 0 and e > 1000:
        raise Err("overflow")
    if c == 0 or e + len(str(c)) < -1000:
        return sign == "-", Fraction(0)
    return sign == "-", Fraction(c) * (Fraction(10) ** e)


def nearest_double(v):
    """Correctly rounded double of the Fraction v >= 0 (ties to even), or Err('overflow')."""
    if v >= OVERFLOW_AT:
        raise Err("overflow")
    f = float(v)
    # Cross-check against both neighbours.
    lo, hi = math.nextafter(f, -math.inf), math.nextafter(f, math.inf)
    d = abs(v - Fraction(f))
    for g in (lo, hi):
        if math.isinf(g) or g < 0:
            continue
        dg = abs(v - Fraction(g))
        assert d <= dg, (v, f, g)
        if d == dg:
            assert struct_bits(f) % 2 == 0, ("tie not to even", v, f)
    return f


def struct_bits(x):
    import struct

    return struct.unpack("<Q", struct.pack("<d", x))[0]


def bits_of(neg, f):
    b = struct_bits(f)
    return b | (1 << 63) if neg else b


def convert(text, unit, pi=PI_Q):
    neg, v = parse_decimal(text)
    return bits_of(neg, nearest_double(v * factor(unit, pi)))


def parse_case(name, text, unit):
    assert '"' not in text and "\\" not in text
    case = {"name": name, "text": text, "unit": unit}
    try:
        bits = convert(text, unit)
        case["bits"] = f"{bits:016x}"
        if unit in ANGLE_PER_HALF_TURN and ANGLE_PER_HALF_TURN[unit] is not None:
            assert convert(text, unit, PI_LO) == bits == convert(text, unit, PI_HI), (
                "256-bit pi changes the answer", text, unit)
    except Err as e:
        case["error"] = e.code
    return case


def dec_text(q):
    """Exact decimal text of a terminating Fraction q > 0 (plain, or `<int>e-<k>`)."""
    k = 0
    while (q * 10**k).denominator != 1:
        k += 1
        assert k < 2000, "not terminating"
    n = (q * 10**k).numerator
    return f"{n}e-{k}" if k else str(n)


def sig_digits(text):
    m = GRAMMAR.match(text)
    return len((m.group(2) + (m.group(3) or "")).lstrip("0"))


def hand_parse_cases():
    c = []
    add = lambda name, text, unit: c.append(parse_case(name, text, unit))  # noqa: E731
    # AC-2 and other landmarks.
    add("ac2_inch", "1", "in")
    add("ac2_tenth_mm", "0.1", "mm")
    add("mm_25_4", "25.4", "mm")
    add("foot", "1", "ft")
    add("thou_1000", "1000", "thou")
    add("cm_one", "1", "cm")
    add("cm_inch", "2.54", "cm")
    add("cm_seven_tenths", "0.7", "cm")
    add("cm_neg_zero", "-0", "cm")
    add("overflow_cm", "1.8e310", "cm")
    add("zero", "0", "m")
    add("neg_zero", "-0", "m")
    add("plus_zero_exp", "+0.0e5", "mm")
    add("upper_e", "1.5E3", "um")
    add("leading_zeros", "000123.4500", "mm")
    add("nm_landmark", "632.9908", "nm")
    # AC-3 error path.
    add("ac3_overflow", "1e400", "m")
    add("ac3_nan", "NaN", "m")
    add("ac3_comma", "1,5", "mm")
    add("ac3_41_digits", "1" * 41, "mm")
    add("err_inf", "-Infinity", "deg")
    add("err_empty", "", "m")
    add("err_point", ".5", "m")
    add("err_trailing_point", "5.", "m")
    add("err_space", " 5", "m")
    add("err_double_sign", "+-1", "m")
    add("err_long_text", "1" + "0" * 1100, "m")
    add("digits_40", "1234567890123456789012345678901234567890", "m")
    add("digits_40_frac", "0.000" + "9" * 40, "in")
    add("digits_40_trailing_zeros", "1." + "0" * 39, "ft")
    add("digits_41_trailing_zeros", "1." + "0" * 40, "ft")
    # Halfway in metres: odd integers between 2^53 and 2^54.
    add("half_2p53_plus_1", "9007199254740993", "m")
    add("half_2p53_plus_3", "9007199254740995", "m")
    add("half_mm", "9007199254740993000", "mm")
    add("half_exp", "9.007199254740993e15", "m")
    add("half_small", dec_text(Fraction(2**53 + 1, 2**54)), "m")
    # Subnormal and zero boundaries.
    add("min_subnormal", "4.9406564584124654e-324", "m")
    add("below_half_min_subnormal", "2.4703282292062327e-324", "m")
    add("above_half_min_subnormal", "2.4703282292062328e-324", "m")
    add("below_min_normal", "2.2250738585072011e-308", "m")
    add("min_normal", "2.2250738585072014e-308", "m")
    add("subnormal_nm", "1e-315", "nm")
    add("subnormal_arcsec", "1e-318", "arcsec")
    add("tiny_to_zero_nm", "1e-320", "nm")
    add("neg_underflow", "-1e-400", "m")
    add("huge_negative_exp", "1e-99999999999", "mm")
    # Overflow boundaries (2^1024 - 2^970 = 1.797693134862315807937289714053034150799341e308).
    add("max_double", "1.7976931348623157e308", "m")
    add("just_below_overflow", "1.797693134862315807937289714053034150799e308", "m")
    add("just_above_overflow", "1.797693134862315807937289714053034150800e308", "m")
    add("overflow_ft", "6e308", "ft")
    add("no_overflow_nm", "1e317", "nm")
    add("overflow_nm", "1.8e317", "nm")
    add("huge_exp", "1e99999999999999999999", "m")
    # Angles.
    for unit, texts in {
        "deg": ["180", "90", "1", "-45", "360", "0.5", "1e-300", "57.29577951308232"],
        "arcmin": ["0.5", "60", "10800"],
        "arcsec": ["1", "3600", "648000", "0.001"],
        "gon": ["200", "400", "100", "1"],
        "rad": ["3.141592653589793", "1", "-2.5e-7"],
    }.items():
        for t in texts:
            add(f"angle_{unit}_{t}", t, unit)
    # Halfway values for every length unit: H = o * 2^j with o odd in (2^53, 2^54), authored
    # as H / factor when that is a terminating decimal with at most 40 digits.
    for unit, f in LENGTH.items():
        found = 0
        o = 2**53 + 1
        while found < 3:
            if (o * f.denominator) % f.numerator == 0:
                for j in (-20, -3, 0, 7, 40):
                    h = Fraction(o) * Fraction(2) ** j
                    t = dec_text(h / f)
                    if sig_digits(t) <= MAX_SIG:
                        add(f"half_{unit}_{found}_{j}", t, unit)
                        # Near-halfway: nudge by one unit in the 40th significant place.
                        q = h / f
                        e = math.floor(math.log10(q)) - 39
                        nudge = Fraction(10) ** e
                        for tag, v in (("up", q + nudge), ("down", q - nudge)):
                            tt = dec_text(v)
                            if sig_digits(tt) <= MAX_SIG:
                                add(f"near_{tag}_{unit}_{found}_{j}", tt, unit)
                found += 1
            o += 2
    return c


def random_text(rng):
    nd = rng.randint(1, MAX_SIG)
    digits = str(rng.randint(1, 9)) + "".join(rng.choice("0123456789") for _ in range(nd - 1))
    point = rng.randint(0, nd)
    if point == nd:
        mant = digits
    elif point == 0:
        mant = "0." + "0" * rng.choice([0, 0, 0, 1, 5]) + digits
    else:
        mant = digits[:point] + "." + digits[point:]
    if rng.random() < 0.1:
        mant = "0" * rng.randint(1, 3) + mant
    sign = rng.choice(["", "", "", "-", "+"])
    mode = rng.randint(0, 3)
    if mode == 0:
        exp = ""
    elif mode == 1:
        exp = rng.choice("eE") + str(rng.randint(-30, 30))
    elif mode == 2:
        exp = "e" + str(rng.randint(-345, 320))
    else:
        exp = "e" + rng.choice(["+", "-", ""]) + str(rng.randint(0, 9))
    return sign + mant + exp


def display_case(name, bits, unit, digits):
    x = struct_unbits(bits)
    q = round(abs(Fraction(x)) / factor(unit) * 10**digits)  # Fraction rounds half to even
    s = str(q).rjust(digits + 1, "0")
    out = s[: len(s) - digits] + ("." + s[len(s) - digits :] if digits else "")
    if (bits >> 63) and q != 0:
        out = "-" + out
    return {"bits": f"{bits:016x}", "digits": digits, "name": name, "out": out, "unit": unit}


def struct_unbits(b):
    import struct

    return struct.unpack("<d", struct.pack("<Q", b))[0]


def hand_display_cases():
    d = []
    add = lambda name, x, unit, n: d.append(display_case(name, x if isinstance(x, int) else struct_bits(x), unit, n))  # noqa: E731
    add("ac4_inch", 0.0254, "in", 6)
    add("tie_down", 0.125, "m", 2)
    add("tie_up", 0.375, "m", 2)
    add("tie_neg", -0.125, "m", 2)
    add("neg_rounds_to_zero", -0.001, "m", 2)
    add("neg_zero", 1 << 63, "mm", 3)
    add("zero_digits0", 0.0, "m", 0)
    add("int_tie_even_down", 2.5, "m", 0)
    add("int_tie_even_up", 3.5, "m", 0)
    add("tenth_mm", 1e-4, "mm", 20)
    add("pi_deg_15", math.pi, "deg", 15)
    add("pi_deg_20", math.pi, "deg", 20)
    add("half_pi_gon", math.pi / 2, "gon", 12)
    add("rad_identity", 1.0, "rad", 20)
    add("arcsec_small", 4.84813681109536e-06, "arcsec", 10)
    add("max_nm", struct_bits(1.7976931348623157e308), "nm", 20)
    add("max_neg_thou", struct_bits(-1.7976931348623157e308), "thou", 3)
    add("min_subnormal", 1, "m", 20)
    add("min_subnormal_nm", 1, "nm", 20)
    add("metre_in_thou", 1.0, "thou", 0)
    add("inch_in_cm", 0.0254, "cm", 2)
    add("metre_in_cm", 1.0, "cm", 3)
    add("max_cm", struct_bits(1.7976931348623157e308), "cm", 20)
    add("foot_20", 0.3048, "ft", 20)
    add("um_landmark", 1.5e-6, "um", 4)
    return d


def random_double(rng):
    sign = rng.getrandbits(1)
    mode = rng.randint(0, 3)
    if mode == 0:
        be = rng.randint(1023 - 30, 1023 + 8)  # about 1e-9 .. 500
    elif mode == 1:
        be = rng.randint(1023 - 60, 1023 + 20)
    elif mode == 2:
        be = rng.randint(1, 2046)
    else:
        be = rng.choice([0, 1, 2045, 2046])
    return (sign << 63) | (be << 52) | rng.getrandbits(52)


def build():
    rng = random.Random(13)
    parse = hand_parse_cases()
    i = 0
    while len(parse) < N_PARSE:
        parse.append(parse_case(f"random_{i}", random_text(rng), rng.choice(UNITS)))
        i += 1
    display = hand_display_cases()
    i = 0
    while len(display) < N_DISPLAY:
        display.append(display_case(f"random_{i}", random_double(rng), rng.choice(UNITS), rng.randint(0, MAX_DIGITS)))
        i += 1
    names = [c["name"] for c in parse] + [c["name"] for c in display]
    assert len(set(c["name"] for c in parse)) == len(parse)
    assert len(set(c["name"] for c in display)) == len(display)
    del names
    dump = lambda c: json.dumps(c, sort_keys=True, separators=(",", ":"))  # noqa: E731
    lines = [
        "{",
        '  "generator": "tools/fixtures/geo_units_cases.py (stdlib fractions, independent of fid-geo-frame)",',
        '  "license": "CC-BY-4.0",',
        f'  "pi_256_hex": "{PI_256:064x}",',
        '  "seed": 13,',
        '  "parse": [',
        ",\n".join("    " + dump(c) for c in parse),
        "  ],",
        '  "display": [',
        ",\n".join("    " + dump(c) for c in display),
        "  ]",
        "}",
    ]
    text = "\n".join(lines) + "\n"
    json.loads(text)  # well-formed
    return text


def main():
    check = "--check" in sys.argv[1:]
    text = build()
    if check:
        if not OUT.exists() or OUT.read_bytes() != text.encode("utf-8"):
            print(f"{OUT.relative_to(ROOT)} is stale; run python tools/fixtures/geo_units_cases.py", file=sys.stderr)
            return 1
        print("geo_units_cases: up to date")
        return 0
    OUT.parent.mkdir(parents=True, exist_ok=True)
    OUT.write_bytes(text.encode("utf-8"))
    print(f"wrote {OUT.relative_to(ROOT)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
