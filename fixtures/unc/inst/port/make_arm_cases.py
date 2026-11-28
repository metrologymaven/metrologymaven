#!/usr/bin/env python3
"""Independent reference for UNC.INST.PORT.ARM (stdlib only; CC-BY-4.0 data).

Writes fixtures/unc/inst/port/arm_cases.json for `fid_kunc::inst::arm::arm_cov`
(algorithm unc.inst.arm_cov@1.0.0). Written from spec/unc/inst/port/arm/NODE.md and the
module documentation of `fid_kunc::inst::arm`, never from the Rust code, and it never runs
the Rust code.

INVENTED DATA (D-038, D-045). Every parameter set here is drawn from a seeded generator or
chosen by hand for tests. None describes a real articulated arm and none was measured.

Two independent parts:

1. The 50-digit reference (AC-1, AC-3): the exact decimal value of every input double, and
   in `decimal` at 100 digits ||p|| = sqrt(p.p), sigma_lat = sigma_p + sigma_d1 ||p||,
   sigma_ax = axial_factor sigma_lat, u_hat = u / ||u|| and
   Sigma = sigma_lat^2 I + (sigma_ax^2 - sigma_lat^2) u_hat u_hat^T exactly as the spec's
   Scope writes it. Each entry is written with 50 significant digits. This part knows
   nothing about the order of the Rust arithmetic, its power-of-two scaling or its
   factored form of sigma_ax^2 - sigma_lat^2.
2. The predicted bits: a model of the documented evaluation order (module doc of
   `fid_kunc::inst::arm`) in CPython floats (IEEE-754 binary64, round to nearest even, no
   FMA). The only operation that is not a single +, -, *, / is the square root; it is
   taken as math.sqrt and then verified to be the correctly rounded one with exact
   fractions.Fraction arithmetic (and corrected if it were not), so no libm decides a
   printed bit. Powers of two come from math.ldexp (exact) and exponents from math.frexp.

Every label is decided exactly with fractions.Fraction (every double is an exact
rational), never with a tolerance:
  - below_minimum_norm: u.u < t^2 with t the double nearest 1e-12 (REQ-002);
  - direction_off_unit: u.u < (1 - d)^2 or u.u > (1 + d)^2 with d the double nearest 1e-9;
  - axial_zero: a direction is given and axial_factor == 0;
  - zero_covariance: every entry is 0; scale_below_validated: 0 < sigma_lat < 2^-480, or
    0 < ||p|| (as computed) < 2^-1022.

The script asserts, for every case whose result carries neither zero_covariance nor
scale_below_validated, that the predicted covariance is within 37 * 2^-53 relative
Frobenius norm of the reference (the bound stated in the module doc, below the 1e-14 of
AC-1), so a fixture that breaks AC-1 cannot be written. For the AC-2 cases it asserts that
every off-diagonal entry is +0 and each diagonal entry is the correctly rounded square of
the returned sigma_lat (and, for the dyadic cases whose sigma_lat is exact, within 1 ulp of
the exact sigma_lat^2). For the AC-3 cases it asserts exact +0 off the diagonal and the
diagonal within 1e-15 relative of sigma_lat^2 (1, 1, 4) from the exact inputs.

Doubles are written as 16 lowercase hex digits of their IEEE-754 bits (no prefix). An
absent axial_factor or direction is an absent key.

Usage: make_arm_cases.py [--check]   (--check compares with the file on disk, writes nothing)
"""

import json
import math
import struct
import sys
from decimal import Decimal, localcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "arm_cases.json"

ALGORITHM = "unc.inst.arm_cov@1.0.0"
SEED = 0x41524D
N_SEEDED = 100
MIN_DIRECTION_NORM = 1e-12
UNIT_TOLERANCE = 1e-9
SIGMA_MIN = 2.0 ** -480
NORM_MIN = 2.0 ** -1022
REL_BOUND = Decimal(37) * Decimal(2) ** -53
AC3_REL = Decimal("1e-15")
MASK64 = (1 << 64) - 1


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def hx(x):
    return "%016x" % bits(x)


def next_up(x):
    """The next double above a finite x >= 0."""
    return from_bits(bits(x) + 1) if x != 0.0 else 5e-324


def next_down(x):
    """The next double below a finite x > 0."""
    return from_bits(bits(x) - 1)


