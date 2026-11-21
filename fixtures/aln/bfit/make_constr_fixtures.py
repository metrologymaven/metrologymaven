#!/usr/bin/env python3
"""Independent reference for ALN.BFIT.CONSTR (CC-BY-4.0 project data).

Writes, next to this script:

- locks-sweep/sweep.txt: the seeded (seed 5) 2,000-point freeform patch, the lock frame and
  start pose, and for each of the 64 lock patterns of (tx, ty, tz, a, b, c) the true
  parameters, the expected sine and cosine bits, the expected pose bits, the SHA-256 of the
  expected moved points, the expected full Jacobian rows at three points and the expected
  locks digest (AC-1, AC-2, AC-3, REQ-001, REQ-003, REQ-008);
- constr-weights/cases.txt: weight cases with the expected combined weight bits and weight
  digest, or the expected refusal (AC-4, AC-5, AC-6, REQ-006, REQ-007, REQ-008).

Every expected value is computed here from the rules pinned in the module documentation of
crates/fid-align-solve/src/constr (the leaf's Design made concrete), never by running the
Rust code:

- sine and cosine are computed exactly with Decimal Taylor series (80 digits) and rounded to
  the nearest binary64. No libm (the math module) is used anywhere. An angle is used only if
  both exact values lie at least 1/4 ulp away from a rounding midpoint, so that any
  implementation with error below 3/4 ulp (fid-num's fdlibm port is far inside) must return
  exactly these bits; the Rust test asserts it does;
- the pose, the moved points and the Jacobian follow the pinned expression order in Python
  binary64 arithmetic (CPython never fuses a multiply-add), so the expected bits are exact;
- the lock-frame and start rotations are exact Decimal Rz*Ry*Rx products rounded entrywise;
- the analytic Jacobian is cross-checked against central differences of the pose map;
- digests are SHA-256 (hashlib) of the canonical bytes pinned in the module docs.

Stdlib only. Run:  python fixtures/aln/bfit/make_constr_fixtures.py [--check]
"""

import hashlib
import struct
import sys
from decimal import Decimal, getcontext
from pathlib import Path

HERE = Path(__file__).resolve().parent
getcontext().prec = 80

MASK64 = (1 << 64) - 1


# ------------------------------------------------------------------ helpers


def hx(x):
    return "0x%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(b):
    return struct.unpack("<d", struct.pack("<Q", b))[0]


def le(xs):
    return b"".join(struct.pack("<d", x) for x in xs)


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
        # (next >> 11) * 2^-53: exact, in [0, 1).
        return (self.next() >> 11) * from_bits(0x3CA0000000000000)

    def uniform(self, lo, hi):
        return lo + (hi - lo) * self.unit()


# ------------------------------------------------------------------ exact trig


def d_atan_inv(n):
    # atan(1/n) by its Taylor series.
    x = Decimal(1) / n
    x2 = x * x
    term = x
    s = Decimal(0)
    k = 0
    eps = Decimal(10) ** -78
    while abs(term) > eps:
        s += term / (2 * k + 1) if k % 2 == 0 else -term / (2 * k + 1)
        term *= x2
        k += 1
    return s


PI = 16 * d_atan_inv(5) - 4 * d_atan_inv(239)


def d_sin(x):
    x = Decimal(x)
    term = x
    s = Decimal(0)
    k = 1
    eps = Decimal(10) ** -78
    while abs(term) > eps:
        s += term
        term = -term * x * x / ((k + 1) * (k + 2))
        k += 2
    return s


def d_cos(x):
    x = Decimal(x)
    term = Decimal(1)
    s = Decimal(0)
    k = 0
    eps = Decimal(10) ** -78
    while abs(term) > eps:
        s += term
        term = -term * x * x / ((k + 1) * (k + 2))
        k += 2
    return s


def round_d(d):
    """Nearest binary64 to the Decimal d and its distance from d in ulps of the gap toward d."""
    f = float(d)  # correctly rounded (CPython converts via its correctly rounded strtod)
    df = Decimal(f)
    if df == d:
        return f, Decimal(0)
    b = bits(f)
    if f > 0:
        nb = b + 1 if d > df else b - 1
    else:
        nb = b - 1 if d > df else b + 1
    gap = abs(Decimal(from_bits(nb)) - df)
    return f, abs(d - df) / gap


def safe_angle(x):
    """True when sin(x) and cos(x) are both >= 1/4 ulp away from a rounding midpoint."""
    for d in (d_sin(x), d_cos(x)):
        _, frac = round_d(d)
        if frac > Decimal("0.25"):
            return False
    return True


def sin_cos(x):
    return round_d(d_sin(x))[0], round_d(d_cos(x))[0]


