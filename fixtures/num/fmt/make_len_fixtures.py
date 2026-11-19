#!/usr/bin/env python3
"""Independent reference for KRN.NUM.FMT.LEN (fmt.length@1, fmt.scalar@1). CC-BY-4.0 project data.

Writes
  fixtures/num/fmt/len-golden-v1.txt  50 000 seeded random and boundary cases (AC-1, AC-7)
  fixtures/num/fmt/len-ac-v1.txt      the named acceptance examples AC-2 .. AC-6 and AC-8

The oracle never runs the Rust code and uses only the standard library:

1. Exact rational arithmetic with `fractions.Fraction`: `Fraction(x)` is the exact dyadic
   value of the double, the display value is `Fraction(x) * scale(unit)` with
   scale(m) = 1, scale(mm) = 1000, scale(um) = 10^6, scale(in) = 10000/254 and scale(1) = 1,
   and `round(Fraction)` rounds half to even exactly once.
2. A second, separate path for every unit except the inch: the `decimal` module (exact
   `Decimal(x)`, `scaleb`, then `quantize` or a context of `n` digits with
   ROUND_HALF_EVEN) printed with `format(d, "f")`. Its string must equal path 1 for every
   case, or the script fails before writing anything.
3. A third, integer-only path for inches: `Decimal(x)` as c * 10^e, times 10^4, exact
   division by 254, and the rounding decided on the integer remainder. Its string must
   equal path 1 for every inch case.
4. At the side-rule cap the oracle rounds toward the violated side (signed ceiling for an
   upper limit, floor for a lower one), and it asserts for every case with a violated limit
   that the printed decimal, converted back exactly, lies strictly beyond that limit.
5. A round-trip assertion: for unit m and unit 1 with 17 significant digits,
   `float(output) == x` bit for bit (except -0.0, whose sign is not printed by rule).

The rules modelled are the ones pinned in spec/krn/num/fmt/len/NODE.md, section Design.

Fixture line (fields separated by one space):
  <x as 16 hex digits> <length|scalar|other> <unit> <dN|sN> <lower|-> <upper|-> <expect>
  expect = ok:<value>:<digits_added>:<tie 0|1>   or   err:<detail>
The last line is `# outputs-sha256: sha256:<hex>`, the SHA-256 of every `expect` field
followed by a newline, in file order (AC-7: the Rust test recomputes it on every target).

Run:  python fixtures/num/fmt/make_len_fixtures.py          (writes both fixtures)
      python fixtures/num/fmt/make_len_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import math
import random
import struct
import sys
from decimal import ROUND_CEILING, ROUND_FLOOR, ROUND_HALF_EVEN, Context, Decimal
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
N_GOLDEN = 50_000
SEED = 20261118
MAX_DEC = 20
MAX_SIG = 17
SCALE = {
    "m": Fraction(1),
    "mm": Fraction(1000),
    "um": Fraction(10**6),
    "in": Fraction(10000, 254),
    "1": Fraction(1),
}
DEC_SHIFT = {"m": 0, "mm": 3, "um": 6, "1": 0}
LENGTH_UNITS = ["m", "mm", "um", "in"]
BIG_CTX = Context(prec=5000, rounding=ROUND_HALF_EVEN, Emin=-999999, Emax=999999)


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def fb(u):
    return struct.unpack("<d", struct.pack("<Q", u))[0]


def hx(x):
    return f"{bits(x):016x}"


# ---------------------------------------------------------------------------------------------
# Path 1: exact rationals
# ---------------------------------------------------------------------------------------------


def dexp(a):
    """k with 10^k <= a < 10^(k+1), for a positive Fraction."""
    k = int((a.numerator.bit_length() - a.denominator.bit_length()) * 0.30103)
    while Fraction(10) ** k > a:
        k -= 1
    while Fraction(10) ** (k + 1) <= a:
        k += 1
    return k


def value_exp(v):
    return 0 if v == 0 else dexp(abs(v))


def round_at(v, mode, n, k, rnd="he"):
    """(q, p): q = v * 10^p rounded to an integer, the output is q * 10^-p.

    rnd: "he" round half to even; "ceil" toward +infinity; "floor" toward -infinity
    (signed, unlike the Rust code, which rounds the magnitude up or down)."""
    p = n if mode == "d" else n - 1 - k
    t = v * Fraction(10) ** p
    q = {"he": round, "ceil": math.ceil, "floor": math.floor}[rnd](t)
    if mode == "s" and abs(q) == 10**n:
        q = q // 10
        p -= 1
    return q, p


def render(neg, q, p):
    digits = str(abs(q))
    sign = "-" if (neg and q != 0) else ""
    if p <= 0:
        return sign + digits + ("0" * (-p) if q != 0 else "")
    digits = digits.rjust(p + 1, "0")
    return sign + digits[:-p] + "." + digits[-p:]


def at_cap(mode, n, k):
    return n >= MAX_SIG if mode == "s" else k + 1 + n >= MAX_SIG


def oracle(x, formatter, unit, prec, lower, upper):
    """Returns the expect field. lower/upper are decimal strings or None."""
    mode, n = prec
    if formatter == "length":
        if unit not in LENGTH_UNITS:
            return "err:unit_not_supported"
    elif formatter == "scalar":
        if unit != "1":
            return "err:unit_not_supported"
    else:
        return "err:unit_not_supported"
    if (mode == "d" and not 0 <= n <= MAX_DEC) or (mode == "s" and not 1 <= n <= MAX_SIG):
        return "err:precision_out_of_range"
    lo = Fraction(Decimal(lower)) if lower is not None else None
    hi = Fraction(Decimal(upper)) if upper is not None else None
    if lo is not None and hi is not None and lo > hi:
        return "err:limits_inverted"
    if not math.isfinite(x):
        return "err:non_finite"
    X = Fraction(x)
    neg = math.copysign(1.0, x) < 0
    v = X * SCALE[unit]
    k = value_exp(v)
    q, p = round_at(v, mode, n, k)
    side = None
    if hi is not None and X > hi:
        side = ("upper", hi)
    elif lo is not None and X < lo:
        side = ("lower", lo)
    added = 0
    tie = 0
    if side is not None:
        which, lim = side
        while True:
            out_m = Fraction(q, 1) / Fraction(10) ** p / SCALE[unit]
            if (which == "upper" and out_m > lim) or (which == "lower" and out_m < lim):
                break
            if at_cap(mode, n, k):
                # REQ-005: at the cap, round toward the violated side instead.
                tie = 1
                q, p = round_at(v, mode, n, k, "ceil" if which == "upper" else "floor")
                break
            n += 1
            added += 1
            q, p = round_at(v, mode, n, k)
    if side is not None:
        # The printed decimal is never on the wrong side of a violated limit: it is beyond
        # it or (never, under this rule) equal to it.
        which, lim = side
        out_m = Fraction(q, 1) / Fraction(10) ** p / SCALE[unit]
        ok = out_m >= lim if which == "upper" else out_m <= lim
        if not ok or out_m == lim:
            raise SystemExit(f"side violated: {hx(x)} {unit} {mode}{n} {which} {lim} -> {render(neg, q, p)}")
    return f"ok:{render(neg, q, p)}:{added}:{tie}"


# ---------------------------------------------------------------------------------------------
# Path 2: the decimal module (not for inches, whose factor is not a finite decimal)
# ---------------------------------------------------------------------------------------------


def decimal_path(x, unit, mode, n, rounding=ROUND_HALF_EVEN):
    d = BIG_CTX.scaleb(Decimal(x), DEC_SHIFT[unit])
    if mode == "d":
        r = d.quantize(Decimal(1).scaleb(-n), rounding=rounding, context=BIG_CTX)
    elif d == 0:
        r = Decimal(0).quantize(Decimal(1).scaleb(-(n - 1)), context=BIG_CTX)
    else:
        r = Context(prec=n, rounding=rounding, Emin=-999999, Emax=999999).plus(d)
        # plus() does not pad an exact short value with zeros; quantize (exact here) does.
        r = r.quantize(Decimal(1).scaleb(r.adjusted() - n + 1), context=BIG_CTX)
    s = format(r, "f")
    if s.startswith("-") and set(s[1:]) <= set("0."):
        s = s[1:]
    return s


# ---------------------------------------------------------------------------------------------
# Path 3: integers only, for inches (Decimal(x) * 10^4, exact division by 254)
# ---------------------------------------------------------------------------------------------


def inch_path(x, mode, n, rounding):
    """x as an exact decimal c * 10^e, v = c * 10^(e+4) / 254, then integer division with
    the rounding decided on the remainder. Shares no code with paths 1 and 2."""
    sign, digs, e = Decimal(x).as_tuple()
    c = int("".join(map(str, digs)) or "0")
    num, den = c, 254
    if e + 4 >= 0:
        num *= 10 ** (e + 4)
    else:
        den *= 10 ** (-(e + 4))
    if mode == "d":
        p = n
    elif num == 0:
        p = n - 1
    else:
        s = max(0, len(str(den)) - len(str(num)) + 2)
        k = len(str(num * 10**s // den)) - 1 - s
        p = n - 1 - k

    def div(p):
        a, b = (num * 10**p, den) if p >= 0 else (num, den * 10 ** (-p))
        q, r = divmod(a, b)
        up_mag = (rounding == "ceil" and not sign) or (rounding == "floor" and sign)
        if rounding == "he":
            if 2 * r > b or (2 * r == b and q % 2 == 1):
                q += 1
        elif up_mag and r:
            q += 1
        return q

    q = div(p)
    if mode == "s" and q == 10**n:
        q, p = 10 ** (n - 1), p - 1
    digits = str(q)
    out = digits + "0" * (-p) if p <= 0 and q else digits
    if p > 0:
        digits = digits.rjust(p + 1, "0")
        out = digits[:-p] + "." + digits[-p:]
    return ("-" if sign and q else "") + out


def cross_check(x, formatter, unit, prec, lower, upper, expect):
    if not expect.startswith("ok:"):
        return
    value, added, tie = expect[3:].rsplit(":", 2)
    mode, n = prec
    n += int(added)
    rnd = "he"
    if tie == "1":
        X = Fraction(x)
        rnd = "ceil" if upper is not None and X > Fraction(Decimal(upper)) else "floor"
    if unit == "in":
        alt = inch_path(x, mode, n, rnd)
        if alt != value:
            raise SystemExit(f"inch path disagrees: {hx(x)} {mode}{n} {rnd}: {alt} != {value}")
    else:
        dec_rnd = {"he": ROUND_HALF_EVEN, "ceil": ROUND_CEILING, "floor": ROUND_FLOOR}[rnd]
        alt = decimal_path(x, unit, mode, n, dec_rnd)
        if alt != value:
            raise SystemExit(f"decimal path disagrees: {hx(x)} {unit} {mode}{n}: {alt} != {value}")
    if unit in ("m", "1") and mode == "s" and n == MAX_SIG and x != 0:
        if bits(float(value)) != bits(x):
            raise SystemExit(f"round trip failed: {hx(x)} -> {value}")
    # Canonical form (REQ-008): ASCII digits, at most one '.', optional leading '-'.
    body = value[1:] if value.startswith("-") else value
    if not body or body.count(".") > 1 or not set(body) <= set("0123456789."):
        raise SystemExit(f"non-canonical output {value!r}")
    if value.startswith("-") and set(body) <= set("0."):
        raise SystemExit(f"minus on zero {value!r}")


# ---------------------------------------------------------------------------------------------
# Case generation
# ---------------------------------------------------------------------------------------------


def dec_str(fr):
    """A Fraction with a power-of-ten-only denominator as canonical kdec text, or None
    if kdec cannot hold it (more than 38 coefficient digits or exponent out of range)."""
    if fr == 0:
        return "0"
    neg = fr < 0
    a = abs(fr)
    e = 0
    num, den = a.numerator, a.denominator
    while den != 1:
        num *= 10
        e -= 1
        g = math.gcd(num, den)
        num //= g
        den //= g
        if e < -800:
            return None
    while num % 10 == 0:
        num //= 10
        e += 1
    if len(str(num)) > 38 or num >= 2**128 or not -400 <= e <= 400:
        return None
    return f"{'-' if neg else ''}{num}e{e}"


def truncate_sig(fr, sig):
    """|fr| truncated toward zero to `sig` significant decimal digits, as a Fraction."""
    if fr == 0:
        return Fraction(0)
    a = abs(fr)
    k = dexp(a)
    p = sig - 1 - k
    t = Fraction(math.floor(a * Fraction(10) ** p)) / Fraction(10) ** p
    return t if fr > 0 else -t


class Gen:
    def __init__(self, seed):
        self.r = random.Random(seed)

    def rint(self, a, b):
        return a + self.r.getrandbits(32) % (b - a + 1)

    def pick(self, xs):
        return xs[self.rint(0, len(xs) - 1)]

    def chance(self, num, den):
        return self.rint(0, den - 1) < num

    def prec(self):
        if self.chance(1, 2):
            n = self.rint(0, 8) if self.chance(9, 10) else self.rint(9, MAX_DEC)
            return ("d", n)
        return ("s", self.rint(1, MAX_SIG))

    def realistic(self):
        mant = self.r.random() * 2.0 - 1.0
        return mant * 10.0 ** self.rint(-9, 1)

    def any_finite(self):
        while True:
            x = fb(self.r.getrandbits(64))
            if math.isfinite(x):
                return x

    def decimal_like(self):
        # Values typed as decimals (e.g. 12.345 mm) and their neighbours.
        c = self.rint(0, 10**self.rint(1, 7))
        x = float(Fraction(c, 10 ** self.rint(0, 9)))
        if self.chance(1, 2):
            x = -x
        step = self.rint(-2, 2)
        for _ in range(abs(step)):
            x = math.nextafter(x, math.inf if step > 0 else -math.inf)
        return x

    def tie(self, unit, prec):
        # Exact dyadic ties in the display unit for decimals p (see the module docstring).
        mode, p = prec
        if mode != "d" or p > 12:
            return None
        odd = 2 * self.rint(0, 5000) + 1
        if unit in ("m", "1"):
            fr = Fraction(odd, 2 ** (p + 1))
        elif unit == "mm":
            fr = Fraction(odd, 2 ** (p + 4))
        elif unit == "um":
            fr = Fraction(odd, 2 ** (p + 7))
        else:
            fr = Fraction(odd * 127, 2 ** (p + 4))
        x = float(fr)
        if Fraction(x) != fr:
            return None
        return -x if self.chance(1, 3) else x

    def near_pow10(self, unit):
        # Values that round across a power of ten (carry in significant-digit mode).
        k = self.rint(-8, 4)
        v = Fraction(10) ** k * (1 - Fraction(1, 10 ** self.rint(2, 18)))
        x = float(v / SCALE[unit])
        return -x if self.chance(1, 3) else x

    def boundary(self):
        return self.pick(
            [
                0.0,
                -0.0,
                fb(1),
                fb(0x000F_FFFF_FFFF_FFFF),
                fb(0x0010_0000_0000_0000),
                sys.float_info.max,
                -sys.float_info.max,
                -fb(1),
                1.0,
                -1.0,
                0.1,
                0.0254,
                1e-12,
                -1e-12,
                5e-5,
                math.nextafter(5e-5, math.inf),
                math.nextafter(5e-5, -math.inf),
                2.5,
                0.125,
                1e22,
                1e23,
                2.0**53,
                2.0**-1022,
            ]
        )

    def limits(self, x, unit, prec):
        """(lower, upper) as kdec strings or None."""
        if self.chance(6, 10):
            return None, None
        X = Fraction(x)
        kind = self.rint(0, 4)
        if kind == 0:
            # The rounded output itself, converted back to metres: the side-rule edge.
            v = X * SCALE[unit]
            q, p = round_at(v, prec[0], prec[1], value_exp(v))
            lim = Fraction(q) / Fraction(10) ** p / SCALE[unit]
        elif kind == 1:
            # A deep limit: x truncated to 18..34 significant digits (often a tie at 17).
            lim = truncate_sig(X, self.rint(18, 34))
        elif kind == 2:
            lim = truncate_sig(X, self.rint(1, 12))
        elif kind == 3:
            lim = Fraction(Decimal(self.pick(["0", "5e-5", "-5e-5", "1e-3", "0.0000505", "2.54e-2"])))
        else:
            # A pair around x, sometimes inverted.
            a = truncate_sig(X, self.rint(1, 6)) - Fraction(1, 10 ** self.rint(0, 9))
            b = truncate_sig(X, self.rint(1, 6)) + Fraction(1, 10 ** self.rint(0, 9))
            if self.chance(1, 10):
                a, b = b, a
            sa, sb = dec_str(a), dec_str(b)
            return sa, sb
        s = dec_str(lim)
        if s is None:
            return None, None
        if X > lim or (X == lim and self.chance(1, 2)):
            other = dec_str(lim - Fraction(1, 10 ** self.rint(0, 6))) if self.chance(1, 3) else None
            return other, s
        other = dec_str(lim + Fraction(1, 10 ** self.rint(0, 6))) if self.chance(1, 3) else None
        return s, other

    def case(self):
        formatter = "scalar" if self.chance(1, 6) else "length"
        unit = "1" if formatter == "scalar" else self.pick(LENGTH_UNITS)
        prec = self.prec()
        roll = self.rint(0, 99)
        x = None
        if roll < 45:
            x = self.realistic()
        elif roll < 65:
            x = self.decimal_like()
        elif roll < 75:
            x = self.tie(unit, prec)
        elif roll < 83:
            x = self.near_pow10(unit)
        elif roll < 93:
            x = self.boundary()
        else:
            x = self.any_finite()
        if x is None:
            x = self.realistic()
        lower, upper = self.limits(x, unit, prec)
        return x, formatter, unit, prec, lower, upper


def error_cases():
    nan = float("nan")
    inf = math.inf
    out = []
    for x in (nan, inf, -inf, fb(0x7FF0_0000_0000_0001), fb(0xFFF8_0000_0000_0000)):
        out.append((x, "length", "mm", ("d", 3), None, None))
        out.append((x, "scalar", "1", ("s", 5), None, None))
    for prec in (("d", 21), ("d", 25), ("d", 255), ("s", 0), ("s", 18), ("s", 255)):
        out.append((1.0, "length", "mm", prec, None, None))
        out.append((nan, "length", "mm", prec, None, None))
    for lo, hi in (("1e-3", "-1e-3"), ("5.1e-5", "5e-5"), ("1", "0")):
        out.append((1e-3, "length", "mm", ("d", 3), lo, hi))
        out.append((nan, "length", "mm", ("d", 3), lo, hi))
    for unit in ("1",):
        out.append((1.0, "length", unit, ("d", 3), None, None))
    for unit in ("m", "mm", "um", "in"):
        out.append((1.0, "scalar", unit, ("d", 3), None, None))
    out.append((1.0, "other", "m", ("d", 3), None, None))
    # Equal limits are not inverted.
    out.append((5e-5, "length", "mm", ("d", 3), "5e-5", "5e-5"))
    out.append((math.nextafter(5e-5, math.inf), "length", "mm", ("d", 3), "5e-5", "5e-5"))
    return out


def fmt_line(c, expect):
    x, formatter, unit, prec, lower, upper = c
    return " ".join(
        [hx(x), formatter, unit, f"{prec[0]}{prec[1]}", lower or "-", upper or "-", expect]
    )


def golden():
    g = Gen(SEED)
    cases = error_cases()
    while len(cases) < N_GOLDEN:
        cases.append(g.case())
    lines = [
        "# KRN.NUM.FMT.LEN golden cases v1 (CC-BY-4.0). Generated by make_len_fixtures.py; do not edit.",
        "# x_hex formatter unit prec lower upper expect",
    ]
    h = hashlib.sha256()
    for c in cases:
        e = oracle(*c)
        cross_check(*c, e)
        lines.append(fmt_line(c, e))
        h.update((e + "\n").encode())
    lines.append(f"# outputs-sha256: sha256:{h.hexdigest()}")
    return "\n".join(lines) + "\n"


def ac_cases():
    nan = float("nan")
    x_ac2 = float(Fraction("0.0254") * Fraction("1.0005"))
    named = [
        ("ac2_inch", (x_ac2, "length", "in", ("d", 3), None, None)),
        ("ac2_mm", (0.01, "length", "mm", ("d", 3), None, None)),
        ("ac3_side", (math.nextafter(5e-5, math.inf), "length", "mm", ("d", 3), None, "0.00005")),
        ("ac4_wrong_side", (5.06e-5, "length", "mm", ("d", 2), None, "0.0000505")),
        ("ac5_conforming", (4.99996e-5, "length", "mm", ("d", 3), None, "0.00005")),
        ("ac6_nan", (nan, "length", "mm", ("d", 3), None, None)),
        ("ac6_inf", (math.inf, "length", "mm", ("d", 3), None, None)),
        ("ac6_dec25", (1e-3, "length", "mm", ("d", 25), None, None)),
        ("ac6_sig18", (1e-3, "length", "mm", ("s", 18), None, None)),
        ("ac6_inverted", (1e-3, "length", "mm", ("d", 3), "0.002", "0.001")),
        ("ac8_no_minus", (-1e-12, "length", "mm", ("d", 3), None, None)),
        ("ac8_lower_zero", (-1e-12, "length", "mm", ("d", 3), "0", None)),
        # Review of #200: half-even at 17 digits lands below the upper limit; the capped
        # output is rounded up instead (REQ-005, AC-9).
        ("ac9_directed_at_cap", (0.506108, "length", "in", ("s", 15), None, "0.506108")),
        ("ac9_directed_lower", (-0.506108, "length", "in", ("s", 15), "-0.506108", None)),
    ]
    lines = [
        "# KRN.NUM.FMT.LEN acceptance examples v1 (CC-BY-4.0). Generated by make_len_fixtures.py.",
        "# name x_hex formatter unit prec lower upper expect",
    ]
    for name, c in named:
        e = oracle(*c)
        cross_check(*c, e)
        lines.append(f"{name} {fmt_line(c, e)}")
    return "\n".join(lines) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    outputs = {"len-golden-v1.txt": golden(), "len-ac-v1.txt": ac_cases()}
    stale = []
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode():
                stale.append(name)
        else:
            path.write_bytes(text.encode())
    if stale:
        raise SystemExit(f"stale fixtures: {', '.join(stale)} (run make_len_fixtures.py)")
    print("ok" if check else f"wrote {', '.join(outputs)}")


if __name__ == "__main__":
    main()