def nudge(x, k):
    """x moved by k doubles (x > 0, k small)."""
    for _ in range(abs(k)):
        x = next_up(x) if k > 0 else next_down(x)
    return x


def ulp(x):
    x = abs(x)
    return next_up(x) - x


# ------------------------------------------------------------------ seeded generator


class SplitMix64:
    def __init__(self, seed):
        self.s = seed & MASK64

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & MASK64
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & MASK64
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & MASK64
        return z ^ (z >> 31)

    def unit(self):
        """A double in [0, 1): the top 53 bits over 2^53 (exact)."""
        return (self.next() >> 11) * (2.0 ** -53)

    def uniform(self, lo, hi):
        return lo + (hi - lo) * self.unit()


# ------------------------------------------------------------------ the documented order


def sqrt_rn(x):
    """The correctly rounded square root of a positive finite double, checked exactly."""
    assert x > 0.0 and math.isfinite(x)
    X = Fraction(x)
    y = math.sqrt(x)
    for _ in range(4):
        lo = (Fraction(next_down(y)) + Fraction(y)) / 2
        hi = (Fraction(y) + Fraction(next_up(y))) / 2
        assert X != lo * lo and X != hi * hi  # no square root of a double is a tie
        if X < lo * lo:
            y = next_down(y)
        elif X > hi * hi:
            y = next_up(y)
        else:
            return y
    raise AssertionError("sqrt_rn did not settle")


