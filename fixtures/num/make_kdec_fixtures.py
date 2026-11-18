#!/usr/bin/env python3
"""Generate fixtures/num/kdec-boundary-v1.txt (KRN.NUM.KDEC AC-1, AC-3, AC-4).

Independent reference for `fid_num::kdec`: every expected value is computed here
with Python integers and `fractions.Fraction` (exact rationals), never by running
the Rust code. Correctly rounded binary64 values are computed by an exact
integer round-half-even and cross-checked against CPython's `float(str)`.
CC-BY-4.0 project data.
Run:  python fixtures/num/make_kdec_fixtures.py

Record format, one per line, fields separated by '|':
  cmp|<decimal>|<shift10>|<f64 bits hex>|<LT|EQ|GT>   ordering of x against decimal*10^shift10
  f64|<decimal>|<f64 bits hex>                       correctly rounded value of the decimal
  dec|<a>|<b>|<LT|EQ|GT>                             ordering of two decimals
  parse|<input>|ok|<neg 0/1>|<coeff>|<exp10>         normalised result, or
  parse|<input>|err|<kind>                           typed error kind
  shift|<a>|<k>|ok|<neg>|<coeff>|<exp10>  or  shift|<a>|<k>|err|<kind>
  mul|<a>|<b>|ok|<neg>|<coeff>|<exp10>    or  mul|<a>|<b>|err|<kind>
"""

import math
import random
import re
import struct
from fractions import Fraction
from pathlib import Path

OUT = Path(__file__).with_name("kdec-boundary-v1.txt")
rng = random.Random(20261117)

MAX_DIGITS = 40
EXP_LIM = 400
MAX_LEN = 1024
U128 = 1 << 128
GRAMMAR = re.compile(r"([+-]?)([0-9]+)(?:\.([0-9]+))?(?:[eE]([+-]?[0-9]+))?")


# ---------------------------------------------------------------- reference parser (spec rules)
def norm(neg, c, e):
    """Normalise (strip trailing zeros) and range-check; returns ('ok', neg, c, e) or ('err', kind)."""
    if c == 0:
        return ("ok", 0, 0, 0)
    while c % 10 == 0:
        c //= 10
        e += 1
    if not -EXP_LIM <= e <= EXP_LIM:
        return ("err", "exponent_range")
    return ("ok", neg, c, e)


def ref_parse(s):
    if len(s.encode()) > MAX_LEN:
        return ("err", "too_many_digits")
    m = GRAMMAR.fullmatch(s)
    if m is None:
        return ("err", "syntax")
    sign, ip, fp, ex = m.group(1), m.group(2), m.group(3) or "", m.group(4)
    digits = (ip + fp).lstrip("0")
    if len(digits) > MAX_DIGITS:
        return ("err", "too_many_digits")
    sig = digits.rstrip("0")  # trailing zeros go to the exponent, not the coefficient
    tz = len(digits) - len(sig)
    c = int(sig) if sig else 0
    if c >= U128:
        return ("err", "coefficient_overflow")
    e = int(ex) if ex is not None else 0
    if abs(e) > EXP_LIM:
        return ("err", "exponent_range")
    return norm(1 if sign == "-" else 0, c, e - len(fp) + tz)


def value(r):
    _, neg, c, e = r
    v = Fraction(c) * (Fraction(10) ** e)
    return -v if neg else v


def val(s):
    r = ref_parse(s)
    assert r[0] == "ok", (s, r)
    return value(r)


