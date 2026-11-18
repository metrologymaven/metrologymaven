#!/usr/bin/env python3
"""Generate the fixtures of spec node HAL.ACQ.CONV (deterministic encoding conversion).

Independent reference: every expected value is computed with exact rational arithmetic
(fractions.Fraction) and a single correct rounding to IEEE 754 binary64 per operation,
exactly as REQ-HAL.ACQ.CONV-001 defines the conversion:

    p = round(scale * x)        # one rounding, never fused
    v = round(offset + p)       # one rounding

Signed zeros follow IEEE 754 round-to-nearest rules (handled explicitly because a Fraction
has no sign of zero). Each result is cross-checked against CPython's own float arithmetic
(which does not fuse multiply-add) and the script stops if the two disagree. Nothing here
runs the Rust code.

It writes, next to itself:

  i32_profile.bin          2000 little-endian i32 raw values (all valid)
  i32_profile.f64.sha256   SHA-256 of the decoded values (scale 1e-6 m, offset 0.05 m)
  f32_mixed.bin            2000 little-endian f32 raw values (NaN and inf only where masked)
  f32_mixed.mask           2000 bytes, 0x01 = valid, 0x00 = masked
  f32_mixed.f64.sha256     SHA-256 of the decoded values (scale 1e-3 m, offset -0.125 m)
  cases-v1.txt             element-level cases for every numeric type and every error

Digest stream: the f64 little-endian bytes of the decoded values of the VALID samples only,
in index order (masked samples are skipped, REQ-HAL.ACQ.CONV-004).

Seeded with random.Random (Mersenne Twister; getrandbits output is stable across CPython
versions). CC-BY-4.0 project data. Run:  python fixtures/hal/acq/conv/make_fixtures.py [--check]
"""

import hashlib
import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
TWO53 = 1 << 53


class NotFinite(Exception):
    pass


def signbit(x):
    return math.copysign(1.0, x) < 0


def to_double(exact, negative_if_zero):
    """Round an exact rational to the nearest binary64, ties to even (CPython int true
    division is correctly rounded, including subnormals). Overflow is NotFinite."""
    if exact == 0:
        return -0.0 if negative_if_zero else 0.0
    try:
        r = exact.numerator / exact.denominator
    except OverflowError:
        raise NotFinite()
    if math.isinf(r):
        raise NotFinite()
    if r == 0.0:
        return math.copysign(0.0, -1.0 if exact < 0 else 1.0)
    return r


def ref_mul(scale, x):
    return to_double(Fraction(scale) * Fraction(x), signbit(scale) != signbit(x))


def ref_add(offset, p):
    # Exact zero sum: -0 only when both operands are -0 (round to nearest).
    return to_double(Fraction(offset) + Fraction(p), signbit(offset) and signbit(p))


def ref_fused(scale, x, offset):
    """What a fused multiply-add would return (used only to prove a case discriminates)."""
    return to_double(Fraction(offset) + Fraction(scale) * Fraction(x), False)


def convert_one(scale, offset, x):
    p = ref_mul(scale, x)
    v = ref_add(offset, p)
    native = offset + scale * x
    if not (math.isinf(native) or math.isnan(native)):
        assert struct.pack("<d", native) == struct.pack("<d", v), (scale, offset, x, native, v)
    return v


def int_ok(x):
    return abs(x) <= TWO53


def decode(kind, scale, offset, raws, mask):
    """Reference converter. Returns ('ok', [value or None]) or ('err', code, index)."""
    out = []
    for i, (r, valid) in enumerate(zip(raws, mask)):
        if not valid:
            out.append(None)
            continue
        if kind in ("f32", "f64"):
            if math.isnan(r) or math.isinf(r):
                return ("err", "encoding_value_not_finite", i)
            x = r
        else:
            if not int_ok(r):
                return ("err", "encoding_precision_loss", i)
            x = r
        try:
            out.append(convert_one(scale, offset, x))
        except NotFinite:
            return ("err", "quantity_not_finite", i)
    return ("ok", out)


def f32(x):
    return struct.unpack("<f", struct.pack("<f", x))[0]


def f32_bits(b):
    return struct.unpack("<f", struct.pack("<I", b))[0]


def dhex(x):
    return struct.pack(">d", x).hex()


def raw_text(kind, r):
    if kind == "f32":
        return struct.pack(">f", r).hex()
    if kind == "f64":
        return struct.pack(">d", r).hex()
    return str(r)


def digest(values):
    h = hashlib.sha256()
    for v in values:
        if v is not None:
            h.update(struct.pack("<d", v))
    return h.hexdigest()


# ---------------------------------------------------------------- binary fixtures

