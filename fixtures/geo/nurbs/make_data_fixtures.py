#!/usr/bin/env python3
"""Independent reference for GEO.NURBS.DATA (CC-BY-4.0 project data).

Writes fixtures/geo/nurbs/valid_set.json (40 curves and surfaces, degrees 1 to 25,
rational and non-rational; hand-built cases plus seeded random cases, seed 41) and
fixtures/geo/nurbs/invalid_set.json (hand-labelled cases, at least one per error
variant, each with the expected error and index).

Every f64 is written as its IEEE 754 bit pattern ("0x" + 16 lowercase hex digits),
so the fixture is exact, can carry NaN and infinities, and AC-3 (bit-identical round
trip) can be checked bit for bit.

The expected domains, rational flags and errors are computed by the small validator
below, written from the rules in spec/geo/nurbs/data/NODE.md (and the judgement calls
stated in the PR), not by the Rust code. Every hand label in the invalid set is
asserted against this validator before anything is written.

Run:  python fixtures/geo/nurbs/make_data_fixtures.py
"""

import json
import math
import random
import struct
from pathlib import Path

HERE = Path(__file__).resolve().parent
MAX_DEGREE = 25


def bits(x):
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", float(x)))[0]


# --- reference validator (spec rules, in the documented pass order) ---------------


def check_knots(knots, p, d):
    for i, k in enumerate(knots):
        if not math.isfinite(k):
            return ("BadKnots", d, i)
        if i > 0 and k < knots[i - 1]:
            return ("BadKnots", d, i)
    first, last = knots[0], knots[-1]
    run = 0
    for i, k in enumerate(knots):
        run = run + 1 if i > 0 and k == knots[i - 1] else 1
        limit = p + 1 if (k == first or k == last) else p
        if run > limit:
            return ("BadKnots", d, i)
    n = len(knots) - p - 1
    if not knots[p] < knots[n]:
        return ("BadKnots", d, n)
    return None


def check_dir(p, knots, n, d):
    if p < 1 or p > MAX_DEGREE:
        return ("BadDegree", d, p)
    return None


def check_count(p, knots, n, d):
    if n < p + 1 or len(knots) != n + p + 1:
        return ("CountMismatch", d, (len(knots), n, p))
    return None


def check_points_and_weights(ctrl, weights):
    for i, c in enumerate(ctrl):
        if not all(math.isfinite(x) for x in c):
            return ("NonFiniteCtrl", None, i)
    if weights is not None:
        if len(weights) != len(ctrl):
            return ("WeightCount", None, (len(weights), len(ctrl)))
        for i, w in enumerate(weights):
            if not (math.isfinite(w) and w > 0.0):
                return ("BadWeight", None, i)
    return None


def validate(case):
    if case["kind"] == "curve":
        p, k, c = case["degree"], case["knots"], case["ctrl"]
        return (
            check_dir(p, k, len(c), "curve")
            or check_count(p, k, len(c), "curve")
            or check_knots(k, p, "curve")
            or check_points_and_weights(c, case["weights"])
        )
    pu, pv, ku, kv = case["degree_u"], case["degree_v"], case["knots_u"], case["knots_v"]
    nu, nv, c = case["n_u"], case["n_v"], case["ctrl"]
    return (
        check_dir(pu, ku, nu, "u")
        or check_dir(pv, kv, nv, "v")
        or check_count(pu, ku, nu, "u")
        or check_count(pv, kv, nv, "v")
        or (("NetShape", None, (len(c), nu, nv)) if len(c) != nu * nv else None)
        or check_knots(ku, pu, "u")
        or check_knots(kv, pv, "v")
        or check_points_and_weights(c, case["weights"])
    )


def rational(weights):
    return weights is not None and any(w != weights[0] for w in weights)


# --- case builders ----------------------------------------------------------------


def curve(name, p, knots, ctrl, weights=None, periodic=False):
    return {"name": name, "kind": "curve", "degree": p, "knots": knots, "ctrl": ctrl,
            "weights": weights, "periodic": periodic}


def surface(name, pu, pv, ku, kv, nu, nv, ctrl, weights=None, pu_flag=False, pv_flag=False):
    return {"name": name, "kind": "surface", "degree_u": pu, "degree_v": pv, "knots_u": ku,
            "knots_v": kv, "n_u": nu, "n_v": nv, "ctrl": ctrl, "weights": weights,
            "periodic_u": pu_flag, "periodic_v": pv_flag}


def clamped(p, interior, a=0.0, b=1.0):
    return [a] * (p + 1) + list(interior) + [b] * (p + 1)


