#!/usr/bin/env python3
"""Independent reference for KRN.ENV.TAN.ROCK (rocking report) fixtures (CC-BY-4.0 project data).

Writes
  fixtures/fit/tan-rock-v1/crowned-plate.txt  the crowned plate (apex and 4 high corners)
  fixtures/fit/tan-rock-v1/flat-plate.txt     the flat plate (4 coplanar high corners)
  fixtures/fit/tan-rock-v1/cases.txt          candidate lists and the expected report bytes

Nothing here runs the Rust code (stdlib only).

1. Geometry, exact. Coordinates are dyadic (a = 2^-4 m, h = 2^-15 m), so every float is
   exact. The stable contact sets of each plate (its resting positions on a surface plate
   above it, material below, outward direction +z) are found by brute force in rational
   arithmetic (fractions.Fraction): every triple of points spanning a plane with a positive
   z normal is tested, and a plane is kept when every point is on or below it; its contact
   set is every point exactly on it. The crowned plate has exactly 4 such sets, each of 3
   points, related by the 90-degree rotation (x, y) -> (-y, x); the flat plate has 1 set of
   4 points. The script asserts both.
2. Candidate values. Each plane's feature is (p = the first contact point, n = the outward
   unit normal). The crowned plate's objective is the constrained-L1 value of the plane (the
   sum of the perpendicular distances of all points below it); the flat plate's is the
   external minimax value (the largest such distance). Both come from exact rationals and one
   correctly rounded square root (integer isqrt, rounding decided exactly), so the four
   crowned facets get bit-equal objectives by symmetry, as AC-1 needs. The multi-start lists
   are simulated (the plane fit, KRN.ENV.TAN.PLANE, is not built yet): each stable set is
   found by one or more starts, with exact repeats, a not-fully-converged repeat one ulp
   worse with a perturbed normal, and unsorted contact lists.
   Which variant rocks on the crowned plate (checked exactly, see `variant_optima`): with
   perpendicular distances the objectives are not convex in the plane parameters. Over a
   grid of supporting planes (gradients in steps of h/(8a) up to 3h/a, each lifted until it
   touches), constrained L1 is smallest exactly at the four facets (strict optima: the
   vertical sum of distances is the same for every supporting plane through the apex, and
   the perpendicular sum divides it by the largest normal tilt, reached at the facets),
   while external minimax (1.5h) and constrained L2 (49h^2) are smallest only at the
   horizontal plane through the apex, contact set {0}. So the fixture's facet candidates are
   real constrained-L1 optima, and KRN.ENV.TAN.PLANE's AC-2 is stated per variant.
3. The report oracle, written from the spec's rules, not from the Rust code: a dict keyed by
   the sorted contact tuple keeps the smallest (objective, canonical feature bytes); the
   filter is decided twice, exactly (obj - best <= rock_tol * best in rationals) and by the
   pinned binary64 rule (obj <= best + rock_tol * best, CPython floats are binary64,
   round-to-nearest-even, never fused), and the script fails if the two disagree for any
   fixture candidate; the order is (objective as a rational, contact tuple) with Python's
   lexicographic tuple order (a proper prefix first).

Run:  python fixtures/fit/make_tan_rock_fixtures.py          (writes the fixtures)
      python fixtures/fit/make_tan_rock_fixtures.py --check  (fails if a fixture is stale)
"""

import itertools
import math
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).parent
DIR = "tan-rock-v1"
A = 2.0**-4  # half the plate side, m
H = 2.0**-15  # crown height and grid depth scale, m


def bits(x):
    return struct.unpack("<Q", struct.pack("<d", x))[0]


def hx(x):
    return f"{bits(x):016x}"


def next_up(x):
    return struct.unpack("<d", struct.pack("<Q", bits(x) + 1))[0]


def sqrt_round(q):
    """The binary64 value nearest to sqrt(q) for a rational q >= 0 (ties cannot occur for an
    irrational root; an exact rational root is converted exactly)."""
    q = Fraction(q)
    if q == 0:
        return 0.0
    num, den = q.numerator, q.denominator
    k = max(0, 160 - (num.bit_length() - den.bit_length())) // 2 + 2
    while True:
        scaled = (num << (2 * k)) // den
        r = math.isqrt(scaled)
        if r * r * den == num << (2 * k):
            return float(Fraction(r, 1 << k))
        lo, hi = float(Fraction(r, 1 << k)), float(Fraction(r + 1, 1 << k))
        if lo == hi:
            return lo
        k += 32


