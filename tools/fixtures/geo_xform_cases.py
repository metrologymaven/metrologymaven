#!/usr/bin/env python3
"""Independent reference for GEO.FRAME.XFORM; writes fixtures/geo/frame/xform_cases.json,
fixtures/geo/frame/euler_cases.json and fixtures/geo/frame/xform_worst_residual.json.

Run:  python tools/fixtures/geo_xform_cases.py [--check]
With --check nothing is written; the script exits non-zero if a file differs.

Stdlib only (fractions, random, math, struct, json); shares no code with fid-geo-frame.

xform_cases.json (AC-1). 200 transforms: 6 hand-labelled ones (identity, exact quarter
turns, a pure translation, an exact scale) then random ones from one `random.Random(11)` stream
(seed 11 per NODE.md, the same stream continuing into the Euler cases; Python's generator stands in for fid-num's counter RNG because the inputs
are stored in the fixture anyway). Rotations come from a quaternion with components uniform in
[-1, 1] (rejection to 0.01 <= |q|^2 <= 1), normalised with a correctly rounded sqrt and turned
into a matrix with +, -, * only; translations are uniform in [-100, 100]; odd cases are
similarities with scale uniform in [0.5, 2]. Only correctly rounded operations are used, so
the bytes are the same on every platform.

For case i the expected values are compose(T_i, T_(i+1 mod 200)) (apply T_(i+1) first) and
inverse(T_i), computed by a bit-exact model of the expressions pinned in the leaf's Design:
CPython floats are IEEE binary64 with round-to-nearest-even and never fuse a multiply-add, so
the model's bits are the Rust results on every target. Each model result is also compared with
the exact rational composition and inverse (`fractions.Fraction`), which must agree within
1e-15 per rotation entry and 1e-13 m per translation component, and every input rotation is
checked exactly: ||R^T R - I||_F <= 1e-12 and |det R - 1| <= 1e-12. The script also maps
20 000 random points in the 100 m cube forward and back through the model (AC-2's operation)
and asserts the largest error is below 1e-12 m.

euler_cases.json (AC-4). For each of the 24 variants (12 sequences, intrinsic and extrinsic):
4 random angle triples away from gimbal lock (expected: the input back, the canonical range
contains it), 1 near-lock triple (middle angle 1e-6 from lock; only the round trip is checked)
and 2 gimbal-lock triples (Tait-Bryan middle angle +pi/2 and -pi/2, proper 0 and pi) whose
expected canonical angles follow from the composition law, not from the Rust formulas:

* intrinsic Tait-Bryan (i, j, k), middle = s * pi/2: R_j(s pi/2) turns axis k into s e i, so
  R_i(a) R_j(b) R_k(c) = R_i(a + s e c) R_j(b): canonical (a + s e c, b, 0);
* intrinsic proper: (a + c, 0, 0) at b = 0 and (a - c, pi, 0) at b = pi;
* extrinsic (a, b, c) about (s0, s1, s2) equals intrinsic (c, b, a) about (s2, s1, s0); the
  third extrinsic angle is set to 0, so R_i(c) R_j(b) R_k(a) = R_j(b) R_k(a + s e' c) gives
  Tait-Bryan (a + s e' c, b, 0) with e' the parity of (s2, s1, s0), and proper (a + c, 0, 0)
  at b = 0, (a - c, pi, 0) at b = pi.

xform_worst_residual.json (REQ-004). Rigid transforms at the largest residual the pinned
residual model accepts (a stretch of the x axis, the same with a translation, a shear, and a
random rotation times a stretch; each between 0.98e-12 and 1e-12), with 8 points each and the
model's forward-and-back bits. Every round trip is checked in exact arithmetic against the
bound residual |x| + 48 * 2^-53 (|x| + |t|).

Here e = +1 when the sequence's second axis follows the first cyclically (X->Y->Z->X), else -1.
Every canonical answer is verified numerically: the matrices of the input and the canonical
angles (built here with math.sin and math.cos, used only for this assertion) agree within 1e-14.
"""

