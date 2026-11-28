#!/usr/bin/env python3
"""Independent reference for UNC.INST.PORT.SPH (stdlib only; CC-BY-4.0 data).

Writes fixtures/unc/inst/port/sph_cases.json for `fid_kunc::inst::sph_cov`
(algorithm unc.inst.sph_cov@1.0.0). Written from spec/unc/inst/port/sph/NODE.md and the
module documentation of `fid_kunc::inst::sph`, never from the Rust code, and it never runs
the Rust code.

INVENTED DATA (D-038, D-045). Every parameter set here is drawn from a seeded generator or
chosen by hand for tests. None describes a real instrument and none was measured.

Two independent parts:

1. The 50-digit reference (AC-1, REQ-UNC.INST.PORT.SPH-002): the exact decimal value of
   every input double, sin and cos from the 90-digit Taylor oracle of
   fixtures/num/make_fn_fixtures.py (`d_trig`: Machin pi, exact-integer reduction; it knows
   nothing about any libm), and J C J^T in `decimal` at 100 digits. Each entry is written
   with 50 significant digits. This part knows nothing about the order of the Rust
   arithmetic.
2. The predicted bits (AC-2, AC-4): a model of the documented evaluation order (module doc
   of `fid_kunc::inst::sph`) in CPython floats (IEEE-754 binary64, round to nearest even, no
   FMA), with sin and cos from the bit-exact Python model of the fid-num vendored libm in
   fixtures/num/make_fn_fixtures.py (`m_sin`, `m_cos`; that model is itself checked against
   fid-num by the KRN.NUM.FN golden digests). A match in CI on x86-64, ARM64 and wasm32
   shows every target computes exactly the documented algorithm (AC-4).

The script asserts, for every case in the validated domain, that the predicted covariance
is within 1e-14 relative Frobenius norm of the reference (so a fixture that breaks REQ-002
cannot be written), and for the axis-aligned case of AC-2 that each diagonal entry is
within 2 ulp of the exact diag(sigma_r^2, (r sigma_az)^2, (r sigma_el)^2) and every
off-diagonal entry is exactly +0.

Rules the expected labels follow (module doc of `fid_kunc::inst::sph`):
  - det = 1 - a^2 - b^2 - c^2 + 2abc with a = rho_r_az, b = rho_r_el, c = rho_az_el,
    evaluated exactly with fractions.Fraction (every double is an exact rational): det > 0 is
    positive_definite, det == 0 is singular, det < 0 is refused (hand-labelled refusal cases
    live in the Rust test, not in this file);
  - det_cases: 200 correlation triples (seed 0x535048 ^ 0xde7 and the review cases), each
    labelled positive_definite, singular or refused by the exact Fraction sign;
  - domain notes: range_outside unless 0.1 <= r <= 100; elevation_outside when |el| > 1.5
    (so also at the poles el = +-pi/2);
    azimuth_outside when |az| > 6.283185307179586; zero_covariance when every entry is 0,
    else scale_below_validated when the largest |entry| is below 2^-962.

Doubles are written as 16 lowercase hex digits of their IEEE-754 bits (no prefix).

Usage: make_sph_cases.py [--check]   (--check compares with the file on disk, writes nothing)
"""

import importlib.util
import json
import math
import struct
import sys
from decimal import Decimal, localcontext
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent.parent
OUT = HERE / "sph_cases.json"


def _load(name, rel):
    spec = importlib.util.spec_from_file_location(name, ROOT / rel)
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


FN = _load("make_fn_fixtures", "fixtures/num/make_fn_fixtures.py")

ALGORITHM = "unc.inst.sph_cov@1.0.0"
SEED = 0x535048
N_SEEDED = 200
SCALE_MIN = 2.0 ** -962
R_MIN = 0.1
R_MAX = 100.0
EL_MAX = 1.5
AZ_MAX = 6.283185307179586
PI = 3.141592653589793
FRAC_PI_2 = 1.5707963267948966
REL_FROB = Decimal("1e-14")
MASK64 = (1 << 64) - 1
PARAM_NAMES = ["sigma_r0", "sigma_r1", "sigma_az", "sigma_el", "rho_r_az", "rho_r_el", "rho_az_el"]


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return "%016x" % bits(x)


