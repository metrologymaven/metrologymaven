#!/usr/bin/env python3
"""Independent reference for KRN.NUM.FMT.REG (formatter registry, fmt.hexbits@1, version guard).
CC-BY-4.0 project data.

Writes
  fixtures/num/fmt/hexbits-golden-v1.txt  the frozen golden corpus of fmt.hexbits@1
  fixtures/num/fmt/reg-digests-v1.txt     one corpus digest per registered formatter

The oracle never runs the Rust code and uses only the standard library.

# fmt.hexbits@1 (pinned in crates/fid-num/src/fmt/registry.rs)

`0x` followed by the 16 lowercase hex digits of the IEEE-754 bits of x (the CORE.JRN.CANON
bit-significant f64 encoding, REQ-CORE.JRN.CANON-003). Two separate paths compute the bits
and must agree for every case, or the script fails before writing anything:
1. `struct.pack(">d", x)`;
2. arithmetic only: sign from `math.copysign`, exponent and mantissa from `math.frexp`,
   assembled into the 64-bit pattern by hand (normal and subnormal cases).
Through `fmt::format` the formatter accepts units m, 1 and rad (the SI input units of the
value formatters; the conversion is the identity, factor `1`), only `Decimals(0)`, and
optional limits; the side is decided exactly with `fractions.Fraction`: above when
x > upper, else below when x < lower, else within (none without a limit value). Errors are
checked in the order unit_not_supported, precision_out_of_range, limits_inverted,
non_finite, as for the other formatters.

Corpus line (fields separated by one space, like the LEN and ANG corpora):
  <x as 16 hex digits> hexbits <unit> <dN|sN> <lower|-> <upper|-> <expect>
  expect = ok:<0x...>:<side>   or   err:<detail>

# Version guard lines and corpus digests

For each registered formatter `fmt.<kind>@<major>`, its corpus is every line of its golden
file whose formatter column equals `<kind>`, in file order (fmt.length@1 and fmt.scalar@1:
len-golden-v1.txt; fmt.angle@1: ang-golden-v1.txt; fmt.hexbits@1: hexbits-golden-v1.txt).
Each case gives one guard line, which serialises the whole `Reported` record or the error:
  ok <value> <from> <to> <factor> <pi_enclosure|-> <digits_added> <tie 0|1> <side>
  err <op> <detail>
`op` is the formatter ID (for example `fmt.length@1`). The values, digits, ties, sides and
enclosure indices come from the `expect` fields the LEN and ANG oracles wrote (and, for
LEN, which does not record the side, from an exact `Fraction` comparison of x with the
limits); the conversion fields come from the fixed tables below. The corpus digest is
`sha256:` plus the hex SHA-256 of all guard lines, each followed by `\\n`.

reg-digests-v1.txt line:  <formatter id> <corpus path> <cases> <sha256:digest>

Run:  python fixtures/num/fmt/make_reg_fixtures.py          (writes both fixtures)
      python fixtures/num/fmt/make_reg_fixtures.py --check  (fails if a fixture is stale)
"""

import hashlib
import math
import random
import struct
import sys
from decimal import Decimal
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
CORPUS_DIR = "fixtures/num/fmt/"
SEED = 20261119
N_RANDOM = 3000

# (formatter id, kind, corpus file); sorted by id.
REGISTRY = [
    ("fmt.angle@1", "angle", "ang-golden-v1.txt"),
    ("fmt.hexbits@1", "hexbits", "hexbits-golden-v1.txt"),
    ("fmt.length@1", "length", "len-golden-v1.txt"),
    ("fmt.scalar@1", "scalar", "len-golden-v1.txt"),
]
LEN_FACTOR = {"m": "1", "mm": "1000", "um": "1000000", "in": "1/0.0254", "1": "1"}
ANG_FACTOR = {"rad": "1", "deg": "180/pi", "dms": "180/pi"}
HEX_UNITS = ["m", "1", "rad"]
ALL_UNITS = ["m", "mm", "um", "in", "1", "rad", "deg", "dms"]


# ---------------------------------------------------------------- hex bits, two paths


def bits_pack(x):
    return struct.unpack(">Q", struct.pack(">d", x))[0]