import json
import math
import random
import struct
import sys
from fractions import Fraction
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
OUT_XFORM = ROOT / "fixtures" / "geo" / "frame" / "xform_cases.json"
OUT_EULER = ROOT / "fixtures" / "geo" / "frame" / "euler_cases.json"
OUT_WORST = ROOT / "fixtures" / "geo" / "frame" / "xform_worst_residual.json"
N_XFORM = 200


def hx(x):
    return f"{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def hxs(xs):
    return " ".join(hx(x) for x in xs)


# ------------------------------------------------------------------ pinned float model


def dot3(a0, a1, a2, b0, b1, b2):
    return (a0 * b0 + a1 * b1) + a2 * b2


def mat_mul(a, b):
    return [[dot3(a[i][0], a[i][1], a[i][2], b[0][j], b[1][j], b[2][j]) for j in range(3)] for i in range(3)]


def mat_vec(r, x):
    return [dot3(r[i][0], r[i][1], r[i][2], x[0], x[1], x[2]) for i in range(3)]


def mat_t_vec(r, x):
    return [dot3(r[0][i], r[1][i], r[2][i], x[0], x[1], x[2]) for i in range(3)]


def transpose(r):
    return [[r[0][i], r[1][i], r[2][i]] for i in range(3)]


def compose(a, b):
    """a o b; a transform is (rigid, R, t, s) with s = 1.0 for rigid."""
    ra, ta, sa = a[1], a[2], a[3]
    rb, tb, sb = b[1], b[2], b[3]
    u = mat_vec(ra, tb)
    return (a[0] and b[0], mat_mul(ra, rb), [sa * u[i] + ta[i] for i in range(3)], sa * sb)


def inverse(a):
    rigid, r, t, s = a
    si = 1.0 / s
    u = mat_t_vec(r, t)
    return (rigid, transpose(r), [-(si * u[i]) for i in range(3)], si)


def apply_point(a, x):
    _, r, t, s = a
    u = mat_vec(r, x)
    return [s * u[i] + t[i] for i in range(3)]


# ------------------------------------------------------------------ exact oracle


def F(x):
    return Fraction(x)


def exact_compose(a, b):
    ra = [[F(v) for v in row] for row in a[1]]
    rb = [[F(v) for v in row] for row in b[1]]
    r = [[sum(ra[i][k] * rb[k][j] for k in range(3)) for j in range(3)] for i in range(3)]
    t = [F(a[3]) * sum(ra[i][k] * F(b[2][k]) for k in range(3)) + F(a[2][i]) for i in range(3)]
    return r, t, F(a[3]) * F(b[3])


def exact_inverse(a):
    r = [[F(v) for v in row] for row in a[1]]
    si = 1 / F(a[3])
    t = [-si * sum(r[k][i] * F(a[2][k]) for k in range(3)) for i in range(3)]
    return [[r[j][i] for j in range(3)] for i in range(3)], t, si


def check_close(model, exact, name):
    _, r, t, s = model
    er, et, es = exact
    for i in range(3):
        for j in range(3):
            assert abs(F(r[i][j]) - er[i][j]) <= Fraction(1, 10**15), (name, "r", i, j)
        assert abs(F(t[i]) - et[i]) <= Fraction(1, 10**13), (name, "t", i)
    assert abs(F(s) - es) <= Fraction(1, 10**15) * es, (name, "s")


def check_rotation_exact(r, name):
    q = [[F(v) for v in row] for row in r]
    g2 = Fraction(0)
    for i in range(3):
        for j in range(3):
            g = sum(q[k][i] * q[k][j] for k in range(3)) - (1 if i == j else 0)
            g2 += g * g
    assert g2 <= Fraction(1, 10**24), (name, "not orthonormal")
    det = (q[0][0] * (q[1][1] * q[2][2] - q[1][2] * q[2][1]) - q[0][1] * (q[1][0] * q[2][2] - q[1][2] * q[2][0])
           + q[0][2] * (q[1][0] * q[2][1] - q[1][1] * q[2][0]))
    assert abs(det - 1) <= Fraction(1, 10**12), (name, "improper")


# ------------------------------------------------------------------ inputs