def signed_sqrt_ratio(n, s):
    """n / sqrt(s), correctly rounded, for rational n and s > 0."""
    v = sqrt_round(Fraction(n) * Fraction(n) / Fraction(s))
    return -v if n < 0 else v


# --- geometry -------------------------------------------------------------------------------


def plate(crowned):
    pts = []
    if crowned:
        pts.append((0.0, 0.0, H))
    pts += [(A, A, 0.0), (-A, A, 0.0), (-A, -A, 0.0), (A, -A, 0.0)]
    grid = [-A, -A / 2, 0.0, A / 2, A]
    for y in grid:
        for x in grid:
            if abs(x) == A and abs(y) == A:
                continue
            if crowned and x == 0.0 and y == 0.0:
                continue
            pts.append((x, y, -H / 2))
    return pts


def sub(u, v):
    return tuple(Fraction(a) - Fraction(b) for a, b in zip(u, v))


def cross(u, v):
    return (u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0])


def dot(u, v):
    return sum(Fraction(a) * Fraction(b) for a, b in zip(u, v))


def stable_sets(pts):
    """{contact tuple: integer normal direction} for every supporting plane from above."""
    found = {}
    for i, j, k in itertools.combinations(range(len(pts)), 3):
        n = cross(sub(pts[j], pts[i]), sub(pts[k], pts[i]))
        if n[2] == 0:
            continue
        if n[2] < 0:
            n = tuple(-c for c in n)
        off = dot(n, pts[i])
        vals = [dot(n, p) for p in pts]
        if all(v <= off for v in vals):
            contacts = tuple(m for m, v in enumerate(vals) if v == off)
            found.setdefault(contacts, n)
    return found


def plane_values(pts, contacts, n, variant):
    """(feature (p, n_unit), objective) of the supporting plane with normal n, for variant
    "minimax" (largest perpendicular distance) or "l1" (sum of perpendicular distances)."""
    s = dot(n, n)
    off = dot(n, pts[contacts[0]])
    gaps = [off - dot(n, p) for p in pts]
    assert min(gaps) == 0
    num = max(gaps) if variant == "minimax" else sum(gaps)
    unit = tuple(signed_sqrt_ratio(c, s) + 0.0 for c in n)
    return (pts[contacts[0]], unit), sqrt_round(num * num / s)


def variant_optima(pts, facets):
    """Exact comparison of the three objectives over a grid of supporting planes
    z = c + gx x + gy y (c lifted until the plane touches). Returns the squared objectives
    (minimax^2, L2, L1^2) of the best plane per variant and the gradients attaining them."""
    step = Fraction(H) / Fraction(A) / 8
    grads = [(i * step, j * step) for i in range(-24, 25) for j in range(-24, 25)]
    for n in facets.values():
        grads.append((-Fraction(n[0]) / n[2], -Fraction(n[1]) / n[2]))
    best = [None, None, None]
    for gx, gy in grads:
        c = max(Fraction(z) - gx * Fraction(x) - gy * Fraction(y) for x, y, z in pts)
        gaps = [c + gx * Fraction(x) + gy * Fraction(y) - Fraction(z) for x, y, z in pts]
        w = 1 + gx * gx + gy * gy
        vals = (max(gaps) ** 2 / w, sum(g * g for g in gaps) / w, sum(gaps) ** 2 / w)
        for v in range(3):
            if best[v] is None or vals[v] < best[v][0]:
                best[v] = (vals[v], {(gx, gy)})
            elif vals[v] == best[v][0]:
                best[v][1].add((gx, gy))
    return best


# --- the report oracle ----------------------------------------------------------------------


def feature_bytes(feat):
    p, n = feat
    return (
        '{"kind":"plane","p":[' + ",".join(f'"{hx(x)}"' for x in p) + '],"n":[' + ",".join(f'"{hx(x)}"' for x in n) + "]}"
    )