def line_pts(n):
    return [[float(i), 0.5 * i, -0.25 * i] for i in range(n)]


def grid(nu, nv):
    return [[float(i), float(j), 0.125 * (i * j)] for i in range(nu) for j in range(nv)]


def hand_valid():
    s = math.sqrt(0.5)
    cases = [
        curve("line deg1", 1, [0.0, 0.0, 1.0, 1.0], [[0.0, 0.0, 0.0], [1.0, 2.0, 3.0]]),
        curve("quadratic bezier", 2, clamped(2, []), line_pts(3)),
        curve("quarter circle rational", 2, clamped(2, []),
              [[1.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0, 0.0]], [1.0, s, 1.0]),
        curve("full circle rational 9pt", 2, clamped(2, [0.25, 0.25, 0.5, 0.5, 0.75, 0.75]),
              [[1, 0, 0], [1, 1, 0], [0, 1, 0], [-1, 1, 0], [-1, 0, 0], [-1, -1, 0], [0, -1, 0],
               [1, -1, 0], [1, 0, 0]], [1, s, 1, s, 1, s, 1, s, 1]),
        curve("equal weights not rational", 2, clamped(2, []), line_pts(3), [2.0, 2.0, 2.0]),
        curve("cubic nonuniform", 3, clamped(3, [0.1, 0.35, 0.36]), line_pts(7)),
        curve("cubic interior mult = degree", 3, clamped(3, [0.5, 0.5, 0.5]), line_pts(7)),
        curve("uniform unclamped periodic cubic", 3, [float(i) for i in range(11)], line_pts(7),
              periodic=True),
        curve("negative domain", 1, [-5.0, -5.0, -2.5, -1e-3, -1e-3], line_pts(3)),
        curve("tiny knot span", 1, [1.0, 1.0, 1.0 + 2.0 ** -40, 1.0 + 2.0 ** -40], line_pts(2)),
        curve("subnormal positive weight", 1, [0.0, 0.0, 1.0, 1.0], line_pts(2), [5e-324, 1.0]),
        curve("negative zero knots", 1, [-0.0, 0.0, 1.0, 1.0], line_pts(2)),
        curve("degree 25 bezier", 25, clamped(25, []), line_pts(26)),
        curve("degree 25 rational clamped", 25, clamped(25, [0.5]), line_pts(27),
              [1.0 + 0.01 * i for i in range(27)]),
        surface("bilinear patch", 1, 1, clamped(1, []), clamped(1, []), 2, 2, grid(2, 2)),
        surface("bicubic", 3, 3, clamped(3, [0.5]), clamped(3, [0.25, 0.75]), 5, 6, grid(5, 6)),
        surface("rational cylinder quarter", 2, 1, clamped(2, []), clamped(1, []), 3, 2,
                [[1, 0, 0], [1, 0, 1], [1, 1, 0], [1, 1, 1], [0, 1, 0], [0, 1, 1]],
                [1, 1, s, s, 1, 1]),
        surface("periodic u surface", 3, 1, [float(i) for i in range(10)], clamped(1, []), 6, 2,
                grid(6, 2), None, True, False),
        surface("degree 25 x 1 surface", 25, 1, clamped(25, []), clamped(1, []), 26, 2, grid(26, 2)),
        surface("equal weights surface", 1, 2, clamped(1, []), clamped(2, []), 2, 3, grid(2, 3),
                [0.5] * 6),
    ]
    for c in cases:  # hand-built inputs use ints in places; store everything as f64
        for key in ("knots", "knots_u", "knots_v", "weights"):
            if c.get(key) is not None:
                c[key] = [float(x) for x in c[key]]
        c["ctrl"] = [[float(x) for x in pt] for pt in c["ctrl"]]
    return cases


def rand_knots(rng, p, n):
    if rng.random() < 0.3:  # unclamped, strictly increasing
        xs = sorted(rng.uniform(-10.0, 10.0) for _ in range(n + p + 1))
        return xs if len(set(xs)) == len(xs) else rand_knots(rng, p, n)
    a = rng.uniform(-5.0, 5.0)
    b = a + rng.uniform(0.5, 20.0)
    interior = []
    while len(interior) < n - p - 1:
        v = rng.uniform(a, b)
        if v == a or v == b or v in interior:
            continue
        m = min(rng.choice([1, 1, 1, p]), p, n - p - 1 - len(interior))
        interior += [v] * m
    return clamped(p, sorted(interior), a, b)


def rand_weights(rng, n):
    r = rng.random()
    if r < 0.4:
        return None
    if r < 0.55:
        w = rng.uniform(0.1, 10.0)
        return [w] * n
    return [rng.uniform(0.1, 10.0) for _ in range(n)]