# ------------------------------------------------------------------ pinned binary64 arithmetic


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
    """a after b, as fid-geo-frame Transform::compose for rigid transforms (s = 1)."""
    ra, ta = a
    rb, tb = b
    r = mat_mul(ra, rb)
    u = mat_vec(ra, tb)
    return (r, [1.0 * u[i] + ta[i] for i in range(3)])


def inverse(a):
    r, t = a
    si = 1.0 / 1.0
    u = mat_t_vec(r, t)
    return (transpose(r), [-(si * u[i]) for i in range(3)])


def apply_point(a, x):
    r, t = a
    u = mat_vec(r, x)
    return [1.0 * u[i] + t[i] for i in range(3)]


def rot_mats(sc):
    (sa, ca), (sb, cb), (sz, cz) = sc
    rx = [[1.0, 0.0, 0.0], [0.0, ca, -sa], [0.0, sa, ca]]
    ry = [[cb, 0.0, sb], [0.0, 1.0, 0.0], [-sb, 0.0, cb]]
    rz = [[cz, -sz, 0.0], [sz, cz, 0.0], [0.0, 0.0, 1.0]]
    drx = [[0.0, 0.0, 0.0], [0.0, -sa, -ca], [0.0, ca, -sa]]
    dry = [[-sb, 0.0, cb], [0.0, 0.0, 0.0], [-cb, 0.0, -sb]]
    drz = [[-sz, -cz, 0.0], [cz, -sz, 0.0], [0.0, 0.0, 0.0]]
    return rx, ry, rz, drx, dry, drz


def pose_of(params, sc, lock, start):
    rx, ry, rz, _, _, _ = rot_mats(sc)
    r = mat_mul(rz, mat_mul(ry, rx))
    d = (r, [params[0], params[1], params[2]])
    m = compose(inverse(lock), start)
    return compose(lock, compose(d, m))


def jac_rows(sc, lock, start, x):
    rx, ry, rz, drx, dry, drz = rot_mats(sc)
    rl = lock[0]
    m = compose(inverse(lock), start)
    q = apply_point(m, x)
    v1 = mat_vec(rx, q)
    v2 = mat_vec(ry, v1)
    cols = [[rl[0][k], rl[1][k], rl[2][k]] for k in range(3)]
    w = mat_vec(rl, mat_vec(rz, mat_vec(ry, mat_vec(drx, q))))
    cols.append(w)
    w = mat_vec(rl, mat_vec(rz, mat_vec(dry, v1)))
    cols.append(w)
    w = mat_vec(rl, mat_vec(drz, v2))
    cols.append(w)
    return [[cols[j][i] for j in range(6)] for i in range(3)]


def exact_rotation(a, b, c):
    """Rz(c) Ry(b) Rx(a) in Decimal, rounded entrywise to binary64."""
    sa, ca = d_sin(Decimal(a)), d_cos(Decimal(a))
    sb, cb = d_sin(Decimal(b)), d_cos(Decimal(b))
    sc, cc = d_sin(Decimal(c)), d_cos(Decimal(c))
    rx = [[1, 0, 0], [0, ca, -sa], [0, sa, ca]]
    ry = [[cb, 0, sb], [0, 1, 0], [-sb, 0, cb]]
    rz = [[cc, -sc, 0], [sc, cc, 0], [0, 0, 1]]

    def mm(p, q):
        return [[sum(Decimal(p[i][k]) * Decimal(q[k][j]) for k in range(3)) for j in range(3)] for i in range(3)]

    r = mm(rz, mm(ry, rx))
    return [[float(r[i][j]) for j in range(3)] for i in range(3)]


# ------------------------------------------------------------------ locks digest


def locks_digest(mask, frame_id, lock):
    r, t = lock
    flat = [r[i][j] for i in range(3) for j in range(3)] + list(t)
    flat = [0.0 if x == 0.0 else x for x in flat]  # -0.0 is written as +0.0
    pre = b"FIDLCK" + bytes([1, mask]) + b"ZYX" + bytes(frame_id) + le(flat)
    return "sha256:" + hashlib.sha256(pre).hexdigest()


# ------------------------------------------------------------------ sweep

N_POINTS = 2000
DEG5 = float(5 * PI / 180)
LOCK_ID = list(range(1, 17))
WORLD_ID = [0] * 16
LOCK = (exact_rotation("0.3", "-0.2", "0.7"), [0.12, -0.034, 0.25])
START = (exact_rotation("-0.05", "0.11", "1.2"), [0.5, 0.2, -0.1])
B_MAX = float(4 * PI / 9)  # 80 degrees, nearest binary64
JAC_POINTS = [0, 1000, 1999]


