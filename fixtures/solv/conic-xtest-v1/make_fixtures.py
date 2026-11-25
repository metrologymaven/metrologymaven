#!/usr/bin/env python3
"""Independent seeded fixture generator for KRN.SOLV.CONIC.XTEST (`fixtures/solv/conic-xtest-v1`).

Generates 165 conic optimisation problems (55 LP, 55 QP, 55 SOCP) in canonical JSON with
exact IEEE-754 f64 hex strings and independently proven offline reference optima (exact
rational convex-hull widths, exact KKT primal-dual certificates, and exact contact-hull
circumscribed/inscribed/projection geometry — no external solver dependency).

Constraint counts span 3 to 5000 across minimum-zone straightness and flatness, constrained
least-squares and datum-plane projections, minimum circumscribed circles and spheres,
maximum inscribed balls, generic multi-cone problems, and primal/dual infeasible cases.

License: CC-BY-4.0 (per spec/krn/solv/conic/xtest/NODE.md).
Run with --check to verify committed fixtures are byte-identical to generator output.
"""

from __future__ import annotations

from fractions import Fraction
import hashlib
import json
import math
from pathlib import Path
import random
import struct
import sys

HERE = Path(__file__).resolve().parent
REPO_ROOT = HERE.parents[2]
MASTER_SEED = 20261122


def f64_hex(x: float) -> str:
    """IEEE-754 binary64 exact bit representation as 16 lowercase hex chars."""
    v = float(x)
    if not math.isfinite(v):
        raise ValueError(f"non-finite float: {v}")
    return struct.pack(">d", v).hex()


def csc_from_triplets(
    nrows: int, ncols: int, triplets: list[tuple[int, int, float]]
) -> dict[str, object]:
    """Builds a sorted CSC matrix dict with IEEE-754 hex values from (row, col, val) triplets."""
    cols: list[dict[int, float]] = [{} for _ in range(ncols)]
    for r, c, v in triplets:
        if r < 0 or r >= nrows or c < 0 or c >= ncols:
            raise ValueError(f"out-of-range triplet ({r}, {c}) for {nrows}x{ncols}")
        if v == 0.0:
            continue
        cols[c][r] = cols[c].get(r, 0.0) + float(v)

    colptr = [0]
    rowval: list[int] = []
    nzval: list[str] = []
    for c in range(ncols):
        for r in sorted(cols[c]):
            v = cols[c][r]
            if v != 0.0:
                rowval.append(r)
                nzval.append(f64_hex(v))
        colptr.append(len(rowval))
    return {
        "nrows": nrows,
        "ncols": ncols,
        "colptr": colptr,
        "rowval": rowval,
        "nzval": nzval,
    }


def make_problem(
    pid: str,
    pclass: str,
    family: str,
    n: int,
    m: int,
    p_csc: dict[str, object] | None,
    q: list[float],
    a_csc: dict[str, object],
    b: list[float],
    cones: list[tuple[str, int]],
    expected_status: str,
    reference_objective: float | None,
) -> dict[str, object]:
    assert len(q) == n, f"{pid}: len(q)={len(q)} != n={n}"
    assert len(b) == m, f"{pid}: len(b)={len(b)} != m={m}"
    assert sum(d for _, d in cones) == m, f"{pid}: cone sum != m={m}"
    return {
        "id": pid,
        "class": pclass,
        "family": family,
        "n": n,
        "m": m,
        "p": p_csc,
        "q": [f64_hex(v) for v in q],
        "a": a_csc,
        "b": [f64_hex(v) for v in b],
        "cones": [{"kind": k, "dim": d} for k, d in cones],
        "expected_status": expected_status,
        "reference_objective": None if reference_objective is None else f64_hex(reference_objective),
    }


# ------------------------------------------------------------------ exact straightness solver


def _cross(o: tuple[Fraction, Fraction], a: tuple[Fraction, Fraction], b: tuple[Fraction, Fraction]) -> Fraction:
    return (a[0] - o[0]) * (b[1] - o[1]) - (a[1] - o[1]) * (b[0] - o[0])


def exact_straightness_half_width(pts: list[tuple[Fraction, Fraction]]) -> Fraction:
    """Exact minimum-zone straightness half-width h* via convex hulls in Fraction arithmetic."""
    spts = sorted(set(pts))
    lower: list[tuple[Fraction, Fraction]] = []
    for p in spts:
        while len(lower) >= 2 and _cross(lower[-2], lower[-1], p) <= 0:
            lower.pop()
        lower.append(p)
    upper: list[tuple[Fraction, Fraction]] = []
    for p in reversed(spts):
        while len(upper) >= 2 and _cross(upper[-2], upper[-1], p) <= 0:
            upper.pop()
        upper.append(p)
    upper.reverse()

    slopes: set[Fraction] = {Fraction(0)}
    for hull in (lower, upper):
        for i in range(len(hull) - 1):
            dx = hull[i + 1][0] - hull[i][0]
            if dx != 0:
                slopes.add((hull[i + 1][1] - hull[i][1]) / dx)

    hull_pts = list(set(lower + upper))
    best: Fraction | None = None
    for a in slopes:
        vals = [y - a * x for x, y in hull_pts]
        w = max(vals) - min(vals)
        if best is None or w < best:
            best = w
    assert best is not None
    return best / 2


# ------------------------------------------------------------------ LP family (55 problems)


