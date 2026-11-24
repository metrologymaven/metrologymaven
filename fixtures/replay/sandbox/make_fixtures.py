#!/usr/bin/env python3
"""Independent fixtures for TRUST.REPLAY.SANDBOX (stdlib only; CC-BY-4.0).

Writes, next to this file:
  <name>.wasm               every hand-written <name>.wat source, assembled by wat.py
  truncated.wasm            kernel-test.wasm without its last 7 bytes (malformed)
  simd.wasm                 a module using a SIMD instruction (refused feature)
  circle-100.bin            100 points (x, y, z little-endian f64) on a circle, rational parametrisation
  kernel-test.manifest.json KRN.MAN.MANI.SCHEMA v1 manifest whose wasm32-wasip1 entry is kernel-test.wasm
  expect.json               hand-labelled outcomes of the hostile set, and the kernel-test call with its
                            envelope computed here (FNV-1a 64 and an exactly rounded f64 sum/sqrt)
  ops.wasm, ops-expect.txt  numeric conformance cases: one exported function per case, with the
                            expected result bits or trap computed by an exact rational model of
                            WebAssembly integer and IEEE 754 arithmetic (no host float operation
                            decides a result bit)
  README.md                 provenance

Usage:
  python fixtures/replay/sandbox/make_fixtures.py          # write files
  python fixtures/replay/sandbox/make_fixtures.py --check  # verify byte-for-byte match
"""

import hashlib
import json
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
sys.path.insert(0, str(HERE))
import wat  # noqa: E402

M32 = (1 << 32) - 1
M64 = (1 << 64) - 1