def ulp(x):
    """ulp of a finite non-zero double (spacing to the next double away from zero)."""
    x = abs(x)
    b = bits(x)
    return struct.unpack("<d", struct.pack("<Q", b + 1))[0] - x


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


def correlation_det(a, b, c):
    """The exact determinant of [[1, a, b], [a, 1, c], [b, c, 1]] as a Fraction."""
    a, b, c = Fraction(a), Fraction(b), Fraction(c)
    return 1 - a * a - b * b - c * c + 2 * a * b * c


def model(p, r, az, el):
    """Bit model of `sph_cov` (module doc of fid_kunc::inst::sph, steps 4 to 7)."""
    sr = p["sigma_r0"] + p["sigma_r1"] * r
    ca = FN.m_cos(az)
    sa = FN.m_sin(az)
    ce = FN.m_cos(el)
    se = FN.m_sin(el)
    rce = r * ce
    rse = r * se
    j = [
        [ce * ca, -(rce * sa), -(rse * ca)],
        [ce * sa, rce * ca, -(rse * sa)],
        [se, 0.0, rce],
    ]
    saz, sel = p["sigma_az"], p["sigma_el"]
    c01 = (p["rho_r_az"] * sr) * saz
    c02 = (p["rho_r_el"] * sr) * sel
    c12 = (p["rho_az_el"] * saz) * sel
    c = [[sr * sr, c01, c02], [c01, saz * saz, c12], [c02, c12, sel * sel]]
    bm = [[(j[i][0] * c[0][k] + j[i][1] * c[1][k]) + j[i][2] * c[2][k] for k in range(3)] for i in range(3)]
    s = [[0.0] * 3 for _ in range(3)]
    for i in range(3):
        for l in range(i, 3):
            v = (bm[i][0] * j[l][0] + bm[i][1] * j[l][1]) + bm[i][2] * j[l][2]
            v = v + 0.0
            s[i][l] = v
            s[l][i] = v
    return sr, s


def reference(p, r, az, el):
    """J C J^T at 100 digits from the exact inputs and the 90-digit trig oracle."""
    sin_az, cos_az = FN.d_trig(az, "sin"), FN.d_trig(az, "cos")
    sin_el, cos_el = FN.d_trig(el, "sin"), FN.d_trig(el, "cos")
    with localcontext() as ctx:
        ctx.prec = 100
        D = Decimal
        rr = D(r)
        sr = D(p["sigma_r0"]) + D(p["sigma_r1"]) * rr
        saz, sel = D(p["sigma_az"]), D(p["sigma_el"])
        j = [
            [cos_el * cos_az, -rr * cos_el * sin_az, -rr * sin_el * cos_az],
            [cos_el * sin_az, rr * cos_el * cos_az, -rr * sin_el * sin_az],
            [sin_el, D(0), rr * cos_el],
        ]
        c01 = D(p["rho_r_az"]) * sr * saz
        c02 = D(p["rho_r_el"]) * sr * sel
        c12 = D(p["rho_az_el"]) * saz * sel
        c = [[sr * sr, c01, c02], [c01, saz * saz, c12], [c02, c12, sel * sel]]
        out = [[sum(j[i][a] * c[a][b] * j[l][b] for a in range(3) for b in range(3)) for l in range(3)] for i in range(3)]
        return out


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


def notes(r, az, el, s):
    out = []
    if not (R_MIN <= r <= R_MAX):
        out.append("range_outside")
    if abs(el) > EL_MAX:
        out.append("elevation_outside")
    if abs(az) > AZ_MAX:
        out.append("azimuth_outside")
    big = max(abs(v) for row in s for v in row)
    if big == 0.0:
        out.append("zero_covariance")
    elif big < SCALE_MIN:
        out.append("scale_below_validated")
    return out


