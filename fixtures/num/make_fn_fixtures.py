#!/usr/bin/env python3
"""Independent reference for KRN.NUM.FN (CC-BY-4.0 project data).

Writes
  fixtures/num/fn-selftest-v1.txt  generator definition + golden SHA-256 per function
  fixtures/num/fn-oracle-v1.txt    high-precision oracle for every self-test input

Two independent parts, neither of which runs the Rust code:

1. A bit-exact Python model of the algorithms in crates/fid-num/src/f/vendor/
   (fdlibm / musl derived, plain IEEE-754 binary64 + and *, no FMA; CPython floats are
   binary64 with round-to-nearest-even). Its outputs give the golden digests, so a match
   in CI shows the Rust code computes exactly the documented algorithm on every target.
   The large-argument trig reduction is modelled with Python big integers.
2. An oracle computed with the `decimal` module at 90 significant digits (about 299 bits,
   above the 200 bits the spec asks for); pi comes from Machin's formula in exact integer
   arithmetic. It knows nothing about the algorithms. Each line gives the correctly rounded
   double r, the exact offset d = (v - r) / ulp(v) and the ulp exponent E of v.

Run:  python fixtures/num/make_fn_fixtures.py          (writes both fixtures, ~2-4 min)
      python fixtures/num/make_fn_fixtures.py --consts (prints the Rust 2/pi tables)
"""

import hashlib
import math
import struct
import sys
from decimal import Decimal, getcontext, localcontext
from pathlib import Path

MASK64 = (1 << 64) - 1
MASK32 = 0xFFFFFFFF


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def fb(u):
    return struct.unpack("<d", struct.pack("<Q", u & MASK64))[0]


def hi_word(x):
    return bits(x) >> 32


def lo_word(x):
    return bits(x) & MASK32


def with_lo_zero(x):
    return fb(bits(x) & 0xFFFFFFFF00000000)


def pow2(k):
    assert -1022 <= k <= 1023
    return fb((k + 1023) << 52)