def trunc_half(k):
    """k / 2 rounded toward zero (Rust integer division)."""
    return -((-k) // 2) if k < 0 else k // 2


def scaled(x):
    """Module rule 4: (v, n, a, b) with v = (x * 2^a) * 2^b, n = sqrt((v0^2 + v1^2) + v2^2).

    None when every component is zero.
    """
    m = max(abs(c) for c in x)
    if m == 0.0:
        return None
    e = math.frexp(m)[1] - 1  # m in [2^e, 2^(e+1)), subnormals included
    a = trunc_half(-e)
    b = -e - a
    v = [(c * math.ldexp(1.0, a)) * math.ldexp(1.0, b) for c in x]
    n = sqrt_rn((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])
    return v, n, a, b


def norm(x):
    s = scaled(x)
    if s is None:
        return 0.0
    _, n, a, b = s
    return (n * math.ldexp(1.0, -a)) * math.ldexp(1.0, -b)


def model(sp, sd, f_opt, p, u):
    """Bit model of `arm_cov` (module doc of fid_kunc::inst::arm, rules 4 to 7)."""
    f = 1.0 if f_opt is None else f_opt
    npt = norm(p)
    sl = sp + sd * npt
    sax = f * sl
    s2 = sl * sl
    s = [[0.0] * 3 for _ in range(3)]
    direction = None
    if u is None:
        for i in range(3):
            s[i][i] = s2 + 0.0
    else:
        v, n, _, _ = scaled(u)
        uh = [vi / n for vi in v]
        direction = [c + 0.0 for c in uh]
        fm1 = f - 1.0
        fp1 = f + 1.0
        da = fm1 * sl
        db = fp1 * sl
        d = da * db
        for i in range(3):
            for l in range(i, 3):
                w = uh[i] * uh[l]
                t = d * w
                val = (s2 + t) if i == l else t
                val = val + 0.0
                s[i][l] = val
                s[l][i] = val
    return {
        "point_norm": npt,
        "sigma_lat": sl + 0.0,
        "sigma_ax": sax + 0.0,
        "axial_factor": f + 0.0,
        "direction": direction,
        "cov": s,
    }


# ------------------------------------------------------------------ exact labels


def sumsq(u):
    return sum(Fraction(c) * Fraction(c) for c in u)


def below_min(u):
    t = Fraction(MIN_DIRECTION_NORM)
    return sumsq(u) < t * t


def off_unit(u):
    d = Fraction(UNIT_TOLERANCE)
    s = sumsq(u)
    return s < (1 - d) ** 2 or s > (1 + d) ** 2


def flags(f_opt, u, out):
    f = 1.0 if f_opt is None else f_opt
    res = []
    if u is not None and off_unit(u):
        res.append("direction_off_unit")
    if u is not None and f == 0.0:
        res.append("axial_zero")
    big = max(abs(v) for row in out["cov"] for v in row)
    sl = out["sigma_lat"]
    npt = out["point_norm"]
    if big == 0.0:
        res.append("zero_covariance")
    if (sl != 0.0 and sl < SIGMA_MIN) or (npt != 0.0 and npt < NORM_MIN):
        res.append("scale_below_validated")
    return res


# ------------------------------------------------------------------ the 100-digit reference


def reference(sp, sd, f_opt, p, u):
    f = 1.0 if f_opt is None else f_opt
    with localcontext() as ctx:
        ctx.prec = 100
        D = Decimal
        npt = sum(D(c) * D(c) for c in p).sqrt()
        sl = D(sp) + D(sd) * npt
        s2 = sl * sl
        out = [[s2 if i == l else D(0) for l in range(3)] for i in range(3)]
        if u is not None:
            nu = sum(D(c) * D(c) for c in u).sqrt()
            uh = [D(c) / nu for c in u]
            sax = D(f) * sl
            d = sax * sax - s2
            out = [[(s2 if i == l else D(0)) + d * uh[i] * uh[l] for l in range(3)] for i in range(3)]
        return out, s2


def rel_frobenius(pred, ref):
    with localcontext() as ctx:
        ctx.prec = 60
        num = sum((Decimal(pred[i][l]) - ref[i][l]) ** 2 for i in range(3) for l in range(3))
        den = sum(ref[i][l] ** 2 for i in range(3) for l in range(3))
        if den == 0:
            return None
        return (num / den).sqrt()


def dec50(v):
    with localcontext() as ctx:
        ctx.prec = 50
        v = +v
        if v == 0:
            return "0"
        return format(v, ".49e")


ACCURACY_FLAGS = ("zero_covariance", "scale_below_validated")


def case(name, sp, sd, f_opt, p, u):
    assert u is None or not below_min(u), name
    assert u is not None or f_opt is None or f_opt == 1.0, name
    out = model(sp, sd, f_opt, p, u)
    ref, _ = reference(sp, sd, f_opt, p, u)
    fl = flags(f_opt, u, out)
    if not any(x in fl for x in ACCURACY_FLAGS):
        err = rel_frobenius(out["cov"], ref)
        assert err is not None and err <= REL_BOUND, (name, err)
    params = {"sigma_p": hx(sp), "sigma_d1": hx(sd)}
    if f_opt is not None:
        params["axial_factor"] = hx(f_opt)
    c = {
        "name": name,
        "params": params,
        "p": [hx(x) for x in p],
        "sigma_lat": hx(out["sigma_lat"]),
        "sigma_ax": hx(out["sigma_ax"]),
        "axial_factor_used": hx(out["axial_factor"]),
        "flags": fl,
        "cov_bits": [hx(out["cov"][i][l]) for i in range(3) for l in range(3)],
        "cov_ref": [dec50(ref[i][l]) for i in range(3) for l in range(3)],
    }
    if u is not None:
        c["u"] = [hx(x) for x in u]
        c["direction"] = [hx(x) for x in out["direction"]]
    return c


def seeded():
    g = SplitMix64(SEED)
    out = []
    for k in range(N_SEEDED):
        sp = g.uniform(5e-6, 5e-5)
        sd = g.uniform(0.0, 2e-5)
        p = [g.uniform(-2.5, 2.5) for _ in range(3)]
        mode = k % 4
        if mode == 0:
            f_opt = None
        elif mode == 1:
            f_opt = g.uniform(0.2, 1.0)
        elif mode == 2:
            f_opt = g.uniform(1.0, 3.0)
        else:
            f_opt = g.uniform(0.0, 4.0)
        if mode == 0 and k % 8 == 0:
            u = None
        else:
            while True:
                u = [g.uniform(-1.0, 1.0) for _ in range(3)]
                if max(abs(c) for c in u) >= 1e-3:
                    break
            if k % 5 == 0:
                n = math.sqrt(sum(c * c for c in u))
                u = [c / n for c in u]  # unit to rounding: not off-unit
            elif k % 5 == 1:
                s = g.uniform(0.5, 3.0)
                u = [c * s for c in u]
        out.append(case("seeded-%03d" % k, sp, sd, f_opt, p, u))
    return out


def edges():
    sp, sd = 1.5e-5, 2.5e-6
    tiny = 2.0 ** -52
    return [
        case("p-origin", sp, sd, 2.0, [0.0, 0.0, 0.0], [0.0, 0.0, 1.0]),
        case("p-exact-norm-13", sp, sd, 0.5, [3.0, 4.0, 12.0], [1.0, 1.0, 1.0]),
        case("p-negative-zero", sp, sd, None, [-0.0, -0.0, -0.0], None),
        case("p-huge", sp, 0.0, 1.5, [1e300, -1e300, 1e300], [0.0, 0.6, 0.8]),
        case("p-mixed-scale", sp, sd, 1.5, [1.0, 1e-200, -5e-324], [0.0, 0.6, 0.8]),
        case("u-within-tolerance", sp, sd, 2.0, [0.4, 0.1, 0.7], [0.0, 0.0, 1.0 + 5e-10]),
        case("u-off-unit-345", sp, sd, 2.0, [0.4, 0.1, 0.7], [0.0, 3.0, 4.0]),
        case("u-negative-axis", sp, sd, 3.0, [0.4, 0.1, 0.7], [-1.0, -0.0, 0.0]),
        case("u-huge", sp, sd, 0.7, [0.4, 0.1, 0.7], [1e300, -2e300, 5e299]),
        case("u-at-min-norm", sp, sd, 1.3, [0.4, 0.1, 0.7], [0.0, MIN_DIRECTION_NORM, 0.0]),
        case("u-mixed-scale", sp, sd, 1.3, [0.4, 0.1, 0.7], [1.0, 1e-200, 5e-324]),
        case("u-subnormal-pair", sp, sd, 1.3, [0.4, 0.1, 0.7], [1e-12, 1e-310, -5e-324]),
        case("f-absent-with-u", sp, sd, None, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("f-one-with-u", sp, sd, 1.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("f-one-without-u", sp, sd, 1.0, [0.4, 0.1, 0.7], None),
        case("f-zero", sp, sd, 0.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("f-negative-zero", sp, sd, -0.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("f-near-one", sp, sd, 1.0 + tiny, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("f-below-one", sp, sd, 1.0 - tiny / 2.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("f-large", sp, sd, 1e3, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("sigma-d1-zero", sp, 0.0, 2.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("sigma-p-zero", 0.0, sd, 2.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        # Flagged: accuracy not claimed (bits and flags only).
        case("all-zero-sigma", 0.0, 0.0, 2.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("all-zero-sigma-iso", -0.0, 0.0, None, [0.4, 0.1, 0.7], None),
        case("p-origin-no-d1", 0.0, sd, None, [0.0, 0.0, 0.0], None),
        case("tiny-sigma", 1e-160, 0.0, 2.0, [0.4, 0.1, 0.7], [0.2, -0.3, 0.9]),
        case("p-subnormal-norm", sp, 1.0, 2.0, [5e-324, 0.0, 0.0], [0.2, -0.3, 0.9]),
    ]


def isotropic():
    """AC-2: no probe direction."""
    out = []
    cases = [
        # Dyadic parameters and points with exact norms: sigma_lat is exact.
        ("dyadic-origin", 2.0 ** -17, 2.0 ** -20, [0.0, 0.0, 0.0], True),
        ("dyadic-122", 2.0 ** -17, 2.0 ** -20, [1.0, 2.0, -2.0], True),
        ("dyadic-3-4-12", 2.0 ** -16, 2.0 ** -21, [-3.0, 4.0, 12.0], True),
        ("dyadic-small", 2.0 ** -18, 2.0 ** -18, [0.375, 0.0, -0.5], True),
        # General values: diagonal = the correctly rounded square of the returned sigma_lat.
        ("general-a", 1.5e-5, 2.5e-6, [0.4, -1.1, 0.7], False),
        ("general-b", 3e-5, 1e-5, [-2.2, 0.3, 1.9], False),
        ("general-c", 7e-6, 0.0, [1.0, 1.0, 1.0], False),
    ]
    for name, sp, sd, p, exact in cases:
        c = case("iso-" + name, sp, sd, None, p, None)
        out_ = model(sp, sd, None, p, None)
        s = out_["cov"]
        sl = out_["sigma_lat"]
        for i in range(3):
            for l in range(3):
                if i != l:
                    assert bits(s[i][l]) == 0, (name, i, l)
            assert bits(s[i][i]) == bits(s[0][0]), name
        # The correctly rounded square of sigma_lat (Fraction -> float rounds correctly).
        assert s[0][0] == float(Fraction(sl) * Fraction(sl)), name
        _, s2_exact = reference(sp, sd, None, p, None)
        if exact:
            assert abs(Decimal(s[0][0]) - s2_exact) <= Decimal(ulp(s[0][0])), name
        c["exact_sigma"] = "yes" if exact else "no"
        c["sigma_lat_sq_exact"] = dec50(s2_exact)
        out.append(c)
    return out


def axial():
    """AC-3: u along z (unit and not), axial_factor 2: eigenvalues s2 (1, 1, 4)."""
    out = []
    cases = [
        ("unit-z", 1.5e-5, 2.5e-6, [0.4, -1.1, 0.7], [0.0, 0.0, 1.0]),
        ("minus-z", 3e-5, 1e-5, [-2.2, 0.3, 1.9], [0.0, 0.0, -1.0]),
        ("seven-z", 7e-6, 4e-6, [1.0, 1.0, 1.0], [0.0, 0.0, 7.0]),
        ("origin", 2e-5, 3e-6, [0.0, 0.0, 0.0], [-0.0, 0.0, 1.0]),
    ]
    for name, sp, sd, p, u in cases:
        c = case("ac3-" + name, sp, sd, 2.0, p, u)
        s = model(sp, sd, 2.0, p, u)["cov"]
        _, s2_exact = reference(sp, sd, 2.0, p, u)
        eig = [s2_exact, s2_exact, 4 * s2_exact]
        for i in range(3):
            for l in range(3):
                if i != l:
                    assert bits(s[i][l]) == 0, (name, i, l)
            with localcontext() as ctx:
                ctx.prec = 60
                rel = abs(Decimal(s[i][i]) - eig[i]) / eig[i]
            assert rel <= AC3_REL, (name, i, rel)
        c["eig_ref"] = [dec50(v) for v in eig]
        out.append(c)
    return out


def direction_cases():
    """Directions on and around the two exact thresholds, labelled with Fraction.

    A component is placed on the root of u.u = target^2 (target t = 1e-12, 1 - d or 1 + d),
    rounded, and moved by up to 3 doubles either way, so the exact side varies case by case.
    """
    g = SplitMix64(SEED ^ 0xD1)
    t = MIN_DIRECTION_NORM
    d = UNIT_TOLERANCE
    vecs = [
        [t, 0.0, 0.0],
        [next_down(t), 0.0, 0.0],
        [0.0, 0.0, -t],
        [0.0, 0.0, 0.0],
        [-0.0, 0.0, -0.0],
        [5e-324, 5e-324, 5e-324],
        [1.0, 0.0, 0.0],
        [1.0 + d, 0.0, 0.0],
        [next_up(1.0 + d), 0.0, 0.0],
        [1.0 - d, 0.0, 0.0],
        [next_down(1.0 - d), 0.0, 0.0],
    ]
    targets = [Fraction(t), 1 - Fraction(d), 1 + Fraction(d)]
    while len(vecs) < 160:
        target = targets[len(vecs) % 3]
        scale = float(target)
        a = g.uniform(-0.7, 0.7) * scale
        b = g.uniform(-0.7, 0.7) * scale
        rest = target * target - Fraction(a) * Fraction(a) - Fraction(b) * Fraction(b)
        if rest <= 0:
            continue
        c = sqrt_rn(float(rest))
        c = nudge(c, (g.next() % 7) - 3)
        if g.next() & 1:
            c = -c
        vecs.append([a, b, c])
    out = []
    for u in vecs:
        if below_min(u):
            label = "refused"
        elif off_unit(u):
            label = "off_unit"
        else:
            label = "unit"
        out.append({"u": [hx(x) for x in u], "expect": label})
    return out


def build():
    doc = {
        "algorithm": ALGORITHM,
        "axial": axial(),
        "cases": seeded(),
        "direction_cases": direction_cases(),
        "edges": edges(),
        "generator": "fixtures/unc/inst/port/make_arm_cases.py",
        "isotropic": isotropic(),
        "licence": "CC-BY-4.0; invented test data (D-038, D-045): no parameter set describes a real articulated arm",
        "seed": "0x41524d",
    }
    text = json.dumps(doc, indent=1, sort_keys=True) + "\n"
    return text.encode("ascii")


def main():
    check = "--check" in sys.argv[1:]
    data = build()
    if check:
        if not OUT.exists() or OUT.read_bytes() != data:
            print(f"stale: {OUT}")
            sys.exit(1)
        print(f"ok: {OUT}")
    else:
        OUT.write_bytes(data)
        print(f"wrote {OUT}")


if __name__ == "__main__":
    main()