def case(name, p, r, az, el, check_accuracy=True):
    det = correlation_det(p["rho_r_az"], p["rho_r_el"], p["rho_az_el"])
    assert det >= 0, name
    corr = "positive_definite" if det > 0 else "singular"
    sr, s = model(p, r, az, el)
    ref = reference(p, r, az, el)
    nts = notes(r, az, el, s)
    err = rel_frobenius(s, ref)
    if check_accuracy and not nts:
        assert err is not None and err <= REL_FROB, (name, err)
    return {
        "name": name,
        "params": {k: hx(p[k]) for k in PARAM_NAMES},
        "obs": {"r": hx(r), "az": hx(az), "el": hx(el)},
        "sigma_r": hx(sr + 0.0),
        "correlation": corr,
        "notes": nts,
        "cov_bits": [hx(s[i][l]) for i in range(3) for l in range(3)],
        "cov_ref": [dec50(ref[i][l]) for i in range(3) for l in range(3)],
    }


def params(r0, r1, saz, sel, a=0.0, b=0.0, c=0.0):
    return {"sigma_r0": r0, "sigma_r1": r1, "sigma_az": saz, "sigma_el": sel, "rho_r_az": a, "rho_r_el": b, "rho_az_el": c}


def seeded():
    g = SplitMix64(SEED)
    out = []
    while len(out) < N_SEEDED:
        k = len(out)
        r0 = g.uniform(1e-6, 5e-5)
        r1 = g.uniform(0.0, 5e-6)
        saz = g.uniform(1e-6, 1e-4)
        sel = g.uniform(1e-6, 1e-4)
        if k % 3 == 0:
            a = b = c = 0.0
        else:
            a = g.uniform(-0.95, 0.95)
            b = g.uniform(-0.95, 0.95)
            c = g.uniform(-0.95, 0.95)
            if correlation_det(a, b, c) <= 0:
                continue  # an indefinite or singular draw is redrawn (deterministic)
        r = g.uniform(R_MIN, R_MAX)
        az = g.uniform(-PI, PI)
        el = g.uniform(-EL_MAX, EL_MAX)
        out.append(case("seeded-%03d" % k, params(r0, r1, saz, sel, a, b, c), r, az, el))
    return out


def edges():
    p = params(1.5e-5, 2.5e-6, 2.5e-5, 2.5e-5, 0.3, -0.2, 0.1)
    out = [
        case("r-min", p, R_MIN, 0.7, 0.2),
        case("r-max", p, R_MAX, -2.1, -0.4),
        case("el-plus-1.5", p, 12.5, 0.4, 1.5),
        case("el-minus-1.5", p, 12.5, -0.4, -1.5),
        case("az-plus-pi", p, 3.0, PI, 0.0),
        case("az-minus-two-pi", p, 3.0, -AZ_MAX, 0.25),
        case("rho-r-az-one", params(1e-5, 1e-6, 2e-5, 3e-5, 1.0, 0.0, 0.0), 7.0, 1.1, -0.3),
        case("rho-az-el-minus-one", params(1e-5, 1e-6, 2e-5, 3e-5, 0.0, 0.0, -1.0), 7.0, 1.1, -0.3),
        case("rho-equicorrelated-half", params(1e-5, 1e-6, 2e-5, 3e-5, -0.5, -0.5, -0.5), 2.0, 0.6, 0.6),
        case("zero-sigma-angles", params(2e-5, 0.0, 0.0, 0.0), 5.0, 0.3, 0.1),
        # Outside the validated domain: bits and notes only (accuracy not claimed there).
        case("out-r-zero", p, 0.0, 0.3, 0.1, check_accuracy=False),
        case("out-r-150", p, 150.0, 0.3, 0.1, check_accuracy=False),
        case("out-el-1.57", p, 20.0, 0.3, 1.57, check_accuracy=False),
        case("out-el-pole-north", p, 20.0, 0.3, FRAC_PI_2, check_accuracy=False),
        case("out-el-pole-south", p, 20.0, -1.2, -FRAC_PI_2, check_accuracy=False),
        case("out-az-7", p, 20.0, 7.0, 0.1, check_accuracy=False),
        case("out-tiny-scale", params(1e-160, 0.0, 1e-161, 1e-161), 1.0, 0.3, 0.1, check_accuracy=False),
        case("out-all-zero", params(0.0, 0.0, 0.0, 0.0), 5.0, 0.3, 0.1, check_accuracy=False),
    ]
    return out