def quat_matrix(w, x, y, z):
    return [
        [1.0 - 2.0 * (y * y + z * z), 2.0 * (x * y - w * z), 2.0 * (x * z + w * y)],
        [2.0 * (x * y + w * z), 1.0 - 2.0 * (x * x + z * z), 2.0 * (y * z - w * x)],
        [2.0 * (x * z - w * y), 2.0 * (y * z + w * x), 1.0 - 2.0 * (x * x + y * y)],
    ]


def random_rotation(rng):
    while True:
        q = [rng.uniform(-1.0, 1.0) for _ in range(4)]
        n2 = ((q[0] * q[0] + q[1] * q[1]) + q[2] * q[2]) + q[3] * q[3]
        if 0.01 <= n2 <= 1.0:
            n = math.sqrt(n2)  # correctly rounded (IEEE 754)
            return quat_matrix(*(v / n for v in q))


def transforms(rng):
    ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
    rz90 = [[0.0, -1.0, 0.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]]
    rx90 = [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]]
    out = [
        ("identity", (True, ident, [0.0, 0.0, 0.0], 1.0)),
        ("rz90_translate", (True, rz90, [1.0, -2.0, 0.5], 1.0)),
        ("rx90_scale2", (False, rx90, [0.0, 0.0, 0.0], 2.0)),
        ("translation_only", (True, ident, [100.0, -100.0, 0.25], 1.0)),
        ("scale_half", (False, ident, [3.0, 4.0, 5.0], 0.5)),
        ("rz90_similarity", (False, rz90, [-7.5, 0.0, 12.0], 1.25)),
    ]
    i = 0
    while len(out) < N_XFORM:
        r = random_rotation(rng)
        t = [rng.uniform(-100.0, 100.0) for _ in range(3)]
        if i % 2 == 0:
            out.append((f"random_{i}", (True, r, t, 1.0)))
        else:
            out.append((f"random_{i}", (False, r, t, rng.uniform(0.5, 2.0))))
        i += 1
    return out


def kind(rigid):
    return "rigid" if rigid else "similarity"


def flat(r):
    return [v for row in r for v in row]


def xform_text(rng):
    ts = transforms(rng)
    lines = []
    for idx, (name, a) in enumerate(ts):
        check_rotation_exact(a[1], name)
        b = ts[(idx + 1) % len(ts)][1]
        c = compose(a, b)
        v = inverse(a)
        check_close(c, exact_compose(a, b), name)
        check_close(v, exact_inverse(a), name)
        case = {
            "name": name,
            "kind": kind(a[0]),
            "r": hxs(flat(a[1])),
            "t": hxs(a[2]),
            "s": hx(a[3]),
            "c_kind": kind(c[0]),
            "c_r": hxs(flat(c[1])),
            "c_t": hxs(c[2]),
            "c_s": hx(c[3]),
            "i_kind": kind(v[0]),
            "i_r": hxs(flat(v[1])),
            "i_t": hxs(v[2]),
            "i_s": hx(v[3]),
        }
        lines.append(json.dumps(case, sort_keys=True, separators=(",", ":")))
    # AC-2's operation on a sample, through the model (identical bits to Rust).
    prng = random.Random(1011)
    worst = 0.0
    for k in range(20000):
        a = ts[k % len(ts)][1]
        x = [prng.uniform(-100.0, 100.0) for _ in range(3)]
        y = apply_point(inverse(a), apply_point(a, x))
        err = math.sqrt(sum((F(y[i]) - F(x[i])) ** 2 for i in range(3)))
        worst = max(worst, err)
    assert worst <= 1e-12, worst
    print(f"AC-2 sample: largest forward-and-back error {worst:.3e} m")
    return wrap(
        "AC-1: 200 rigid and similarity transforms; expected compose(T_i, T_(i+1 mod 200)) "
        "(apply T_(i+1) first) and inverse(T_i). Hex IEEE 754 bits, row-major, space-separated.",
        lines,
    )


def wrap(note, lines):
    head = [
        "{",
        '  "generator": "tools/fixtures/geo_xform_cases.py (stdlib, independent of fid-geo-frame)",',
        '  "license": "CC-BY-4.0",',
        f'  "note": "{note}",',
        '  "seed": 11,',
        '  "cases": [',
    ]
    text = "\n".join(head) + "\n" + ",\n".join("    " + l for l in lines) + "\n  ]\n}\n"
    json.loads(text)
    return text


