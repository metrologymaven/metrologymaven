#!/usr/bin/env python3
"""Generate the CORE.BUS.UNIT fixtures (AC-1, AC-2, AC-3, AC-6).

Writes, next to this script:

* ``catalogue.json``: the quantity catalogue the tests normalise against. It is the
  ``GEO.FRAME.UNITS`` catalogue (``fixtures/core/reg/def/schema/catalogue/geo_frame_units.json``)
  plus a ``time`` kind (``h``, ``min``, ``ms``, ``s``), so integer factors and a kind beyond
  length and angle are exercised. ``Catalogue::from_json`` accepts it because it only adds a kind.
* ``literals.csv``: ``literal,unit,kind,expected`` rows. ``expected`` is the 16 lowercase hex
  digits of the IEEE 754 binary64 result, or ``error:<reason>`` with the bus error catalogue's
  ``validation`` reason.
* ``ac2_record.json``: the normalisation record of AC-2 (``10.1 mm``), RFC 8785 canonical JSON.

Expected values are computed here with exact rationals (``fractions.Fraction``) and an exact
round-half-to-even into binary64, cross-checked against CPython's correctly rounded integer true
division (``int / int``). Nothing comes from the Rust code. The error rules are stated in the
leaf's Design notes as pinned in ``crates/fid-bus/src/unit/mod.rs``; this script re-implements
them independently from the text, in the same order:

1. more than 1024 bytes: ``literal_out_of_range``;
2. not ``[+-] digits [ "." digits ] [ ("e"|"E") [+-] digits ]`` (ASCII): ``literal_syntax``;
3. more than 64 significant digits (first non-zero digit to the last digit written):
   ``literal_out_of_range``;
4. decimal exponent outside -400..=400: for a non-zero literal the exponent of its first
   significant digit (``1.23e-3`` and ``0.00123`` both have -3), for zero the written exponent:
   ``literal_out_of_range``;
5. unit symbol not 1 to 16 printable ASCII bytes, kind unknown, or unit not accepted by the kind:
   ``unit_mismatch``;
6. correctly rounded result beyond the largest finite binary64, or a non-zero literal that rounds
   to zero: ``literal_out_of_range``.

The sign of the result is the sign written (``-0`` gives ``-0.0``).

stdlib only; deterministic (own splitmix64 generator). CC-BY-4.0 project data.
Run:  python fixtures/core/bus/unit/make_literals.py [--check]
"""

import json
import re
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
GEO = ROOT / "fixtures/core/reg/def/schema/catalogue/geo_frame_units.json"

MAX_BYTES = 1024
MAX_SIG = 64
EXP_MIN, EXP_MAX = -400, 400
GRAMMAR = re.compile(r"[+-]?[0-9]+(?:\.[0-9]+)?(?:[eE][+-]?[0-9]+)?")

TIME = {"canonical": "s", "kind": "time", "units": [
    {"den": "1", "num": "3600", "pi": False, "symbol": "h"},
    {"den": "1", "num": "60", "pi": False, "symbol": "min"},
    {"den": "1000", "num": "1", "pi": False, "symbol": "ms"},
    {"den": "1", "num": "1", "pi": False, "symbol": "s"},
]}