def build_lp_problems() -> list[dict[str, object]]:
    out: list[dict[str, object]] = []
    idx = 0

    # A1: Minimum-zone straightness (18 problems, 8..5000 constraints)
    straight_sizes = [4, 5, 6, 8, 10, 12, 16, 20, 25, 30, 40, 50, 64, 80, 100, 150, 300, 2500]
    for k, n_pts in enumerate(straight_sizes):
        idx += 1
        pid = f"lp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 1000 + idx)
        slope = Fraction(rng.randint(-15, 15), 10)
        intercept = Fraction(rng.randint(-20, 20), 10)
        amp = Fraction(rng.randint(4, 15), 10)
        pts: list[tuple[Fraction, Fraction]] = []
        for i in range(n_pts):
            t = Fraction(i * 10, max(1, n_pts - 1))
            if i == 0:
                res = amp
            elif i == n_pts // 2:
                res = -amp
            elif i == n_pts - 1:
                res = amp
            else:
                # Seeded wavy + pseudo-random profile within [-amp, amp]
                res = Fraction(rng.randint(-95, 95), 100) * amp
            y = slope * t + intercept + res
            pts.append((t, y))
        h_star = exact_straightness_half_width(pts)
        assert float(h_star) >= 0.05

        triplets: list[tuple[int, int, float]] = []
        b: list[float] = []
        for i, (t, y) in enumerate(pts):
            tf = float(t)
            yf = float(y)
            r0 = 2 * i
            r1 = 2 * i + 1
            triplets.extend([(r0, 0, tf), (r0, 1, 1.0), (r0, 2, -1.0)])
            b.append(yf)
            triplets.extend([(r1, 0, -tf), (r1, 1, -1.0), (r1, 2, -1.0)])
            b.append(-yf)
        m = 2 * n_pts
        out.append(
            make_problem(
                pid,
                "lp",
                "straightness_mz",
                3,
                m,
                None,
                [0.0, 0.0, 1.0],
                csc_from_triplets(m, 3, triplets),
                b,
                [("nonneg", m)],
                "solved",
                float(h_star),
            )
        )

    # A2: Minimum-zone flatness (16 problems, 10..2000 constraints)
    flat_sizes = [5, 6, 8, 10, 14, 18, 24, 32, 48, 64, 80, 100, 150, 200, 400, 1000]
    for k, n_pts in enumerate(flat_sizes):
        idx += 1
        pid = f"lp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 2000 + idx)
        a0 = rng.randint(-12, 12) / 10.0
        b0 = rng.randint(-12, 12) / 10.0
        c0 = rng.randint(-10, 10) / 10.0
        h_star = rng.randint(2, 15) / 10.0
        # Seeded affine transform of the canonical equioscillating 3-vs-1 pattern
        sx = rng.randint(5, 20) / 10.0
        sy = rng.randint(5, 20) / 10.0
        sh = rng.randint(-4, 4) / 10.0
        ox = rng.randint(-5, 5) / 10.0
        oy = rng.randint(-5, 5) / 10.0

        def map_uv(u0: float, v0: float) -> tuple[float, float]:
            return (sx * u0 + sh * v0 + ox, sy * v0 + oy)

        # 3 positive contacts forming triangle enclosing 1 negative contact at (0, 0)
        contacts = [
            (map_uv(-1.0, -1.0), +h_star),
            (map_uv(2.0, -1.0), +h_star),
            (map_uv(-1.0, 2.0), +h_star),
            (map_uv(0.0, 0.0), -h_star),
        ]
        triplets = []
        b = []
        pts3: list[tuple[float, float, float]] = []
        for (u, v), res in contacts:
            pts3.append((u, v, a0 * u + b0 * v + c0 + res))
        for _ in range(n_pts - 4):
            u0 = rng.randint(-200, 200) / 100.0
            v0 = rng.randint(-200, 200) / 100.0
            u, v = map_uv(u0, v0)
            res = (rng.randint(-85, 85) / 100.0) * h_star
            pts3.append((u, v, a0 * u + b0 * v + c0 + res))

        for i, (u, v, z) in enumerate(pts3):
            r0 = 2 * i
            r1 = 2 * i + 1
            triplets.extend([(r0, 0, u), (r0, 1, v), (r0, 2, 1.0), (r0, 3, -1.0)])
            b.append(z)
            triplets.extend([(r1, 0, -u), (r1, 1, -v), (r1, 2, -1.0), (r1, 3, -1.0)])
            b.append(-z)
        m = 2 * n_pts
        out.append(
            make_problem(
                pid,
                "lp",
                "flatness_mz",
                4,
                m,
                None,
                [0.0, 0.0, 0.0, 1.0],
                csc_from_triplets(m, 4, triplets),
                b,
                [("nonneg", m)],
                "solved",
                h_star,
            )
        )

    # A3: Generic KKT-certified LPs (14 problems, m = 3 .. 300, mixing zero + nonneg cones)
    gen_specs = [
        (2, 1, 2),   # m = 3 (smallest constraint count in spec!)
        (2, 0, 4),   # m = 4
        (3, 1, 4),   # m = 5
        (3, 0, 6),   # m = 6
        (4, 1, 7),   # m = 8
        (4, 2, 8),   # m = 10
        (5, 1, 11),  # m = 12
        (6, 2, 14),  # m = 16
        (8, 2, 22),  # m = 24
        (10, 2, 30), # m = 32
        (12, 3, 45), # m = 48
        (15, 3, 77), # m = 80
        (18, 4, 146),# m = 150
        (20, 5, 295),# m = 300
    ]
    for n, m_zero, m_nonneg in gen_specs:
        idx += 1
        pid = f"lp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 3000 + idx)
        # Pick non-zero integer/half-integer primal optimum x*
        x_star = [Fraction(rng.choice([-3, -2, -1, 1, 2, 3]), 2) for _ in range(n)]
        rows_zero: list[tuple[list[Fraction], Fraction, Fraction]] = []
        rows_nonneg: list[tuple[list[Fraction], Fraction, Fraction]] = []

        # Build independent rows for Zero (first m_zero) and active Nonneg (next n - m_zero)
        # Using a diagonally dominant lower-triangular + random integer basis ensures boundedness.
        for i in range(n):
            row = [Fraction(0)] * n
            row[i] = Fraction(2)
            for j in range(i):
                if rng.random() < 0.4:
                    row[j] = Fraction(rng.choice([-1, 1]), 2)
            rhs = sum(row[j] * x_star[j] for j in range(n))
            if i < m_zero:
                y_i = Fraction(rng.choice([-2, -1, 1, 2]), 2)
                rows_zero.append((row, rhs, y_i))
            else:
                # Active inequality: s_i = 0, y_i > 0
                y_i = Fraction(rng.randint(1, 4), 2)
                rows_nonneg.append((row, rhs, y_i))

        # Add remaining inactive Nonneg rows (s_i > 0, y_i = 0)
        extra = m_nonneg - len(rows_nonneg)
        assert extra >= 0
        for _ in range(extra):
            row = [Fraction(0)] * n
            # Sparse row with 2-3 nonzeros
            cols_pick = rng.sample(range(n), min(n, 3))
            for c in cols_pick:
                row[c] = Fraction(rng.choice([-2, -1, 1, 2]), 2)
            slack = Fraction(rng.randint(1, 5), 2)
            rhs = sum(row[j] * x_star[j] for j in range(n)) + slack
            rows_nonneg.append((row, rhs, Fraction(0)))

        all_rows = rows_zero + rows_nonneg
        q_frac = [
            -sum(row[j] * y_i for row, _, y_i in all_rows)
            for j in range(n)
        ]
        obj_frac = sum(q_frac[j] * x_star[j] for j in range(n))
        # Ensure |obj| >= 0.1 by shifting one active y_i if needed
        if abs(float(obj_frac)) < 0.1:
            row0, rhs0, y0 = rows_nonneg[0]
            rows_nonneg[0] = (row0, rhs0, y0 + Fraction(3))
            all_rows = rows_zero + rows_nonneg
            q_frac = [-sum(row[j] * y_i for row, _, y_i in all_rows) for j in range(n)]
            obj_frac = sum(q_frac[j] * x_star[j] for j in range(n))
        assert abs(float(obj_frac)) >= 0.1

        triplets = []
        b = []
        for r_idx, (row, rhs, _) in enumerate(all_rows):
            for c_idx, val in enumerate(row):
                if val != 0:
                    triplets.append((r_idx, c_idx, float(val)))
            b.append(float(rhs))
        cones: list[tuple[str, int]] = []
        if m_zero > 0:
            cones.append(("zero", m_zero))
        if m_nonneg > 0:
            cones.append(("nonneg", m_nonneg))
        m = m_zero + m_nonneg
        out.append(
            make_problem(
                pid,
                "lp",
                "generic_lp",
                n,
                m,
                None,
                [float(v) for v in q_frac],
                csc_from_triplets(m, n, triplets),
                b,
                cones,
                "solved",
                float(obj_frac),
            )
        )

    # A4: Infeasible LPs (4 primal_infeasible + 3 dual_infeasible)
    primal_inf_lps = [
        # x0 <= 1, -x0 <= -2 (i.e. x0 >= 2), x1 <= 5
        (2, [(0, 0, 1.0), (1, 0, -1.0), (2, 1, 1.0)], [1.0, -2.0, 5.0], [("nonneg", 3)], [1.0, 1.0]),
        # x0 + x1 = 1, x0 + x1 = 3, x0 >= 0
        (2, [(0, 0, 1.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 1.0), (2, 0, -1.0)], [1.0, 3.0, 0.0], [("zero", 2), ("nonneg", 1)], [1.0, 0.5]),
        # x0 >= 3, x1 >= 3, x0 + x1 <= 4
        (2, [(0, 0, -1.0), (1, 1, -1.0), (2, 0, 1.0), (2, 1, 1.0)], [-3.0, -3.0, 4.0], [("nonneg", 3)], [1.0, 2.0]),
        # 3D conflicting slabs: x0+x1+x2 >= 10, x0<=1, x1<=1, x2<=1
        (3, [(0, 0, -1.0), (0, 1, -1.0), (0, 2, -1.0), (1, 0, 1.0), (2, 1, 1.0), (3, 2, 1.0)], [-10.0, 1.0, 1.0, 1.0], [("nonneg", 4)], [1.0, 1.0, 1.0]),
    ]
    for n, trips, b_vec, cones, q_vec in primal_inf_lps:
        idx += 1
        m = len(b_vec)
        out.append(
            make_problem(
                f"lp-{idx:03d}",
                "lp",
                "infeasible_lp",
                n,
                m,
                None,
                q_vec,
                csc_from_triplets(m, n, trips),
                b_vec,
                cones,
                "primal_infeasible",
                None,
            )
        )

    dual_inf_lps = [
        # min -x0 s.t. x0 >= 0, x1 >= 0, x1 <= 5
        (2, [(0, 0, -1.0), (1, 1, -1.0), (2, 1, 1.0)], [0.0, 0.0, 5.0], [("nonneg", 3)], [-1.0, 1.0]),
        # min -x0 - 2*x1 s.t. x0 - x1 = 0, x0 >= 0, x1 >= 0
        (2, [(0, 0, 1.0), (0, 1, -1.0), (1, 0, -1.0), (2, 1, -1.0)], [0.0, 0.0, 0.0], [("zero", 1), ("nonneg", 2)], [-1.0, -2.0]),
        # min -x2 s.t. x0+x1 <= 4, x0 >= 0, x2 >= 1
        (3, [(0, 0, 1.0), (0, 1, 1.0), (1, 0, -1.0), (2, 2, -1.0)], [4.0, 0.0, -1.0], [("nonneg", 3)], [0.0, 0.0, -1.0]),
    ]
    for n, trips, b_vec, cones, q_vec in dual_inf_lps:
        idx += 1
        m = len(b_vec)
        out.append(
            make_problem(
                f"lp-{idx:03d}",
                "lp",
                "unbounded_lp",
                n,
                m,
                None,
                q_vec,
                csc_from_triplets(m, n, trips),
                b_vec,
                cones,
                "dual_infeasible",
                None,
            )
        )

    assert len(out) == 55
    return out