# ------------------------------------------------------------------ worst accepted residual

EPS = 2.0**-53
C_BOUND = 48.0


def model_residual(r):
    """The pinned residual: sqrt of the row-major sum of squares of R^T R - I (dot3 columns)."""
    total = 0.0
    for i in range(3):
        for j in range(3):
            g = dot3(r[0][i], r[1][i], r[2][i], r[0][j], r[1][j], r[2][j])
            d = g - 1.0 if i == j else g
            total += d * d
    return math.sqrt(total)


def model_det(r):
    return (r[0][0] * (r[1][1] * r[2][2] - r[1][2] * r[2][1]) - r[0][1] * (r[1][0] * r[2][2] - r[1][2] * r[2][0])) + r[0][2] * (
        r[1][0] * r[2][1] - r[1][1] * r[2][0])


def model_bound(rho, x, t):
    nx = math.sqrt(dot3(x[0], x[1], x[2], x[0], x[1], x[2]))
    nt = math.sqrt(dot3(t[0], t[1], t[2], t[0], t[1], t[2]))
    return rho * nx + C_BOUND * (2.0**-52) * 0.5 * (nx + nt)


def largest_accepted(make, start):
    """make(k) -> matrix; the largest k <= start (stepping down) whose model residual <= 1e-12."""
    k = start
    while True:
        r = make(k)
        if model_residual(r) <= 1e-12 and abs(model_det(r) - 1.0) <= 1e-12:
            return r
        k -= 1