def canonical(obj):
    """RFC 8785 for the shapes used here (strings, booleans, objects, arrays; ASCII only)."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def build_catalogue():
    geo = json.loads(GEO.read_text(encoding="utf-8"))
    kinds = geo["kinds"] + [TIME]
    kinds.sort(key=lambda k: k["kind"].encode())
    return {"format": geo["format"], "kinds": kinds, "pi_hex": geo["pi_hex"]}


CAT = build_catalogue()
PI = Fraction(int(CAT["pi_hex"], 16), 2 ** 254)
FACTORS = {}
KIND_UNITS = {}
for k in CAT["kinds"]:
    KIND_UNITS[k["kind"]] = [u["symbol"] for u in k["units"]]
    for u in k["units"]:
        f = Fraction(int(u["num"]), int(u["den"]))
        FACTORS[u["symbol"]] = f * PI if u["pi"] else f


class Refused(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


def parse(text):
    """Returns (negative, exact |value| as Fraction) or raises Refused (rules 1-4)."""
    if len(text.encode("utf-8")) > MAX_BYTES:
        raise Refused("literal_out_of_range")
    if not text.isascii() or GRAMMAR.fullmatch(text) is None:
        raise Refused("literal_syntax")
    neg = text.startswith("-")
    body = text.lstrip("+-")
    mant, _, exp = body.replace("E", "e").partition("e")
    written = int(exp) if exp else 0
    ip, _, fp = mant.partition(".")
    digits = ip + fp
    sig = digits.lstrip("0")
    if len(sig) > MAX_SIG:
        raise Refused("literal_out_of_range")
    if sig:
        lead = written + (len(ip) - 1) - (len(digits) - len(sig))
    else:
        lead = written
    if not EXP_MIN <= lead <= EXP_MAX:
        raise Refused("literal_out_of_range")
    value = Fraction(int(digits)) * Fraction(10) ** (written - len(fp))
    return neg, value


def round_binary64(q):
    """Bits of the binary64 nearest to q >= 0, ties to even; raises Refused on overflow or on a
    non-zero q that rounds to zero."""
    if q == 0:
        return 0
    n, d = q.numerator, q.denominator
    e = n.bit_length() - d.bit_length()
    if (n < d << e) if e >= 0 else (n << -e < d):
        e -= 1
    assert Fraction(2) ** e <= q < Fraction(2) ** (e + 1)
    shift = 52 - e if e >= -1022 else 1074
    scaled = q * Fraction(2) ** shift
    m = scaled.numerator // scaled.denominator
    rest = scaled - m
    if rest > Fraction(1, 2) or (rest == Fraction(1, 2) and m % 2 == 1):
        m += 1
    if e >= -1022:
        if m == 2 ** 53:
            m, e = 2 ** 52, e + 1
        if e > 1023:
            check_overflow_agrees(q)
            raise Refused("literal_out_of_range")
        bits = ((e + 1023) << 52) | (m - 2 ** 52)
    else:
        if m == 0:
            assert n / d == 0.0, q
            raise Refused("literal_out_of_range")
        bits = m
    # Independent cross-check: CPython's int true division is correctly rounded.
    try:
        ref = struct.unpack("<Q", struct.pack("<d", n / d))[0]
    except OverflowError:
        raise AssertionError("own rounding is finite where CPython overflows: %r" % q)
    assert ref == bits, (q, hex(bits), hex(ref))
    return bits


def expected(literal, unit, kind):
    try:
        neg, value = parse(literal)
        ok_symbol = 1 <= len(unit.encode("utf-8")) <= 16 and all(0x21 <= ord(c) <= 0x7E for c in unit)
        if not ok_symbol or kind not in KIND_UNITS or unit not in KIND_UNITS[kind]:
            raise Refused("unit_mismatch")
        bits = round_binary64(value * FACTORS[unit])
        return "%016x" % (bits | (1 << 63 if neg else 0))
    except Refused as r:
        return "error:" + r.reason


def check_overflow_agrees(q):
    """For a row we refuse as overflow, CPython must overflow or give the largest finite / inf."""
    n, d = q.numerator, q.denominator
    try:
        x = n / d
    except OverflowError:
        return
    raise AssertionError("CPython rounds %r to the finite %r where we refuse overflow" % (q, x))


# ------------------------------------------------------------------ deterministic generator

class Rng:
    def __init__(self, seed):
        self.s = seed & (2 ** 64 - 1)

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & (2 ** 64 - 1)
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & (2 ** 64 - 1)
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & (2 ** 64 - 1)
        return z ^ (z >> 31)

    def below(self, n):
        return self.next() % n

    def pick(self, seq):
        return seq[self.below(len(seq))]


def frac_to_decimal(q):
    """Exact decimal text of a Fraction whose denominator is 2^a 5^b (positional notation)."""
    assert q >= 0
    n, d = q.numerator, q.denominator
    k = 0
    while (10 ** k) % d != 0:
        k += 1
    n = n * (10 ** k // d)
    s = str(n)
    if k == 0:
        return s
    s = s.rjust(k + 1, "0")
    return s[:-k] + "." + s[-k:]


def sig_digits(text):
    body = text.lstrip("+-").split("e")[0].split("E")[0].replace(".", "")
    return len(body.lstrip("0"))


def shift_point(text, j):
    """text * 10^j for a positional decimal text (no sign, no exponent), as e-notation-free text."""
    ip, _, fp = text.partition(".")
    digits = ip + fp
    point = len(ip) + j
    if point <= 0:
        out = "0." + "0" * (-point) + digits
    elif point >= len(digits):
        out = digits + "0" * (point - len(digits))
    else:
        out = digits[:point] + "." + digits[point:]
    ip, _, fp = out.partition(".")
    ip = ip.lstrip("0") or "0"
    fp = fp.rstrip("0")
    return ip + ("." + fp if fp else "")


def bits_to_frac(bits):
    x = struct.unpack("<d", struct.pack("<Q", bits))[0]
    return Fraction(x)


ROWS = []


def row(literal, unit, kind):
    for field in (literal, unit, kind):
        assert "," not in field and '"' not in field and "\n" not in field and "\r" not in field
    ROWS.append((literal, unit, kind, expected(literal, unit, kind)))


def hand_rows():
    # AC-2, AC-3 and simple conversions.
    for lit, unit, kind in [
        ("10.1", "mm", "length"), ("0.1", "in", "length"), ("0.3", "in", "length"), ("1", "m", "length"), ("25.4", "mm", "length"),
        ("1", "in", "length"), ("1", "ft", "length"), ("1", "thou", "length"), ("1", "um", "length"),
        ("1", "nm", "length"), ("0.001", "um", "length"), ("12.7", "um", "length"), ("-3.5", "um", "length"),
        ("+2", "mm", "length"), ("007.50", "mm", "length"), ("1.000", "m", "length"), ("1e3", "mm", "length"),
        ("1E-3", "m", "length"), ("1.5e+2", "mm", "length"), ("0", "mm", "length"), ("-0", "mm", "length"),
        ("0.000", "in", "length"), ("-0e-400", "m", "length"), ("0e400", "m", "length"), ("+0", "deg", "angle"),
        ("1", "deg", "angle"), ("90", "deg", "angle"), ("180", "deg", "angle"), ("360", "deg", "angle"),
        ("-45", "deg", "angle"), ("0.5", "deg", "angle"), ("1", "arcmin", "angle"), ("1", "arcsec", "angle"),
        ("100", "gon", "angle"), ("400", "gon", "angle"), ("1", "rad", "angle"), ("3.14159265358979323846", "rad", "angle"),
        ("57.29577951308232", "deg", "angle"), ("1", "s", "time"), ("1", "min", "time"), ("1", "h", "time"),
        ("2.5", "h", "time"), ("1", "ms", "time"), ("0.001", "s", "time"), ("1.5", "min", "time"),
    ]:
        row(lit, unit, kind)
    # Halfway cases in metres, chosen by hand (2^53 + odd is exactly between two binary64).
    for lit in ["9007199254740993", "9007199254740995", "9007199254740997", "-9007199254740993",
                "18014398509481986", "18014398509481990", "4503599627370496.5", "4503599627370497.5",
                "0.5000000000000000555111512312578270211815834045410156250",
                "1.00000000000000011102230246251565404236316680908203125",
                "1.00000000000000033306690738754696212708950042724609375"]:
        row(lit, "m", "length")
    row("9007199254740993000", "mm", "length")
    row("9007199254740995000000", "um", "length")
    # Overflow and underflow boundaries.
    for lit, unit in [
        ("1.7976931348623157e308", "m"), ("1.7976931348623158e308", "m"), ("1.7976931348623159e308", "m"),
        ("1.79769313486231580793728971405301e308", "m"), ("1.79769313486231580793728971405302e308", "m"),
        ("1e308", "ft"), ("1e309", "mm"), ("1e310", "mm"), ("1e311", "mm"), ("1e400", "nm"), ("1e308", "in"),
        ("5e-324", "m"), ("4.9406564584124654e-324", "m"), ("2.4703282292062328e-324", "m"),
        ("2.4703282292062327e-324", "m"), ("2.47032822920623272088e-324", "m"),
        ("2.47032822920623272089e-324", "m"), ("1e-400", "m"), ("1e-321", "nm"), ("1e-315", "nm"),
        ("2.2250738585072014e-308", "m"), ("2.2250738585072011e-308", "m"), ("2.2250738585072009e-308", "m"),
        ("2.2250738585072014e-305", "mm"), ("1e-320", "m"), ("-1e-320", "um"), ("1e-330", "m"),
    ]:
        row(lit, unit, "length")
    for lit, unit in [("1e306", "deg"), ("1e307", "arcsec"), ("1e-321", "arcsec"), ("1e-324", "deg"),
                      ("1.7976931348623157e308", "rad"), ("1e305", "h")]:
        row(lit, unit, "angle" if unit != "h" else "time")
    # Significant-digit and exponent limits.
    row("1" * 64, "m", "length")
    row("1" * 65, "m", "length")
    row("0.000" + "9" * 64, "mm", "length")
    row("0.000" + "9" * 65, "mm", "length")
    row("1" + "0" * 63, "nm", "length")
    row("1" + "0" * 64, "nm", "length")
    row("00000" + "1" * 64 + ".0e-10", "m", "length")
    row("1" * 32 + "." + "1" * 32, "in", "length")
    row("1e400", "m", "length")
    row("1e401", "m", "length")
    row("1e-401", "m", "length")
    row("0.1e-399", "m", "length")
    row("10e-401", "m", "length")
    row("0.01e402", "nm", "length")
    row("0e401", "m", "length")
    row("0e-401", "m", "length")
    row("1e999999999999", "m", "length")
    row("1e-999999999999", "m", "length")
    row("0." + "0" * 398 + "1", "m", "length")
    row("0." + "0" * 1100 + "1", "m", "length")
    row("1" + "0" * 400, "nm", "length")
    # Syntax.
    for lit in ["", ".5", "1.", "1e", "1e+", "+", "-", "--1", "+-1", "1,5", " 1", "1 ", "1_000", "0x10",
                "nan", "NaN", "inf", "-Infinity", "1.2.3", "1e2.5", "e5", "1ee5", "½", "١",
                "１", "1d5", "1.5f"]:
        if "," in lit:
            continue
        row(lit, "mm", "length")
    # Unit errors.
    for lit, unit, kind in [("5", "deg", "length"), ("5", "mm", "angle"), ("5", "MM", "length"),
                            ("5", "µm", "length"), ("5", "", "length"), ("5", "m m", "length"),
                            ("5", "s", "length"), ("5", "rad", "time"), ("5", "mm", "mass"),
                            ("5", "mm", "Length"), ("5", "x" * 17, "length"), ("5", "degC", "angle"),
                            ("5", "um ", "length"), ("1.", "deg", "length")]:
        row(lit, unit, kind)


def halfway_rows(rng, count):
    """Exact midpoints between adjacent binary64 values, plus just-above and just-below."""
    made = 0
    while made < count:
        p = rng.below(107) - 6  # binary exponent of the lower neighbour, [-6, 100]
        m = (1 << 52) | rng.below(1 << 52)
        lo = Fraction(m) * Fraction(2) ** (p - 52)
        mid = lo + Fraction(2) ** (p - 53)
        text = frac_to_decimal(mid)
        unit, j = rng.pick([("m", 0), ("mm", 3), ("um", 6), ("nm", 9)])
        lit = shift_point(text, j)
        if sig_digits(lit) > 60:
            continue
        variant = rng.below(3)
        if variant == 1 or variant == 2:
            ip, _, fp = lit.partition(".")
            room = 64 - sig_digits(lit)
            if room < 2:
                continue
            tail = "0" * (room - 1)
            if variant == 1:
                lit = ip + "." + fp + tail + "1"
            else:
                # just below: subtract one unit in the last place of a longer expansion
                digits = int(ip + fp + tail + "0")
                digits -= 1
                total_frac = len(fp) + len(tail) + 1
                s = str(digits).rjust(total_frac + 1, "0")
                lit = s[:-total_frac] + "." + s[-total_frac:]
                lit = lit.lstrip("0") or "0"
                if lit.startswith("."):
                    lit = "0" + lit
            if sig_digits(lit) > 64:
                continue
        if rng.below(4) == 0:
            lit = "-" + lit
        row(lit, unit, "length")
        made += 1


def subnormal_rows(rng, count):
    """Literals whose SI value is subnormal, printed with 17 to 30 significant digits."""
    for _ in range(count):
        m = 1 + rng.below((1 << 52) - 1)
        x = Fraction(m) * Fraction(2) ** -1074
        unit, scale = rng.pick([("m", 0), ("mm", 3), ("nm", 9), ("um", 6)])
        ndig = 17 + rng.below(14)
        v = x * Fraction(10) ** scale
        # first significant digit position
        lead = 0
        while Fraction(10) ** (lead + 1) <= v:
            lead += 1
        while Fraction(10) ** lead > v:
            lead -= 1
        q = v / Fraction(10) ** (lead - ndig + 1)
        c = q.numerator // q.denominator
        # random perturbation of the last digit keeps rows away from always-truncated values
        c += rng.below(3)
        lit = "%s.%se%d" % (str(c)[0], str(c)[1:], lead + (len(str(c)) - ndig))
        if rng.below(5) == 0:
            lit = "-" + lit
        row(lit, unit, "length")


def random_rows(rng, count):
    kinds = [k["kind"] for k in CAT["kinds"]]
    for _ in range(count):
        kind = rng.pick(kinds)
        unit = rng.pick(KIND_UNITS[kind])
        r = rng.below(10)
        ndig = 1 + (rng.below(64) if r < 3 else rng.below(20))
        digits = str(1 + rng.below(9)) + "".join(str(rng.below(10)) for _ in range(ndig - 1))
        lead = rng.below(641) - 330 if r == 9 else rng.below(41) - 20
        form = rng.below(3)
        if form == 0:
            lit = digits[0] + ("." + digits[1:] if len(digits) > 1 else "") + "e" + str(lead)
        elif form == 1:
            lit = digits + "E" + ("+" if rng.below(2) else "") + str(lead - ndig + 1)
        else:
            exp = lead - ndig + 1
            if exp >= 0 and exp < 40:
                lit = digits + "0" * exp
            elif exp < 0 and -exp < 80:
                k = -exp
                s = digits.rjust(k + 1, "0")
                lit = s[:-k] + "." + s[-k:]
            else:
                lit = digits + "e" + str(exp)
        if rng.below(4) == 0:
            lit = "-" + lit
        row(lit, unit, kind)


def build():
    rng = Rng(0x0C0DE_B05_0417)
    hand_rows()
    halfway_rows(rng, 160)
    subnormal_rows(rng, 60)
    random_rows(rng, 360)
    lines = ["literal,unit,kind,expected"] + [",".join(r) for r in ROWS]
    csv = "\n".join(lines) + "\n"
    record = canonical({
        "algorithm": "core.unit.normalise@1.0.0",
        "literal": {"unit": "mm", "value": "10.1"},
        "si_hex": expected("10.1", "mm", "length"),
    })
    # AC-2 and AC-3 hold by construction: bits of the binary64 nearest to 0.0101 and 0.00254.
    assert expected("10.1", "mm", "length") == "%016x" % round_binary64(Fraction("0.0101"))
    assert expected("0.1", "in", "length") == "%016x" % round_binary64(Fraction("0.00254"))
    values = sum(1 for r in ROWS if not r[3].startswith("error:"))
    assert values >= 500, values
    return {
        "catalogue.json": canonical(CAT) + "\n",
        "literals.csv": csv,
        "ac2_record.json": record + "\n",
    }


def main():
    files = build()
    check = "--check" in sys.argv[1:]
    bad = []
    for name, text in files.items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(name)
        else:
            path.write_bytes(data)
    if bad:
        print("stale fixtures: " + ", ".join(bad) + " (run python fixtures/core/bus/unit/make_literals.py)")
        sys.exit(1)
    rows = len(ROWS)
    errs = sum(1 for r in ROWS if r[3].startswith("error:"))
    print("%s %d rows (%d values, %d refusals)" % ("ok" if check else "wrote", rows, rows - errs, errs))


if __name__ == "__main__":
    main()