def sha(data):
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canonical_json(value):
    return json.dumps(value, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


# ------------------------------------------------------------------ exact IEEE 754 model

class Fmt:
    def __init__(self, mant, ebits):
        self.mant = mant
        self.ebits = ebits
        self.bias = (1 << (ebits - 1)) - 1
        self.emin = 1 - self.bias
        self.emax = self.bias
        self.width = 1 + ebits + mant
        self.sign_bit = 1 << (self.width - 1)
        self.exp_mask = ((1 << ebits) - 1) << mant
        self.canon_nan = self.exp_mask | (1 << (mant - 1))
        self.inf = self.exp_mask


F32 = Fmt(23, 8)
F64 = Fmt(52, 11)


def decode(fmt, bits):
    """('nan',) | ('inf', neg) | ('fin', neg, Fraction magnitude)."""
    neg = bool(bits & fmt.sign_bit)
    e = (bits & fmt.exp_mask) >> fmt.mant
    m = bits & ((1 << fmt.mant) - 1)
    if e == (1 << fmt.ebits) - 1:
        return ("nan",) if m else ("inf", neg)
    if e == 0:
        return ("fin", neg, Fraction(m, 1 << (fmt.mant - fmt.emin)))
    return ("fin", neg, Fraction((1 << fmt.mant) | m) * Fraction(2) ** (e - fmt.bias - fmt.mant))


def floor_log2(q):
    """Largest e with 2**e <= q, for a positive Fraction q."""
    e = q.numerator.bit_length() - q.denominator.bit_length()
    if Fraction(2) ** e > q:
        e -= 1
    return e


def encode(fmt, neg, mag):
    """Rounds magnitude `mag` (Fraction >= 0) to nearest, ties to even; overflow gives infinity."""
    s = fmt.sign_bit if neg else 0
    if mag == 0:
        return s
    e = max(floor_log2(mag), fmt.emin)
    quantum = Fraction(2) ** (e - fmt.mant)
    q = mag / quantum
    n = q.numerator // q.denominator
    rem = q - n
    if rem > Fraction(1, 2) or (rem == Fraction(1, 2) and n % 2 == 1):
        n += 1
    if n == 1 << (fmt.mant + 1):
        n >>= 1
        e += 1
    if e > fmt.emax:
        return s | fmt.inf
    if n < 1 << fmt.mant:
        return s | n  # subnormal (e == emin)
    return s | ((e + fmt.bias) << fmt.mant) | (n - (1 << fmt.mant))


def f_add(fmt, a, b):
    x, y = decode(fmt, a), decode(fmt, b)
    if x[0] == "nan" or y[0] == "nan":
        return fmt.canon_nan
    if x[0] == "inf" and y[0] == "inf":
        return fmt.canon_nan if x[1] != y[1] else (fmt.sign_bit if x[1] else 0) | fmt.inf
    if x[0] == "inf" or y[0] == "inf":
        z = x if x[0] == "inf" else y
        return (fmt.sign_bit if z[1] else 0) | fmt.inf
    vx = -x[2] if x[1] else x[2]
    vy = -y[2] if y[1] else y[2]
    v = vx + vy
    if v == 0:
        return fmt.sign_bit if (x[1] and y[1]) else 0
    return encode(fmt, v < 0, abs(v))


def f_neg(fmt, a):
    return a ^ fmt.sign_bit


def f_sub(fmt, a, b):
    if decode(fmt, b)[0] == "nan":
        return fmt.canon_nan
    return f_add(fmt, a, f_neg(fmt, b))


def f_mul(fmt, a, b):
    x, y = decode(fmt, a), decode(fmt, b)
    if x[0] == "nan" or y[0] == "nan":
        return fmt.canon_nan
    neg = x[1] != y[1]
    s = fmt.sign_bit if neg else 0
    if x[0] == "inf" or y[0] == "inf":
        other = y if x[0] == "inf" else x
        if other[0] == "fin" and other[2] == 0:
            return fmt.canon_nan
        return s | fmt.inf
    return encode(fmt, neg, x[2] * y[2])


def f_div(fmt, a, b):
    x, y = decode(fmt, a), decode(fmt, b)
    if x[0] == "nan" or y[0] == "nan":
        return fmt.canon_nan
    neg = x[1] != y[1]
    s = fmt.sign_bit if neg else 0
    if x[0] == "inf":
        return fmt.canon_nan if y[0] == "inf" else s | fmt.inf
    if y[0] == "inf":
        return s
    if y[2] == 0:
        return fmt.canon_nan if x[2] == 0 else s | fmt.inf
    return encode(fmt, neg, x[2] / y[2])


def isqrt(n):
    if n == 0:
        return 0
    x = 1 << ((n.bit_length() + 1) // 2)
    while True:
        y = (x + n // x) // 2
        if y >= x:
            return x
        x = y


def f_sqrt(fmt, a):
    x = decode(fmt, a)
    if x[0] == "nan":
        return fmt.canon_nan
    if x[0] == "inf":
        return fmt.canon_nan if x[1] else fmt.inf
    if x[2] == 0:
        return a
    if x[1]:
        return fmt.canon_nan
    k = 1200  # 4**k * value is an integer for every f64 value
    scaled = x[2] * Fraction(4) ** k
    n = scaled.numerator // scaled.denominator
    r = isqrt(n)
    if r * r == n:
        return encode(fmt, False, Fraction(r, 1 << k))
    # Strictly between r and r + 1 units of 2**-k: enough to round correctly.
    return encode(fmt, False, Fraction(2 * r + 1, 1 << (k + 1)))


def f_round(fmt, a, mode):
    x = decode(fmt, a)
    if x[0] == "nan":
        return fmt.canon_nan
    if x[0] == "inf" or x[2] == 0:
        return a
    v = -x[2] if x[1] else x[2]
    fl = v.numerator // v.denominator
    if mode == "floor":
        r = fl
    elif mode == "ceil":
        r = fl if fl == v else fl + 1
    elif mode == "trunc":
        r = fl if v >= 0 else (fl if fl == v else fl + 1)
    else:  # nearest, ties to even
        rem = v - fl
        r = fl + 1 if (rem > Fraction(1, 2) or (rem == Fraction(1, 2) and fl % 2 == 1)) else fl
    if r == 0:
        return fmt.sign_bit if x[1] else 0
    return encode(fmt, r < 0, Fraction(abs(r)))


def f_value(fmt, a):
    x = decode(fmt, a)
    if x[0] != "fin":
        return None
    return -x[2] if x[1] else x[2]


def f_minmax(fmt, a, b, is_min):
    x, y = decode(fmt, a), decode(fmt, b)
    if x[0] == "nan" or y[0] == "nan":
        return fmt.canon_nan
    zx = x[0] == "fin" and x[2] == 0
    zy = y[0] == "fin" and y[2] == 0
    if zx and zy:
        return (a | b) if is_min else (a & b)

    def key(z):
        if z[0] == "inf":
            return (1 if not z[1] else -1, 0)
        return (0, -z[2] if z[1] else z[2])

    kx, ky = key(x), key(y)
    if is_min:
        return a if kx < ky else b
    return a if kx > ky else b


def f_cmp(fmt, a, b, op):
    x, y = decode(fmt, a), decode(fmt, b)
    if x[0] == "nan" or y[0] == "nan":
        return 1 if op == "ne" else 0

    def key(z):
        if z[0] == "inf":
            return (1 if not z[1] else -1, Fraction(0))
        return (0, -z[2] if z[1] else z[2])

    kx, ky = key(x), key(y)
    return int({"eq": kx == ky, "ne": kx != ky, "lt": kx < ky, "gt": kx > ky, "le": kx <= ky, "ge": kx >= ky}[op])


def trunc_to_int(fmt, a, signed, bits, saturating):
    """Returns ('ok', value) or ('trap', kind)."""
    x = decode(fmt, a)
    lo = -(1 << (bits - 1)) if signed else 0
    hi = (1 << (bits - 1)) - 1 if signed else (1 << bits) - 1
    if x[0] == "nan":
        return ("ok", 0) if saturating else ("trap", "bad_conversion_to_integer")
    if x[0] == "inf":
        if saturating:
            return ("ok", lo if x[1] else hi)
        return ("trap", "integer_overflow")
    v = -x[2] if x[1] else x[2]
    t = v.numerator // v.denominator
    if t < 0 and t != v:
        t += 1  # toward zero
    if t < lo or t > hi:
        if saturating:
            return ("ok", lo if t < lo else hi)
        return ("trap", "integer_overflow")
    return ("ok", t)


# ------------------------------------------------------------------ integer model

def sx(v, bits):
    v &= (1 << bits) - 1
    return v - (1 << bits) if v >> (bits - 1) else v


def int_op(name, bits, a, b=None):
    m = (1 << bits) - 1
    sa, sb = sx(a, bits), (sx(b, bits) if b is not None else None)
    if name == "clz":
        return ("ok", bits - a.bit_length())
    if name == "ctz":
        return ("ok", bits if a == 0 else (a & -a).bit_length() - 1)
    if name == "popcnt":
        return ("ok", bin(a).count("1"))
    if name == "eqz":
        return ("ok", int(a == 0))
    if name == "add":
        return ("ok", (a + b) & m)
    if name == "sub":
        return ("ok", (a - b) & m)
    if name == "mul":
        return ("ok", (a * b) & m)
    if name in ("div_s", "div_u", "rem_s", "rem_u") and b == 0:
        return ("trap", "integer_divide_by_zero")
    if name == "div_s":
        if sa == -(1 << (bits - 1)) and sb == -1:
            return ("trap", "integer_overflow")
        q = abs(sa) // abs(sb)
        return ("ok", (q if (sa < 0) == (sb < 0) else -q) & m)
    if name == "div_u":
        return ("ok", a // b)
    if name == "rem_s":
        r = abs(sa) % abs(sb)
        return ("ok", (-r if sa < 0 else r) & m)
    if name == "rem_u":
        return ("ok", a % b)
    if name == "and":
        return ("ok", a & b)
    if name == "or":
        return ("ok", a | b)
    if name == "xor":
        return ("ok", a ^ b)
    k = b % bits if b is not None else 0
    if name == "shl":
        return ("ok", (a << k) & m)
    if name == "shr_s":
        return ("ok", (sa >> k) & m)
    if name == "shr_u":
        return ("ok", a >> k)
    if name == "rotl":
        return ("ok", ((a << k) | (a >> (bits - k))) & m)
    if name == "rotr":
        return ("ok", ((a >> k) | (a << (bits - k))) & m)
    cmp = {
        "eq": a == b, "ne": a != b, "lt_s": sa < sb, "lt_u": a < b, "gt_s": sa > sb, "gt_u": a > b,
        "le_s": sa <= sb, "le_u": a <= b, "ge_s": sa >= sb, "ge_u": a >= b,
    }
    return ("ok", int(cmp[name]))


# ------------------------------------------------------------------ ops conformance cases

class Rng:
    """xorshift64*: deterministic, independent of Python's random module."""

    def __init__(self, seed):
        self.s = seed & M64 or 1

    def next(self):
        x = self.s
        x ^= (x >> 12)
        x ^= (x << 25) & M64
        x ^= (x >> 27)
        self.s = x
        return (x * 0x2545F4914F6CDD1D) & M64


I32_EDGE = [0, 1, 2, 7, 31, 32, 0x7FFFFFFF, 0x80000000, 0x80000001, 0xFFFFFFFF, 0xFFFFFFF9, 0x12345678]
I64_EDGE = [0, 1, 2, 7, 63, 64, 0x7FFFFFFFFFFFFFFF, 0x8000000000000000, 0x8000000000000001, M64,
            0xFFFFFFFFFFFFFFF9, 0x0123456789ABCDEF]

F32_EDGE = [
    0x00000000, 0x80000000, 0x3F800000, 0xBF800000, 0x3FC00000, 0x40200000, 0xC0200000, 0x3DCCCCCD,
    0x7F7FFFFF, 0x00800000, 0x00000001, 0x80000001, 0x7F800000, 0xFF800000, 0x7FC00000, 0x7FA00001,
    0xFFC00000, 0x4B000001, 0x3F000000, 0xBF000000, 0x4EFFFFFF, 0xCF000000, 0x4F800000, 0x3EAAAAAB,
]
F64_EDGE = [
    0x0000000000000000, 0x8000000000000000, 0x3FF0000000000000, 0xBFF0000000000000, 0x3FF8000000000000,
    0x4004000000000000, 0xC004000000000000, 0x3FB999999999999A, 0x7FEFFFFFFFFFFFFF, 0x0010000000000000,
    0x0000000000000001, 0x8000000000000001, 0x7FF0000000000000, 0xFFF0000000000000, 0x7FF8000000000000,
    0x7FF4000000000001, 0xFFF8000000000000, 0x4340000000000001, 0x3FE0000000000000, 0xBFE0000000000000,
    0x41DFFFFFFFC00000, 0xC1E0000000000000, 0x43E0000000000000, 0x3FD5555555555555,
]

INT_BIN = "add sub mul div_s div_u rem_s rem_u and or xor shl shr_s shr_u rotl rotr".split()
INT_CMP = "eq ne lt_s lt_u gt_s gt_u le_s le_u ge_s ge_u".split()
INT_UN = "clz ctz popcnt eqz".split()
F_BIN = "add sub mul div min max copysign".split()
F_UN = "abs neg ceil floor trunc nearest sqrt".split()
F_CMP = "eq ne lt gt le ge".split()


def const(ty, bits):
    if ty == "i32":
        return f"i32.const {bits:#x}"
    if ty == "i64":
        return f"i64.const {bits:#x}"
    if ty == "f32":
        return f"i32.const {bits:#x} f32.reinterpret_i32"
    return f"i64.const {bits:#x} f64.reinterpret_i64"


def result_tail(ty):
    return {"f32": " i32.reinterpret_f32", "f64": " i64.reinterpret_f64"}.get(ty, "")


def wasm_ty(ty):
    return {"f32": "i32", "f64": "i64"}.get(ty, ty)


def pairs(edge, rng, extra, width):
    out = [(a, b) for a in edge[:6] for b in edge[:6]]
    for _ in range(extra):
        out.append((edge[rng.next() % len(edge)], rng.next() & ((1 << width) - 1)))
        out.append((rng.next() & ((1 << width) - 1), edge[rng.next() % len(edge)]))
    return out


def float_fn(fmt, name):
    return {
        "add": lambda a, b: f_add(fmt, a, b), "sub": lambda a, b: f_sub(fmt, a, b),
        "mul": lambda a, b: f_mul(fmt, a, b), "div": lambda a, b: f_div(fmt, a, b),
        "min": lambda a, b: f_minmax(fmt, a, b, True), "max": lambda a, b: f_minmax(fmt, a, b, False),
        "copysign": lambda a, b: (a & ~fmt.sign_bit) | (b & fmt.sign_bit),
        "abs": lambda a: a & ~fmt.sign_bit, "neg": lambda a: a ^ fmt.sign_bit,
        "ceil": lambda a: f_round(fmt, a, "ceil"), "floor": lambda a: f_round(fmt, a, "floor"),
        "trunc": lambda a: f_round(fmt, a, "trunc"), "nearest": lambda a: f_round(fmt, a, "nearest"),
        "sqrt": lambda a: f_sqrt(fmt, a),
    }[name]


def ops_cases():
    """[(wat body, result type, ('ok', bits) | ('trap', kind))]."""
    rng = Rng(0x5A4D_B0C5_2026_1123)
    cases = []
    for ty, bits, edge in (("i32", 32, I32_EDGE), ("i64", 64, I64_EDGE)):
        for op in INT_UN:
            for a in edge + [rng.next() & ((1 << bits) - 1) for _ in range(8)]:
                res_ty = "i32" if op == "eqz" else ty
                cases.append((f"{const(ty, a)} {ty}.{op}", res_ty, int_op(op, bits, a)))
        for op in INT_BIN + INT_CMP:
            res_ty = "i32" if op in INT_CMP else ty
            for a, b in pairs(edge, rng, 4, bits) + [(x, y) for x in edge[6:] for y in edge[6:9]]:
                cases.append((f"{const(ty, a)} {const(ty, b)} {ty}.{op}", res_ty, int_op(op, bits, a, b)))
        for n in (8, 16, 32):
            if n == 32 and ty == "i32":
                continue
            for a in edge:
                cases.append((f"{const(ty, a)} {ty}.extend{n}_s", ty, ("ok", sx(a, n) & ((1 << bits) - 1))))
    for ty, fmt, edge in (("f32", F32, F32_EDGE), ("f64", F64, F64_EDGE)):
        width = fmt.width
        for op in F_UN:
            fn = float_fn(fmt, op)
            for a in edge + [rng.next() & ((1 << width) - 1) for _ in range(12)]:
                cases.append((f"{const(ty, a)} {ty}.{op}{result_tail(ty)}", wasm_ty(ty), ("ok", fn(a))))
        for op in F_BIN:
            fn = float_fn(fmt, op)
            for a, b in pairs(edge, rng, 6, width) + [(x, y) for x in edge[6:18] for y in edge[10:16]]:
                body = f"{const(ty, a)} {const(ty, b)} {ty}.{op}{result_tail(ty)}"
                cases.append((body, wasm_ty(ty), ("ok", fn(a, b))))
        for op in F_CMP:
            for a, b in pairs(edge, rng, 4, width) + [(x, y) for x in edge[10:17] for y in edge[10:17]]:
                cases.append((f"{const(ty, a)} {const(ty, b)} {ty}.{op}", "i32", ("ok", f_cmp(fmt, a, b, op))))
    # conversions
    trunc_vals32 = F32_EDGE + [0x4EFFFFFF, 0x4F000000, 0xCF000001, 0x5EFFFFFF, 0x5F000000, 0x5F800000,
                                0xBF7FFFFF, 0x4F7FFFFF, 0xDF000000, 0xDF000001]
    trunc_vals64 = F64_EDGE + [0x41DFFFFFFFFFFFFF, 0x41E0000000000000, 0xC1E0000000200000, 0xC1E00000001FFFFF,
                               0x41EFFFFFFFFFFFFF, 0x41F0000000000000, 0x43DFFFFFFFFFFFFF, 0x43E0000000000000,
                               0xC3E0000000000000, 0xC3E0000000000001, 0x43EFFFFFFFFFFFFF, 0x43F0000000000000,
                               0xBFEFFFFFFFFFFFFF]
    for src, fmt, vals in (("f32", F32, trunc_vals32), ("f64", F64, trunc_vals64)):
        for dst, bits in (("i32", 32), ("i64", 64)):
            for signed in (True, False):
                sfx = "s" if signed else "u"
                for v in vals:
                    r = trunc_to_int(fmt, v, signed, bits, False)
                    r = ("ok", r[1] & ((1 << bits) - 1)) if r[0] == "ok" else r
                    cases.append((f"{const(src, v)} {dst}.trunc_{src}_{sfx}", dst, r))
                    r = trunc_to_int(fmt, v, signed, bits, True)
                    cases.append((f"{const(src, v)} {dst}.trunc_sat_{src}_{sfx}", dst, ("ok", r[1] & ((1 << bits) - 1))))
    int_vals = {"i32": I32_EDGE + [0x01000001, 0xFFFFFF7F, 0x7FFFFFC0],
                "i64": I64_EDGE + [0x0020000000000001, 0xFFFFFFFFFFFFFC01, 0x7FFFFFFFFFFFFDFF, 0x0000000001000001]}
    for src, bits in (("i32", 32), ("i64", 64)):
        for dst, fmt in (("f32", F32), ("f64", F64)):
            for signed in (True, False):
                sfx = "s" if signed else "u"
                for v in int_vals[src]:
                    n = sx(v, bits) if signed else v
                    r = encode(fmt, n < 0, Fraction(abs(n)))
                    cases.append((f"{const(src, v)} {dst}.convert_{src}_{sfx}{result_tail(dst)}", wasm_ty(dst), ("ok", r)))
    for v in F64_EDGE + [0x36A0000000000000, 0x3690000000000001, 0x47EFFFFFF0000000, 0x47EFFFFFE0000000, 0x3FF0000010000000]:
        x = decode(F64, v)
        if x[0] == "nan":
            r = F32.canon_nan
        elif x[0] == "inf":
            r = (F32.sign_bit if x[1] else 0) | F32.inf
        else:
            r = encode(F32, x[1], x[2])
        cases.append((f"{const('f64', v)} f32.demote_f64 i32.reinterpret_f32", "i32", ("ok", r)))
    for v in F32_EDGE:
        x = decode(F32, v)
        if x[0] == "nan":
            r = F64.canon_nan
        elif x[0] == "inf":
            r = (F64.sign_bit if x[1] else 0) | F64.inf
        else:
            r = encode(F64, x[1], x[2])
        cases.append((f"{const('f32', v)} f64.promote_f32 i64.reinterpret_f64", "i64", ("ok", r)))
    for v in I64_EDGE:
        cases.append((f"{const('i64', v)} i32.wrap_i64", "i32", ("ok", v & M32)))
    for v in I32_EDGE:
        cases.append((f"{const('i32', v)} i64.extend_i32_s", "i64", ("ok", sx(v, 32) & M64)))
        cases.append((f"{const('i32', v)} i64.extend_i32_u", "i64", ("ok", v)))
    # NaN payloads survive bit operations and loads/stores, but not arithmetic.
    cases.append(("i32.const 0 i32.const 0x7fa00001 i32.store i32.const 0 f32.load i32.reinterpret_f32", "i32",
                  ("ok", 0x7FA00001)))
    cases.append(("i32.const 8 i64.const 0x7ff4000000000001 i64.store i32.const 8 f64.load f64.neg i64.reinterpret_f64",
                  "i64", ("ok", 0xFFF4000000000001)))
    cases.append(("i32.const 8 i64.const 0x7ff4000000000001 i64.store i32.const 8 f64.load f64.const1 f64.mul"
                  .replace("f64.const1", "i64.const 0x3ff0000000000000 f64.reinterpret_i64") + " i64.reinterpret_f64",
                  "i64", ("ok", F64.canon_nan)))
    # Memory: sign/zero extension of narrow loads, little-endian order, and bounds.
    cases.append(("i32.const 16 i64.const 0x8899aabbccddeeff i64.store i32.const 16 i32.load8_s", "i32", ("ok", 0xFFFFFFFF)))
    cases.append(("i32.const 16 i64.const 0x8899aabbccddeeff i64.store i32.const 17 i64.load16_s", "i64",
                  ("ok", 0xFFFFFFFFFFFFDDEE)))
    cases.append(("i32.const 16 i64.const 0x8899aabbccddeeff i64.store i32.const 19 i64.load32_u", "i64", ("ok", 0x99AABBCC)))
    cases.append(("i32.const 65532 i32.const 1 i32.store i32.const 65532 i32.load", "i32", ("ok", 1)))
    cases.append(("i32.const 65533 i32.load", "i32", ("trap", "out_of_bounds_memory_access")))
    cases.append(("i32.const 0 i32.load offset=65536", "i32", ("trap", "out_of_bounds_memory_access")))
    cases.append(("i32.const 0 i32.const 7 i32.const 100 memory.fill i32.const 50 i32.const 0 i32.const 100 memory.copy "
                  "i32.const 149 i32.load8_u", "i32", ("ok", 7)))
    cases.append(("i32.const 1 i32.const 2 i32.const 65535 memory.copy i32.const 0", "i32",
                  ("trap", "out_of_bounds_memory_access")))
    cases.append(("i32.const 0 memory.grow memory.size i32.add", "i32", ("ok", 2)))
    return cases


def ops_module(cases):
    lines = ["(module", "  (memory 1 1)"]
    for k, (body, ty, _) in enumerate(cases):
        lines.append(f'  (func (export "c{k}") (result {ty}) {body})')
    lines.append(")")
    text = "\n".join(lines) + "\n"
    expect = []
    for k, (_, ty, res) in enumerate(cases):
        if res[0] == "ok":
            width = 8 if ty == "i32" else 16
            expect.append(f"c{k} {ty} {res[1]:0{width}x}")
        else:
            expect.append(f"c{k} trap {res[1]}")
    return wat.assemble(text), ("\n".join(expect) + "\n").encode("ascii")


# ------------------------------------------------------------------ kernel-test call

def circle_points():
    """100 points on a circle of radius 5 about (10, -3, 0.25) from the rational parametrisation
    (1 - t^2, 2t) / (1 + t^2) with t = k / 37 - 1.3; Python float division is IEEE 754."""
    out = bytearray()
    for k in range(100):
        t = k / 37.0 - 1.3
        d = 1.0 + t * t
        x = 10.0 + 5.0 * ((1.0 - t * t) / d)
        y = -3.0 + 5.0 * ((2.0 * t) / d)
        for v in (x, y, 0.25):
            out += struct.pack("<d", v)
    return bytes(out)


def call_bytes(algorithm, params, seed, inputs):
    """Canonical KRN.MAN.WASM call JSON (keys in code-unit order, no whitespace)."""
    obj = {
        "algorithm": algorithm,
        "inputs": [{"bytes": b.hex(), "sha256": sha(b)} for b in inputs],
        "params": params.hex(),
    }
    if seed is not None:
        obj["seed"] = str(seed)
    return canonical_json(obj)


def fnv1a64(data):
    h = 0xCBF29CE484222325
    for b in data:
        h = ((h ^ b) * 0x100000001B3) & M64
    return h


def sqrt_sum_bits(data):
    """sqrt(sum f64(b) * 0.1) with every operation rounded exactly by the model above."""
    tenth = 0x3FB999999999999A
    acc = 0
    for b in data:
        prod = f_mul(F64, encode(F64, False, Fraction(b)), tenth)
        acc = f_add(F64, acc, prod)
    return f_sqrt(F64, acc)


# ------------------------------------------------------------------ hand-labelled hostile set

HOSTILE = [
    # module, stdin, limit overrides, expected code, trap kind
    ("loop.wasm", "", {"fuel_per_call": "1000000"}, "sandbox.budget_exhausted", None),
    ("loop.wasm", "", {"fuel_per_call": "1000000000000", "wall_clock_ms": "1500"}, "sandbox.budget_exhausted", None),
    ("grow.wasm", "", {}, "sandbox.memory_limit", None),
    ("grow.wasm", "", {"memory_bytes": "16777216"}, "sandbox.memory_limit", None),
    ("big-memory.wasm", "", {}, "sandbox.memory_limit", None),
    ("big-memory.wasm", "", {"memory_bytes": "1073741824"}, "sandbox.memory_limit", None),
    ("big-memory.wasm", "", {"memory_bytes": "4294967296"}, "sandbox.trap", "unexpected_exit"),
    ("imports-fd.wasm", "", {}, "sandbox.forbidden_import", None),
    ("imports-random.wasm", "", {}, "sandbox.forbidden_import", None),
    ("imports-clock.wasm", "", {}, "sandbox.forbidden_import", None),
    ("imports-sock.wasm", "", {}, "sandbox.forbidden_import", None),
    ("imports-env.wasm", "", {}, "sandbox.forbidden_import", None),
    ("imports-memory.wasm", "", {}, "sandbox.forbidden_import", None),
    ("imports-badsig.wasm", "", {}, "sandbox.forbidden_import", None),
    ("clock-trap.wasm", "", {}, "sandbox.trap", "host_trap"),
    ("recurse.wasm", "", {}, "sandbox.trap", "call_stack_exhausted"),
    ("traps.wasm", "0", {}, "sandbox.trap", "unreachable"),
    ("traps.wasm", "1", {}, "sandbox.trap", "out_of_bounds_memory_access"),
    ("traps.wasm", "2", {}, "sandbox.trap", "integer_divide_by_zero"),
    ("traps.wasm", "3", {}, "sandbox.trap", "integer_overflow"),
    ("traps.wasm", "4", {}, "sandbox.trap", "bad_conversion_to_integer"),
    ("traps.wasm", "5", {}, "sandbox.trap", "indirect_call_to_null"),
    ("traps.wasm", "6", {}, "sandbox.trap", "bad_signature"),
    ("exits.wasm", "0", {}, "sandbox.trap", "unexpected_exit"),
    ("exits.wasm", "1", {}, "sandbox.trap", "unexpected_exit"),
    ("exits.wasm", "2", {}, "sandbox.trap", "unexpected_exit"),
    ("exits.wasm", "3", {"max_output_bytes": "1048576"}, "sandbox.memory_limit", None),
    ("truncated.wasm", "", {}, "sandbox.malformed_module", None),
    ("simd.wasm", "", {}, "sandbox.malformed_module", None),
]

# A function body using v128.const (0xFD 12): SIMD is refused.
SIMD = (
    b"\x00asm\x01\x00\x00\x00"
    b"\x01\x04\x01\x60\x00\x00"          # type [] -> []
    b"\x03\x02\x01\x00"                  # one function
    b"\x07\x0a\x01\x06_start\x00\x00"    # export _start
    b"\x0a\x17\x01\x15\x00\xfd\x0c" + bytes(16) + b"\x1a\x0b"
)


def build_all():
    files = {}
    for src in sorted(HERE.glob("*.wat")):
        files[src.with_suffix(".wasm")] = wat.assemble(src.read_text(encoding="utf-8"))
    kernel = files[HERE / "kernel-test.wasm"]
    files[HERE / "truncated.wasm"] = kernel[:-7]
    files[HERE / "simd.wasm"] = SIMD
    points = circle_points()
    files[HERE / "circle-100.bin"] = points
    manifest = {
        "artifacts": [{"role": "wasm", "sha256": sha(kernel), "target": "wasm32-wasip1"}],
        "core": {"algorithms": [], "name": "fid-sandbox-test-kernel", "numeric_profile": "knp-1",
                 "schema_version": "1", "semver": "0.0.0-sandbox-test"},
    }
    files[HERE / "kernel-test.manifest.json"] = canonical_json(manifest)

    params = canonical_json({"tolerance": "1e-9"})
    call = call_bytes("test.sandbox.fnv@1", params, 42, [points])
    envelope = (
        '{"algorithm":"test.sandbox.fnv@1","fnv1a64":"%016x","sqrt_sum":"%016x"}'
        % (fnv1a64(call), sqrt_sum_bits(call))
    ).encode("ascii")
    other = call_bytes("fit.circle.lsq@1", params, None, [points])
    expect = {
        "kernel": {
            "algorithm": "test.sandbox.fnv@1",
            "params_hex": params.hex(),
            "seed": "42",
            "inputs": ["circle-100.bin"],
            "call_sha256": sha(call),
            "envelope": envelope.decode("ascii"),
            "envelope_sha256": sha(envelope),
        },
        "kernel_error": {
            "algorithm": "fit.circle.lsq@1",
            "call_sha256": sha(other),
            "exit_code": "1",
            "stdout": '{"error":"unknown_algorithm"}',
        },
        "probe": {"module": "probe.wasm", "envelope": '{"algorithm":"probe","results":"0088"}'},
        "hostile": [
            {"module": m, "stdin": s, "limits": lim, "code": code, **({"trap": kind} if kind else {})}
            for m, s, lim, code, kind in HOSTILE
        ],
    }
    files[HERE / "expect.json"] = canonical_json(expect)
    ops_wasm, ops_expect = ops_module(ops_cases())
    files[HERE / "ops.wasm"] = ops_wasm
    files[HERE / "ops-expect.txt"] = ops_expect
    files[HERE / "README.md"] = README.encode("utf-8")
    return files


README = """# TRUST.REPLAY.SANDBOX fixtures

Licence: CC-BY-4.0. Generated by `make_fixtures.py` (Python stdlib only); `--check` verifies every
file byte for byte.

- `*.wat`: hand-written WebAssembly text sources; `*.wasm` are their binaries, assembled by
  `wat.py`, an independent encoder of the core binary format.
- `kernel-test.wat`: a WASI command that speaks the KRN.MAN.WASM call protocol (canonical call on
  stdin, envelope on stdout, exit 0/1/2) and stands in for the KRN.MAN test kernel until a
  reference kernel registers `fit.circle.lsq@1`. Its envelope for the call in `expect.json` is
  computed here: FNV-1a 64 of the call bytes and the f64 bits of a square root of a byte sum, each
  operation rounded by an exact rational model of IEEE 754.
- `kernel-test.manifest.json`: kernel manifest (KRN.MAN.MANI.SCHEMA v1 shape) whose
  `wasm32-wasip1` entry is the SHA-256 of `kernel-test.wasm`.
- `circle-100.bin`: 100 points on a circle (little-endian f64 x, y, z).
- `expect.json` `hostile`: the hostile-module set. Each entry names the module, its stdin, limit
  overrides and the hand-labelled outcome from the spec's rules (REQ-001..REQ-006):
  - `loop.wasm`: infinite loop; `sandbox.budget_exhausted` by fuel and by wall clock.
  - `grow.wasm`: unbounded `memory.grow`; `sandbox.memory_limit`.
  - `big-memory.wasm`: 4 GiB initial memory; `sandbox.memory_limit` under the default 1 GiB cap
    (KRN.MAN.WASM `max_memory_size`) or any cap below 4 GiB, and under a 4 GiB cap it writes the
    last byte of memory without allocating the untouched pages, then exits without an envelope.
  - `imports-*.wasm`: file, randomness, clock, socket, environment, memory and wrong-signature
    imports; `sandbox.forbidden_import` at load.
  - `clock-trap.wasm`: calls the sealed `fid_trap` clock; `sandbox.trap` `host_trap`.
  - `recurse.wasm`: unbounded recursion; `sandbox.trap` `call_stack_exhausted`.
  - `traps.wasm`: one trap kind per stdin digit.
  - `exits.wasm`: stderr output, exit code 7, empty stdout and an output flood.
  - `probe.wasm`: reads the environment and fds 3 and 5; the envelope must report an empty
    environment and `EBADF`.
  - `truncated.wasm`, `simd.wasm`: malformed or refused-feature modules.
- `ops.wasm`, `ops-expect.txt`: numeric conformance cases (one exported function `cN` each) with
  expected result bits or trap kinds from the exact integer and IEEE 754 model in
  `make_fixtures.py` (round to nearest even, subnormals, NaN canonicalisation of arithmetic results).
"""


def main():
    check = "--check" in sys.argv[1:]
    files = build_all()
    if check:
        ok = True
        for path, expected in sorted(files.items()):
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
            elif path.read_bytes() != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    for path, content in files.items():
        path.write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