# ---------------------------------------------------------------------------------------
# pi and 2/pi in exact integer arithmetic (Machin: pi/4 = 4 atan(1/5) - atan(1/239)).
# ---------------------------------------------------------------------------------------
def _atan_inv(n, one):
    total, term, k, sign = 0, one // n, 1, 1
    n2 = n * n
    while term:
        total += sign * (term // k)
        term //= n2
        k += 2
        sign = -sign
    return total


GUARD = 64
PI_BITS = 1600
_ONE = 1 << (PI_BITS + GUARD)
PI_FIX = 4 * (4 * _atan_inv(5, _ONE) - _atan_inv(239, _ONE))  # pi * 2^(PI_BITS+GUARD)
TWO_OVER_PI_INT = ((2 << (PI_BITS + GUARD)) << 1280) // PI_FIX  # floor(2/pi * 2^1280) (guard ok)
PIO2_FIX = PI_FIX >> (PI_BITS + GUARD + 1 - 127)  # floor(pi/2 * 2^127)
assert PIO2_FIX >> 127 == 1
TWO_OVER_PI_WORDS = [(TWO_OVER_PI_INT >> (1280 - 32 * (i + 1))) & MASK32 for i in range(40)]
assert TWO_OVER_PI_WORDS[0] == 0xA2F9836E  # 2/pi = 0.A2F9836E4E44... (hex)


# ---------------------------------------------------------------------------------------
# Part 1: bit-exact model of the vendored algorithms.
# ---------------------------------------------------------------------------------------
FRAC_PI_2 = fb(0x3FF921FB54442D18)
FRAC_PI_4 = fb(0x3FE921FB54442D18)
PI = fb(0x400921FB54442D18)
FRAC_2_PI = fb(0x3FE45F306DC9C883)
LOG2_E = fb(0x3FF71547652B82FE)

# Every constant: (hex bits, decimal as published in fdlibm comments). Checked below.
C = {
    "pio2_1": (0x3FF921FB54400000, "1.57079632673412561417e+00"),
    "pio2_1t": (0x3DD0B4611A626331, "6.07710050650619224932e-11"),
    "pio2_2": (0x3DD0B4611A600000, "6.07710050630396597660e-11"),
    "pio2_2t": (0x3BA3198A2E037073, "2.02226624879595063154e-21"),
    "pio2_3": (0x3BA3198A2E000000, "2.02226624871116645580e-21"),
    "pio2_3t": (0x397B839A252049C1, "8.47842766036889956997e-32"),
    "S1": (0xBFC5555555555549, "-1.66666666666666324348e-01"),
    "S2": (0x3F8111111110F8A6, "8.33333333332248946124e-03"),
    "S3": (0xBF2A01A019C161D5, "-1.98412698298579493134e-04"),
    "S4": (0x3EC71DE357B1FE7D, "2.75573137070700676789e-06"),
    "S5": (0xBE5AE5E68A2B9CEB, "-2.50507602534068634195e-08"),
    "S6": (0x3DE5D93A5ACFD57C, "1.58969099521155010221e-10"),
    "C1": (0x3FA555555555554C, "4.16666666666666019037e-02"),
    "C2": (0xBF56C16C16C15177, "-1.38888888888741095749e-03"),
    "C3": (0x3EFA01A019CB1590, "2.48015872894767294178e-05"),
    "C4": (0xBE927E4F809C52AD, "-2.75573143513906633035e-07"),
    "C5": (0x3E21EE9EBDB4B1C4, "2.08757232129817482790e-09"),
    "C6": (0xBDA8FAE9BE8838D4, "-1.13596475577881948265e-11"),
    "T0": (0x3FD5555555555563, "3.33333333333334091986e-01"),
    "T1": (0x3FC111111110FE7A, "1.33333333333201242699e-01"),
    "T2": (0x3FABA1BA1BB341FE, "5.39682539762260521377e-02"),
    "T3": (0x3F9664F48406D637, "2.18694882948595424599e-02"),
    "T4": (0x3F8226E3E96E8493, "8.86323982359930005737e-03"),
    "T5": (0x3F6D6D22C9560328, "3.59207910759131235356e-03"),
    "T6": (0x3F57DBC8FEE08315, "1.45620945432529025516e-03"),
    "T7": (0x3F4344D8F2F26501, "5.88041240820264096874e-04"),
    "T8": (0x3F3026F71A8D1068, "2.46463134818469906812e-04"),
    "T9": (0x3F147E88A03792A6, "7.81794442939557092300e-05"),
    "T10": (0x3F12B80F32F0A7E9, "7.14072491382608190305e-05"),
    "T11": (0xBEF375CBDB605373, "-1.85586374855275456654e-05"),
    "T12": (0x3EFB2A7074BF7AD4, "2.59073051863633712884e-05"),
    "pio4lo": (0x3C81A62633145C07, "3.06161699786838301793e-17"),
    "pio2_lo": (0x3C91A62633145C07, "6.12323399573676603587e-17"),
    "pS0": (0x3FC5555555555555, "1.66666666666666657415e-01"),
    "pS1": (0xBFD4D61203EB6F7D, "-3.25565818622400915405e-01"),
    "pS2": (0x3FC9C1550E884455, "2.01212532134862925881e-01"),
    "pS3": (0xBFA48228B5688F3B, "-4.00555345006794114027e-02"),
    "pS4": (0x3F49EFE07501B288, "7.91534994289814532176e-04"),
    "pS5": (0x3F023DE10DFDF709, "3.47933107596021167570e-05"),
    "qS1": (0xC0033A271C8A2D4B, "-2.40339491173441421878e+00"),
    "qS2": (0x40002AE59C598AC8, "2.02094576023350569471e+00"),
    "qS3": (0xBFE6066C1B8D0159, "-6.88283971605453293030e-01"),
    "qS4": (0x3FB3B8C5B12E9282, "7.70381505559019352791e-02"),
    "atanhi0": (0x3FDDAC670561BB4F, "4.63647609000806093515e-01"),
    "atanhi2": (0x3FEF730BD281F69B, "9.82793723247329054082e-01"),
    "atanlo0": (0x3C7A2B7F222F65E2, "2.26987774529616870924e-17"),
    "atanlo1": (0x3C81A62633145C07, "3.06161699786838301793e-17"),
    "atanlo2": (0x3C7007887AF0CBBD, "1.39033110312309984516e-17"),
    "atanlo3": (0x3C91A62633145C07, "6.12323399573676603587e-17"),
    "aT0": (0x3FD555555555550D, "3.33333333333329318027e-01"),
    "aT1": (0xBFC999999998EBC4, "-1.99999999998764832476e-01"),
    "aT2": (0x3FC24924920083FF, "1.42857142725034663711e-01"),
    "aT3": (0xBFBC71C6FE231671, "-1.11111104054623557880e-01"),
    "aT4": (0x3FB745CDC54C206E, "9.09088713343650656196e-02"),
    "aT5": (0xBFB3B0F2AF749A6D, "-7.69187620504482999495e-02"),
    "aT6": (0x3FB10D66A0D03D51, "6.66107313738753120669e-02"),
    "aT7": (0xBFADDE2D52DEFD9A, "-5.83357013379057348645e-02"),
    "aT8": (0x3FA97B4B24760DEB, "4.97687799461593236017e-02"),
    "aT9": (0xBFA2B4442C6A6C2F, "-3.65315727442169155270e-02"),
    "aT10": (0x3F90AD3AE322DA11, "1.62858201153657823623e-02"),
    "pi_lo": (0x3CA1A62633145C07, "1.2246467991473531772E-16"),
    "ln2hi": (0x3FE62E42FEE00000, "6.93147180369123816490e-01"),
    "ln2lo": (0x3DEA39EF35793C76, "1.90821492927058770002e-10"),
    "P1": (0x3FC555555555553E, "1.66666666666666019037e-01"),
    "P2": (0xBF66C16C16BEBD93, "-2.77777777770155933842e-03"),
    "P3": (0x3F11566AAF25DE2C, "6.61375632143793436117e-05"),
    "P4": (0xBEBBBD41C5D26BF1, "-1.65339022054652515390e-06"),
    "P5": (0x3E66376972BEA4D0, "4.13813679705723846039e-08"),
    "exp_hi": (0x40862E42FEFA39EF, "709.782712893383973096"),
    "Lg1": (0x3FE5555555555593, "6.666666666666735130e-01"),
    "Lg2": (0x3FD999999997FA04, "3.999999999940941908e-01"),
    "Lg3": (0x3FD2492494229359, "2.857142874366239149e-01"),
    "Lg4": (0x3FCC71C51D8E78AF, "2.222219843214978396e-01"),
    "Lg5": (0x3FC7466496CB03DE, "1.818357216161805012e-01"),
    "Lg6": (0x3FC39A09D078C69F, "1.531383769920937332e-01"),
    "Lg7": (0x3FC2F112DF3E5244, "1.479819860511658591e-01"),
    "CP0": (0x3FFE03E60F61E692, "1.87595182427177009643"),
    "CP1": (0xBFFE28E092F02420, "-1.88497979543377169875"),
    "CP2": (0x3FF9F1604A49D6C2, "1.621429720105354466140"),
    "CP3": (0xBFE844CBBEE751D9, "-0.758397934778766047437"),
    "CP4": (0x3FC2B000D4E4EDD7, "0.145996192886612446982"),
}
for _name, (_h, _dec) in C.items():
    assert bits(float(_dec)) == _h, (_name, hex(bits(float(_dec))), hex(_h))
K = {name: fb(h) for name, (h, _d) in C.items()}
TOINT = 1.5 / 2.220446049250313e-16  # 1.5 * 2^52


def rem_pio2_large(x):
    """|x| >= 2^20 * pi/2: Payne-Hanek with big integers (model of vendor/rem_pio2.rs)."""
    u = bits(x)
    m = (u & ((1 << 52) - 1)) | (1 << 52)
    e = ((u >> 52) & 0x7FF) - 1075
    s = e + 254
    g = (TWO_OVER_PI_INT >> (1280 - s)) & ((1 << 256) - 1)
    p = (m * g) & ((1 << 256) - 1)
    n = p >> 254
    f = p & ((1 << 254) - 1)
    neg = False
    if f >> 253:
        n += 1
        f = (1 << 254) - f
        neg = True
    n &= 3
    if f == 0:
        return (n if u >> 63 == 0 else (-n) & 3), 0.0, 0.0
    lead = f.bit_length() - 1
    mm = f >> (lead - 127) if lead >= 127 else f << (127 - lead)
    q = mm * PIO2_FIX
    if q >> 255:
        h, ex = q >> 128, lead - 508 + 128
    else:
        h, ex = q >> 127, lead - 508 + 127
    t, rest = h >> 75, h & ((1 << 75) - 1)
    if rest > (1 << 74) or (rest == (1 << 74) and t & 1):
        t += 1
    r = h - (t << 75)
    y0 = float(t) * pow2(ex + 75)
    y1 = float(r) * pow2(ex)
    if neg:
        y0, y1 = -y0, -y1
    if u >> 63:
        return (-n) & 3, -y0, -y1
    return n, y0, y1


def rem_pio2(x):
    ix = hi_word(x) & 0x7FFFFFFF
    if ix >= 0x413921FB:
        return rem_pio2_large(x)
    fn = (x * FRAC_2_PI + TOINT) - TOINT
    n = int(fn)
    r = x - fn * K["pio2_1"]
    w = fn * K["pio2_1t"]
    if r - w < -FRAC_PI_4:
        n -= 1
        fn -= 1.0
        r = x - fn * K["pio2_1"]
        w = fn * K["pio2_1t"]
    elif r - w > FRAC_PI_4:
        n += 1
        fn += 1.0
        r = x - fn * K["pio2_1"]
        w = fn * K["pio2_1t"]
    y0 = r - w
    ey = (bits(y0) >> 52) & 0x7FF
    ex = ix >> 20
    if ex - ey > 16:
        t = r
        w = fn * K["pio2_2"]
        r = t - w
        w = fn * K["pio2_2t"] - ((t - r) - w)
        y0 = r - w
        ey = (bits(y0) >> 52) & 0x7FF
        if ex - ey > 49:
            t = r
            w = fn * K["pio2_3"]
            r = t - w
            w = fn * K["pio2_3t"] - ((t - r) - w)
            y0 = r - w
    y1 = (r - y0) - w
    return n & 3, y0, y1


def k_sin(x, y, iy):
    z = x * x
    w = z * z
    r = K["S2"] + z * (K["S3"] + z * K["S4"]) + z * w * (K["S5"] + z * K["S6"])
    v = z * x
    if iy == 0:
        return x + v * (K["S1"] + z * r)
    return x - ((z * (0.5 * y - v * r) - y) - v * K["S1"])


def k_cos(x, y):
    z = x * x
    w = z * z
    r = z * (K["C1"] + z * (K["C2"] + z * K["C3"])) + w * w * (K["C4"] + z * (K["C5"] + z * K["C6"]))
    hz = 0.5 * z
    w = 1.0 - hz
    return w + (((1.0 - w) - hz) + (z * r - x * y))


def k_tan(x, y, odd):
    T = [K["T%d" % i] for i in range(13)]
    hx = hi_word(x)
    big = (hx & 0x7FFFFFFF) >= 0x3FE59428
    sign = hx >> 31
    if big:
        if sign:
            x, y = -x, -y
        x = (FRAC_PI_4 - x) + (K["pio4lo"] - y)
        y = 0.0
    z = x * x
    w = z * z
    r = T[1] + w * (T[3] + w * (T[5] + w * (T[7] + w * (T[9] + w * T[11]))))
    v = z * (T[2] + w * (T[4] + w * (T[6] + w * (T[8] + w * (T[10] + w * T[12])))))
    s = z * x
    r = y + z * (s * (r + v) + y) + s * T[0]
    w = x + r
    if big:
        s = float(1 - 2 * odd)
        v = s - 2.0 * (x + (r - w * w / (w + s)))
        return -v if sign else v
    if not odd:
        return w
    w0 = with_lo_zero(w)
    v = r - (w0 - x)
    a = -1.0 / w
    a0 = with_lo_zero(a)
    return a0 + a * (1.0 + a0 * w0 + a0 * v)


def m_sin(x):
    ix = hi_word(x) & 0x7FFFFFFF
    if ix <= 0x3FE921FB:
        if ix < 0x3E500000:
            return x
        return k_sin(x, 0.0, 0)
    n, y0, y1 = rem_pio2(x)
    return [k_sin(y0, y1, 1), k_cos(y0, y1), -k_sin(y0, y1, 1), -k_cos(y0, y1)][n]


def m_cos(x):
    ix = hi_word(x) & 0x7FFFFFFF
    if ix <= 0x3FE921FB:
        if ix < 0x3E46A09E:
            return 1.0
        return k_cos(x, 0.0)
    n, y0, y1 = rem_pio2(x)
    return [k_cos(y0, y1), -k_sin(y0, y1, 1), -k_cos(y0, y1), k_sin(y0, y1, 1)][n]


def m_tan(x):
    ix = hi_word(x) & 0x7FFFFFFF
    if ix <= 0x3FE921FB:
        if ix < 0x3E400000:
            return x
        return k_tan(x, 0.0, 0)
    n, y0, y1 = rem_pio2(x)
    return k_tan(y0, y1, n & 1)


def big_r(z):
    p = z * (K["pS0"] + z * (K["pS1"] + z * (K["pS2"] + z * (K["pS3"] + z * (K["pS4"] + z * K["pS5"])))))
    q = 1.0 + z * (K["qS1"] + z * (K["qS2"] + z * (K["qS3"] + z * K["qS4"])))
    return p / q


def m_asin(x):
    hx = hi_word(x)
    ix = hx & 0x7FFFFFFF
    if ix >= 0x3FF00000:  # caller guarantees |x| <= 1, so |x| == 1
        return -FRAC_PI_2 if hx >> 31 else FRAC_PI_2
    if ix < 0x3FE00000:
        if ix < 0x3E500000:
            return x
        return x + x * big_r(x * x)
    z = (1.0 - abs(x)) * 0.5
    s = math.sqrt(z)
    r = big_r(z)
    if ix >= 0x3FEF3333:
        x = FRAC_PI_2 - (2.0 * (s + s * r) - K["pio2_lo"])
    else:
        f = with_lo_zero(s)
        c = (z - f * f) / (s + f)
        x = 0.5 * FRAC_PI_2 - (2.0 * s * r - (K["pio2_lo"] - 2.0 * c) - (0.5 * FRAC_PI_2 - 2.0 * f))
    return -x if hx >> 31 else x


def m_acos(x):
    hx = hi_word(x)
    ix = hx & 0x7FFFFFFF
    if ix >= 0x3FF00000:
        return PI if hx >> 31 else 0.0
    if ix < 0x3FE00000:
        if ix <= 0x3C600000:
            return FRAC_PI_2
        return FRAC_PI_2 - (x - (K["pio2_lo"] - x * big_r(x * x)))
    if hx >> 31:
        z = (1.0 + x) * 0.5
        s = math.sqrt(z)
        w = big_r(z) * s - K["pio2_lo"]
        return 2.0 * (FRAC_PI_2 - (s + w))
    z = (1.0 - x) * 0.5
    s = math.sqrt(z)
    df = with_lo_zero(s)
    c = (z - df * df) / (s + df)
    w = big_r(z) * s + c
    return 2.0 * (df + w)


ATANHI = [K["atanhi0"], FRAC_PI_4, K["atanhi2"], FRAC_PI_2]
ATANLO = [K["atanlo0"], K["atanlo1"], K["atanlo2"], K["atanlo3"]]


def atan_parts(ax, c):
    """musl atan on ax >= 0 as hi + lo (hi + lo rounds to musl's result when c = 0).
    c is a small correction added before the final rounding (atan2 only)."""
    ix = hi_word(ax)
    aT = [K["aT%d" % i] for i in range(11)]
    x = ax
    if ix >= 0x44100000:
        return FRAC_PI_2, 0.0
    if ix < 0x3FDC0000:
        if ix < 0x3E400000:
            return x, 0.0
        idx = -1
    elif ix < 0x3FF30000:
        if ix < 0x3FE60000:
            idx = 0
            x = (2.0 * x - 1.0) / (2.0 + x)
        else:
            idx = 1
            x = (x - 1.0) / (x + 1.0)
    elif ix < 0x40038000:
        idx = 2
        x = (x - 1.5) / (1.0 + 1.5 * x)
    else:
        idx = 3
        x = -1.0 / x
    z = x * x
    w = z * z
    s1 = z * (aT[0] + w * (aT[2] + w * (aT[4] + w * (aT[6] + w * (aT[8] + w * aT[10])))))
    s2 = w * (aT[1] + w * (aT[3] + w * (aT[5] + w * (aT[7] + w * aT[9]))))
    if idx < 0:
        return x, -(x * (s1 + s2) - c)
    return ATANHI[idx], -(((x * (s1 + s2) - ATANLO[idx]) - x) - c)


def m_atan(x):
    h, lo = atan_parts(abs(x), 0.0)
    z = h + lo
    return -z if hi_word(x) >> 31 else z


def two_prod(a, b):
    """Dekker: a*b = p + e exactly (no FMA); |a|, |b| < 2^995."""
    ca = a * SPLIT
    ah = ca - (ca - a)
    al = a - ah
    cb = b * SPLIT
    bh = cb - (cb - b)
    bl = b - bh
    p = a * b
    e = (((ah * bh - p) + ah * bl) + al * bh) + al * bl
    return p, e


def div_corr(ay, ax, t):
    """(ay/ax - t) / (1 + t^2) for t = RN(ay/ax) in [2^-27, 2^27], else 0 (project addition:
    removes the division rounding that takes musl atan2 above 1 ulp)."""
    if not (pow2(-27) <= t <= pow2(27)):
        return 0.0
    if ax < pow2(-1022):
        ax *= pow2(54)
        ay *= pow2(54)
    k = (bits(ax) >> 52) - 1023
    h1 = (-k) >> 1
    h2 = -k - h1
    sx = (ax * pow2(h1)) * pow2(h2)
    sy = (ay * pow2(h1)) * pow2(h2)
    p, e = two_prod(t, sx)
    r = (sy - p) - e
    return (r / sx) / (1.0 + t * t)


def m_atan2(y, x):
    ux, uy = bits(x), bits(y)
    ix, lx = ux >> 32, ux & MASK32
    iy, ly = uy >> 32, uy & MASK32
    if ((ix - 0x3FF00000) & MASK32) | lx == 0:
        return m_atan(y)
    m = ((iy >> 31) & 1) | ((ix >> 30) & 2)
    ix &= 0x7FFFFFFF
    iy &= 0x7FFFFFFF
    if iy | ly == 0:
        return [y, y, PI, -PI][m]
    if ix | lx == 0:
        return -FRAC_PI_2 if m & 1 else FRAC_PI_2
    if ix + (64 << 20) < iy:
        return -FRAC_PI_2 if m & 1 else FRAC_PI_2
    if (m & 2) and iy + (64 << 20) < ix:
        h, lo = 0.0, 0.0
    else:
        t = abs(y / x)
        h, lo = atan_parts(t, div_corr(abs(y), abs(x), t))
    if m < 2:
        z = h + lo
        return -z if m == 1 else z
    # pi - (h + lo) with one final rounding (project change; musl rounds z first).
    s = PI - h
    err = (PI - s) - h
    z = s + ((err + K["pi_lo"]) - lo)
    return -z if m == 3 else z


def scalbn(y, n):
    if n > 1023:
        y *= pow2(1023)
        n -= 1023
        if n > 1023:
            y *= pow2(1023)
            n -= 1023
            if n > 1023:
                n = 1023
    elif n < -1022:
        y *= pow2(-1022) * pow2(53)
        n += 1022 - 53
        if n < -1022:
            y *= pow2(-1022) * pow2(53)
            n += 1022 - 53
            if n < -1022:
                n = -1022
    return y * pow2(n)


def m_exp(x):
    hx = hi_word(x)
    sign = hx >> 31
    hx &= 0x7FFFFFFF
    if hx >= 0x4086232B:
        if x > K["exp_hi"]:
            return math.inf
        if x < -745.13321910194110842:
            return 0.0
    if hx > 0x3FD62E42:
        if hx >= 0x3FF0A2B2:
            k = int(LOG2_E * x + (-0.5 if sign else 0.5))
        else:
            k = 1 - sign - sign
        hi = x - float(k) * K["ln2hi"]
        lo = float(k) * K["ln2lo"]
        x = hi - lo
    elif hx > 0x3E300000:
        k = 0
        hi = x
        lo = 0.0
    else:
        return 1.0 + x
    xx = x * x
    c = x - xx * (K["P1"] + xx * (K["P2"] + xx * (K["P3"] + xx * (K["P4"] + xx * K["P5"]))))
    y = 1.0 + (((x * c) / (2.0 - c) - lo) + hi)
    if k == 0:
        return y
    return scalbn(y, k)


def m_log(x):
    u = bits(x)
    hx = u >> 32
    k = 0
    if hx < 0x00100000:  # caller guarantees x > 0: subnormal
        k -= 54
        x *= pow2(54)
        u = bits(x)
        hx = u >> 32
    elif hx == 0x3FF00000 and (u & MASK32) == 0:
        return 0.0
    hx = (hx + 0x3FF00000 - 0x3FE6A09E) & MASK32
    k += (hx >> 20) - 0x3FF
    hx = (hx & 0x000FFFFF) + 0x3FE6A09E
    x = fb((hx << 32) | (u & MASK32))
    f = x - 1.0
    hfsq = 0.5 * f * f
    s = f / (2.0 + f)
    z = s * s
    w = z * z
    t1 = w * (K["Lg2"] + w * (K["Lg4"] + w * K["Lg6"]))
    t2 = z * (K["Lg1"] + w * (K["Lg3"] + w * (K["Lg5"] + w * K["Lg7"])))
    r = t2 + t1
    dk = float(k)
    return (((s * (hfsq + r) + dk * K["ln2lo"]) - hfsq) + f) + dk * K["ln2hi"]


SPLIT = 134217729.0  # 2^27 + 1


def sq(x):
    xc = x * SPLIT
    xh = (x - xc) + xc
    xl = x - xh
    hi = x * x
    lo = ((xh * xh - hi) + 2.0 * xh * xl) + xl * xl
    return hi, lo


def m_hypot(x, y):
    ux, uy = bits(x) & (MASK64 >> 1), bits(y) & (MASK64 >> 1)
    if ux < uy:
        ux, uy = uy, ux
    ex, ey = ux >> 52, uy >> 52
    x, y = fb(ux), fb(uy)
    if uy == 0:
        return x
    if ex - ey > 64:
        return x + y
    z = 1.0
    if ex > 0x3FF + 510:
        z = pow2(700)
        x *= pow2(-700)
        y *= pow2(-700)
    elif ey < 0x3FF - 450:
        z = pow2(-700)
        x *= pow2(700)
        y *= pow2(700)
    hx, lx = sq(x)
    hy, ly = sq(y)
    return z * math.sqrt(((ly + lx) + hy) + hx)


B1, B2 = 715094163, 696219795


def m_cbrt(x):
    u = bits(x)
    hx = (u >> 32) & 0x7FFFFFFF
    if hx < 0x00100000:
        u = bits(x * pow2(54))
        hx = (u >> 32) & 0x7FFFFFFF
        if hx == 0:
            return x
        hx = hx // 3 + B2
    else:
        hx = hx // 3 + B1
    u = (bits(x) & (1 << 63)) | (hx << 32)
    t = fb(u)
    r = (t * t) * (t / x)
    t = t * ((K["CP0"] + r * (K["CP1"] + r * K["CP2"])) + ((r * r) * r) * (K["CP3"] + r * K["CP4"]))
    t = fb((bits(t) + 0x80000000) & 0xFFFFFFFFC0000000)
    s = t * t
    r = x / s
    w = t + t
    r = (r - t) / (w + r)
    return t + t * r


# ---------------------------------------------------------------------------------------
# Self-test input generator (the "generator definition" of fn-selftest-v1).
# ---------------------------------------------------------------------------------------
NAMES = ["sqrt", "sin", "cos", "tan", "asin", "acos", "atan", "atan2", "exp", "ln", "hypot", "cbrt"]
N_PER_FN = 10_000
SEED_BASE = 0x5EED_F00D_0000_0000


def splitmix64(state):
    """Yields successive splitmix64 outputs (Steele, Lea, Flood 2014)."""
    while True:
        state = (state + 0x9E3779B97F4A7C15) & MASK64
        z = state
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
        yield z ^ (z >> 31)


def from_draw(r, lo, hi, signed):
    """sign (bit 63 if signed) | biased exponent lo + (bits 52..62 mod (hi-lo+1)) | bits 0..51."""
    e = lo + ((r >> 52) & 0x7FF) % (hi - lo + 1)
    s = (r >> 63) if signed else 0
    return fb((s << 63) | (e << 52) | (r & ((1 << 52) - 1)))


B = 1023  # biased exponent of 1.0
# name -> (arity, list of (lo, hi, signed) cycled by index, accept-predicate or None)
DOMAINS = {
    "sqrt": [(1, 2046, False)],
    "sin": [(B - 30, B + 21, True)] * 3 + [(1, 2046, True)],
    "cos": [(B - 30, B + 21, True)] * 3 + [(1, 2046, True)],
    "tan": [(B - 30, B + 21, True)] * 3 + [(1, 2046, True)],
    "asin": [(B - 40, B - 1, True)],
    "acos": [(B - 40, B - 1, True)],
    "atan": [(B - 40, B + 40, True)],
    "atan2": [(B - 40, B + 40, True)],
    "exp": [(B - 40, B + 9, True)],
    "ln": [(1, 2046, False)],
    "hypot": [(B - 500, B + 500, True)],
    "cbrt": [(1, 2046, True)],
}
BINARY = {"atan2", "hypot"}

# Edge inputs come first, as hex bit patterns (binary functions: (y or x, second) pairs).
TRIG_EDGES = [
    0x0000000000000000, 0x8000000000000000, 0x0000000000000001, 0x0010000000000000,
    0x3E40000000000000, 0x3E46A09E00000000, 0x3E50000000000000, 0x3FE921FB54442D18,
    0xBFE921FB54442D18, 0x3FE921FC00000000, 0x3FF921FB54442D18, 0xBFF921FB54442D18,
    0x400921FB54442D18, 0xC00921FB54442D18, 0x4012D97C7F3321D2, 0x401921FB54442D18,
    0x402F6A7A2955385E, 0x413921FB54442D18, 0x413921FA00000000, 0x413921FC00000000,
    0x4415AF1D78B58C40, 0x7FEFFFFFFFFFFFFF, 0xFFEFFFFFFFFFFFFF, 0x7E37E43C8800759C,
    0x3FF0000000000000, 0x4024000000000000, 0x4480F0CF064DD592, 0x7506AC5B262CA1FF,
    0xF506AC5B262CA1FF,
]
EDGES = {
    "sqrt": [0x0, 0x8000000000000000, 0x1, 0x0010000000000000, 0x7FEFFFFFFFFFFFFF,
             0x3FF0000000000000, 0x4000000000000000, 0x4010000000000000, 0x3FEFFFFFFFFFFFFF],
    "sin": TRIG_EDGES,
    "cos": TRIG_EDGES,
    "tan": TRIG_EDGES,
    "asin": [0x0, 0x8000000000000000, 0x1, 0x3E50000000000000, 0x3FE0000000000000, 0xBFE0000000000000,
             0x3FEF333333333333, 0x3FEF333400000000, 0x3FF0000000000000, 0xBFF0000000000000,
             0x3FEFFFFFFFFFFFFF, 0xBFEFFFFFFFFFFFFF],
    "acos": [0x0, 0x8000000000000000, 0x1, 0x3C60000000000000, 0x3C70000000000000, 0x3FE0000000000000,
             0xBFE0000000000000, 0x3FF0000000000000, 0xBFF0000000000000, 0x3FEFFFFFFFFFFFFF,
             0xBFEFFFFFFFFFFFFF],
    "atan": [0x0, 0x8000000000000000, 0x1, 0x3E40000000000000, 0x3FDC000000000000, 0x3FE6000000000000,
             0x3FF3000000000000, 0x4003800000000000, 0x3FF0000000000000, 0xBFF0000000000000,
             0x4410000000000000, 0xC410000000000000, 0x7FEFFFFFFFFFFFFF, 0xFFEFFFFFFFFFFFFF],
    "atan2": [(0x0, 0x3FF0000000000000), (0x8000000000000000, 0x3FF0000000000000),
              (0x0, 0xBFF0000000000000), (0x8000000000000000, 0xBFF0000000000000),
              (0x3FF0000000000000, 0x0), (0xBFF0000000000000, 0x0),
              (0x3FF0000000000000, 0x3FF0000000000000), (0x3FF0000000000000, 0xBFF0000000000000),
              (0xBFF0000000000000, 0xBFF0000000000000), (0x7E37E43C8800759C, 0x01A56E1FC2F8F359),
              (0x01A56E1FC2F8F359, 0xFE37E43C8800759C), (0x7FEFFFFFFFFFFFFF, 0x7FEFFFFFFFFFFFFF),
              (0x0000000000000001, 0x0000000000000001), (0x3FF0000000000000, 0x3B90000000000000),
              (0x3B90000000000000, 0xBFF0000000000000), (0x4000000000000000, 0x3FF0000000000001)],
    "exp": [0x0, 0x8000000000000000, 0x1, 0x3E30000000000000, 0x3FD62E42FEFA39EF, 0x3FF0A2B23F3BAB73,
            0x3FF0000000000000, 0xBFF0000000000000, 0x4086200000000000, 0xC086200000000000,
            0x40862E42FEFA39EF, 0x4085E00000000000, 0xC085E00000000000],
    "ln": [0x3FF0000000000000, 0x4000000000000000, 0x3FE0000000000000, 0x1, 0x0010000000000000,
           0x7FEFFFFFFFFFFFFF, 0x4005BF0A8B145769, 0x3FF0000000000001, 0x3FEFFFFFFFFFFFFF,
           0x4024000000000000, 0x3FE6A09E667F3BCD],
    "hypot": [(0x0, 0x0), (0x0, 0x8000000000000000), (0x4008000000000000, 0x4010000000000000),
              (0xC008000000000000, 0x4010000000000000), (0x7FEFFFFFFFFFFFFF, 0x0),
              (0x7FE1CCF385EBC8A0, 0x7FAC7B1F3CAC7433), (0x1, 0x1), (0x3FF0000000000000, 0x3B90000000000000),
              (0x01A56E1FC2F8F359, 0x01A56E1FC2F8F359), (0x7FDFFFFFFFFFFFFF, 0x7FDFFFFFFFFFFFFF)],
    "cbrt": [0x0, 0x8000000000000000, 0x1, 0x8000000000000001, 0x0010000000000000, 0x7FEFFFFFFFFFFFFF,
             0xFFEFFFFFFFFFFFFF, 0x403B000000000000, 0xC020000000000000, 0x01A56E1FC2F8F359,
             0x4000000000000000],
}


def selftest_inputs(name, idx):
    """Edge inputs, then splitmix64 draws from seed SEED_BASE | idx until N_PER_FN inputs."""
    out = []
    for e in EDGES[name]:
        out.append((fb(e[0]), fb(e[1])) if name in BINARY else (fb(e), 0.0))
    gen = splitmix64(SEED_BASE | idx)
    doms = DOMAINS[name]
    i = len(out)
    while len(out) < N_PER_FN:
        lo, hi, signed = doms[i % len(doms)]
        a = from_draw(next(gen), lo, hi, signed)
        b = from_draw(next(gen), lo, hi, signed) if name in BINARY else 0.0
        if name == "exp" and abs(a) > 708.0:
            continue  # rejection: keep exp(x) normal and finite
        out.append((a, b))
        i += 1
    return out


MODEL = {
    "sqrt": lambda a, b: math.sqrt(a), "sin": lambda a, b: m_sin(a), "cos": lambda a, b: m_cos(a),
    "tan": lambda a, b: m_tan(a), "asin": lambda a, b: m_asin(a), "acos": lambda a, b: m_acos(a),
    "atan": lambda a, b: m_atan(a), "atan2": m_atan2, "exp": lambda a, b: m_exp(a),
    "ln": lambda a, b: m_log(a), "hypot": m_hypot, "cbrt": lambda a, b: m_cbrt(a),
}


# ---------------------------------------------------------------------------------------
# Part 2: decimal oracle (90 digits), independent of the algorithms above.
# ---------------------------------------------------------------------------------------
PREC = 90
getcontext().Emax = 10**6
getcontext().Emin = -(10**6)
with localcontext() as _c:
    _c.prec = 700
    D_PI = Decimal(PI_FIX) / Decimal(2) ** (PI_BITS + GUARD)


def d_sin_cos(r):
    """sin(r), cos(r) for |r| <= 1 by Taylor series."""
    r2 = r * r
    s, c, term_s, term_c, k = Decimal(0), Decimal(0), r, Decimal(1), 0
    eps = Decimal(10) ** (-PREC - 5)
    while abs(term_s) > eps * abs(s) or abs(term_c) > eps or k < 4:
        s += term_s
        c += term_c
        term_s = -term_s * r2 / ((2 * k + 2) * (2 * k + 3))
        term_c = -term_c * r2 / ((2 * k + 1) * (2 * k + 2))
        k += 1
        if term_s == 0 and term_c == 0:
            break
    return s, c


def d_trig(x, which):
    xd = Decimal(x)
    with localcontext() as c:
        c.prec = 700
        half_pi = D_PI / 2
        k = (xd / half_pi).to_integral_value()
        r = xd - k * half_pi
    with localcontext() as c:
        c.prec = PREC + 10
        s, co = d_sin_cos(+r)
        q = int(k) % 4
        sin_x = [s, co, -s, -co][q]
        cos_x = [co, -s, -co, s][q]
        if which == "sin":
            return +sin_x
        if which == "cos":
            return +cos_x
        return sin_x / cos_x


def d_atan(t):
    with localcontext() as c:
        c.prec = PREC + 20
        if t == 0:
            return Decimal(0)
        neg = t < 0
        t = abs(t)
        if t > 1:
            res = D_PI / 2 - d_atan(1 / t)
            return -res if neg else res
        halvings = 0
        while t > Decimal("0.05"):
            t = t / (1 + (1 + t * t).sqrt())
            halvings += 1
        t2 = t * t
        s, term, k = Decimal(0), t, 0
        eps = Decimal(10) ** (-PREC - 15)
        while abs(term) > eps * abs(s) or k == 0:
            s += term / (2 * k + 1)
            term = -term * t2
            k += 1
        res = s * (2**halvings)
        return -res if neg else res


def d_cbrt(x):
    with localcontext() as c:
        c.prec = PREC + 20
        neg = x < 0
        a = abs(x)
        y = Decimal(repr(math.copysign(abs(float(x)) ** (1.0 / 3.0), 1.0)))
        if y == 0:
            y = Decimal(2) ** -358
        for _ in range(12):
            y = y - (y * y * y - a) / (3 * y * y)
        return -y if neg else y


def oracle(name, a, b):
    with localcontext() as c:
        c.prec = PREC
        A, Bd = Decimal(a), Decimal(b)
        if name == "sqrt":
            return A.sqrt() if a != 0 else Decimal(0)
        if name in ("sin", "cos", "tan"):
            if a == 0:
                return Decimal(1) if name == "cos" else Decimal(0)
            return d_trig(a, name)
        if name == "asin":
            if abs(a) == 1:
                return D_PI / 2 * (1 if a > 0 else -1)
            return d_atan(A / (1 - A * A).sqrt()) if a != 0 else Decimal(0)
        if name == "acos":
            if a == 1:
                return Decimal(0)
            if a == -1:
                return +D_PI
            return D_PI / 2 - d_atan(A / (1 - A * A).sqrt())
        if name == "atan":
            return d_atan(A)
        if name == "atan2":
            y, x = A, Bd
            if y == 0:
                if math.copysign(1.0, b) > 0:
                    return Decimal(0)
                return +D_PI if math.copysign(1.0, a) > 0 else -D_PI
            if x == 0:
                return D_PI / 2 if y > 0 else -D_PI / 2
            t = d_atan(y / x)
            if x > 0:
                return t
            return t + D_PI if y > 0 else t - D_PI
        if name == "exp":
            return A.exp()
        if name == "ln":
            return A.ln()
        if name == "hypot":
            return (A * A + Bd * Bd).sqrt()
        if name == "cbrt":
            return d_cbrt(A) if a != 0 else Decimal(0)
    raise ValueError(name)


def ulp_exp(v):
    """E such that ulp(v) = 2^E for the binade containing |v| (subnormal floor 2^-1074)."""
    av = abs(v)
    m, e = math.frexp(float(av))  # float(av) may round up into the next binade
    e -= 1
    if av < Decimal(2) ** e:
        e -= 1
    return max(e, -1022) - 52


def oracle_line(v):
    if v == 0:
        return "z", None
    with localcontext() as c:
        c.prec = PREC
        r = float(v)  # correctly rounded (decimal -> str -> float)
        E = ulp_exp(v)
        d = (v - Decimal(r)) / (Decimal(2) ** E)
    return "%016x %s %d" % (bits(r), format(d, ".6f"), E), (r, d, E)


def ulp_err(y, rde):
    r, d, E = rde
    return abs((Decimal(y) - Decimal(r)) / (Decimal(2) ** E) - d)


def main():
    if "--consts" in sys.argv:
        print("const TWO_OVER_PI: [u32; 40] = [")
        for i in range(0, 40, 6):
            print("    " + ", ".join("0x%04X_%04X" % (w >> 16, w & 0xFFFF) for w in TWO_OVER_PI_WORDS[i:i + 6]) + ",")
        print("];")
        print("const PIO2_FIX: u128 = 0x%s;" % "_".join(
            ("%032X" % PIO2_FIX)[i:i + 4] for i in range(0, 32, 4)))
        return
    here = Path(__file__).parent
    sel = ["# KRN.NUM.FN AC-1: self-test generator definition and golden digests (CC-BY-4.0).",
           "# Generated by make_fn_fixtures.py. Inputs per function: the edge list, then",
           "# splitmix64 draws from seed %#x | fn_index until %d inputs." % (SEED_BASE, N_PER_FN),
           "# A draw r maps to sign (bit 63 if signed) | biased exponent lo + (bits 52..62 mod",
           "# (hi-lo+1)) | mantissa bits 0..51; the (lo, hi, signed) list is cycled by input index.",
           "# Binary functions take two draws per input (first, second). exp rejects |x| > 708.",
           "# inputs = SHA-256 of the input bit patterns (u64 little-endian, a then b, b = 0 for unary);",
           "# outputs = SHA-256 of the output bit patterns (u64 little-endian), in input order.",
           "# maxulp = max |y - f(x)| / ulp(f(x)) of the modelled outputs against fn-oracle-v1.",
           "# name index edges domains inputs outputs maxulp"]
    orc = ["# KRN.NUM.FN AC-2: oracle values at 90 significant decimal digits (CC-BY-4.0).",
           "# Generated by make_fn_fixtures.py. One line per self-test input, functions in index",
           "# order, each preceded by '@ <name>'. Line: r d E, with r the correctly rounded",
           "# double (hex bits), d = (v - r) / 2^E to 6 decimals, 2^E = ulp of the binade of v;",
           "# 'z' means v = 0 exactly."]
    for idx, name in enumerate(NAMES):
        ins = selftest_inputs(name, idx)
        hin, hout = hashlib.sha256(), hashlib.sha256()
        orc.append("@ " + name)
        worst = 0
        for a, b in ins:
            hin.update(struct.pack("<QQ", bits(a), bits(b)))
            y = MODEL[name](a, b)
            assert math.isfinite(y), (name, a, b, y)
            hout.update(struct.pack("<Q", bits(y)))
            line, rde = oracle_line(oracle(name, a, b))
            orc.append(line)
            if rde is None:
                assert y == 0, (name, a, b, y)
                continue
            err = ulp_err(y, rde)
            if err > worst:
                worst = err
        doms = ";".join("%d:%d:%s" % (lo, hi, "s" if s else "u") for lo, hi, s in DOMAINS[name])
        sel.append("%s %d %d %s sha256:%s sha256:%s %.4f" % (
            name, idx, len(EDGES[name]), doms, hin.hexdigest(), hout.hexdigest(), worst))
        print(name, "max ulp", "%.4f" % worst, flush=True)
    (here / "fn-selftest-v1.txt").write_text("\n".join(sel) + "\n", encoding="utf-8", newline="\n")
    (here / "fn-oracle-v1.txt").write_text("\n".join(orc) + "\n", encoding="utf-8", newline="\n")


if __name__ == "__main__":
    main()