def rand_pts(rng, n):
    return [[rng.uniform(-1000.0, 1000.0) for _ in range(3)] for _ in range(n)]


def random_valid(rng, count):
    out = []
    for i in range(count):
        if i % 2 == 0:
            p = rng.randint(1, MAX_DEGREE)
            n = p + 1 + rng.randint(0, 8)
            out.append(curve(f"random curve {i}", p, rand_knots(rng, p, n), rand_pts(rng, n),
                             rand_weights(rng, n), rng.random() < 0.2))
        else:
            pu, pv = rng.randint(1, 6), rng.randint(1, 6)
            nu, nv = pu + 1 + rng.randint(0, 4), pv + 1 + rng.randint(0, 4)
            out.append(surface(f"random surface {i}", pu, pv, rand_knots(rng, pu, nu),
                               rand_knots(rng, pv, nv), nu, nv, rand_pts(rng, nu * nv),
                               rand_weights(rng, nu * nv), rng.random() < 0.2, rng.random() < 0.2))
    return out


def invalid():
    nan, inf = float("nan"), float("inf")
    k1 = [0.0, 0.0, 1.0, 1.0]
    c2 = line_pts(2)
    q = clamped(2, [0.5])  # 7 knots, 4 points
    c4 = line_pts(4)
    return [
        (curve("degree 0", 0, [0.0, 1.0], line_pts(1)), {"variant": "BadDegree", "dir": "curve", "degree": 0}),
        (curve("degree 26", 26, clamped(26, []), line_pts(27)),
         {"variant": "BadDegree", "dir": "curve", "degree": 26}),
        (curve("too many knots", 1, [0.0, 0.0, 0.5, 1.0, 1.0], c2),
         {"variant": "CountMismatch", "dir": "curve", "knots": 5, "ctrl": 2, "degree": 1}),
        (curve("too few points for degree", 2, [0.0, 0.0, 1.0, 1.0], line_pts(1)),
         {"variant": "CountMismatch", "dir": "curve", "knots": 4, "ctrl": 1, "degree": 2}),
        (curve("no points", 1, [0.0, 1.0], []),
         {"variant": "CountMismatch", "dir": "curve", "knots": 2, "ctrl": 0, "degree": 1}),
        (curve("decreasing knot", 2, [0.0, 0.0, 0.0, 0.6, 0.4, 1.0, 1.0, 1.0][:7], c4),
         {"variant": "BadKnots", "dir": "curve", "index": 4}),
        (curve("nan knot", 2, [0.0, 0.0, 0.0, nan, 1.0, 1.0, 1.0], c4),
         {"variant": "BadKnots", "dir": "curve", "index": 3}),
        (curve("infinite knot", 1, [0.0, 0.0, 1.0, inf], c2),
         {"variant": "BadKnots", "dir": "curve", "index": 3}),
        (curve("interior multiplicity above degree", 2, [0.0, 0.0, 0.0, 0.5, 0.5, 0.5, 1.0, 1.0, 1.0],
               line_pts(6)), {"variant": "BadKnots", "dir": "curve", "index": 5}),
        (curve("end multiplicity above degree+1", 1, [0.0, 0.0, 0.0, 1.0, 1.0], line_pts(3)),
         {"variant": "BadKnots", "dir": "curve", "index": 2}),
        (curve("empty domain", 2, [0.0, 1.0, 2.0, 2.0, 3.0, 4.0], line_pts(3)),
         {"variant": "BadKnots", "dir": "curve", "index": 3}),
        (curve("nan control point", 2, q, [[0.0, 0.0, 0.0], [1.0, 1.0, 1.0], [2.0, nan, 2.0], [3.0, 3.0, 3.0]]),
         {"variant": "NonFiniteCtrl", "index": 2}),
        (curve("infinite control point", 1, k1, [[0.0, 0.0, 0.0], [1.0, 1.0, -inf]]),
         {"variant": "NonFiniteCtrl", "index": 1}),
        (curve("weight count", 1, k1, c2, [1.0, 1.0, 1.0]),
         {"variant": "WeightCount", "weights": 3, "ctrl": 2}),
        (curve("zero weight", 2, q, c4, [1.0, 1.0, 0.0, 1.0]), {"variant": "BadWeight", "index": 2}),
        (curve("negative zero weight", 1, k1, c2, [-0.0, 1.0]), {"variant": "BadWeight", "index": 0}),
        (curve("negative weight", 2, q, c4, [1.0, 1.0, 1.0, -0.5]), {"variant": "BadWeight", "index": 3}),
        (curve("nan weight", 1, k1, c2, [1.0, nan]), {"variant": "BadWeight", "index": 1}),
        (curve("infinite weight", 1, k1, c2, [inf, 1.0]), {"variant": "BadWeight", "index": 0}),
        (surface("surface degree v 26", 1, 26, k1, clamped(26, []), 2, 27, grid(2, 27)),
         {"variant": "BadDegree", "dir": "v", "degree": 26}),
        (surface("surface count u", 1, 1, [0.0, 0.0, 0.5, 1.0, 1.0], k1, 2, 2, grid(2, 2)),
         {"variant": "CountMismatch", "dir": "u", "knots": 5, "ctrl": 2, "degree": 1}),
        (surface("surface net shape", 1, 1, k1, k1, 2, 2, grid(2, 2)[:3]),
         {"variant": "NetShape", "ctrl": 3, "n_u": 2, "n_v": 2}),
        (surface("surface bad knots v", 1, 2, k1, [0.0, 0.0, 0.0, 1.0, 0.9, 1.0], 2, 3, grid(2, 3)),
         {"variant": "BadKnots", "dir": "v", "index": 4}),
        (surface("surface nan ctrl", 1, 1, k1, k1, 2, 2, [[0, 0, 0], [0, 1, 0], [1, 0, 0], [1, 1, nan]]),
         {"variant": "NonFiniteCtrl", "index": 3}),
        (surface("surface bad weight", 1, 1, k1, k1, 2, 2, grid(2, 2), [1.0, 1.0, -1.0, 1.0]),
         {"variant": "BadWeight", "index": 2}),
    ]