def worst_text(rng):
    ulp = 2.0**-52
    q = random_rotation(rng)
    stretch = lambda k: [[1.0 + k * ulp, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]  # noqa: E731
    shear = lambda k: [[1.0, k * 2.0**-60, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]  # noqa: E731
    mats = [
        ("stretch_x", largest_accepted(stretch, 2400), [0.0, 0.0, 0.0]),
        ("stretch_x_translated", largest_accepted(stretch, 2400), [100.0, -100.0, 100.0]),
        ("shear_xy", largest_accepted(shear, 900000), [0.0, 0.0, 0.0]),
        ("rotated_stretch", largest_accepted(lambda k: mat_mul(q, stretch(k)), 2400), [-100.0, 100.0, 37.5]),
    ]
    points = [[100.0, 100.0, 100.0], [-100.0, 100.0, -100.0], [100.0, -100.0, 100.0], [100.0, 0.0, 0.0]]
    points += [[rng.uniform(-100.0, 100.0) for _ in range(3)] for _ in range(4)]
    lines = []
    for name, r, t in mats:
        check_rotation_exact(r, name)
        rho = model_residual(r)
        assert 0.98e-12 <= rho <= 1e-12, (name, rho)
        a = (True, r, t, 1.0)
        inv = inverse(a)
        backs = []
        worst = 0.0
        for x in points:
            y = apply_point(inv, apply_point(a, x))
            err = sum((F(y[i]) - F(x[i])) ** 2 for i in range(3))
            bound = F(model_bound(rho, x, t))
            assert err <= bound * bound, (name, x)
            worst = max(worst, math.sqrt(err) / model_bound(rho, x, t))
            backs.append(hxs(y))
        print(f"{name}: residual {rho:.4e}, worst error / bound {worst:.3f}")
        case = {
            "name": name,
            "r": hxs(flat(r)),
            "t": hxs(t),
            "residual": hx(rho),
            "points": ",".join(hxs(x) for x in points),
            "back": ",".join(backs),
        }
        lines.append(json.dumps(case, sort_keys=True, separators=(",", ":")))
    return wrap(
        "REQ-004: rigid transforms at the largest accepted residual (model residual <= 1e-12); each "
        "point mapped forward and back; back = expected bits; error <= residual |x| + 48 eps (|x| + |t|).",
        lines,
    )


# ------------------------------------------------------------------ Euler

AXES = "XYZ"
SEQS = ["XYZ", "XZY", "YXZ", "YZX", "ZXY", "ZYX", "XYX", "XZX", "YXY", "YZY", "ZXZ", "ZYZ"]


def parity(i, j):
    return 1 if j == (i + 1) % 3 else -1


def elemental(axis, th):
    c, s = math.cos(th), math.sin(th)
    if axis == 0:
        return [[1, 0, 0], [0, c, -s], [0, s, c]]
    if axis == 1:
        return [[c, 0, s], [0, 1, 0], [-s, 0, c]]
    return [[c, -s, 0], [s, c, 0], [0, 0, 1]]


def mm(a, b):
    return [[sum(a[i][k] * b[k][j] for k in range(3)) for j in range(3)] for i in range(3)]


def euler_matrix(seq, kind_, ang):
    ax = [AXES.index(c) for c in seq]
    r = [elemental(ax[n], ang[n]) for n in range(3)]
    return mm(mm(r[0], r[1]), r[2]) if kind_ == "intrinsic" else mm(mm(r[2], r[1]), r[0])


def canonical_lock(seq, kind_, a, b, c):
    ax = [AXES.index(ch) for ch in seq]
    proper = ax[0] == ax[2]
    if proper:
        sign = 1 if b == 0.0 else -1
        return (a + sign * c, b, 0.0) if kind_ == "intrinsic" else (a + sign * c, b, 0.0)
    s = 1 if b > 0 else -1
    if kind_ == "intrinsic":
        return (a + s * parity(ax[0], ax[1]) * c, b, 0.0)
    return (a + s * parity(ax[2], ax[1]) * c, b, 0.0)


def euler_text(rng):
    lines = []
    for seq in SEQS:
        proper = seq[0] == seq[2]
        for kind_ in ("intrinsic", "extrinsic"):
            cases = []
            for n in range(4):
                a, c = rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)
                b = rng.uniform(0.2, 2.9) if proper else rng.uniform(-1.4, 1.4)
                cases.append((f"random_{n}", (a, b, c), (a, b, c), False, True))
            a, c = rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)
            b = 1e-6 if proper else math.pi / 2 - 1e-6
            cases.append(("near_lock", (a, b, c), (a, b, c), False, False))
            for n, b in enumerate((0.0, math.pi) if proper else (math.pi / 2, -math.pi / 2)):
                a, c = rng.uniform(-1.2, 1.2), rng.uniform(-1.2, 1.2)
                canon = canonical_lock(seq, kind_, a, b, c)
                m1, m2 = euler_matrix(seq, kind_, (a, b, c)), euler_matrix(seq, kind_, canon)
                diff = max(abs(m1[i][j] - m2[i][j]) for i in range(3) for j in range(3))
                assert diff <= 1e-14, (seq, kind_, b, diff)
                assert abs(canon[0]) < math.pi - 0.2
                cases.append((f"lock_{n}", (a, b, c), canon, True, True))
            for name, ang, exp, lock, check_angles in cases:
                case = {
                    "name": f"{seq}_{kind_}_{name}",
                    "seq": seq,
                    "kind": kind_,
                    "angles": hxs(ang),
                    "expect": hxs(exp),
                    "lock": lock,
                    "check_angles": check_angles,
                }
                lines.append(json.dumps(case, sort_keys=True, separators=(",", ":")))
    return wrap(
        "AC-4: Euler angles (hex IEEE 754 bits) for the 24 variants; expect = canonical angles "
        "(the input itself away from gimbal lock; third angle 0 at lock). check_angles false: "
        "round trip only.",
        lines,
    )


def build():
    rng = random.Random(11)  # one stream: the transforms first, then the Euler angles
    xform = xform_text(rng)
    euler = euler_text(rng)
    return {OUT_XFORM: xform, OUT_EULER: euler, OUT_WORST: worst_text(rng)}


def main():
    check = "--check" in sys.argv[1:]
    stale = 0
    for path, text in build().items():
        rel = path.relative_to(ROOT)
        if check:
            if not path.exists() or path.read_bytes() != text.encode("utf-8"):
                print(f"{rel} is stale; run python tools/fixtures/geo_xform_cases.py", file=sys.stderr)
                stale = 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(text.encode("utf-8"))
            print(f"wrote {rel}")
    if check and not stale:
        print("geo_xform_cases: up to date")
    return stale


if __name__ == "__main__":
    sys.exit(main())