def report(cands, rock_tol):
    assert 0.0 <= rock_tol <= 1.0
    groups = {}
    for contacts, obj, feat in cands:
        key = tuple(sorted(contacts))
        assert key and len(set(key)) == len(key)
        obj = obj + 0.0
        assert obj >= 0.0
        groups.setdefault(key, []).append((Fraction(obj), feature_bytes(feat), obj, feat))
    best_of = {}
    for key, members in groups.items():
        lo = min(members, key=lambda m: m[0])
        if any(m[0] == lo[0] and m[1] != lo[1] for m in members):
            raise SystemExit(f"ambiguous candidate {key}: tied objective, different features")
        best_of[key] = (None, lo[2], lo[3])
    best = min(Fraction(v[1]) for v in best_of.values())
    best_f = float(best)
    bound = best_f + rock_tol * best_f
    kept = []
    excluded = 0
    for key, (val, obj, feat) in best_of.items():
        exact = Fraction(obj) - best <= Fraction(rock_tol) * best
        rounded = obj <= bound
        if exact != rounded:
            raise SystemExit(f"rock_tol boundary: exact and binary64 filters disagree for {key}")
        if exact:
            kept.append((Fraction(obj), key, obj, feat))
        else:
            excluded += 1
    kept.sort(key=lambda t: (t[0], t[1]))
    if len(kept) == 1:
        tie_break = "single"
    elif kept[0][0] == kept[1][0]:
        tie_break = "contact_order"
    else:
        tie_break = "objective"
    merged = len(cands) - len(best_of)
    out = '{"rock_tol":"' + hx(rock_tol + 0.0) + '","selected":"0","rocking":' + ("true" if len(kept) > 1 else "false")
    out += ',"tie_break":"' + tie_break + '","merged":"' + str(merged)
    out += '","excluded":"' + str(excluded) + '","candidates":['
    parts = []
    for _, key, obj, feat in kept:
        parts.append(
            '{"contacts":['
            + ",".join(f'"{c}"' for c in key)
            + '],"objective":"'
            + hx(obj)
            + '","feature":'
            + feature_bytes(feat)
            + "}"
        )
    return out + ",".join(parts) + "]}", [k for _, k, _, _ in kept]


# --- cases ----------------------------------------------------------------------------------


def perturb(feat):
    p, n = feat
    return p, (n[0], next_up(n[1]) if n[1] != 0.0 else n[1], n[2] if n[1] != 0.0 else next_up(n[2]))


def plate_cases():
    crowned, flat = plate(True), plate(False)
    cs = stable_sets(crowned)
    want = {(0, 1, 2), (0, 2, 3), (0, 3, 4), (0, 1, 4)}
    assert set(cs) == want, sorted(cs)
    vals = {k: plane_values(crowned, k, n, "l1") for k, n in cs.items()}
    objs = {bits(v[1]) for v in vals.values()}
    assert len(objs) == 1, "the four facets must have bit-equal objectives"
    facet_obj = next(iter(vals.values()))[1]
    # Which variant rocks: L1 is smallest exactly at the four facets; minimax and L2 only at
    # the horizontal plane through the apex (gradient 0), with values 1.5h and 49h^2.
    best = variant_optima(crowned, cs)
    facet_grads = {(-Fraction(n[0]) / n[2], -Fraction(n[1]) / n[2]) for n in cs.values()}
    assert best[2][1] == facet_grads, best[2][1]
    assert best[0][1] == {(0, 0)} and best[0][0] == (Fraction(3, 2) * Fraction(H)) ** 2
    assert best[1][1] == {(0, 0)} and best[1][0] == 49 * Fraction(H) ** 2
    horiz = {"minimax": 1.5 * H, "l2": float(best[1][0]), "l1": 34 * H}
    fs = stable_sets(flat)
    assert list(fs) == [(0, 1, 2, 3)], sorted(fs)
    flat_feat, flat_obj = plane_values(flat, (0, 1, 2, 3), fs[(0, 1, 2, 3)], "minimax")

    def c(key, contacts=None, worse=False):
        feat, obj = vals[key]
        if worse:
            return (list(contacts or key), next_up(obj), perturb(feat))
        return (list(contacts or key), obj, feat)

    crowned_cands = [
        c((0, 1, 2)),
        c((0, 3, 4)),
        c((0, 1, 2)),
        c((0, 2, 3)),
        c((0, 1, 4), contacts=(4, 1, 0)),
        c((0, 2, 3), worse=True),
        c((0, 3, 4), contacts=(4, 0, 3)),
        c((0, 1, 4)),
    ]
    flat_cands = [
        ([0, 1, 2, 3], flat_obj, flat_feat),
        ([3, 1, 0, 2], flat_obj, flat_feat),
        ([0, 1, 2, 3], next_up(flat_obj), perturb(flat_feat)),
        ([2, 3, 0, 1], flat_obj, flat_feat),
    ]
    return crowned, flat, facet_obj, horiz, [
        ("crowned_l1", "AC-1/AC-2: 8 simulated constrained-L1 starts on the crowned plate", 1e-9, crowned_cands),
        ("flat_minimax", "AC-3: 4 simulated starts on the flat plate", 1e-9, flat_cands),
    ]