def bits_arith(x):
    """IEEE-754 binary64 bits of a finite x without struct."""
    sign = 1 if math.copysign(1.0, x) < 0 else 0
    a = abs(x)
    if a == 0:
        return sign << 63
    m, e = math.frexp(a)  # a = m * 2^e, 0.5 <= m < 1
    # Normal: a = (1 + f / 2^52) * 2^(E - 1023) with E = e - 1 + 1023.
    biased = e - 1 + 1023
    if biased >= 1:
        frac = Fraction(m) * 2 * 2**52 - 2**52
        assert frac.denominator == 1 and 0 <= frac < 2**52
        return (sign << 63) | (biased << 52) | int(frac)
    # Subnormal: a = f * 2^-1074.
    frac = Fraction(a) * 2**1074
    assert frac.denominator == 1 and 0 < frac < 2**52
    return (sign << 63) | int(frac)


def hexbits(x):
    b1 = bits_pack(x)
    b2 = bits_arith(x)
    assert b1 == b2, (x, hex(b1), hex(b2))
    s = f"0x{b1:016x}"
    assert len(s) == 18 and s == s.lower()
    return s


def from_hex(h):
    return struct.unpack(">d", struct.pack(">Q", int(h, 16)))[0]


# ---------------------------------------------------------------- hexbits corpus


def side(x, lower, upper):
    if lower is None and upper is None:
        return "none"
    X = Fraction(x)
    if upper is not None and X > Fraction(Decimal(upper)):
        return "above"
    if lower is not None and X < Fraction(Decimal(lower)):
        return "below"
    return "within"


def hex_oracle(xh, unit, prec, lower, upper):
    x = from_hex(xh)
    if unit not in HEX_UNITS:
        return "err:unit_not_supported"
    if prec != "d0":
        return "err:precision_out_of_range"
    if lower is not None and upper is not None and Fraction(Decimal(lower)) > Fraction(Decimal(upper)):
        return "err:limits_inverted"
    if not math.isfinite(x):
        return "err:non_finite"
    return f"ok:{hexbits(x)}:{side(x, lower, upper)}"


def dec_str(fr):
    """A terminating-decimal Fraction as kdec text `<c>e<k>`, or None if kdec cannot hold it."""
    if fr == 0:
        return "0"
    neg = fr < 0
    num, den, e = abs(fr).numerator, abs(fr).denominator, 0
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
    if len(str(num)) > 38 or not -400 <= e <= 400:
        return None
    return f"{'-' if neg else ''}{num}e{e}"


def hex_cases():
    r = random.Random(SEED)
    special = [
        0.0, -0.0, 1.0, -1.0, 0.5, 0.1, -0.1, 2.0, 1e-3, 5e-5, math.pi, -math.pi,
        5e-324, -5e-324, 2.2250738585072009e-308, 2.2250738585072014e-308,
        -2.2250738585072014e-308, 1.7976931348623157e308, -1.7976931348623157e308,
        math.nextafter(1.0, 2.0), math.nextafter(1.0, 0.0), 1e300, 1e-300, 123456.789,
    ]
    cases = []
    for x in special:
        for unit in HEX_UNITS:
            cases.append((f"{bits_pack(x):016x}", unit, "d0", None, None))
    # Non-finite bit patterns: every one is an error.
    for h in ["7ff8000000000000", "fff8000000000000", "7ff0000000000000", "fff0000000000000",
              "7ff0000000000001", "7fffffffffffffff", "fff0000000000001", "ffffffffffffffff"]:
        cases.append((h, "m", "d0", None, None))
    # Error order: unit before precision before limits before non-finite.
    for unit in ALL_UNITS:
        for xh in ["3ff0000000000000", "7ff8000000000000"]:
            cases.append((xh, unit, "d0", None, None))
            cases.append((xh, unit, "d1", None, None))
    for prec in ["d1", "d3", "d20", "d21", "s1", "s17", "s0"]:
        for xh in ["3ff0000000000000", "7ff0000000000000"]:
            cases.append((xh, "m", prec, "1", "0"))
    for xh in ["3ff0000000000000", "7ff8000000000000", "8000000000000000"]:
        cases.append((xh, "1", "d0", "1", "0"))
        cases.append((xh, "1", "d0", "1e-3", "-1e-3"))
    # Limits: equal to x, just around x, one side only.
    for x in [0.0, -0.0, 0.5, -0.5, 1.0, 5e-5, 0.1, -2.5, 1e-310]:
        X = Fraction(x)
        s = dec_str(X)
        for lo, hi in [(s, s), (None, s), (s, None), ("0", None), (None, "0"),
                       (dec_str(X - Fraction(1, 10**20)), None), (None, dec_str(X - Fraction(1, 10**20))),
                       (dec_str(X + Fraction(1, 10**20)), None), (None, dec_str(X + Fraction(1, 10**20)))]:
            cases.append((f"{bits_pack(x):016x}", "m", "d0", lo, hi))
    # Seeded random finite bit patterns, with and without limits.
    while len(cases) < N_RANDOM:
        b = r.getrandbits(64)
        x = from_hex(f"{b:016x}")
        if not math.isfinite(x):
            cases.append((f"{b:016x}", r.choice(HEX_UNITS), "d0", None, None))
            continue
        unit = r.choice(HEX_UNITS)
        lo = hi = None
        if r.randrange(4) == 0 and abs(x) < 1e30 and (x == 0 or abs(x) > 1e-30):
            X = Fraction(x)
            # x truncated to 1..17 significant digits, as a limit on either side.
            k = math.floor(math.log10(abs(x))) if x != 0 else 0
            p = r.randrange(1, 18) - 1 - k
            t = Fraction(math.floor(abs(X) * Fraction(10) ** p)) / Fraction(10) ** p
            t = t if X >= 0 else -t
            if r.randrange(2):
                lo = dec_str(t)
            else:
                hi = dec_str(t)
        cases.append((f"{b:016x}", unit, "d0", lo, hi))
    return cases