def make_i32_profile():
    rng = random.Random(20261118)
    raws = [0, 1, -1, 2**31 - 1, -(2**31), -50000, 50000, 134217727]
    while len(raws) < 2000:
        # A smooth profile around +-20 mm plus noise, and some full-range values.
        if len(raws) % 97 == 0:
            v = rng.getrandbits(32)
            raws.append(v - (1 << 32) if v >= 1 << 31 else v)
        else:
            raws.append(rng.randint(-20000, 20000) * 1000 + rng.randint(-999, 999))
    scale, offset = 1e-6, 0.05
    res = decode("i32", scale, offset, raws, [True] * len(raws))
    assert res[0] == "ok"
    data = struct.pack("<%di" % len(raws), *raws)
    return data, digest(res[1]) + "\n"


def make_f32_mixed():
    rng = random.Random(1999)
    nan_bits = [0x7FC00000, 0xFFC00000, 0x7F800001, 0x7FFFFFFF]
    bits, mask = [], []
    specials_valid = [0.0, -0.0, f32_bits(0x00000001), f32_bits(0x80000001), f32_bits(0x007FFFFF),
                      f32_bits(0x7F7FFFFF), f32_bits(0xFF7FFFFF), 1.0, -1.0, f32(0.1)]
    for v in specials_valid:
        bits.append(struct.unpack("<I", struct.pack("<f", v))[0])
        mask.append(True)
    # Masked samples holding NaN (quiet and signalling), inf and ordinary numbers.
    for b in nan_bits + [0x7F800000, 0xFF800000, 0x41480000]:
        bits.append(b)
        mask.append(False)
    while len(bits) < 2000:
        k = rng.random()
        if k < 0.05:
            bits.append(rng.choice(nan_bits + [0x7F800000, 0xFF800000]))
            mask.append(False)
        elif k < 0.10:
            bits.append(struct.unpack("<I", struct.pack("<f", f32(rng.uniform(-50.0, 50.0))))[0])
            mask.append(False)
        elif k < 0.15:
            # Arbitrary finite bit patterns (any exponent) at valid positions.
            while True:
                b = rng.getrandbits(32)
                if (b >> 23) & 0xFF != 0xFF:
                    break
            bits.append(b)
            mask.append(True)
        else:
            bits.append(struct.unpack("<I", struct.pack("<f", f32(rng.uniform(-50.0, 50.0))))[0])
            mask.append(True)
    scale, offset = 1e-3, -0.125
    # Masked entries are never read as floats, so NaN payloads cannot be altered.
    raws = [f32_bits(b) if m else None for b, m in zip(bits, mask)]
    res = decode("f32", scale, offset, raws, mask)
    assert res[0] == "ok"
    assert sum(1 for b, m in zip(bits, mask) if not m and (b >> 23) & 0xFF == 0xFF) > 50
    data = struct.pack("<%dI" % len(bits), *bits)
    assert len(data) == 8000
    mask_bytes = bytes(1 if m else 0 for m in mask)
    return data, mask_bytes, digest(res[1]) + "\n"


# ---------------------------------------------------------------- element cases