# ------------------------------------------------------------------ QP family (55 problems)


def build_qp_problems() -> list[dict[str, object]]:
    out: list[dict[str, object]] = []
    idx = 0

    def build_kkt_qp(
        pid: str,
        family: str,
        n: int,
        m_zero: int,
        m_nonneg: int,
        identity_p: bool,
        seed: int,
    ) -> dict[str, object]:
        rng = random.Random(seed)
        # Build SPD upper-triangular P (either I or diagonal + positive off-diagonal banded)
        p_triplets: list[tuple[int, int, float]] = []
        p_full = [[Fraction(0)] * n for _ in range(n)]
        if identity_p:
            for i in range(n):
                p_full[i][i] = Fraction(1)
                p_triplets.append((i, i, 1.0))
        else:
            # Tridiagonal SPD: diagonal in [3, 6], super-diagonal in [-1/2, 1/2]
            for i in range(n):
                diag = Fraction(rng.randint(3, 6), 1)
                p_full[i][i] = diag
                p_triplets.append((i, i, float(diag)))
                if i + 1 < n and rng.random() < 0.7:
                    off = Fraction(rng.choice([-1, 1]), 2)
                    p_full[i][i + 1] = off
                    p_full[i + 1][i] = off
                    p_triplets.append((i, i + 1, float(off)))

        x_star = [Fraction(rng.choice([-3, -2, -1, 1, 2, 3]), 2) for _ in range(n)]
        all_rows: list[tuple[list[tuple[int, Fraction]], Fraction, Fraction]] = []

        for r in range(m_zero):
            c0 = r % n
            row_sparse: list[tuple[int, Fraction]] = [(c0, Fraction(1))]
            if n > 1:
                c1 = (c0 + 1) % n
                row_sparse.append((c1, Fraction(rng.choice([-1, 1]), 2)))
            rhs = sum(val * x_star[c] for c, val in row_sparse)
            y_r = Fraction(rng.choice([-2, -1, 1, 2]), 2)
            all_rows.append((row_sparse, rhs, y_r))

        n_active = min(m_nonneg, max(1, n // 2))
        for r in range(m_nonneg):
            c0 = r % n
            if n > 1 and r >= n:
                c1 = (c0 + 1 + (r // n) % (n - 1)) % n
                row_sparse = [(c0, Fraction(rng.choice([-2, -1, 1, 2]), 2)), (c1, Fraction(rng.choice([-1, 1]), 2))]
            else:
                row_sparse = [(c0, Fraction(rng.choice([-1, 1]), 1))]
            ax = sum(val * x_star[c] for c, val in row_sparse)
            if r < n_active:
                y_r = Fraction(rng.randint(1, 3), 2)
                rhs = ax
            else:
                y_r = Fraction(0)
                rhs = ax + Fraction(rng.randint(1, 6), 2)
            all_rows.append((row_sparse, rhs, y_r))

        # q = -(P x* + A^T y*)
        px = [sum(p_full[i][j] * x_star[j] for j in range(max(0, i - 1), min(n, i + 2))) for i in range(n)]
        aty = [Fraction(0)] * n
        for row_sparse, _, y_r in all_rows:
            if y_r != 0:
                for c, val in row_sparse:
                    aty[c] += val * y_r
        q_frac = [-(px[i] + aty[i]) for i in range(n)]
        quad = sum(x_star[i] * px[i] for i in range(n)) / 2
        lin = sum(q_frac[i] * x_star[i] for i in range(n))
        obj_frac = quad + lin
        if abs(float(obj_frac)) < 0.1:
            # Shift x_star[0] and rebuild cleanly (rare)
            x_star[0] += Fraction(2)
            return build_kkt_qp(pid, family, n, m_zero, m_nonneg, identity_p, seed + 9973)

        a_triplets: list[tuple[int, int, float]] = []
        b_vec: list[float] = []
        for r_idx, (row_sparse, rhs, _) in enumerate(all_rows):
            for c, val in row_sparse:
                a_triplets.append((r_idx, c, float(val)))
            b_vec.append(float(rhs))
        cones: list[tuple[str, int]] = []
        if m_zero > 0:
            cones.append(("zero", m_zero))
        if m_nonneg > 0:
            cones.append(("nonneg", m_nonneg))
        m = m_zero + m_nonneg
        return make_problem(
            pid,
            "qp",
            family,
            n,
            m,
            csc_from_triplets(n, n, p_triplets),
            [float(v) for v in q_frac],
            csc_from_triplets(m, n, a_triplets),
            b_vec,
            cones,
            "solved",
            float(obj_frac),
        )

    # B1: Constrained least-squares / tolerance envelope QPs (25 problems, m = 3..2000)
    b1_specs = [
        (2, 0, 3), (2, 1, 2), (3, 0, 4), (3, 1, 4), (4, 0, 6),
        (4, 1, 7), (5, 0, 10), (5, 2, 10), (6, 1, 15), (8, 0, 20),
        (8, 2, 24), (10, 2, 30), (12, 0, 40), (12, 3, 45), (15, 2, 60),
        (16, 3, 80), (18, 4, 100), (20, 0, 140), (24, 4, 180), (28, 4, 240),
        (32, 4, 320), (36, 5, 450), (40, 5, 650), (48, 6, 994), (60, 10, 1990),
    ]
    for n, mz, mn in b1_specs:
        idx += 1
        out.append(build_kkt_qp(f"qp-{idx:03d}", "constrained_lsq_qp", n, mz, mn, False, MASTER_SEED + 4000 + idx))

    # B2: Point-to-polytope / datum-plane projection QPs (15 problems, P = I)
    b2_specs = [
        (2, 0, 3), (2, 1, 3), (3, 0, 5), (3, 1, 5), (3, 1, 8),
        (4, 0, 10), (4, 2, 10), (5, 1, 14), (6, 2, 18), (6, 0, 24),
        (8, 2, 32), (10, 2, 48), (12, 3, 72), (15, 3, 120), (20, 4, 200),
    ]
    for n, mz, mn in b2_specs:
        idx += 1
        out.append(build_kkt_qp(f"qp-{idx:03d}", "datum_projection_qp", n, mz, mn, True, MASTER_SEED + 5000 + idx))

    # B3: Regularised envelope QPs (10 problems, m = 20..500)
    b3_specs = [
        (3, 0, 20), (4, 0, 30), (4, 0, 45), (5, 0, 65), (6, 0, 90),
        (8, 0, 120), (10, 0, 180), (12, 0, 260), (14, 0, 360), (16, 0, 500),
    ]
    for n, mz, mn in b3_specs:
        idx += 1
        out.append(build_kkt_qp(f"qp-{idx:03d}", "regularised_envelope_qp", n, mz, mn, False, MASTER_SEED + 6000 + idx))

    # B4: Infeasible QPs (3 primal_infeasible + 2 dual_infeasible)
    primal_inf_qps = [
        (2, [(0, 0, 2.0), (1, 1, 2.0)], [(0, 0, 1.0), (1, 0, -1.0), (2, 1, 1.0)], [1.0, -3.0, 2.0], [("nonneg", 3)], [0.0, 0.0]),
        (2, [(0, 0, 1.0), (1, 1, 1.0)], [(0, 0, 1.0), (0, 1, 1.0), (1, 0, 1.0), (1, 1, 1.0), (2, 0, -1.0)], [1.0, 5.0, 0.0], [("zero", 2), ("nonneg", 1)], [-1.0, -1.0]),
        (3, [(0, 0, 1.0), (1, 1, 1.0), (2, 2, 1.0)], [(0, 0, -1.0), (1, 1, -1.0), (2, 2, -1.0), (3, 0, 1.0), (3, 1, 1.0), (3, 2, 1.0)], [-2.0, -2.0, -2.0, 4.0], [("nonneg", 4)], [0.0, 0.0, 0.0]),
    ]
    for n, p_trips, a_trips, b_vec, cones, q_vec in primal_inf_qps:
        idx += 1
        m = len(b_vec)
        out.append(
            make_problem(
                f"qp-{idx:03d}",
                "qp",
                "infeasible_qp",
                n,
                m,
                csc_from_triplets(n, n, p_trips),
                q_vec,
                csc_from_triplets(m, n, a_trips),
                b_vec,
                cones,
                "primal_infeasible",
                None,
            )
        )

    dual_inf_qps = [
        # P = diag(0, 2), min -x0 + x1^2 s.t. x0 >= 0, x1 >= 0, x1 <= 4
        (2, [(1, 1, 2.0)], [(0, 0, -1.0), (1, 1, -1.0), (2, 1, 1.0)], [0.0, 0.0, 4.0], [("nonneg", 3)], [-1.0, 0.0]),
        # P = diag(1, 0, 2), min 0.5*x0^2 - 2*x1 + x2^2 s.t. x0>=0, x1>=1, x2>=0
        (3, [(0, 0, 1.0), (2, 2, 2.0)], [(0, 0, -1.0), (1, 1, -1.0), (2, 2, -1.0)], [0.0, -1.0, 0.0], [("nonneg", 3)], [0.0, -2.0, 0.0]),
    ]
    for n, p_trips, a_trips, b_vec, cones, q_vec in dual_inf_qps:
        idx += 1
        m = len(b_vec)
        out.append(
            make_problem(
                f"qp-{idx:03d}",
                "qp",
                "unbounded_qp",
                n,
                m,
                csc_from_triplets(n, n, p_trips),
                q_vec,
                csc_from_triplets(m, n, a_trips),
                b_vec,
                cones,
                "dual_infeasible",
                None,
            )
        )

    assert len(out) == 55
    return out


# ------------------------------------------------------------------ SOCP family (55 problems)


def build_socp_problems() -> list[dict[str, object]]:
    out: list[dict[str, object]] = []
    idx = 0

    # C1: Minimum Circumscribed Circle (MCC, 2D) (15 problems, 9..1800 constraints)
    mcc_sizes = [3, 4, 5, 6, 8, 10, 12, 16, 24, 32, 50, 75, 100, 200, 600]
    for n_pts in mcc_sizes:
        idx += 1
        pid = f"socp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 7000 + idx)
        cx = rng.randint(-50, 50) / 10.0
        cy = rng.randint(-50, 50) / 10.0
        r_star = rng.randint(10, 80) / 10.0
        theta0 = rng.uniform(0.0, 2.0 * math.pi)
        # 3 contact points forming an acute triangle strictly enclosing (cx, cy)
        angles = [
            theta0,
            theta0 + 2.0 * math.pi / 3.0 + rng.uniform(-0.2, 0.2),
            theta0 + 4.0 * math.pi / 3.0 + rng.uniform(-0.2, 0.2),
        ]
        pts: list[tuple[float, float]] = [
            (cx + r_star * math.cos(th), cy + r_star * math.sin(th)) for th in angles
        ]
        for _ in range(n_pts - 3):
            rad = r_star * rng.uniform(0.05, 0.90)
            th = rng.uniform(0.0, 2.0 * math.pi)
            pts.append((cx + rad * math.cos(th), cy + rad * math.sin(th)))
        rng.shuffle(pts)

        # x = [cx, cy, R], min R, b - A x = [R, cx - u, cy - v] in SOC(3)
        triplets: list[tuple[int, int, float]] = []
        b_vec: list[float] = []
        cones: list[tuple[str, int]] = []
        for i, (u, v) in enumerate(pts):
            base = 3 * i
            triplets.extend([(base, 2, -1.0), (base + 1, 0, -1.0), (base + 2, 1, -1.0)])
            b_vec.extend([0.0, -u, -v])
            cones.append(("soc", 3))
        m = 3 * n_pts
        out.append(
            make_problem(
                pid,
                "socp",
                "mcc_2d",
                3,
                m,
                None,
                [0.0, 0.0, 1.0],
                csc_from_triplets(m, 3, triplets),
                b_vec,
                cones,
                "solved",
                r_star,
            )
        )

    # C2: Minimum Circumscribed Sphere (MCS, 3D) (15 problems, 16..1200 constraints)
    mcs_sizes = [4, 5, 6, 8, 10, 12, 16, 20, 30, 40, 60, 80, 100, 150, 300]
    inv_sqrt3 = 1.0 / math.sqrt(3.0)
    base_tet = [
        (inv_sqrt3, inv_sqrt3, inv_sqrt3),
        (inv_sqrt3, -inv_sqrt3, -inv_sqrt3),
        (-inv_sqrt3, inv_sqrt3, -inv_sqrt3),
        (-inv_sqrt3, -inv_sqrt3, inv_sqrt3),
    ]
    for n_pts in mcs_sizes:
        idx += 1
        pid = f"socp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 8000 + idx)
        cx = rng.randint(-40, 40) / 10.0
        cy = rng.randint(-40, 40) / 10.0
        cz = rng.randint(-40, 40) / 10.0
        r_star = rng.randint(10, 60) / 10.0
        # Seeded 3D rotation via Euler angles (orthogonal, preserves unit sphere & centroid=0)
        a1 = rng.uniform(0.0, 2.0 * math.pi)
        a2 = rng.uniform(0.0, 2.0 * math.pi)
        c1, s1 = math.cos(a1), math.sin(a1)
        c2, s2 = math.cos(a2), math.sin(a2)

        def rot(ux: float, uy: float, uz: float) -> tuple[float, float, float]:
            x1, y1, z1 = c1 * ux - s1 * uy, s1 * ux + c1 * uy, uz
            return (x1, c2 * y1 - s2 * z1, s2 * y1 + c2 * z1)

        pts3: list[tuple[float, float, float]] = []
        for ux, uy, uz in base_tet:
            rx, ry, rz = rot(ux, uy, uz)
            pts3.append((cx + r_star * rx, cy + r_star * ry, cz + r_star * rz))
        for _ in range(n_pts - 4):
            # Random direction inside sphere of radius 0.90 * r_star
            z_dir = rng.uniform(-1.0, 1.0)
            phi = rng.uniform(0.0, 2.0 * math.pi)
            xy = math.sqrt(max(0.0, 1.0 - z_dir * z_dir))
            rad = r_star * rng.uniform(0.05, 0.90)
            pts3.append((cx + rad * xy * math.cos(phi), cy + rad * xy * math.sin(phi), cz + rad * z_dir))
        rng.shuffle(pts3)

        triplets = []
        b_vec = []
        cones = []
        for i, (u, v, w) in enumerate(pts3):
            base = 4 * i
            triplets.extend([(base, 3, -1.0), (base + 1, 0, -1.0), (base + 2, 1, -1.0), (base + 3, 2, -1.0)])
            b_vec.extend([0.0, -u, -v, -w])
            cones.append(("soc", 4))
        m = 4 * n_pts
        out.append(
            make_problem(
                pid,
                "socp",
                "mcs_3d",
                4,
                m,
                None,
                [0.0, 0.0, 0.0, 1.0],
                csc_from_triplets(m, 4, triplets),
                b_vec,
                cones,
                "solved",
                r_star,
            )
        )

    # C3a: Maximum Inscribed Circle/Sphere inside polygon/polyhedron + SOC centre-bound (5 problems)
    for k, (dim, n_planes) in enumerate([(2, 6), (2, 16), (2, 40), (3, 12), (3, 32)]):
        idx += 1
        pid = f"socp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 9000 + idx)
        c_star = [rng.randint(-30, 30) / 10.0 for _ in range(dim)]
        r_star = rng.randint(10, 40) / 10.0
        # Tangent unit outward normals whose convex hull encloses 0
        if dim == 2:
            th0 = rng.uniform(0.0, 2.0 * math.pi)
            contact_normals = [
                (math.cos(th0 + 2.0 * math.pi * j / 3.0), math.sin(th0 + 2.0 * math.pi * j / 3.0))
                for j in range(3)
            ]
        else:
            contact_normals = list(base_tet)

        triplets = []
        b_vec = []
        row = 0
        # Nonneg constraints: n_j^T c + r <= n_j^T c* + r*
        for norm in contact_normals:
            for d_i in range(dim):
                triplets.append((row, d_i, norm[d_i]))
            triplets.append((row, dim, 1.0))
            rhs = sum(norm[d_i] * c_star[d_i] for d_i in range(dim)) + r_star
            b_vec.append(rhs)
            row += 1
        for _ in range(n_planes - len(contact_normals)):
            if dim == 2:
                th = rng.uniform(0.0, 2.0 * math.pi)
                norm = (math.cos(th), math.sin(th))
            else:
                z_dir = rng.uniform(-1.0, 1.0)
                phi = rng.uniform(0.0, 2.0 * math.pi)
                xy = math.sqrt(max(0.0, 1.0 - z_dir * z_dir))
                norm = (xy * math.cos(phi), xy * math.sin(phi), z_dir)
            for d_i in range(dim):
                triplets.append((row, d_i, norm[d_i]))
            triplets.append((row, dim, 1.0))
            margin = rng.uniform(0.2, 1.5) * r_star
            rhs = sum(norm[d_i] * c_star[d_i] for d_i in range(dim)) + r_star + margin
            b_vec.append(rhs)
            row += 1
        # Non-binding SOC(dim+1): (R_bound, c - c0) in SOC(dim+1)
        c0 = [c_star[d_i] + rng.uniform(-0.3, 0.3) for d_i in range(dim)]
        r_bound = 2.0 * r_star + 5.0
        # s = b - Ax = [r_bound, c_0 - c0_0, ...] => A has -1 on c coords, 0 on row 0
        b_vec.append(r_bound)
        row += 1
        for d_i in range(dim):
            triplets.append((row, d_i, -1.0))
            b_vec.append(-c0[d_i])
            row += 1
        n_vars = dim + 1
        q_vec = [0.0] * dim + [-1.0]
        cones = [("nonneg", n_planes), ("soc", dim + 1)]
        out.append(
            make_problem(
                pid,
                "socp",
                "inscribed_ball_socp",
                n_vars,
                row,
                None,
                q_vec,
                csc_from_triplets(row, n_vars, triplets),
                b_vec,
                cones,
                "solved",
                -r_star,
            )
        )

    # C3b: Point-to-ball projection SOCPs (5 problems)
    for k, dim in enumerate([2, 2, 3, 3, 4]):
        idx += 1
        pid = f"socp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 9500 + idx)
        c = [rng.randint(-20, 20) / 10.0 for _ in range(dim)]
        r_ball = rng.randint(10, 30) / 10.0
        # Unit direction from c to p0
        raw_dir = [rng.choice([-2, -1, 1, 2]) + rng.uniform(-0.2, 0.2) for _ in range(dim)]
        nrm = math.sqrt(sum(v * v for v in raw_dir))
        u_dir = [v / nrm for v in raw_dir]
        dist_out = rng.randint(5, 35) / 10.0
        p0 = [c[i] + (r_ball + dist_out) * u_dir[i] for i in range(dim)]
        # Variables x = [x_0..x_{d-1}, t], min t s.t. (t, x - p0) in SOC(d+1), (r_ball, x - c) in SOC(d+1)
        triplets = []
        b_vec = []
        # Cone 1: (t, x - p0)
        triplets.append((0, dim, -1.0))
        b_vec.append(0.0)
        for i in range(dim):
            triplets.append((1 + i, i, -1.0))
            b_vec.append(-p0[i])
        # Cone 2: (r_ball, x - c)
        off = dim + 1
        b_vec.append(r_ball)
        for i in range(dim):
            triplets.append((off + 1 + i, i, -1.0))
            b_vec.append(-c[i])
        m = 2 * (dim + 1)
        q_vec = [0.0] * dim + [1.0]
        out.append(
            make_problem(
                pid,
                "socp",
                "ball_projection_socp",
                dim + 1,
                m,
                None,
                q_vec,
                csc_from_triplets(m, dim + 1, triplets),
                b_vec,
                [("soc", dim + 1), ("soc", dim + 1)],
                "solved",
                dist_out,
            )
        )

    # C4: Generic KKT-certified SOCPs (10 problems, starting from m = 3!)
    # Smallest SOCP: m = 3, n = 2: min t s.t. (t, x1 - 3, 4) in SOC(3)? Even cleaner:
    # min t + 0.5*x1 s.t. (t, x1, 2) in SOC(3) -> exact KKT optimum!
    # General KKT construction for m = sum(cone_dims) with exact primal-dual boundary cones.
    gen_socp_specs = [
        # (n, m_zero, m_nonneg, soc_dims)
        (2, 0, 0, [3]),            # m = 3! (single SOC(3))
        (3, 0, 0, [3]),            # m = 3!
        (3, 1, 0, [3]),            # m = 4
        (3, 0, 2, [3]),            # m = 5
        (4, 1, 2, [4]),            # m = 7
        (4, 0, 2, [3, 3]),         # m = 8
        (5, 1, 3, [3, 4]),         # m = 11
        (6, 2, 4, [4, 4]),         # m = 14
        (8, 2, 6, [3, 4, 5]),      # m = 20
        (10, 2, 8, [4, 5, 6, 5]),  # m = 30
    ]
    for n, m_zero, m_nonneg, soc_dims in gen_socp_specs:
        idx += 1
        pid = f"socp-{idx:03d}"
        rng = random.Random(MASTER_SEED + 9800 + idx)
        x_star = [rng.choice([-2.0, -1.0, 1.0, 2.0]) + 0.25 * rng.randint(-1, 1) for _ in range(n)]
        a_rows: list[list[float]] = []
        s_star: list[float] = []
        y_star: list[float] = []
        cones: list[tuple[str, int]] = []

        # Zero cone rows
        for r in range(m_zero):
            row = [0.0] * n
            row[r % n] = 1.0
            row[(r + 1) % n] = rng.choice([-0.5, 0.5])
            a_rows.append(row)
            s_star.append(0.0)
            y_star.append(rng.choice([-1.5, -0.5, 0.5, 1.5]))
        if m_zero > 0:
            cones.append(("zero", m_zero))

        # Nonneg cone rows
        for r in range(m_nonneg):
            row = [0.0] * n
            row[r % n] = rng.choice([-1.0, 1.0])
            a_rows.append(row)
            if r % 2 == 0:
                s_star.append(0.0)
                y_star.append(rng.uniform(0.5, 2.0))
            else:
                s_star.append(rng.uniform(0.5, 2.5))
                y_star.append(0.0)
        if m_nonneg > 0:
            cones.append(("nonneg", m_nonneg))

        # SOC cone rows with exact complementary boundary pairs (s*, y*)
        col_cursor = 0
        for d in soc_dims:
            alpha = rng.uniform(0.8, 2.5)
            beta = rng.uniform(0.8, 2.5)
            raw_u = [rng.choice([-2, -1, 1, 2]) + rng.uniform(-0.3, 0.3) for _ in range(d - 1)]
            nrm = math.sqrt(sum(v * v for v in raw_u))
            u = [v / nrm for v in raw_u]
            s_cone = [alpha] + [alpha * v for v in u]
            y_cone = [beta] + [-beta * v for v in u]
            for local_r in range(d):
                row = [0.0] * n
                c0 = col_cursor % n
                col_cursor += 1
                row[c0] = -1.0 if local_r == 0 else rng.choice([-1.0, 1.0])
                if n > 2 and local_r > 0:
                    c1 = (c0 + 1) % n
                    row[c1] = rng.choice([-0.5, 0.5])
                a_rows.append(row)
            s_star.extend(s_cone)
            y_star.extend(y_cone)
            cones.append(("soc", d))

        m = len(a_rows)
        b_vec = [sum(a_rows[r][j] * x_star[j] for j in range(n)) + s_star[r] for r in range(m)]
        q_vec = [-sum(a_rows[r][j] * y_star[r] for r in range(m)) for j in range(n)]
        obj = sum(q_vec[j] * x_star[j] for j in range(n))
        # Ensure |obj| >= 0.1 and primal/dual objectives agree
        dual_obj = -sum(b_vec[r] * y_star[r] for r in range(m))
        assert abs(obj - dual_obj) < 1e-11, f"{pid}: duality gap {obj} vs {dual_obj}"
        if abs(obj) < 0.1:
            # Rescale x_star slightly is not needed; just pick another seed? Should not happen often.
            raise RuntimeError(f"{pid}: near-zero objective {obj}")

        triplets = []
        for r in range(m):
            for j in range(n):
                if a_rows[r][j] != 0.0:
                    triplets.append((r, j, a_rows[r][j]))
        out.append(
            make_problem(
                pid,
                "socp",
                "generic_socp",
                n,
                m,
                None,
                q_vec,
                csc_from_triplets(m, n, triplets),
                b_vec,
                cones,
                "solved",
                obj,
            )
        )

    # C5: Infeasible SOCPs (3 primal_infeasible + 2 dual_infeasible)
    primal_inf_socps = [
        # Two disjoint 2D balls: ||x - (0,0)|| <= 1 and ||x - (5,0)|| <= 1
        (
            2,
            [(1, 0, -1.0), (2, 1, -1.0), (4, 0, -1.0), (5, 1, -1.0)],
            [1.0, 0.0, 0.0, 1.0, -5.0, 0.0],
            [("soc", 3), ("soc", 3)],
            [1.0, 0.0],
        ),
        # Two disjoint 3D balls: ||x|| <= 1 and ||x - (4,4,0)|| <= 1
        (
            3,
            [(1, 0, -1.0), (2, 1, -1.0), (3, 2, -1.0), (5, 0, -1.0), (6, 1, -1.0), (7, 2, -1.0)],
            [1.0, 0.0, 0.0, 0.0, 1.0, -4.0, -4.0, 0.0],
            [("soc", 4), ("soc", 4)],
            [0.0, 1.0, 0.0],
        ),
        # Ball ||x|| <= 1 with halfspace x0 >= 3
        (
            2,
            [(0, 0, -1.0), (2, 0, -1.0), (3, 1, -1.0)],
            [-3.0, 1.0, 0.0, 0.0],
            [("nonneg", 1), ("soc", 3)],
            [1.0, 1.0],
        ),
    ]
    for n, trips, b_vec, cones, q_vec in primal_inf_socps:
        idx += 1
        m = len(b_vec)
        out.append(
            make_problem(
                f"socp-{idx:03d}",
                "socp",
                "infeasible_socp",
                n,
                m,
                None,
                q_vec,
                csc_from_triplets(m, n, trips),
                b_vec,
                cones,
                "primal_infeasible",
                None,
            )
        )

    dual_inf_socps = [
        # min -x0 s.t. (x0, x1, x2) in SOC(3) -> s = [x0, x1, x2] => A = -I, b = 0
        (3, [(0, 0, -1.0), (1, 1, -1.0), (2, 2, -1.0)], [0.0, 0.0, 0.0], [("soc", 3)], [-1.0, 0.0, 0.0]),
        # min -x0 - 0.5*x1 s.t. (x0, x1, x2, x3) in SOC(4)
        (4, [(0, 0, -1.0), (1, 1, -1.0), (2, 2, -1.0), (3, 3, -1.0)], [0.0, 0.0, 0.0, 0.0], [("soc", 4)], [-1.0, -0.5, 0.0, 0.0]),
    ]
    for n, trips, b_vec, cones, q_vec in dual_inf_socps:
        idx += 1
        m = len(b_vec)
        out.append(
            make_problem(
                f"socp-{idx:03d}",
                "socp",
                "unbounded_socp",
                n,
                m,
                None,
                q_vec,
                csc_from_triplets(m, n, trips),
                b_vec,
                cones,
                "dual_infeasible",
                None,
            )
        )

    assert len(out) == 55
    return out


def canonical_bytes(obj: object) -> bytes:
    return (json.dumps(obj, ensure_ascii=False, separators=(",", ":"), sort_keys=True) + "\n").encode("utf-8")


def build_all() -> dict[str, bytes]:
    lps = build_lp_problems()
    qps = build_qp_problems()
    socps = build_socp_problems()
    problems = sorted(lps + qps + socps, key=lambda p: str(p["id"]))
    assert len(problems) == 165

    problems_doc = {
        "format": "fid-conic-xtest-problems/1",
        "version": "conic-xtest-v1",
        "problems": problems,
    }
    problems_bytes = canonical_bytes(problems_doc)
    problems_digest = "sha256:" + hashlib.sha256(problems_bytes).hexdigest()

    min_m = min(int(p["m"]) for p in problems)
    max_m = max(int(p["m"]) for p in problems)
    assert min_m == 3 and max_m == 5000, f"expected m range 3..5000, got {min_m}..{max_m}"

    manifest_doc = {
        "format": "fid-conic-xtest-manifest/1",
        "version": "conic-xtest-v1",
        "license": "CC-BY-4.0",
        "seed": MASTER_SEED,
        "counts": {
            "lp": len(lps),
            "qp": len(qps),
            "socp": len(socps),
            "total": len(problems),
            "solved": sum(1 for p in problems if p["expected_status"] == "solved"),
            "infeasible": sum(1 for p in problems if p["expected_status"] != "solved"),
        },
        "constraint_range": {"min": min_m, "max": max_m},
        "problems_file": "problems.json",
        "problems_digest": problems_digest,
    }
    manifest_bytes = (json.dumps(manifest_doc, indent=2, sort_keys=True) + "\n").encode("utf-8")

    return {
        "manifest.json": manifest_bytes,
        "problems.json": problems_bytes,
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv
    files = build_all()
    mismatches: list[str] = []
    for name, content in files.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != content:
                mismatches.append(str(path.relative_to(REPO_ROOT)))
        else:
            path.write_bytes(content)
    if check and mismatches:
        sys.stderr.write("KRN.SOLV.CONIC.XTEST fixtures out of date:\n")
        for m in mismatches:
            sys.stderr.write(f"  - {m}\n")
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