def hex_golden():
    lines = [
        "# KRN.NUM.FMT.REG fmt.hexbits@1 golden cases v1 (CC-BY-4.0). Generated by make_reg_fixtures.py; do not edit.",
        "# x_hex formatter unit prec lower upper expect",
    ]
    for xh, unit, prec, lo, hi in hex_cases():
        e = hex_oracle(xh, unit, prec, lo, hi)
        lines.append(" ".join([xh, "hexbits", unit, prec, lo or "-", hi or "-", e]))
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- guard lines


def guard_line(fid, kind, fields):
    xh, _, unit, _, lower, upper, expect = fields
    lower = None if lower == "-" else lower
    upper = None if upper == "-" else upper
    if expect.startswith("err:"):
        return f"err {fid} {expect[4:]}"
    assert expect.startswith("ok:"), expect
    body = expect[3:]
    if kind in ("length", "scalar"):
        value, digits, tie = body.split(":")
        frm = "m" if kind == "length" else "1"
        return f"ok {value} {frm} {unit} {LEN_FACTOR[unit]} - {digits} {tie} {side(from_hex(xh), lower, upper)}"
    if kind == "angle":
        value, digits, tie, sd, pi = body.rsplit(":", 4)
        return f"ok {value} rad {unit} {ANG_FACTOR[unit]} {pi} {digits} {tie} {sd}"
    if kind == "hexbits":
        value, sd = body.split(":")
        return f"ok {value} {unit} {unit} 1 - 0 0 {sd}"
    raise AssertionError(kind)


def digests(texts):
    lines = ["# KRN.NUM.FMT.REG corpus digests v1 (CC-BY-4.0). Generated by make_reg_fixtures.py; do not edit.",
             "# formatter corpus cases digest"]
    for fid, kind, name in REGISTRY:
        h = hashlib.sha256()
        n = 0
        for line in texts[name].splitlines():
            if line.startswith("#") or not line:
                continue
            fields = line.split(" ")
            assert len(fields) == 7, line
            if fields[1] != kind:
                continue
            h.update((guard_line(fid, kind, fields) + "\n").encode())
            n += 1
        assert n > 0, fid
        lines.append(f"{fid} {CORPUS_DIR}{name} {n} sha256:{h.hexdigest()}")
    return "\n".join(lines) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    hx = hex_golden()
    texts = {
        "len-golden-v1.txt": (HERE / "len-golden-v1.txt").read_text(encoding="ascii"),
        "ang-golden-v1.txt": (HERE / "ang-golden-v1.txt").read_text(encoding="ascii"),
        "hexbits-golden-v1.txt": hx,
    }
    outputs = {"hexbits-golden-v1.txt": hx, "reg-digests-v1.txt": digests(texts)}
    stale = []
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode():
                stale.append(name)
        else:
            path.write_bytes(text.encode())
    if stale:
        raise SystemExit(f"stale fixtures: {', '.join(stale)} (run make_reg_fixtures.py)")
    print("ok" if check else f"wrote {', '.join(outputs)}")


if __name__ == "__main__":
    main()