# ---------------------------------------------------------------- exact binary64 rounding
def bits_of(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def round_exact(q):
    """Correctly rounded (ties to even) binary64 bits of the rational q, or None on overflow."""
    neg = q < 0
    q = -q if neg else q
    sign = (1 << 63) if neg else 0
    if q == 0:
        return sign
    # find e with 2^52 <= q / 2^e < 2^53 (integer arithmetic), clamped to the subnormal exponent
    n, d = q.numerator, q.denominator

    def scaled(e):
        return (n, d << e) if e >= 0 else (n << -e, d)

    e = n.bit_length() - d.bit_length() - 53
    while True:
        a, b = scaled(e)
        if a >= b << 53:
            e += 1
        elif a < b << 52:
            e -= 1
        else:
            break
    e = max(e, -1074)
    a, b = scaled(e)
    m, rem = divmod(a, b)
    if 2 * rem > b or (2 * rem == b and m % 2 == 1):
        m += 1
    if m == 2**53:
        m //= 2
        e += 1
    if e > 971:
        return None
    if m < 2**52:
        return sign | m  # subnormal (e == -1074)
    return sign | ((e + 1075) << 52) | (m - 2**52)


def order(a, b):
    return "LT" if a < b else ("GT" if a > b else "EQ")


up = lambda x: math.nextafter(x, math.inf)
down = lambda x: math.nextafter(x, -math.inf)


def steps(x, n):
    f = up if n > 0 else down
    for _ in range(abs(n)):
        x = f(x)
    return x


# ---------------------------------------------------------------- decimal string generators
def digits(n, lead_nonzero=True):
    s = "".join(rng.choice("0123456789") for _ in range(n))
    if lead_nonzero and n:
        s = rng.choice("123456789") + s[1:]
    return s


def mm_limit():
    """A limit as authored on a drawing, in mm."""
    r = rng.random()
    if r < 0.55:  # typical nominal +/- tolerance
        s = f"{rng.randint(0, 1999)}.{digits(rng.randint(1, 6), False)}"
    elif r < 0.70:  # dyadic, so some f64 equals the limit exactly
        j = rng.randint(1, 12)
        s = str(Fraction(rng.randint(1, 4000 * 2**j), 2**j).limit_denominator())
        n, d = (int(p) for p in (s.split("/") if "/" in s else (s, "1")))
        whole, rest = divmod(n, d)
        frac = ""
        while rest:
            rest *= 10
            frac += str(rest // d)
            rest %= d
        s = str(whole) + ("." + frac if frac else "")
    elif r < 0.82:  # small tolerance limits, often negative
        s = f"{rng.choice(['-', '', '+'])}0.{digits(rng.randint(1, 3), False)}{digits(rng.randint(1, 3))}"
    else:  # wide: up to 40 significant digits and a scientific exponent
        n = rng.randint(1, MAX_DIGITS)
        mant = digits(n)
        k = rng.randint(1, n)
        s = mant[:k] + ("." + mant[k:] if k < n else "")
        s = rng.choice(["", "-"]) + s + rng.choice(["e", "E"]) + str(rng.randint(-320, 300))
    return s


def sci_string():
    n = rng.randint(1, MAX_DIGITS)
    mant = digits(n)
    s = mant[0] + ("." + mant[1:] if n > 1 else "")
    return rng.choice(["", "-"]) + s + "e" + str(rng.randint(-340, 308))


def to_dec_string(q, nd, mode):
    """A decimal string of q with nd significant digits, rounded down/up (magnitude) or exact if mode 'exact'."""
    neg = q < 0
    q = abs(q)
    e = len(str(q.numerator)) - len(str(q.denominator))
    while Fraction(10) ** e > q:
        e -= 1
    while Fraction(10) ** (e + 1) <= q:
        e += 1
    scale = Fraction(10) ** (nd - 1 - e)
    t = q * scale
    c = t.numerator // t.denominator
    if mode == "up" and Fraction(c) != t:
        c += 1
    return ("-" if neg else "") + f"{c}e{e - (nd - 1)}"


def exact_dec_string(q):
    """Exact decimal string of a dyadic rational q (terminates)."""
    neg = q < 0
    q = abs(q)
    k = 0
    while (q * 10**k).denominator != 1:
        k += 1
    c = (q * 10**k).numerator
    return ("-" if neg else "") + (f"{c}e-{k}" if k else str(c))


def random_double():
    e = rng.randint(1, 2046)
    return struct.unpack("<d", struct.pack("<Q", (e << 52) | rng.getrandbits(52)))[0]


lines = []

# ---------------------------------------------------------------- AC-1: cmp boundary cases
limits = []
while len(limits) < 2500:
    s = mm_limit()
    r = ref_parse(s)
    if r[0] != "ok" or r[2] == 0:
        continue
    if norm(r[1], r[2], r[3] - 3)[0] != "ok":
        continue
    v = value(r)
    if abs(v) >= Fraction(2) ** 1023 or abs(v) < Fraction(10) ** -320:
        continue
    limits.append(s)

n_cmp = 0
for s in limits:
    for shift in (0, -3):  # as authored in mm, and the same limit in metres
        d = val(s) * Fraction(10) ** shift
        x0 = float(d)  # CPython: correctly rounded nearest
        assert bits_of(x0) == round_exact(d)
        for x in (x0, up(x0), down(x0), steps(x0, 10), steps(x0, -10)):
            assert math.isfinite(x)
            lines.append(f"cmp|{s}|{shift}|{bits_of(x):016x}|{order(Fraction(x), d)}")
            n_cmp += 1

# Hand-picked extremes; orderings still computed exactly here.
MIN_SUB = struct.unpack("<d", struct.pack("<Q", 1))[0]
MAX = struct.unpack("<d", struct.pack("<Q", 0x7FEFFFFFFFFFFFFF))[0]
edge = [
    ("1e-400", 0, [MIN_SUB, 0.0, -0.0, -MIN_SUB]),
    ("-1e-400", 0, [MIN_SUB, 0.0, -0.0, -MIN_SUB]),
    ("1e400", 0, [MAX, -MAX, 1.0]),
    ("-1e400", 0, [MAX, -MAX, -1.0]),
    ("9999999999999999999999999999999999999e400", 0, [MAX, -MAX]),
    ("1e-400", -0, [MAX]),
    ("0", 0, [0.0, -0.0, MIN_SUB, -MIN_SUB]),
    ("-0.000", 0, [0.0, -0.0]),
    ("4.9406564584124654e-324", 0, [MIN_SUB, up(MIN_SUB), 0.0]),
    ("2.4703282292062327e-324", 0, [MIN_SUB, 0.0]),
    ("1.7976931348623157e308", 0, [MAX, down(MAX)]),
    ("1.79769313486231570814527423731704356798e308", 0, [MAX, down(MAX)]),
    ("0.5", 0, [0.5, up(0.5), down(0.5)]),
    ("0.50", 0, [0.5]),
    ("0.1", 0, [0.1, up(0.1), down(0.1)]),
    ("1", 0, [1.0, up(1.0), down(1.0)]),
    ("10.050", -3, [0.0100500004, 0.01005, up(0.01005), down(0.01005)]),
    ("0.0254", 0, [0.0254]),
    ("1e-400", 400, [1.0, up(1.0), down(1.0)]),
    ("1e400", -400, [1.0]),
    ("12345678901234567890123456789012345678e-400", 0, [MIN_SUB, 0.0]),
]
for s, shift, xs in edge:
    d = val(s) * Fraction(10) ** shift
    for x in xs:
        lines.append(f"cmp|{s}|{shift}|{bits_of(x):016x}|{order(Fraction(x), d)}")
        n_cmp += 1

# ---------------------------------------------------------------- AC-3: correctly rounded to_f64
f64_strings = list(limits)
f64_strings += [s + "e-3" if "e" not in s.lower() else None for s in limits[:1000]]
f64_strings = [s for s in f64_strings if s is not None]
while len(f64_strings) < 6000:
    s = sci_string()
    if ref_parse(s)[0] == "ok":
        f64_strings.append(s)
while len(f64_strings) < 8500:  # one unit below or above a halfway point in the last of 20-38 digits
    a = random_double()
    b = up(a)
    if not math.isfinite(b):
        continue
    mid = (Fraction(a) + Fraction(b)) / 2
    if rng.random() < 0.3:
        mid = -mid
    s = to_dec_string(mid, rng.choice([20, 30, 38]), rng.choice(["down", "up"]))
    if ref_parse(s)[0] == "ok":
        f64_strings.append(s)
while len(f64_strings) < 10000:  # exact ties: 54 significant bits
    m = (rng.getrandbits(53) | (1 << 53)) | 1
    if rng.random() < 0.5:
        q = Fraction(m * 2 ** rng.randint(0, 70))
    else:
        q = Fraction(m, 2 ** rng.randint(1, 30))
    s = exact_dec_string(q)
    if ref_parse(s)[0] == "ok":
        f64_strings.append(s)

n_f64 = 0
for s in f64_strings:
    r = ref_parse(s)
    assert r[0] == "ok", (s, r)
    b = round_exact(value(r))
    if b is None:
        continue  # overflow: covered by unit tests
    assert b == bits_of(float(s)), s  # cross-check against CPython's own parser
    lines.append(f"f64|{s}|{b:016x}")
    n_f64 += 1

# ---------------------------------------------------------------- Decimal vs Decimal
pairs = [("0.5", "0.50"), ("0.5", "5e-1"), ("-0", "0"), ("0", "-0.0e7"), ("1", "1.0000000000000000000000000000000000000"),
         ("10.050", "10.05"), ("10.050", "10.0500000000000000000000000000000000001"), ("1e400", "1e-400"),
         ("-1e400", "-1e-400"), ("123e-3", "0.123"), ("-0.1", "0.1"), ("99999999999999999999999999999999999999", "1e38")]
for _ in range(2000):
    a = rng.choice(limits)
    r = rng.random()
    if r < 0.3:
        b = mm_limit()
    elif r < 0.6:  # same value, different spelling
        ra = ref_parse(a)
        z = rng.randint(0, 3)
        b = ("-" if ra[1] else "") + f"{ra[2]}{'0' * z}e{ra[3] - z}"
    else:  # one unit apart in the last digit
        ra = ref_parse(a)
        b = ("-" if ra[1] else "") + f"{ra[2] + rng.choice([-1, 1])}e{ra[3]}"
    pairs.append((a, b))
n_dec = 0
for a, b in pairs:
    if ref_parse(a)[0] != "ok" or ref_parse(b)[0] != "ok":
        continue
    lines.append(f"dec|{a}|{b}|{order(val(a), val(b))}")
    n_dec += 1

# ---------------------------------------------------------------- AC-4 and parser rules
parse_inputs = [
    "1e999", "--1", "0x10", "", "NaN", "1" * 41, "1234567890123456789012345678901234567890.1",
    "nan", "inf", "-inf", "Infinity", "+", "-", ".", ".5", "5.", "1e", "1e+", "1.e5", "1..2", " 1", "1 ", "1_000",
    "+-1", "1e5e5", "１", "0", "-0", "+0", "000", "0.000", "-0.0e-999", "0e400", "0e401", "1e400", "1e401",
    "1e-400", "1e-401", "10e400", "0.1e-400", "1.5e-400", "1" * 40, "9" * 39, "9" * 40, "1" + "0" * 39,
    "3" + "0" * 38 + "e1", "340282366920938463463374607431768211455", "340282366920938463463374607431768211456",
    "0." + "0" * 300 + "1", "0." + "0" * 500 + "1", "1" + "0" * 400, "1" + "0" * 401, "0" * 1025, "0" * 1024 + "1",
    "10.050", "+10.050", "-10.050", "10.050e-3", "10.050E+0", "1E0005", "0.0254", "25.4e-3", "1." + "0" * 39,
    "1." + "0" * 40, "0." + "0" * 60 + "1" + "0" * 39, "1e-0", "-1E-00",
]
for _ in range(300):
    parse_inputs.append(rng.choice([mm_limit(), sci_string()]))
for s in parse_inputs:
    assert "|" not in s and "\n" not in s
    r = ref_parse(s)
    if r[0] == "ok":
        lines.append(f"parse|{s}|ok|{r[1]}|{r[2]}|{r[3]}")
    else:
        lines.append(f"parse|{s}|err|{r[1]}")


# ---------------------------------------------------------------- exact unit shifts and products
def out_norm(prefix, r):
    lines.append(prefix + ("|ok|%d|%d|%d" % r[1:] if r[0] == "ok" else f"|err|{r[1]}"))


shift_cases = [("10.050", -3), ("1", 400), ("1", 401), ("1e-400", -1), ("0", 9999), ("-25.4", -3), ("1e400", -800),
               ("5", -400), ("5", -401)]
for s in limits[:500]:
    shift_cases.append((s, rng.choice([-9, -6, -3, 3, 6])))
for s, k in shift_cases:
    r = ref_parse(s)
    out_norm(f"shift|{s}|{k}", ("ok", 0, 0, 0) if r[2] == 0 else norm(r[1], r[2], r[3] + k))

mul_cases = [("0.0254", "1"), ("0.0254", "10.050"), ("-0.0254", "0.5"), ("2", "5"), ("25", "4e-3"), ("0", "-1e400"),
             ("1e200", "1e200"), ("1e200", "1e201"), ("1e-200", "1e-201"), ("18446744073709551616", "18446744073709551616"),
             ("18446744073709551617", "18446744073709551617"), ("-3", "-7"), ("1.5", "4"),
             ("20000000000000000000000000000000000001", "17")]
for s in limits[:500]:
    mul_cases.append((s, rng.choice(["0.0254", "25.4", "1e-3", "0.3048", "-1", "1e-6"])))
for a, b in mul_cases:
    ra, rb = ref_parse(a), ref_parse(b)
    if ra[2] == 0 or rb[2] == 0:
        out_norm(f"mul|{a}|{b}", ("ok", 0, 0, 0))
    elif ra[2] * rb[2] >= U128:
        out_norm(f"mul|{a}|{b}", ("err", "coefficient_overflow"))
    else:
        out_norm(f"mul|{a}|{b}", norm(ra[1] ^ rb[1], ra[2] * rb[2], ra[3] + rb[3]))

header = [
    "# KRN.NUM.KDEC fixture v1. Generated by make_kdec_fixtures.py (exact rationals). CC-BY-4.0 project data.",
    f"# cmp records: {n_cmp}; f64 records: {n_f64}; dec records: {n_dec}.",
]
OUT.write_text("\n".join(header + lines) + "\n", encoding="utf-8", newline="\n")
print(f"wrote {OUT.name}: cmp={n_cmp} f64={n_f64} dec={n_dec} parse={len(parse_inputs)} "
      f"shift={len(shift_cases)} mul={len(mul_cases)}")
