#!/usr/bin/env python3
"""Independent reference for KRN.NUM.FMT.ANG (fmt.angle@1). CC-BY-4.0 project data.

Writes
  fixtures/num/fmt/ang-pi-v1.txt      pi to 2100 decimals (Chudnovsky series, integers only)
  fixtures/num/fmt/ang-golden-v1.txt  20 000 seeded random and boundary cases (AC-1, AC-7)
  fixtures/num/fmt/ang-ladder-v1.txt  3 000 cases under a deliberately coarse ladder
                                      (enclosures of 3, 6 and 10 decimals), which exercises
                                      moving up the ladder, `undecidable_rounding` and
                                      `undecidable_side` (the real ladder never needs them)
  fixtures/num/fmt/ang-ac-v1.txt      the named acceptance examples AC-2 .. AC-4 and AC-6

The oracle never runs the Rust code and uses only the standard library:

1. pi to 2100 + 30 decimals from the Chudnovsky series by binary splitting and
   `math.isqrt` (integers only), truncated to 2100 decimals after checking that the 30
   guard digits are clear of the truncation point; its first 50 decimals are asserted
   against the published expansion. The Rust test compares its ladder against this file
   and, separately, against pi from Machin's formula (AC-5).
2. The ladder model (the rules pinned in spec/krn/num/fmt/ang/NODE.md, Design): each rung
   is [floor(pi * 10^d), floor(pi * 10^d) + 1] / 10^d for d = 50, 200, 1000. A decision
   (a rounding, the decimal exponent, the side of a limit) is taken at the first rung,
   from the current one up, where both ends of the interval agree; the reported index is
   the last rung reached. Exact integer arithmetic throughout.
3. The "true" result: the same rules over a single rung of 2100 decimals. Every golden
   case must give the same value, digits_added, tie flag and side as the ladder model
   (the strings come from 2100-digit pi, AC-1); coarse-ladder cases must agree with it
   whenever they decide.
4. A second path for the printed digits: the `decimal` module at 2300 digits
   (x * 180 / pi, or * 648000 / pi for seconds), quantized with ROUND_HALF_EVEN, or
   ROUND_CEILING / ROUND_FLOOR for a capped side-rule output; the dms fields are split
   from the quantized seconds with integer divmod. Its string must equal path 2.
5. Side safety, asserted on every case: when the exact angle violates a limit (decided
   with 2100-digit pi), the printed string, parsed back to an exact rational in degrees
   (dms parsed field by field), lies strictly beyond the limit, never on it; when no
   limit is violated, no digit is added. The rad unit is the fmt.scalar@1 rule of
   make_len_fixtures.py (exact factor 1), whose own oracle asserts the same.

Fixture line (fields separated by one space):
  <x as 16 hex digits> <angle|other> <unit> <dN|sN> <lower|-> <upper|-> <expect>
  expect = ok:<value>:<digits_added>:<tie 0|1>:<side>:<pi enclosure index or ->
           or err:<detail>
  side   = none | within | above | below | undecidable
Limits are in radians for unit rad and in decimal degrees for deg and dms.
The last line is `# outputs-sha256: sha256:<hex>`, the SHA-256 of every `expect` field
followed by a newline, in file order (AC-7: the Rust test recomputes it on every target).

Run:  python fixtures/num/fmt/make_ang_fixtures.py          (writes the fixtures)
      python fixtures/num/fmt/make_ang_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import math
import struct
import sys
from decimal import ROUND_CEILING, ROUND_FLOOR, ROUND_HALF_EVEN, Context, Decimal
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
sys.path.insert(0, str(HERE))
import make_len_fixtures as lenfx  # noqa: E402  (fmt.scalar@1 oracle for the rad unit)

N_GOLDEN = 20_000
N_LADDER = 3_000
SEED = 20261119
MAX_DEC = 20
MAX_SIG = 17
TRUE_DIGITS = 2100
LADDER_DIGITS = (50, 200, 1000)
COARSE_DIGITS = (3, 6, 10)
PUBLISHED_50 = "3.14159265358979323846264338327950288419716939937510"
SCALE = {"deg": 180, "dms": 648000}


class Undecidable(Exception):
    pass


# ---------------------------------------------------------------------------------------------
# pi
# ---------------------------------------------------------------------------------------------


def pi_floor(n):
    """floor(pi * 10^n) by the Chudnovsky series (binary splitting, integers only)."""
    guard = 30
    digits = n + guard
    c3_24 = 640320**3 // 24

    def bs(a, b):
        if b - a == 1:
            if a == 0:
                p = q = 1
            else:
                p = (6 * a - 5) * (2 * a - 1) * (6 * a - 1)
                q = a * a * a * c3_24
            t = p * (13591409 + 545140134 * a)
            if a & 1:
                t = -t
            return p, q, t
        m = (a + b) // 2
        p1, q1, t1 = bs(a, m)
        p2, q2, t2 = bs(m, b)
        return p1 * p2, q1 * q2, q2 * t1 + p1 * t2

    terms = digits // 14 + 2
    _, q, t = bs(0, terms)
    one = 10**digits
    sqrt_c = math.isqrt(10005 * one * one)
    approx = q * 426880 * sqrt_c // t
    head, tail = divmod(approx, 10**guard)
    # The series and the square root are within a few units of the last guard digit.
    if not 1000 < tail < 10**guard - 1000:
        raise SystemExit("pi guard digits too close to the truncation point")
    return head


PI_INT = pi_floor(TRUE_DIGITS)
PI_STR = str(PI_INT)
assert len(PI_STR) == TRUE_DIGITS + 1
if "3." + PI_STR[1:51] != PUBLISHED_50:
    raise SystemExit("pi disagrees with the published 50-decimal expansion")


def rung(d):
    lo = PI_INT // 10 ** (TRUE_DIGITS - d)
    return (lo, lo + 1, d)


def rung_strings(d):
    lo, hi, _ = rung(d)
    s_lo, s_hi = str(lo), str(hi)
    return s_lo[0] + "." + s_lo[1:], s_hi[0] + "." + s_hi[1:]


LADDER = [rung(d) for d in LADDER_DIGITS]
COARSE = [rung(d) for d in COARSE_DIGITS]
TRUE = [rung(TRUE_DIGITS)]
POW10 = {}


def p10(k):
    if k not in POW10:
        POW10[k] = 10**k
    return POW10[k]


# ---------------------------------------------------------------------------------------------
# Path 2 and 3: the ladder model on exact integers
# ---------------------------------------------------------------------------------------------


def dexp_int(num, den):
    """k with 10^k <= num/den < 10^(k+1), num > 0."""
    k = (num.bit_length() - den.bit_length()) * 30103 // 100000
    while num * p10(max(-k, 0)) < den * p10(max(k, 0)):
        k -= 1
    while num * p10(max(-k - 1, 0)) >= den * p10(max(k + 1, 0)):
        k += 1
    return k


def round_mag(num, den, mode, n, rnd):
    """(q, p): the magnitude num/den rounded at the precision; rnd 'he' half even, 'up'
    away from zero, 'down' toward zero. A carry into a new decade keeps n digits."""
    k = dexp_int(num, den) if num else 0
    p = n if mode == "d" else n - 1 - k
    a, b = (num * p10(p), den) if p >= 0 else (num, den * p10(-p))
    q, r = divmod(a, b)
    if rnd == "he":
        if 2 * r > b or (2 * r == b and q & 1):
            q += 1
    elif rnd == "up":
        if r:
            q += 1
    if mode == "s" and q == p10(n):
        q, p = p10(n - 1), p - 1
    return q, p


class Model:
    """Decisions over a ladder for |x| = nx / dx and display scale c (180 or 648000)."""

    def __init__(self, nx, dx, c, rungs):
        self.nx, self.dx, self.c, self.rungs = nx, dx, c, rungs
        self.level = 0

    def ends(self, i):
        plo, phi, d = self.rungs[i]
        num = self.nx * self.c * p10(d)
        return (num, self.dx * phi), (num, self.dx * plo)

    def decide(self, f):
        for i in range(self.level, len(self.rungs)):
            lo, hi = self.ends(i)
            a, b = f(*lo), f(*hi)
            if a == b:
                self.level = i
                return a
        raise Undecidable()

    def side(self, neg, lim):
        """Sign of x - lim * pi / 180 (lim a Fraction in degrees), or None if undecided."""
        sx = 0 if self.nx == 0 else (-1 if neg else 1)
        sl = (lim > 0) - (lim < 0)
        if sx != sl or sx == 0:
            return (sx > sl) - (sx < sl)
        la = abs(lim)
        for i in range(self.level, len(self.rungs)):
            plo, phi, d = self.rungs[i]
            left = self.nx * 180 * p10(d) * la.denominator
            r_lo = la.numerator * plo * self.dx
            r_hi = la.numerator * phi * self.dx
            if left >= r_hi:
                mag = 1
            elif left <= r_lo:
                mag = -1
            else:
                continue
            self.level = i
            return mag * sx
        self.level = len(self.rungs) - 1
        return None


def out_degrees(neg, q, p, unit):
    v = Fraction(q) / Fraction(10) ** p
    if unit == "dms":
        v /= 3600
    return -v if neg else v


def render_dms(neg, q, p):
    unit = p10(p)
    d, r = divmod(q, 3600 * unit)
    m, s = divmod(r, 60 * unit)
    s_int, s_frac = divmod(s, unit)
    sec = f"{s_int:02d}" + ("." + str(s_frac).rjust(p, "0") if p > 0 else "")
    return ("-" if neg and q else "") + f"{d}:{m:02d}:{sec}"


def render(neg, q, p, unit):
    return render_dms(neg, q, p) if unit == "dms" else lenfx.render(neg, q, p)


def check_spec(formatter, unit, prec, lower, upper):
    mode, n = prec
    if formatter != "angle" or unit not in ("rad", "deg", "dms"):
        return "err:unit_not_supported"
    if mode == "d" and not 0 <= n <= MAX_DEC:
        return "err:precision_out_of_range"
    if mode == "s" and (unit == "dms" or not 1 <= n <= MAX_SIG):
        return "err:precision_out_of_range"
    if lower is not None and upper is not None and Fraction(Decimal(lower)) > Fraction(Decimal(upper)):
        return "err:limits_inverted"
    return None


def model_angle(x, unit, prec, lower, upper, rungs):
    """(value, added, tie, side, level, q, p, final_prec, rnd) or an 'err:' string."""
    mode, n = prec
    neg = math.copysign(1.0, x) < 0
    X = Fraction(abs(x))
    M = Model(X.numerator, X.denominator, SCALE[unit], rungs)
    lo = Fraction(Decimal(lower)) if lower is not None else None
    hi = Fraction(Decimal(upper)) if upper is not None else None
    try:
        q, p = M.decide(lambda a, b: round_mag(a, b, mode, n, "he"))
        outcome = "none" if lo is None and hi is None else "within"
        viol = None
        if hi is not None:
            s = M.side(neg, hi)
            if s is None:
                outcome = "undecidable"
            elif s > 0:
                viol, outcome = ("upper", hi), "above"
        if viol is None and outcome != "undecidable" and lo is not None:
            s = M.side(neg, lo)
            if s is None:
                outcome = "undecidable"
            elif s < 0:
                viol, outcome = ("lower", lo), "below"
        added, tie, rnd = 0, 0, "he"
        if viol is not None:
            which, lim = viol
            k = M.decide(lambda a, b: dexp_int(a, b) if a else 0) if mode == "d" else 0
            while True:
                o = out_degrees(neg, q, p, unit)
                if (which == "upper" and o > lim) or (which == "lower" and o < lim):
                    break
                if mode == "s" and n < MAX_SIG:
                    nxt = n + 1
                elif mode == "d" and k + 1 + n < MAX_SIG:
                    nxt = max(-k - 1, n + 1) if q == 0 else n + 1
                    nxt = min(nxt, MAX_SIG - 1 - k)
                else:
                    tie = 1
                    signed = "ceil" if which == "upper" else "floor"
                    rnd = signed
                    mag = "up" if (signed == "ceil") != neg else "down"
                    q, p = M.decide(lambda a, b: round_mag(a, b, mode, n, mag))
                    break
                added += nxt - n
                n = nxt
                q, p = M.decide(lambda a, b: round_mag(a, b, mode, n, "he"))
    except Undecidable:
        return "err:undecidable_rounding"
    return (render(neg, q, p, unit), added, tie, outcome, M.level, q, p, (mode, n), rnd)


def parse_output(value, unit):
    """The printed string back to an exact rational in degrees (or radians for rad)."""
    neg = value.startswith("-")
    body = value[1:] if neg else value
    if unit == "dms":
        d, m, s = body.split(":")
        assert len(m) == 2 and len(s.split(".")[0]) == 2 and int(m) < 60 and int(s.split(".")[0]) < 60, value
        v = Fraction(int(d)) + Fraction(int(m), 60) + Fraction(Decimal(s)) / 3600
    else:
        v = Fraction(Decimal(body))
    return -v if neg else v


DEC_CTX = Context(prec=2300, rounding=ROUND_HALF_EVEN, Emin=-999999, Emax=999999)
PI_DEC = Decimal(PI_STR[0] + "." + PI_STR[1:])


def decimal_path(x, unit, prec, rnd):
    """Second path: the decimal module, 2300 digits."""
    mode, n = prec
    v = DEC_CTX.divide(DEC_CTX.multiply(Decimal(x), Decimal(SCALE[unit])), PI_DEC)
    rounding = {"he": ROUND_HALF_EVEN, "ceil": ROUND_CEILING, "floor": ROUND_FLOOR}[rnd]
    if mode == "d":
        r = v.quantize(Decimal(1).scaleb(-n), rounding=rounding, context=DEC_CTX)
    elif v == 0:
        r = Decimal(0).quantize(Decimal(1).scaleb(-(n - 1)), context=DEC_CTX)
    else:
        r = Context(prec=n, rounding=rounding, Emin=-999999, Emax=999999).plus(v)
        r = r.quantize(Decimal(1).scaleb(r.adjusted() - n + 1), context=DEC_CTX)
    if unit == "dms":
        neg = r.is_signed()
        q = int(r.copy_abs().scaleb(n, context=DEC_CTX))
        return render_dms(neg, q, n)
    s = format(r, "f")
    if s.startswith("-") and set(s[1:]) <= set("0."):
        s = s[1:]
    return s


def rad_side(x, lower, upper):
    if lower is None and upper is None:
        return "none"
    X = Fraction(x)
    if upper is not None and X > Fraction(Decimal(upper)):
        return "above"
    if lower is not None and X < Fraction(Decimal(lower)):
        return "below"
    return "within"


def oracle(x, formatter, unit, prec, lower, upper, rungs=LADDER):
    err = check_spec(formatter, unit, prec, lower, upper)
    if err is not None:
        return err
    if not math.isfinite(x):
        return "err:non_finite"
    if unit == "rad":
        e = lenfx.oracle(x, "scalar", "1", prec, lower, upper)
        side = rad_side(x, lower, upper)
        value = e[3:].rsplit(":", 2)[0]
        safety(x, unit, value, side, lower, upper, e.endswith(":0:0"))
        return f"{e}:{side}:-"
    got = model_angle(x, unit, prec, lower, upper, rungs)
    true = model_angle(x, unit, prec, lower, upper, TRUE)
    if isinstance(true, str):
        raise SystemExit(f"2100-digit pi does not decide {lenfx.hx(x)} {unit} {prec}")
    tv, ta, tt, ts, tl, _, _, tprec, trnd = true
    if tl != 0 or ts == "undecidable":
        raise SystemExit("2100-digit pi left a side undecided")
    alt = decimal_path(x, unit, tprec, trnd)
    if alt != tv:
        raise SystemExit(f"decimal path disagrees: {lenfx.hx(x)} {unit} {tprec} {trnd}: {alt} != {tv}")
    safety(x, unit, tv, ts, lower, upper, ta == 0 and tt == 0)
    if isinstance(got, str):
        if rungs is LADDER:
            raise SystemExit(f"real ladder undecided: {lenfx.hx(x)} {unit} {prec}")
        return got
    v, a, t, s, lvl = got[:5]
    if rungs is LADDER and (v, a, t, s) != (tv, ta, tt, ts):
        raise SystemExit(f"ladder and 2100-digit pi disagree: {lenfx.hx(x)} {unit} {prec}")
    if s != "undecidable" and (v, a, t, s) != (tv, ta, tt, ts):
        raise SystemExit(f"coarse ladder decided wrongly: {lenfx.hx(x)} {unit} {prec}")
    if s == "undecidable":
        # The requested rounding, nothing added: whatever the true side.
        req = model_angle(x, unit, prec, None, None, TRUE)
        if (v, a, t) != (req[0], 0, 0):
            raise SystemExit("undecidable side must print the requested rounding")
    return f"ok:{v}:{a}:{t}:{s}:{lvl}"


def safety(x, unit, value, side, lower, upper, nothing_added):
    """Side safety (REQ-005): a violated limit is shown strictly on its side."""
    if side in ("above", "below"):
        lim = Fraction(Decimal(upper if side == "above" else lower))
        out = parse_output(value, unit)
        if not (out > lim if side == "above" else out < lim):
            raise SystemExit(f"side violated: {lenfx.hx(x)} {unit} {side} {lim} -> {value}")
        # And the exact angle really is beyond it (2100-digit pi, or exact for rad).
        if unit != "rad":
            X = Fraction(x)
            pi_lo, pi_hi = Fraction(PI_INT, p10(TRUE_DIGITS)), Fraction(PI_INT + 1, p10(TRUE_DIGITS))
            lo_deg = min(X * 180 / pi_lo, X * 180 / pi_hi)
            hi_deg = max(X * 180 / pi_lo, X * 180 / pi_hi)
            if not (lo_deg > lim if side == "above" else hi_deg < lim):
                raise SystemExit(f"side claimed but not certain: {lenfx.hx(x)} {unit} {side}")
    elif side in ("none", "within") and not nothing_added:
        raise SystemExit(f"digits added without a violated limit: {lenfx.hx(x)}")
    # Canonical form: ASCII digits, '.', ':' (dms only), optional leading '-'.
    body = value[1:] if value.startswith("-") else value
    allowed = set("0123456789.:" if unit == "dms" else "0123456789.")
    if not body or body.count(".") > 1 or not set(body) <= allowed:
        raise SystemExit(f"non-canonical output {value!r}")
    if value.startswith("-") and set(body) <= set("0.:"):
        raise SystemExit(f"minus on zero {value!r}")


# ---------------------------------------------------------------------------------------------
# Case generation
# ---------------------------------------------------------------------------------------------

PI_FR = Fraction(PI_INT, p10(TRUE_DIGITS))


def deg_value(x):
    """|x| in degrees to about 2100 digits, signed (for building limits and targets)."""
    return Fraction(x) * 180 / PI_FR


def from_deg(t):
    """The double nearest to t degrees in radians."""
    return float(t * PI_FR / 180)


def nudge(x, steps):
    for _ in range(abs(steps)):
        x = math.nextafter(x, math.inf if steps > 0 else -math.inf)
    return x


class Gen(lenfx.Gen):
    def unit(self):
        r = self.rint(0, 99)
        return "rad" if r < 20 else ("deg" if r < 62 else "dms")

    def aprec(self, unit):
        if unit != "dms" and self.chance(1, 4):
            return ("s", self.rint(1, MAX_SIG))
        n = self.rint(0, 8) if self.chance(9, 10) else self.rint(9, MAX_DEC)
        return ("d", n)

    def realistic_angle(self):
        return (self.r.random() * 2.0 - 1.0) * 7.0

    def small_angle(self):
        return (self.r.random() * 2.0 - 1.0) * 10.0 ** -self.rint(1, 12)

    def big_angle(self):
        return (self.r.random() * 2.0 - 1.0) * 10.0 ** self.rint(1, 8)

    def deg_target(self, unit, prec):
        """A typed degree value (or a dms value near a carry), then a few ulps either way."""
        if unit == "dms" or self.chance(1, 3):
            d = self.rint(0, 359)
            m = self.pick([0, 1, 30, 58, 59, self.rint(0, 59)])
            s_choices = ["59.9", "59.95", "59.995", "59.9995", "59.99995", "0", "30", "0.05"]
            s = Fraction(Decimal(self.pick(s_choices)))
            t = d + Fraction(m, 60) + s / 3600
        else:
            t = Fraction(self.rint(0, 10 ** self.rint(1, 7)), 10 ** self.rint(0, 5))
        if self.chance(1, 3):
            t = -t
        return nudge(from_deg(t), self.rint(-2, 2))

    def tie_target(self, unit, prec):
        """Exact half-way points of the requested rounding (in deg, or seconds for dms)."""
        mode, n = prec
        if mode != "d":
            return None
        q = self.rint(0, 10 ** self.rint(1, 6))
        t = (Fraction(q) + Fraction(1, 2)) / Fraction(10) ** n
        if unit == "dms":
            t /= 3600
        if self.chance(1, 3):
            t = -t
        return nudge(from_deg(t), self.rint(-1, 1))

    def pi_multiple(self):
        a = self.pick([1, 1, 1, 2, 3, 5, -1, -3, 7, 11])
        b = self.pick([1, 2, 3, 4, 6, 8, 12, 180, 360, 10800, 648000])
        return nudge(float(PI_FR * a / b), self.rint(-1, 1))

    def angle_boundary(self):
        return self.pick(
            [
                0.0,
                -0.0,
                lenfx.fb(1),
                -lenfx.fb(1),
                lenfx.fb(0x000F_FFFF_FFFF_FFFF),
                lenfx.fb(0x0010_0000_0000_0000),
                sys.float_info.max,
                -sys.float_info.max,
                1.0,
                -1.0,
                math.pi,
                -math.pi,
                2 * math.pi,
                math.pi / 2,
                math.pi / 4,
                math.pi / 6,
                math.nextafter(math.pi / 6, math.inf),
                1e-300,
                1e300,
            ]
        )

    def ang_limits(self, x, unit, prec):
        if self.chance(6, 10):
            return None, None
        if unit == "rad":
            return self.limits(x, "1", prec)
        v = deg_value(x)
        kind = self.rint(0, 4)
        if kind == 0:
            # The true rounded output itself, in degrees: the side-rule edge.
            r = model_angle(x, unit, prec, None, None, TRUE)
            if isinstance(r, str):
                return None, None
            lim = out_degrees(math.copysign(1.0, x) < 0, r[5], r[6], unit)
        elif kind == 1:
            lim = lenfx.truncate_sig(v, self.rint(18, 34))
        elif kind == 2:
            lim = lenfx.truncate_sig(v, self.rint(1, 12))
        elif kind == 3:
            lim = Fraction(Decimal(self.pick(["0", "30", "30.000", "45", "-90", "90", "180", "360", "0.5"])))
        else:
            a = lenfx.truncate_sig(v, self.rint(1, 6)) - Fraction(1, 10 ** self.rint(0, 9))
            b = lenfx.truncate_sig(v, self.rint(1, 6)) + Fraction(1, 10 ** self.rint(0, 9))
            if self.chance(1, 10):
                a, b = b, a
            return lenfx.dec_str(a), lenfx.dec_str(b)
        s = lenfx.dec_str(lim)
        if s is None:
            return None, None
        if v > lim or (v == lim and self.chance(1, 2)):
            other = lenfx.dec_str(lim - Fraction(1, 10 ** self.rint(0, 6))) if self.chance(1, 3) else None
            return other, s
        other = lenfx.dec_str(lim + Fraction(1, 10 ** self.rint(0, 6))) if self.chance(1, 3) else None
        return s, other

    def ang_case(self, units=None):
        unit = self.unit() if units is None else self.pick(units)
        prec = self.aprec(unit)
        roll = self.rint(0, 99)
        x = None
        if roll < 30:
            x = self.realistic_angle()
        elif roll < 38:
            x = self.small_angle()
        elif roll < 44:
            x = self.big_angle()
        elif roll < 64:
            x = self.deg_target(unit, prec)
        elif roll < 74:
            x = self.tie_target(unit, prec)
        elif roll < 82:
            x = self.pi_multiple()
        elif roll < 92:
            x = self.angle_boundary()
        else:
            x = self.any_finite()
        if x is None:
            x = self.realistic_angle()
        lower, upper = self.ang_limits(x, unit, prec)
        return x, "angle", unit, prec, lower, upper


def error_cases():
    nan, inf = float("nan"), math.inf
    out = []
    for x in (nan, inf, -inf, lenfx.fb(0x7FF0_0000_0000_0001), lenfx.fb(0xFFF8_0000_0000_0000)):
        for unit, prec in (("rad", ("d", 3)), ("deg", ("d", 3)), ("dms", ("d", 1)), ("deg", ("s", 5))):
            out.append((x, "angle", unit, prec, None, None))
    for unit in ("rad", "deg", "dms"):
        for prec in (("d", 21), ("d", 255), ("s", 0), ("s", 18)):
            out.append((1.0, "angle", unit, prec, None, None))
            out.append((nan, "angle", unit, prec, None, None))
    for n in (1, 5, 17):
        out.append((1.0, "angle", "dms", ("s", n), None, None))
    for unit in ("rad", "deg", "dms"):
        for lo, hi in (("1", "-1"), ("30.0001", "30"), ("1", "0")):
            out.append((0.5, "angle", unit, ("d", 3), lo, hi))
            out.append((nan, "angle", unit, ("d", 3), lo, hi))
    for unit in ("m", "mm", "1", "in"):
        out.append((1.0, "angle", unit, ("d", 3), None, None))
    out.append((1.0, "other", "deg", ("d", 3), None, None))
    # Equal limits are not inverted; x = 0 against a zero limit is on it.
    out.append((0.0, "angle", "deg", ("d", 3), "0", "0"))
    out.append((-0.0, "angle", "dms", ("d", 0), "0", "0"))
    out.append((lenfx.fb(1), "angle", "deg", ("d", 3), "0", "0"))
    out.append((-lenfx.fb(1), "angle", "dms", ("d", 2), "0", "0"))
    return out


def fmt_line(c, expect):
    x, formatter, unit, prec, lower, upper = c
    return " ".join([lenfx.hx(x), formatter, unit, f"{prec[0]}{prec[1]}", lower or "-", upper or "-", expect])


def corpus(title, cases, rungs, extra_header=()):
    lines = [
        f"# KRN.NUM.FMT.ANG {title} (CC-BY-4.0). Generated by make_ang_fixtures.py; do not edit.",
        *extra_header,
        "# x_hex formatter unit prec lower upper expect",
    ]
    h = hashlib.sha256()
    for c in cases:
        e = oracle(*c, rungs=rungs)
        lines.append(fmt_line(c, e))
        h.update((e + "\n").encode())
    lines.append(f"# outputs-sha256: sha256:{h.hexdigest()}")
    return "\n".join(lines) + "\n"


def golden():
    g = Gen(SEED)
    cases = error_cases()
    while len(cases) < N_GOLDEN:
        cases.append(g.ang_case())
    return corpus("golden cases v1", cases, LADDER)


def ladder_corpus():
    g = Gen(SEED + 1)
    cases = []
    while len(cases) < N_LADDER:
        x, f, unit, prec, lo, hi = g.ang_case(units=["deg", "dms"])
        if g.chance(1, 2) and lo is None and hi is None:
            # A limit next to x, so that the coarse ladder has sides to decide.
            v = deg_value(x)
            s = lenfx.dec_str(lenfx.truncate_sig(v, g.rint(4, 14)))
            if s is not None:
                lo, hi = (None, s) if g.chance(1, 2) else (s, None)
        cases.append((x, f, unit, prec, lo, hi))
    header = [f"# rung {lo} {hi}" for lo, hi in (rung_strings(d) for d in COARSE_DIGITS)]
    return corpus("coarse-ladder cases v1", cases, COARSE, header)


def ac_cases():
    nan = float("nan")
    # AC-3: 10 deg 59 min 59.96 s, which rounds to 60.0 s at one decimal and carries.
    carry = from_deg(10 + Fraction(59, 60) + Fraction(Decimal("59.96")) / 3600)
    # AC-4: the next double above the double nearest pi/6 (math.pi / 6 is one ulp below it).
    above_pi6 = math.nextafter(float(PI_FR / 6), math.inf)
    named = [
        ("ac2_quarter_pi_deg", (math.pi / 4, "angle", "deg", ("d", 6), None, None)),
        ("ac2_half_pi_dms", (math.pi / 2, "angle", "dms", ("d", 1), None, None)),
        ("ac3_carry", (carry, "angle", "dms", ("d", 1), None, None)),
        ("ac3_carry_neg", (-carry, "angle", "dms", ("d", 1), None, None)),
        ("ac4_side", (above_pi6, "angle", "deg", ("d", 3), None, "30.000")),
        ("ac4_side_dms", (above_pi6, "angle", "dms", ("d", 0), None, "30")),
        ("ac6_nan", (nan, "angle", "deg", ("d", 3), None, None)),
        ("ac6_inf", (math.inf, "angle", "dms", ("d", 1), None, None)),
        ("ac6_neg_inf", (-math.inf, "angle", "rad", ("d", 3), None, None)),
    ]
    lines = [
        "# KRN.NUM.FMT.ANG acceptance examples v1 (CC-BY-4.0). Generated by make_ang_fixtures.py.",
        "# name x_hex formatter unit prec lower upper expect",
    ]
    for name, c in named:
        lines.append(f"{name} {fmt_line(c, oracle(*c))}")
    return "\n".join(lines) + "\n"


def pi_file():
    lines = [
        "# pi to 2100 decimals (CC-BY-4.0; a mathematical constant). Chudnovsky series,",
        "# integers only, generated by make_ang_fixtures.py. One line: 3.<2100 digits>.",
        "3." + PI_STR[1:],
    ]
    return "\n".join(lines) + "\n"


def rust_ladder():
    """The PI_LADDER constant for crates/fid-num/src/fmt/ang.rs (printed with --rust)."""
    out = []
    for d in LADDER_DIGITS:
        lo, hi = rung_strings(d)
        out.append(f"    // {d} decimals")
        for name, s in (("lo", lo), ("hi", hi)):
            chunks = [s[i : i + 80] for i in range(0, len(s), 80)]
            body = "\n".join(f'            "{c}",' for c in chunks)
            out.append(f"        {name}: concat!(\n{body}\n        ),")
    return "\n".join(out)


def main():
    if "--rust" in sys.argv[1:]:
        print(rust_ladder())
        return
    check = "--check" in sys.argv[1:]
    outputs = {
        "ang-pi-v1.txt": pi_file(),
        "ang-ac-v1.txt": ac_cases(),
        "ang-ladder-v1.txt": ladder_corpus(),
        "ang-golden-v1.txt": golden(),
    }
    stale = []
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode():
                stale.append(name)
        else:
            path.write_bytes(text.encode())
    if stale:
        raise SystemExit(f"stale fixtures: {', '.join(stale)} (run make_ang_fixtures.py)")
    print("ok" if check else f"wrote {', '.join(outputs)}")


if __name__ == "__main__":
    main()