def synth_cases():
    pa = (0.0, 0.0, 0.0)
    up = (0.0, 0.0, 1.0)
    tilt = (0.0, 0.6, 0.8)
    tilt2 = (0.6, 0.0, 0.8)
    x = 1.5e-5
    prefix = [
        ([5, 2], x, (pa, up)),
        ([2, 5, 9], x, (pa, up)),
        ([2, 5], 2 * x, (pa, tilt)),
        ([8, 1, 7], x + x * 5e-4, (pa, tilt2)),
        ([3], 2 * x, (pa, up)),
        ([2, 5], x, (pa, up)),
    ]
    y = 1e-5
    by_objective = [
        ([4, 5, 6], y, (pa, up)),
        ([1, 2, 3], 1.000000001e-5, (pa, tilt)),
        ([0, 1, 2], 1.1e-5, (pa, tilt2)),
    ]
    zero = [
        ([1, 2, 3], 0.0, (pa, up)),
        ([0, 1, 2], 5e-324, (pa, tilt)),
        ([9, 8, 7], -0.0, (pa, tilt2)),
    ]
    return [
        ("synth_prefix", "duplicates, prefix order, exclusion", 1e-3, prefix),
        ("synth_objective", "selection by a strictly lower objective", 1e-6, by_objective),
        ("synth_zero", "best objective zero, -0.0 objective, subnormal excluded", 0.5, zero),
    ]


def points_text(name, pts, note):
    out = [
        f"# KRN.ENV.TAN.ROCK {name}: {len(pts)} points (x y z, binary64 bits), material below,",
        "# outward direction +z. Generated by fixtures/fit/make_tan_rock_fixtures.py (CC-BY-4.0).",
    ]
    out += [f"# {line}" for line in note]
    out += [f"p {hx(x)} {hx(y)} {hx(z)}" for x, y, z in pts]
    return "\n".join(out) + "\n"


def cases_text(cases):
    out = [
        "# KRN.ENV.TAN.ROCK cases: candidate lists (input order) and the expected canonical report.",
        "# Generated by fixtures/fit/make_tan_rock_fixtures.py (CC-BY-4.0). Do not edit.",
        "# cand <contacts, comma separated, as given> <objective bits> plane <p bits x3> <n bits x3>",
        "# order <selected-first contact sets, ; separated>",
    ]
    for name, note, tol, cands in cases:
        enc, order = report(cands, tol)
        out.append(f"case {name}")
        out.append(f"# {note}")
        out.append(f"rock_tol {hx(tol)}")
        for contacts, obj, (p, n) in cands:
            out.append(
                f"cand {','.join(str(c) for c in contacts)} {hx(obj)} plane "
                + " ".join(hx(v) for v in p)
                + " "
                + " ".join(hx(v) for v in n)
            )
        out.append("order " + ";".join(",".join(str(c) for c in k) for k in order))
        out.append("expect " + enc)
        out.append("end")
    return "\n".join(out) + "\n"


def main():
    crowned, flat, facet_obj, horiz, pcases = plate_cases()
    files = {
        f"{DIR}/crowned-plate.txt": points_text(
            "crowned plate",
            crowned,
            [
                "Point 0 is the apex (0, 0, h), points 1-4 the high corners (+-a, +-a, 0), the rest a",
                "5x5 grid at z = -h/2; a = 2^-4 m, h = 2^-15 m. Stable sets: {0,1,2} {0,1,4} {0,2,3}",
                f"{{0,3,4}}, each facet's constrained-L1 value {facet_obj!r} m ({hx(facet_obj)}): the four",
                f"strict L1 optima (horizontal plane through the apex: {horiz['l1']!r} m). External minimax",
                f"and constrained L2 have one optimum, the horizontal plane through the apex, contact {{0}}:",
                f"minimax 1.5h = {horiz['minimax']!r} m, L2 49h^2 = {horiz['l2']!r} m^2 (script docstring).",
            ],
        ),
        f"{DIR}/flat-plate.txt": points_text(
            "flat plate",
            flat,
            ["Points 0-3 are the high corners (+-a, +-a, 0), the rest a 5x5 grid at z = -h/2.", "Stable set: {0,1,2,3}."],
        ),
        f"{DIR}/cases.txt": cases_text(pcases + synth_cases()),
    }
    check = "--check" in sys.argv[1:]
    stale = False
    for name, text in files.items():
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: fixtures/fit/{name}")
                stale = True
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            print(f"wrote fixtures/fit/{name} ({len(data)} bytes)")
    if stale:
        sys.exit(1)


if __name__ == "__main__":
    main()
