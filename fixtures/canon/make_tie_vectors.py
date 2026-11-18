"""Independent reference vectors for RFC 8785 / ECMAScript number formatting, focused on
exact decimal ties (ECMA-262 Number::toString, Note 2: when two shortest digit strings are
equally close to the double, the even one wins).

Expected strings come from CPython's repr() (shortest round-trip digits, ties to even),
laid out with the ECMAScript thresholds (plain notation for 1e-6 <= |v| < 1e21, otherwise
exponent form "de+x" / "d.ddde-x"). Nothing here runs the Rust code.

Output: tie_vectors.json, a JSON array of ["0x<16 hex digits of the IEEE 754 bits>", "<JCS text>"].
Run: python fixtures/canon/make_tie_vectors.py   (deterministic; seed fixed)
"""

import json
import math
import os
import random
import struct
from fractions import Fraction

SEED = 20261117
N_MIDPOINTS = 2400
N_FLOAT32 = 2400
N_RANDOM_BITS = 600


def bits(v: float) -> str:
    return "0x%016x" % struct.unpack("<Q", struct.pack("<d", v))[0]


def jcs_number(v: float) -> str:
    """ECMAScript Number::toString for a finite double, from repr() digits."""
    assert math.isfinite(v)
    if v == 0.0:
        return "0"
    sign = "-" if v < 0 else ""
    r = repr(abs(v))
    if "e" in r:
        mant, exp = r.split("e")
        exp = int(exp)
    else:
        mant, exp = r, 0
    if "." in mant:
        ip, fp = mant.split(".")
    else:
        ip, fp = mant, ""
    digits = ip + fp
    n = len(ip) + exp  # value = 0.digits * 10^n
    stripped = digits.lstrip("0")
    n -= len(digits) - len(stripped)
    digits = stripped.rstrip("0")
    k = len(digits)
    if k <= n <= 21:
        body = digits + "0" * (n - k)
    elif 0 < n <= 21:
        body = digits[:n] + "." + digits[n:]
    elif -6 < n <= 0:
        body = "0." + "0" * (-n) + digits
    else:
        e = n - 1
        body = digits[0] + ("." + digits[1:] if k > 1 else "") + "e" + ("-" if e < 0 else "+") + str(abs(e))
    return sign + body


def exact_digits(v: float):
    """Exact decimal expansion of |v| as (digit string without trailing zeros, n) with |v| = 0.digits * 10^n."""
    f = Fraction(abs(v))
    num, den = f.numerator, f.denominator
    t = 0
    while den != 1:  # den is a power of two
        num *= 5
        den //= 2
        t += 1
    s = str(num)
    n = len(s) - t
    return s.rstrip("0"), n


def is_shortest_tie(v: float) -> bool:
    """True if |v| is exactly halfway between two k-digit decimals that both round to v,
    where k is the shortest round-trip length."""
    d, n = exact_digits(v)
    if not d.endswith("5") or len(d) < 2:
        return False
    k = len(d) - 1
    lo = int(d[:-1])
    hi = lo + 1
    scale = Fraction(10) ** (n - k)
    if float(lo * scale) != abs(v) or float(hi * scale) != abs(v):
        return False
    shortest = len(repr(abs(v)).split("e")[0].replace(".", "").strip("0"))
    return shortest == k


def main() -> None:
    rng = random.Random(SEED)
    out = []
    seen = set()

    def add(v: float, tag: str) -> None:
        b = bits(v)
        if b in seen:
            return
        seen.add(b)
        out.append([b, jcs_number(v)])
        tags[tag] = tags.get(tag, 0) + 1

    tags = {}

    # Examples from the review (ECMA-262 ties-to-even), plus their negatives.
    examples = [
        (277575414244990.125, "277575414244990.12"),
        (64.044097900390625, "64.04409790039062"),
        (0.00048923492431640625, "0.0004892349243164062"),
    ]
    for v, want in examples:
        assert jcs_number(v) == want, (v, jcs_number(v), want)
        assert is_shortest_tie(v), v
        add(v, "example")
        add(-v, "example")

    # Deliberately constructed exact midpoints: v = m / 2^t with m odd, so the exact decimal
    # expansion is m * 5^t * 10^-t and ends in 5. Keep those that are shortest-length ties.
    midpoints = 0
    while midpoints < N_MIDPOINTS:
        t = rng.randint(1, 26)
        p5 = 5 ** t
        lo_m = max(1, -(-10**16 // p5))
        hi_m = min(2**53 - 1, (10**18 - 1) // p5)
        if lo_m > hi_m:
            continue
        m = rng.randint(lo_m, hi_m) | 1
        if m > hi_m:
            continue
        v = float(Fraction(m, 2**t))
        assert Fraction(v) == Fraction(m, 2**t)
        if not is_shortest_tie(v):
            continue
        # repr must pick the even candidate (sanity check of the reference itself).
        last = repr(v).split("e")[0].rstrip("0")[-1]
        assert last in "02468", (v, repr(v))
        if rng.random() < 0.25:
            v = -v
        before = len(seen)
        add(v, "midpoint")
        midpoints += len(seen) - before

    # Every power of two from 2^-1074 to 2^1023.
    for i in range(-1074, 1024):
        add(math.ldexp(1.0, i), "pow2")

    # float32 values widened to double: random finite binary32 bit patterns.
    f32 = 0
    while f32 < N_FLOAT32:
        u = rng.getrandbits(32)
        if (u >> 23) & 0xFF == 0xFF:
            continue
        v = struct.unpack("<f", struct.pack("<I", u))[0]
        if v == 0.0:
            continue
        before = len(seen)
        add(v, "float32")
        f32 += len(seen) - before

    # float32 values of moderate magnitude (where the ties live).
    f32m = 0
    while f32m < N_FLOAT32:
        v = struct.unpack("<f", struct.pack("<f", rng.uniform(-1e5, 1e5) * 10 ** rng.randint(-6, 3)))[0]
        if v == 0.0:
            continue
        before = len(seen)
        add(v, "float32_moderate")
        f32m += len(seen) - before

    # Some arbitrary finite doubles for regression coverage of the non-tie path.
    rb = 0
    while rb < N_RANDOM_BITS:
        u = rng.getrandbits(64)
        if (u >> 52) & 0x7FF == 0x7FF:
            continue
        v = struct.unpack("<d", struct.pack("<Q", u))[0]
        before = len(seen)
        add(v, "random_bits")
        rb += len(seen) - before

    ties = sum(1 for b, _ in out if is_shortest_tie(struct.unpack("<d", struct.pack("<Q", int(b, 16)))[0]))
    path = os.path.join(os.path.dirname(os.path.abspath(__file__)), "tie_vectors.json")
    with open(path, "w", encoding="utf-8", newline="\n") as fh:
        fh.write("[\n")
        fh.write(",\n".join(json.dumps(row) for row in out))
        fh.write("\n]\n")
    print(f"wrote {len(out)} vectors ({ties} exact shortest ties) to {path}: {tags}")


if __name__ == "__main__":
    main()