def patch(rng):
    pts = []
    for _ in range(N_POINTS):
        u = rng.uniform(-0.06, 0.07)
        v = rng.uniform(-0.045, 0.05)
        z = ((3.0 * u * u * u + 0.4 * u * v) + 0.9 * v * v) - 0.02 * u + 0.01
        pts.append([u, v, z])
    return pts


def draw_angle(rng):
    while True:
        x = rng.uniform(-DEG5, DEG5)
        if safe_angle(x):
            return x


def fd_check(params, lock, start, x, rows):
    # Central differences of the pose map against the analytic rows (float, loose tolerance).
    h = [1e-7, 1e-7, 1e-7, 1e-6, 1e-6, 1e-6]
    for j in range(6):
        pp = list(params)
        pm = list(params)
        pp[j] += h[j]
        pm[j] -= h[j]
        # finite differences need trig of perturbed angles: exact Decimal then rounded
        yp = apply_point(pose_of(pp, [sin_cos(pp[3 + k]) for k in range(3)], lock, start), x)
        ym = apply_point(pose_of(pm, [sin_cos(pm[3 + k]) for k in range(3)], lock, start), x)
        for i in range(3):
            fd = (yp[i] - ym[i]) / (2 * h[j])
            if abs(fd - rows[i][j]) > 1e-6:
                raise SystemExit("Jacobian self-check failed: col %d row %d %r vs %r" % (j, i, fd, rows[i][j]))


def sweep_text():
    rng = SplitMix64(5)
    pts = patch(rng)
    out = [
        "# ALN.BFIT.CONSTR locks sweep (seed 5). Generated by make_constr_fixtures.py; do not edit.",
        "# Parameter order tx ty tz a b c; mask bit k locks parameter k. CC-BY-4.0.",
        "seed 5",
        "n_points %d" % N_POINTS,
        "b_max %s" % hx(B_MAX),
        "lock_id %s" % bytes(LOCK_ID).hex(),
        "lock %s" % " ".join(hx(x) for x in [e for row in LOCK[0] for e in row] + LOCK[1]),
        "start %s" % " ".join(hx(x) for x in [e for row in START[0] for e in row] + START[1]),
    ]
    for p in pts:
        out.append("p %s" % " ".join(hx(x) for x in p))
    for mask in range(64):
        raw = [rng.uniform(-0.005, 0.005) for _ in range(3)] + [draw_angle(rng) for _ in range(3)]
        params = [0.0 if mask >> k & 1 else raw[k] for k in range(6)]
        if mask == 63:
            out.append("case 63 err all_dof_locked")
            continue
        sc = [sin_cos(params[3 + k]) for k in range(3)]
        pose = pose_of(params, sc, LOCK, START)
        moved = [apply_point(pose, x) for x in pts]
        tdig = "sha256:" + hashlib.sha256(le([c for y in moved for c in y])).hexdigest()
        fields = ["case", str(mask), "ok"]
        fields += ["params"] + [hx(x) for x in params]
        fields += ["sincos"] + [hx(v) for s_c in sc for v in s_c]
        fields += ["pose"] + [hx(x) for x in [e for row in pose[0] for e in row] + pose[1]]
        fields += ["targets", tdig]
        for idx in JAC_POINTS:
            rows = jac_rows(sc, LOCK, START, pts[idx])
            if mask == 0 and idx == 0:
                fd_check(params, LOCK, START, pts[idx], rows)
            fields += ["jac", str(idx)] + [hx(v) for row in rows for v in row]
        fields += ["locks", locks_digest(mask, LOCK_ID, LOCK)]
        out.append(" ".join(fields))
    ident = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0])
    for mask in (0, 0b011100, 0b111110):
        out.append("locks_default %d %s" % (mask, locks_digest(mask, WORLD_ID, ident)))
    return "\n".join(out) + "\n"


# ------------------------------------------------------------------ weights

MIN_POSITIVE = from_bits(0x0010000000000000)


def ok_weight(w):
    return w == w and abs(w) != float("inf") and (w == 0.0 or w >= MIN_POSITIVE)


def combine(n, per_point, regions):
    """Returns ('ok', combined) or ('err', code, detail) following the pinned check order."""
    if n == 0:
        return ("err", "invalid_weight", "all_zero")
    if per_point is not None:
        if len(per_point) != n:
            return ("err", "invalid_weight", "per_point_length")
        for i, w in enumerate(per_point):
            if not ok_weight(w):
                return ("err", "invalid_weight", "per_point %d" % i)
    for r, (w, members) in enumerate(regions):
        if not ok_weight(w):
            return ("err", "invalid_weight", "region %d" % r)
        prev = -1
        for i in members:
            if i >= n or i <= prev:
                return ("err", "invalid_region", "region %d" % r)
            prev = i
    out = [1.0] * n if per_point is None else [0.0 if w == 0.0 else w for w in per_point]
    for w, members in regions:
        rw = 0.0 if w == 0.0 else w
        for i in members:
            a = out[i]
            p = a * rw
            bad = (not ok_weight(p)) or (p == 0.0 and a != 0.0 and rw != 0.0)
            if bad:
                return ("err", "invalid_weight", "combined %d" % i)
            out[i] = p
    if not any(w > 0.0 for w in out):
        return ("err", "invalid_weight", "all_zero")
    return ("ok", out)