def axis_aligned():
    """AC-2: az = el = 0, r = 10 m, zero correlations."""
    p = params(1.5e-5, 2.5e-6, 3e-5, 4e-5)
    r = 10.0
    c = case("axis-aligned", p, r, 0.0, 0.0)
    sr, s = model(p, r, 0.0, 0.0)
    with localcontext() as ctx:
        ctx.prec = 100
        D = Decimal
        srx = D(p["sigma_r0"]) + D(p["sigma_r1"]) * D(r)
        exact = [srx * srx, (D(r) * D(p["sigma_az"])) ** 2, (D(r) * D(p["sigma_el"])) ** 2]
    for i in range(3):
        for l in range(3):
            if i != l:
                assert bits(s[i][l]) == 0, ("off-diagonal not +0", i, l)
    diag_nearest = []
    for i in range(3):
        nearest = float(exact[i])  # Decimal -> float is correctly rounded
        assert abs(Decimal(s[i][i]) - exact[i]) <= 2 * Decimal(ulp(nearest)), ("AC-2 ulp", i)
        diag_nearest.append(hx(nearest))
    c["diag_exact"] = [dec50(v) for v in exact]
    c["diag_nearest"] = diag_nearest
    return c


def next_toward(x, up):
    """The neighbouring double of x (x finite, non-zero or zero)."""
    if x == 0.0:
        return 5e-324 if up else -5e-324
    b = bits(x)
    step = 1 if (x > 0) == up else -1
    return struct.unpack("<d", struct.pack("<Q", b + step))[0]


def det_cases():
    """Correlation triples around the boundary det = 0, labelled by the exact Fraction sign.

    c is placed on a root of det(c) = 0 (c = ab +- sqrt((1 - a^2)(1 - b^2)), rounded) and
    moved by up to 3 doubles either way, so the exact sign varies case by case. Plus the
    hand-picked triples of the review of #687.
    """
    g = SplitMix64(SEED ^ 0xDE7)
    trip = [
        (0.9, 0.9, -0.9),
        (-0.5, -0.5, -0.5),
        (-0.5 - 2.0 ** -52, -0.5 - 2.0 ** -52, -0.5 - 2.0 ** -52),
        (-0.5 + 2.0 ** -54, -0.5 + 2.0 ** -54, -0.5 + 2.0 ** -54),
        (1.0, 1e-300, 2e-300),
        (1.0, 1e-300, 1e-300),
        (1.0, 5e-324, 1e-323),
        (-1.0, 1e-300, -1e-300),
        (1.0, 1.0, 1.0),
        (0.0, 0.0, 0.0),
    ]
    while len(trip) < 200:
        a = g.uniform(-1.0, 1.0)
        b = g.uniform(-1.0, 1.0)
        root = math.sqrt((1.0 - a * a) * (1.0 - b * b))  # correctly rounded (IEEE 754)
        c = a * b + (root if g.next() & 1 else -root)
        if abs(c) > 1.0:
            continue
        k = (g.next() % 7) - 3
        for _ in range(abs(k)):
            c = next_toward(c, k > 0)
        if abs(c) > 1.0:
            continue
        trip.append((a, b, c))
    out = []
    for a, b, c in trip:
        d = correlation_det(a, b, c)
        label = "positive_definite" if d > 0 else ("singular" if d == 0 else "refused")
        out.append({"rho": [hx(a), hx(b), hx(c)], "expect": label})
    return out


def build():
    doc = {
        "algorithm": ALGORITHM,
        "axis_aligned": axis_aligned(),
        "cases": seeded(),
        "det_cases": det_cases(),
        "edges": edges(),
        "generator": "fixtures/unc/inst/port/make_sph_cases.py",
        "licence": "CC-BY-4.0; invented test data (D-038, D-045): no parameter set describes a real instrument",
        "seed": "0x535048",
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