def cases():
    c = []  # (name, kind, scale, offset, raws, mask)

    def add(name, kind, scale, offset, raws, mask=None):
        c.append((name, kind, scale, offset, raws, mask if mask is not None else [True] * len(raws)))

    add("i16_range", "i16", 2.0 ** -15, 0.0, [-32768, -1, 0, 1, 32767])
    add("i16_neg_scale", "i16", -1e-5, 1.0, [-32768, 0, 32767, 12345])
    add("u16_range", "u16", 1e-4, -3.2768, [0, 1, 32768, 65535])
    add("i32_range", "i32", 1e-6, 0.05, [-(2**31), -50000, -1, 0, 1, 50000, 2**31 - 1])
    add("u32_range", "u32", 1e-9, 0.0, [0, 1, 2**31, 2**32 - 1])
    add("u32_kelvin", "u32", 0.01, 273.15, [0, 2315, 4294967295])
    add("i64_exact_edges", "i64", 1e-9, 0.0, [TWO53, -TWO53, TWO53 - 1, 0])
    add("i64_precision_loss", "i64", 1e-9, 0.0, [0, 1, TWO53 + 1, 5])
    add("i64_precision_loss_neg", "i64", 1e-9, 0.0, [-(TWO53 + 1)])
    add("i64_min", "i64", 1.0, 0.0, [7, -(2**63)])
    add("i64_max", "i64", 1.0, 0.0, [2**63 - 1])
    add("i64_masked_big_skipped", "i64", 1e-9, 0.0, [TWO53 + 1, 3, -(2**63)], [False, True, False])
    # Discriminates fused from unfused evaluation (checked below).
    add("no_fma_i32", "i32", 1.0 + 2.0 ** -27, -(2.0 ** 27), [134217727])
    add("no_fma_f64", "f64", 1.0 + 2.0 ** -27, -1.0, [1.0 - 2.0 ** -27])
    add("signed_zero_f32", "f32", 1.0, 0.0, [-0.0, 0.0])
    add("signed_zero_neg_offset", "f32", -2.0, -0.0, [0.0, -0.0])
    add("f32_subnormal", "f32", 0.5, 0.0, [f32_bits(1), f32_bits(0x80000001)])
    add("f32_extremes", "f32", 1e-3, -0.125, [f32_bits(0x7F7FFFFF), f32_bits(0xFF7FFFFF), f32(0.1)])
    add("f32_nan_valid", "f32", 1e-3, 0.0, [1.0, f32_bits(0x7FC00000)])
    add("f32_inf_valid", "f32", 1e-3, 0.0, [1.0, 2.0, f32_bits(0x7F800000)])
    add("f32_neg_inf_valid", "f32", 1e-3, 0.0, [f32_bits(0xFF800000)])
    add("f32_nan_masked", "f32", 1e-3, 0.0, [1.0, f32_bits(0x7FC00000), f32_bits(0xFF800000), 4.0],
        [True, False, False, True])
    add("f64_values", "f64", 1.0, 0.0, [0.1, -2.5e-300, 5e-324, 1.7976931348623157e308])
    add("f64_nan_valid", "f64", 1.0, 0.0, [float("nan")])
    add("f64_inf_masked", "f64", 1.0, 0.0, [float("inf"), 1.0], [False, True])
    add("overflow_product", "f64", 1e300, 0.0, [1.0, 1e300])
    add("overflow_sum", "f64", 1.0, 1.7976931348623157e308, [1.0, 1.7976931348623157e308])
    add("underflow_to_zero", "f64", 1e-300, 0.0, [1e-300, -1e-300])
    add("all_masked", "i32", 1.0, 0.0, [1, 2, 3], [False, False, False])
    add("empty", "u16", 1.0, 0.0, [])
    add("first_error_wins", "f32", 1.0, 0.0, [1.0, f32_bits(0x7F800000), f32_bits(0x7FC00000)])

    # The no-FMA cases really discriminate.
    for name, kind, scale, offset, raws, _ in c:
        if name.startswith("no_fma"):
            x = raws[0]
            fused = ref_fused(scale, x, offset)
            two_step = ref_add(offset, ref_mul(scale, x))
            assert dhex(fused) != dhex(two_step), name

    lines = [
        "# HAL.ACQ.CONV element cases (generated by make_fixtures.py; do not edit).",
        "# name type scale offset raws mask -> ok outs | err code index",
        "# scale/offset/outs: binary64 big-endian hex; raws: decimal integers or big-endian hex",
        "# float bits; mask: 1 = valid, 0 = masked; outs: '-' for masked; '.' = empty list.",
    ]
    for name, kind, scale, offset, raws, mask in c:
        res = decode(kind, scale, offset, raws, mask)
        r = ",".join(raw_text(kind, x) for x in raws) or "."
        m = "".join("1" if v else "0" for v in mask) or "."
        if res[0] == "ok":
            outs = ",".join("-" if v is None else dhex(v) for v in res[1]) or "."
            tail = "ok " + outs
        else:
            tail = "err %s %d" % (res[1], res[2])
        lines.append("%s %s %s %s %s %s -> %s" % (name, kind, dhex(scale), dhex(offset), r, m, tail))
    return "\n".join(lines) + "\n"


# ---------------------------------------------------------------- self-checks

def self_check():
    # Hand-labelled spot values, independent of the rounding helper.
    assert convert_one(1e-6, 0.05, 0) == 0.05
    assert convert_one(0.5, 1.0, 2) == 2.0
    assert dhex(ref_add(-0.0, ref_mul(1.0, -0.0))) == dhex(-0.0)
    assert dhex(ref_add(0.0, ref_mul(1.0, -0.0))) == dhex(0.0)
    assert dhex(ref_mul(-2.0, 0.0)) == dhex(-0.0)
    assert ref_mul(1e-300, 1e-300) == 0.0 and signbit(ref_mul(-1e-300, 1e-300))
    try:
        ref_mul(1e300, 1e300)
        raise AssertionError("overflow not detected")
    except NotFinite:
        pass
    # Ties to even: 1 - 2^-54 is halfway between 1 - 2^-53 and 1.
    assert to_double(Fraction(1) - Fraction(1, 2**54), False) == 1.0


def outputs():
    i32_bin, i32_sha = make_i32_profile()
    f32_bin, f32_mask, f32_sha = make_f32_mixed()
    return {
        "i32_profile.bin": i32_bin,
        "i32_profile.f64.sha256": i32_sha.encode(),
        "f32_mixed.bin": f32_bin,
        "f32_mixed.mask": f32_mask,
        "f32_mixed.f64.sha256": f32_sha.encode(),
        "cases-v1.txt": cases().encode(),
    }


def main():
    self_check()
    check = "--check" in sys.argv[1:]
    stale = []
    files = outputs()
    for name, data in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale))
        sys.exit(1)
    print("ok" if check else "wrote %d files" % len(files))


if __name__ == "__main__":
    main()