def expected_tuple(e):
    v = e["variant"]
    if v == "BadDegree":
        return (v, e["dir"], e["degree"])
    if v == "CountMismatch":
        return (v, e["dir"], (e["knots"], e["ctrl"], e["degree"]))
    if v == "BadKnots":
        return (v, e["dir"], e["index"])
    if v == "NetShape":
        return (v, None, (e["ctrl"], e["n_u"], e["n_v"]))
    if v == "WeightCount":
        return (v, None, (e["weights"], e["ctrl"]))
    return (v, None, e["index"])


def encode(case):
    out = {}
    for key, val in case.items():
        if key in ("knots", "knots_u", "knots_v") or (key == "weights" and val is not None):
            out[key] = [bits(x) for x in val]
        elif key == "ctrl":
            out[key] = [[bits(x) for x in pt] for pt in val]
        else:
            out[key] = val
    return out


def main():
    rng = random.Random(41)
    valid = hand_valid()
    valid += random_valid(rng, 40 - len(valid))
    assert len(valid) == 40
    degrees = set()
    rows = []
    for c in valid:
        err = validate(c)
        assert err is None, (c["name"], err)
        if c["kind"] == "curve":
            p, k = c["degree"], c["knots"]
            dom = [bits(k[p]), bits(k[len(c["ctrl"])])]
            degrees.add(p)
        else:
            ku, kv = c["knots_u"], c["knots_v"]
            dom = [[bits(ku[c["degree_u"]]), bits(ku[c["n_u"]])], [bits(kv[c["degree_v"]]), bits(kv[c["n_v"]])]]
            degrees.update((c["degree_u"], c["degree_v"]))
        e = encode(c)
        e["expect"] = {"domain": dom, "rational": rational(c["weights"])}
        rows.append(e)
    assert 1 in degrees and MAX_DEGREE in degrees
    assert any(r["expect"]["rational"] for r in rows) and any(not r["expect"]["rational"] for r in rows)

    bad = []
    seen = set()
    for c, e in invalid():
        got = validate(c)
        assert got == expected_tuple(e), (c["name"], got, e)
        seen.add(e["variant"])
        row = encode(c)
        row["error"] = e
        bad.append(row)
    assert seen == {"BadDegree", "CountMismatch", "BadKnots", "NetShape", "NonFiniteCtrl", "WeightCount",
                    "BadWeight"}

    note = ("GEO.NURBS.DATA fixture, CC-BY-4.0. Generated by make_data_fixtures.py (seed 41). "
            "Every float is its IEEE 754 binary64 bit pattern in hex.")
    (HERE / "valid_set.json").write_text(json.dumps({"note": note, "cases": rows}, indent=1) + "\n",
                                         encoding="utf-8", newline="\n")
    (HERE / "invalid_set.json").write_text(json.dumps({"note": note, "cases": bad}, indent=1) + "\n",
                                           encoding="utf-8", newline="\n")
    print(f"valid: {len(rows)} cases, degrees {sorted(degrees)}; invalid: {len(bad)} cases")


if __name__ == "__main__":
    main()