def weight_digest(ws):
    return "sha256:" + hashlib.sha256(le(ws)).hexdigest()


def hand_cases():
    rng = SplitMix64(7)
    w10 = [rng.uniform(0.5, 2.0) for _ in range(10)]
    nan = from_bits(0x7FF8000000000000)
    inf = from_bits(0x7FF0000000000000)
    sub = from_bits(0x0000000000000001)
    return [
        # AC-4: overlapping regions 2 and 0.5 (points 4, 5 in both), a zero region excludes 8.
        ("ac4_overlap", 10, w10, [(2.0, [2, 3, 4, 5]), (0.5, [4, 5, 6]), (0.0, [8])]),
        ("regions_only", 6, None, [(3.0, [0, 1]), (0.25, [1, 2, 5])]),
        ("per_point_only", 4, [1.0, 0.0, 2.5, 1e-3], []),
        ("uniform", 3, None, []),
        ("negative_zero", 3, [-0.0, 1.0, 2.0], [(-0.0, [2])]),
        ("empty_region", 3, None, [(5.0, [])]),
        # AC-5 and REQ-007 refusals.
        ("negative_weight", 5, [1.0, 1.0, 1.0, -1.0, 1.0], []),
        ("nan_weight", 3, [1.0, nan, 1.0], []),
        ("inf_region", 4, None, [(1.0, [0]), (inf, [1])]),
        ("negative_region", 4, [1.0, 1.0, -2.0, 1.0], [(-1.0, [1])]),
        ("subnormal_weight", 2, [sub, 1.0], []),
        ("all_zero", 3, [0.0, 0.0, 0.0], []),
        ("all_zero_by_region", 3, [1.0, 0.0, 1.0], [(0.0, [0, 2])]),
        ("no_points", 0, None, []),
        ("per_point_length", 3, [1.0, 1.0], []),
        ("member_out_of_range", 3, None, [(2.0, [0, 3])]),
        ("member_not_increasing", 4, None, [(2.0, [0, 2, 2])]),
        ("overflow", 3, [1.0, 1e300, 1.0], [(1e10, [1])]),
        ("underflow", 3, [1.0, 1e-300, 1.0], [(1e-10, [0, 1])]),
    ]


def seeded_case():
    """AC-6: 100,000 points and 16 regions, from SplitMix64 seed 11 (rule in the Rust test)."""
    rng = SplitMix64(11)
    n = 100000
    two_m52 = from_bits(0x3CB0000000000000)
    pp = [(rng.next() >> 11) * two_m52 for _ in range(n)]
    regions = []
    for r in range(16):
        w = 0.0 if r == 15 else (rng.next() >> 11) * two_m52
        members = [i for i in range(n) if rng.next() % 4 == 0]
        regions.append((w, members))
    return n, pp, regions


def weights_text():
    out = [
        "# ALN.BFIT.CONSTR weight cases. Generated by make_constr_fixtures.py; do not edit. CC-BY-4.0.",
    ]
    for name, n, pp, regions in hand_cases():
        out.append("case %s" % name)
        out.append("n %d" % n)
        out.append("per_point none" if pp is None else "per_point " + " ".join(hx(w) for w in pp))
        for w, members in regions:
            out.append(("region %s %s" % (hx(w), " ".join(str(i) for i in members))).rstrip())
        res = combine(n, pp, regions)
        if res[0] == "ok":
            out.append("expect ok %s %s" % (weight_digest(res[1]), " ".join(hx(w) for w in res[1])))
        else:
            out.append("expect err %s %s" % (res[1], res[2]))
        out.append("end")
    n, pp, regions = seeded_case()
    res = combine(n, pp, regions)
    if res[0] != "ok":
        raise SystemExit("seeded case unexpectedly refused")
    used = sum(1 for w in res[1] if w > 0.0)
    out.append("seeded 11 %d 16 %d %s" % (n, used, weight_digest(res[1])))
    return "\n".join(out) + "\n"


# ------------------------------------------------------------------ main


def outputs():
    return {
        "locks-sweep/sweep.txt": sweep_text(),
        "constr-weights/cases.txt": weights_text(),
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, text in outputs().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print("STALE", name)
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
            print("wrote", name)
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
