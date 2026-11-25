#!/usr/bin/env python3
"""Independent oracle for GDT.PROF.LINE.SECT (fid-gps `prof::line::sect`).

Writes next to this script:

- `cyl_sections.json` (AC-1): nominal cylinder r = 20 mm along z, 3 600 points at
  z = 5, 10, 15 mm +- 0.01 mm with planted radial deviations.
- `cone_inplane.json` (AC-2): cone of half-angle 30 deg, sections perpendicular to the axis.
- `cases.json` (AC-3, AC-4, AC-5 and the edge, error and honesty cases).
- `det_pin.json` (AC-6): the SHA-256 of every output bit for 1 000 000 seeded points.

All geometry is invented. Every expected value is computed here, never by the Rust code:

- Band membership is decided in exact rational arithmetic (`fractions.Fraction`, or
  integers at a fixed power-of-two scale for AC-6) on the binary64 values of the section
  normal, the mapped point and the position: a point is in band k exactly when
  |n . q - s_k| <= w. The gap test of overlapping bands is exact too.
- The mapped point q = R p + t replicates GEO.FRAME.XFORM `apply_point` operation by
  operation in binary64 (Python floats: IEEE-754 double, round to nearest, no FMA):
  u_i = (r_i0 p_0 + r_i1 p_1) + r_i2 p_2, q_i = 1.0 u_i + t_i. Stepped positions are
  start + k * step, each operation rounded once.
- In-plane deviations: `cases.json` and `cone_inplane.json` give d / m with m the exact
  in-plane length of the binary64 surface normal (60-digit `decimal`), compared at 1e-12
  relative; in `cases.json` the oblique cut-off and the division follow the module's
  declared rule, the binary64 m (replicated), and every m checks the documented bound
  |m_rounded - m_exact| <= 2^-50 max(1, |ns|), also on a seeded sweep of 20 000 normals. AC-6 replicates the documented binary64 formula
  c = (ns0 n0 + ns1 n1) + ns2 n2, v_i = ns_i - c n_i, m = sqrt((v0 v0 + v1 v1) + v2 v2),
  dev = d / m (math.sqrt is correctly rounded), because a bit pin needs the exact bits.

Usage: `python3 make_sect_fixtures.py` (write) or `python3 make_sect_fixtures.py --check`
(regenerate in memory and compare with the committed files byte for byte). Stdlib only.
"""

import hashlib
import json
import math
import struct
import sys
from bisect import bisect_left, bisect_right
from decimal import Decimal, getcontext
from fractions import Fraction
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent

IDENTITY = {"r": [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], "t": [0.0, 0.0, 0.0]}
FLAG_ORDER = ["empty_section", "oblique_section", "no_projection", "withheld"]


def hx(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def hx3(p):
    return [hx(float(c)) for c in p]


def dstr(x: Decimal) -> str:
    if x == 0:
        return "0"
    return format(x, ".29e")


def pose_json(pose):
    return {"r": [hx3(row) for row in pose["r"]], "t": hx3(pose["t"])}


def apply_pose(pose, p):
    r, t = pose["r"], pose["t"]
    out = []
    for i in range(3):
        u = (r[i][0] * p[0] + r[i][1] * p[1]) + r[i][2] * p[2]
        out.append(1.0 * u + t[i])
    return out


def exact_dot(n, q):
    return sum((Fraction(a) * Fraction(b) for a, b in zip(n, q)), Fraction(0))


def in_band(v: Fraction, s: float, w: float) -> bool:
    return abs(v - Fraction(s)) <= Fraction(w)


def float_norm(n):
    return math.sqrt((n[0] * n[0] + n[1] * n[1]) + n[2] * n[2])


def float_m(ns, n):
    """The in-plane length by the module's declared binary64 formula (the cut-off rule)."""
    c = (ns[0] * n[0] + ns[1] * n[1]) + ns[2] * n[2]
    v = [ns[0] - c * n[0], ns[1] - c * n[1], ns[2] - c * n[2]]
    return math.sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])


M_BOUND = Decimal(2) ** -50  # documented |m_rounded - m_exact| <= 2^-50 * max(1, |ns|)


def check_m_bound(ns, n):
    """Asserts the documented forward-error bound of the rounded m on these inputs."""
    ns_len = Decimal(sum(Fraction(c) ** 2 for c in ns).numerator) / Decimal(
        sum(Fraction(c) ** 2 for c in ns).denominator)
    scale = max(Decimal(1), ns_len.sqrt())
    err = abs(Decimal(float_m(ns, n)) - exact_m(ns, n))
    assert err <= M_BOUND * scale, (ns, n, err)


def exact_m(ns, n):
    """The exact in-plane length |ns - (ns . n) n| of the binary64 vectors, as Decimal."""
    nsF = [Fraction(c) for c in ns]
    nF = [Fraction(c) for c in n]
    c = sum((a * b for a, b in zip(nsF, nF)), Fraction(0))
    v = [a - c * b for a, b in zip(nsF, nF)]
    sq = sum((x * x for x in v), Fraction(0))
    return (Decimal(sq.numerator) / Decimal(sq.denominator)).sqrt()


# ------------------------------------------------------------------ generic reference


def positions_of(spec):
    pos = spec["positions"]
    if pos is None:
        return None
    if "explicit" in pos:
        return list(pos["explicit"])
    start, step, count = pos["start"], pos["step"], pos["count"]
    return [start + float(k) * step for k in range(count)]


def reference(case):
    """The expected outcome of one case (sections or an error), from the rules."""
    spec = case["spec"]
    pos = positions_of(spec)
    if not pos:
        return {"error": {"code": "section_positions_missing"}}
    explicit = "explicit" in spec["positions"]
    for s in pos:
        if explicit and not math.isfinite(s):
            return {"error": {"code": "nan_input", "index": None}}
        if not math.isfinite(s) or abs(s) > 2.0 ** 480:
            return {"error": {"code": "out_of_range", "index": None}}
    n = spec["normal"]
    if not all(math.isfinite(c) for c in n):
        return {"error": {"code": "nan_input", "index": None}}
    if abs(float_norm(n) - 1.0) > 1e-12:
        return {"error": {"code": "non_unit_direction"}}
    w = spec["w"]
    if not math.isfinite(w):
        return {"error": {"code": "nan_input", "index": None}}
    if w < 0:
        return {"error": {"code": "bad_half_width"}}
    if w > 2.0 ** 480:
        return {"error": {"code": "out_of_range", "index": None}}
    method = spec["method"]
    if method != "normal3d":
        mi = method["inplane"]
        if not math.isfinite(mi):
            return {"error": {"code": "nan_input", "index": None}}
        if not (0.0 < mi <= 1.0):
            return {"error": {"code": "bad_min_inplane"}}
    order = sorted(range(len(pos)), key=lambda k: (pos[k], k))
    for a, b in zip(order, order[1:]):
        gap = Fraction(pos[b]) - Fraction(pos[a])
        if gap == 0 or gap < 2 * Fraction(w):
            return {"error": {"code": "overlapping_bands", "a": min(a, b), "b": max(a, b)}}
    pts = case["points"]
    devs = case["devs"]
    if len(devs) != len(pts) or case.get("swap_dev_index"):
        return {"error": {"code": "deviation_count_mismatch"}}
    if case["devs_pose"] != case["pose"]:
        return {"error": {"code": "pose_mismatch"}}
    secs = [{"index": k, "position": pos[k], "members": [], "dev": [], "withheld": [], "oblique": [],
             "no_projection": 0} for k in range(len(pos))]
    for i, (p, dv) in enumerate(zip(pts, devs)):
        if not all(math.isfinite(c) for c in p):
            return {"error": {"code": "nan_input", "index": i}}
        if dv is not None:
            if dv["normal"] is not None and not all(math.isfinite(c) for c in dv["normal"]):
                return {"error": {"code": "nan_input", "index": i}}
            if dv["signed"] is not None and not math.isfinite(dv["signed"]):
                return {"error": {"code": "nan_input", "index": i}}
        q = apply_pose(case["pose"], p)
        if any(abs(c) > 2.0 ** 480 for c in q):
            return {"error": {"code": "out_of_range", "index": i}}
        v = exact_dot(n, q)
        hits = [k for k in range(len(pos)) if in_band(v, pos[k], w)]
        if hits and case.get("needs_exact_out_of_domain", {}).get(str(i)):
            return {"error": {"code": "out_of_range", "index": i}}
        for k in hits:
            sec = secs[k]
            if dv is None:
                sec["no_projection"] += 1
            elif dv["signed"] is None:
                sec["withheld"].append([i, "no_sign"])
            elif method == "normal3d":
                sec["members"].append(i)
                sec["dev"].append(hx(dv["signed"]))
            elif dv["normal"] is None:
                sec["withheld"].append([i, "no_normal"])
            else:
                # The declared rule: the cut-off and the division use the rounded m.
                check_m_bound(dv["normal"], n)
                m = float_m(dv["normal"], n)
                if m < method["inplane"]:
                    sec["oblique"].append(i)
                else:
                    sec["members"].append(i)
                    sec["dev"].append(dstr(Decimal(dv["signed"]) / Decimal(m)))
    out = []
    for sec in secs:
        flags = []
        if not sec["members"]:
            flags.append("empty_section")
        if sec["oblique"]:
            flags.append("oblique_section")
        if sec["no_projection"]:
            flags.append("no_projection")
        if sec["withheld"]:
            flags.append("withheld")
        sec["flags"] = flags
        sec["position"] = hx(sec["position"])
        out.append(sec)
    return {"sections": out}


def spec_json(spec):
    pos = spec["positions"]
    if pos is None:
        pj = None
    elif "explicit" in pos:
        pj = {"explicit": [hx(s) for s in pos["explicit"]]}
    else:
        pj = {"start": hx(pos["start"]), "step": hx(pos["step"]), "count": pos["count"]}
    method = spec["method"]
    mj = "normal3d" if method == "normal3d" else {"inplane": hx(method["inplane"])}
    return {"normal": hx3(spec["normal"]), "positions": pj, "w": hx(spec["w"]), "method": mj}


def dev_json(dv):
    if dv is None:
        return None
    return {"signed": None if dv["signed"] is None else hx(dv["signed"]),
            "normal": None if dv["normal"] is None else hx3(dv["normal"])}


def make_case(cid, note, spec, points, devs, pose=IDENTITY, devs_pose=None, **extra):
    case = {"spec": spec, "points": points, "devs": devs, "pose": pose,
            "devs_pose": pose if devs_pose is None else devs_pose}
    case.update(extra)
    expect = reference(case)
    out = {"id": cid, "note": note, "spec": spec_json(spec), "pose": pose_json(pose),
           "devs_pose": pose_json(case["devs_pose"]), "points": [hx3(p) for p in points],
           "devs": [dev_json(d) for d in devs], "expect": expect}
    if case.get("swap_dev_index"):
        out["swap_dev_index"] = True
    return out


def spec(normal, positions, w, method="normal3d"):
    return {"normal": normal, "positions": positions, "w": w, "method": method}


def plain(signed, normal=(1.0, 0.0, 0.0)):
    return {"signed": signed, "normal": list(normal) if normal is not None else None}


Z = [0.0, 0.0, 1.0]

# ------------------------------------------------------------------ AC-1: cylinder


def cyl_fixture():
    zs = ["0.005", "0.010", "0.015"]
    pts = []
    for k, zk in enumerate(zs):
        for j in range(1200):
            dz = Decimal((j * 7919 + k * 13) % 2001 - 1000) * Decimal("1e-8")  # +-0.01 mm
            d = Decimal((j * 104729 + k * 31) % 1001 - 500) * Decimal("1e-7")  # +-0.05 mm
            pts.append([str(Decimal(zk) + dz), str(d)])
    # Exact membership of every point (n = z: n . q = z exactly; x and y do not enter).
    pos = [float(z) for z in zs]
    w = 5e-5
    members = [[], [], []]
    for i, (z, _) in enumerate(pts):
        v = Fraction(float(z))
        hits = [k for k in range(3) if in_band(v, pos[k], w)]
        assert len(hits) == 1 and hits[0] == i // 1200, (i, z)
        members[hits[0]].append(i)
    assert [len(m) for m in members] == [1200, 1200, 1200]
    return {
        "note": "AC-1: nominal cylinder r = 20 mm along z; point i is at angle 2 pi (i mod 1200) / 1200 "
                "and height z_i, radius 0.02 + d_i (d_i the planted radial deviation, the signed deviation "
                "of GDT.PROF.DEV). With n = z the band verdict depends on z only.",
        "radius": "0.02",
        "normal": hx3(Z),
        "positions": [hx(p) for p in pos],
        "w": hx(w),
        "points": pts,
        "expect_members": [[1200 * k, 1200 * k + 1199] for k in range(3)],
    }


# ------------------------------------------------------------------ AC-2: cone


def dcos(x: Decimal) -> Decimal:
    # Taylor series, 60 digits; |x| <= 2 pi.
    getcontext().prec += 5
    s, term, k = Decimal(1), Decimal(1), 0
    while True:
        k += 2
        term = -term * x * x / (k * (k - 1))
        if abs(term) < Decimal("1e-70"):
            break
        s += term
    getcontext().prec -= 5
    return +s


def dsin(x: Decimal) -> Decimal:
    getcontext().prec += 5
    s, term, k = x, x, 1
    while True:
        k += 2
        term = -term * x * x / (k * (k - 1))
        if abs(term) < Decimal("1e-70"):
            break
        s += term
    getcontext().prec -= 5
    return +s


DPI = Decimal("3.14159265358979323846264338327950288419716939937510582097494")


def cone_fixture():
    cos30 = Decimal(3).sqrt() / 2
    sin30 = Decimal("0.5")
    tan30 = sin30 / cos30
    zs = ["0.010", "0.020", "0.030"]
    pos = [float(z) for z in zs]
    w = 5e-5
    n = Z
    pts, normals, devs = [], [], []
    for k, zk in enumerate(zs):
        for j in range(120):
            th = 2 * DPI * j / 120
            c, s = dcos(th), dsin(th)
            zf = Decimal(zk) + Decimal((j * 37 + k) % 41 - 20) * Decimal("5e-7")  # +-0.01 mm
            rho = zf * tan30
            d = Decimal((j * 53 + k * 7) % 101 - 50) * Decimal("1e-6")  # +-0.05 mm
            normal = [cos30 * c, cos30 * s, -sin30]  # outward from the solid cone
            foot = [rho * c, rho * s, zf]
            p = [float(foot[i] + d * normal[i]) for i in range(3)]
            pts.append(p)
            normals.append([float(x) for x in normal])
            devs.append(float(d))
    expect_dev = []
    for i, (p, ns, d) in enumerate(zip(pts, normals, devs)):
        v = exact_dot(n, p)
        hits = [k for k in range(3) if in_band(v, pos[k], w)]
        assert hits == [i // 120], (i, hits)
        check_m_bound(ns, n)
        m = exact_m(ns, n)
        e = Decimal(d) / m
        lit = Decimal(d) / cos30  # AC-2 as stated: d / cos 30 deg
        if lit != 0:
            assert abs(e - lit) / abs(lit) < Decimal("1e-14")
        else:
            assert e == 0
        expect_dev.append(dstr(e))
    return {
        "note": "AC-2: cone of half-angle 30 deg about z (apex at the origin, material inside); sections "
                "perpendicular to the axis; expect_dev = d / m with m the exact in-plane length of the given "
                "normal, within 1e-14 of d / cos 30 deg (checked here).",
        "cos30": dstr(cos30),
        "normal": hx3(n),
        "positions": [hx(p) for p in pos],
        "w": hx(w),
        "min_inplane": hx(0.5),
        "points": [hx3(p) for p in pts],
        "surface_normals": [hx3(x) for x in normals],
        "signed": [hx(d) for d in devs],
        "expect_members": [[120 * k, 120 * k + 119] for k in range(3)],
        "expect_dev": expect_dev,
    }


# ------------------------------------------------------------------ cases


def exact_trap_points(n, s, w):
    """Points whose naive float verdict |fl(n . q) - s| <= w differs from the exact one."""
    found_in, found_out = [], []
    x = 0.0
    for t in range(1, 4000):
        x = t * 1.37e-5
        for bound in (s + w, s - w):
            z0 = (bound - n[0] * x) / n[2]
            for k in range(-40, 41):
                z = z0 + k * math.ulp(z0)
                q = [x, 0.0, z]
                naive = abs(((n[0] * q[0] + n[1] * q[1]) + n[2] * q[2]) - s) <= w
                exact = in_band(exact_dot(n, q), s, w)
                if naive and not exact and len(found_out) < 3:
                    found_out.append(q)
                if exact and not naive and len(found_in) < 3:
                    found_in.append(q)
        if len(found_in) >= 3 and len(found_out) >= 3:
            break
    assert len(found_in) == 3 and len(found_out) == 3
    return found_in + found_out


def cases_fixture():
    cases = []
    s7, w14 = 2.0 ** -7, 2.0 ** -14
    one = [[0.0, 0.0, 0.01]]
    one_dev = [plain(1e-6)]

    # AC-3
    cases.append(make_case("ac3_overlap", "AC-3: positions 5 and 5.05 mm with w = 0.05 mm overlap",
                           spec(Z, {"explicit": [0.005, 0.00505]}, 5e-5), one, one_dev))
    cases.append(make_case("ac3_missing", "AC-3: no positions stated",
                           spec(Z, None, 5e-5), one, one_dev))
    cases.append(make_case("missing_empty_list", "an empty explicit list states no positions",
                           spec(Z, {"explicit": []}, 5e-5), one, one_dev))
    cases.append(make_case("missing_zero_count", "a stepped count of 0 states no positions",
                           spec(Z, {"start": 0.005, "step": 0.005, "count": 0}, 5e-5), one, one_dev))
    cases.append(make_case("overlap_unsorted", "overlap found after sorting; named by chain index",
                           spec(Z, {"explicit": [0.0101, 0.005, 0.01]}, 5e-5), one, one_dev))
    cases.append(make_case("overlap_identical_w0", "identical positions overlap even with w = 0",
                           spec(Z, {"explicit": [0.01, 0.01]}, 0.0), one, one_dev))
    trap = [2.0 ** -70, 2.0 ** -13]
    assert trap[1] - trap[0] == 2 * w14 and Fraction(trap[1]) - Fraction(trap[0]) < 2 * Fraction(w14)
    cases.append(make_case("overlap_rounded_gap", "the float gap rounds to 2w but the exact gap is smaller",
                           spec(Z, {"explicit": trap}, w14), one, one_dev))
    # Bands exactly 2w apart: not overlapping; the shared bound belongs to both.
    tp = [s7, s7 + 2.0 ** -13]
    pts = [[0.0, 0.0, s7 + w14], [0.0, 0.0, s7], [0.0, 0.0, s7 + 2.0 ** -13]]
    cases.append(make_case("touching_bands", "bands exactly 2w apart; the shared bound is in both",
                           spec(Z, {"explicit": tp}, w14), pts, [plain(1e-6), plain(-2e-6), plain(3e-6)]))
    # AC-4
    pts = [[0.0, 0.0, s7 + w14], [0.0, 0.0, s7 - w14], [0.0, 0.0, s7],
           [0.0, 0.0, math.nextafter(s7 + w14, math.inf)], [0.0, 0.0, math.nextafter(s7 - w14, -math.inf)]]
    cases.append(make_case("ac4_inclusive", "AC-4: |n . p - s| = w is a member; one ulp beyond is not",
                           spec(Z, {"explicit": [s7]}, w14), pts, [plain(1e-6 * (i + 1)) for i in range(5)]))
    # Exact versus naive float verdicts on an oblique normal.
    n = [0.6, 0.0, 0.8]
    pts = exact_trap_points(n, 0.01, 1e-4)
    cases.append(make_case("exact_not_naive", "points the rounded dot product would misplace",
                           spec(n, {"explicit": [0.01]}, 1e-4), pts, [plain(1e-6 * (i + 1)) for i in range(6)]))
    # Stepped positions.
    pts = [[0.0, 0.0, z] for z in (0.005, 0.00509, 0.0101, 0.0149, 0.0152, 0.0075)]
    cases.append(make_case("stepped", "start 5 mm, step 5 mm, count 3",
                           spec(Z, {"start": 0.005, "step": 0.005, "count": 3}, 1e-4), pts,
                           [plain(1e-6 * (i + 1)) for i in range(6)]))
    # Chain order is kept for unsorted explicit positions.
    pts = [[0.0, 0.0, 0.02], [0.0, 0.0, 0.01], [0.0, 0.0, 0.0300001]]
    cases.append(make_case("unsorted_chain_order", "sections are reported in chain order",
                           spec(Z, {"explicit": [0.03, 0.01, 0.02]}, 1e-4), pts,
                           [plain(1e-6), plain(2e-6), plain(3e-6)]))
    # REQ-008 and honesty.
    pts = [[0.0, 0.0, 0.01], [0.0, 0.0, 0.01005], [0.0, 0.0, 0.015], [0.0, 0.0, 0.02], [0.0, 0.0, 0.01999],
           [0.0, 0.0, 0.02001], [0.0, 0.0, 0.00995]]
    devs = [plain(1e-6), None, None, plain(None), plain(2e-6, None), plain(-3e-6), plain(4e-6)]
    cases.append(make_case("honesty_normal3d", "no_projection counted, unsigned withheld (normal3d)",
                           spec(Z, {"explicit": [0.01, 0.02]}, 1e-4), pts, devs))
    cases.append(make_case("honesty_inplane", "as above under inplane: a missing normal is withheld too",
                           spec(Z, {"explicit": [0.01, 0.02]}, 1e-4, {"inplane": 0.5}), pts, devs))
    # AC-5: plane face, surface normal parallel to n.
    pts = [[0.001 * (i - 10), 0.0007 * i, ((i * 7) % 11 - 5) * 1e-6] for i in range(21)]
    devs = [plain(p[2], Z) for p in pts]
    cases.append(make_case("ac5_plane_oblique", "AC-5: normal parallel to n, min_inplane 0.5: all oblique",
                           spec(Z, {"explicit": [0.0]}, 5e-5, {"inplane": 0.5}), pts, devs))
    cases.append(make_case("plane_normal3d", "the same plane points under normal3d are all members",
                           spec(Z, {"explicit": [0.0]}, 5e-5), pts, devs))
    # m exactly at the threshold is accepted (m < min_inplane excludes).
    ns = [0.5, 0.0, math.sqrt(0.75)]
    pts = [[0.0, 0.0, 0.01], [0.0, 0.0, 0.01]]
    devs = [plain(1e-5, ns), plain(-1e-5, [0.4999999999999999, 0.0, math.sqrt(0.75)])]
    cases.append(make_case("inplane_threshold", "m = 0.5 exactly is kept; one ulp below is oblique",
                           spec(Z, {"explicit": [0.01]}, 1e-4, {"inplane": 0.5}), pts, devs))
    # Reviewer's counter-example (#614): exact m = 0.5 - 1.1e-18, rounded m = 0.5 exactly.
    # By the declared rule (rounded m) the point is kept at min_inplane 0.5 and oblique one
    # ulp above.
    n = [0.0, 0.6, 0.8]
    ns = [float.fromhex("0x1.3d21e59798201p-2"), float.fromhex("0x1.a509a19658804p-3"),
          float.fromhex("0x1.db4fcbdab5654p-1")]
    assert float_m(ns, n) == 0.5 and exact_m(ns, n) < Decimal("0.5")
    pts = [[0.0, 0.006, 0.008]]
    cases.append(make_case("inplane_rounded_cutoff", "exact m < 0.5 but rounded m = 0.5: kept by the declared rule",
                           spec(n, {"explicit": [0.01]}, 1e-4, {"inplane": 0.5}), pts, [plain(1e-5, ns)]))
    cases.append(make_case("inplane_rounded_cutoff_above", "the same point at min_inplane one ulp above 0.5",
                           spec(n, {"explicit": [0.01]}, 1e-4, {"inplane": math.nextafter(0.5, 1.0)}), pts,
                           [plain(1e-5, ns)]))
    # Pose.
    tz = {"r": IDENTITY["r"], "t": [0.0, 0.0, 0.001]}
    pts = [[0.0, 0.0, 0.009], [0.0, 0.0, 0.01], [0.0, 0.0, 0.00905]]
    cases.append(make_case("pose_translation", "membership is on q = R p + t",
                           spec(Z, {"explicit": [0.01]}, 1e-4), pts, [plain(1e-6), plain(2e-6), plain(3e-6)],
                           pose=tz))
    rx = {"r": [[1.0, 0.0, 0.0], [0.0, 0.0, -1.0], [0.0, 1.0, 0.0]], "t": [0.0, 0.0, 0.0]}
    pts = [[0.0, 0.01, 0.0], [0.0, 0.0, 0.01]]
    cases.append(make_case("pose_rotation", "rotation by 90 deg about x: q_z = p_y",
                           spec(Z, {"explicit": [0.01]}, 1e-4), pts, [plain(1e-6), plain(2e-6)], pose=rx))
    cases.append(make_case("pose_mismatch", "the deviations were taken under another pose",
                           spec(Z, {"explicit": [0.01]}, 1e-4), one, one_dev, pose=tz, devs_pose=IDENTITY))
    # Counts.
    cases.append(make_case("count_mismatch", "one deviation for two points",
                           spec(Z, {"explicit": [0.01]}, 1e-4), [[0.0, 0.0, 0.01], [0.0, 0.0, 0.02]], one_dev))
    cases.append(make_case("index_mismatch", "deviation indexes out of input order",
                           spec(Z, {"explicit": [0.01]}, 1e-4), [[0.0, 0.0, 0.01], [0.0, 0.0, 0.02]],
                           [plain(1e-6), plain(2e-6)], swap_dev_index=True))
    # Direction.
    cases.append(make_case("non_unit", "|n| - 1 = 4e-12",
                           spec([0.0, 0.0, 1.0 + 4e-12], {"explicit": [0.01]}, 1e-4), one, one_dev))
    near = [0.0, 0.0, 1.0 + 2.5e-13]
    cases.append(make_case("near_unit", "|n| - 1 = 2.5e-13 is unit; membership uses n as given",
                           spec(near, {"explicit": [0.01]}, 1e-4), [[0.0, 0.0, 0.0101], [0.0, 0.0, 0.0099]],
                           [plain(1e-6), plain(2e-6)]))
    # Bad values.
    nan = float("nan")
    cases.append(make_case("nan_point", "a NaN coordinate",
                           spec(Z, {"explicit": [0.01]}, 1e-4), [[0.0, 0.0, 0.01], [0.0, nan, 0.01]],
                           [plain(1e-6), plain(2e-6)]))
    cases.append(make_case("nan_w", "a NaN half-width", spec(Z, {"explicit": [0.01]}, nan), one, one_dev))
    cases.append(make_case("nan_normal", "a NaN normal", spec([0.0, nan, 1.0], {"explicit": [0.01]}, 1e-4),
                           one, one_dev))
    cases.append(make_case("inf_position", "an infinite position",
                           spec(Z, {"explicit": [0.01, math.inf]}, 1e-4), one, one_dev))
    cases.append(make_case("nan_signed", "a NaN signed deviation",
                           spec(Z, {"explicit": [0.01]}, 1e-4), [[0.0, 0.0, 0.01], [0.0, 0.0, 0.5]],
                           [plain(1e-6), plain(nan)]))
    cases.append(make_case("nan_surface_normal", "a NaN surface normal",
                           spec(Z, {"explicit": [0.01]}, 1e-4), one, [plain(1e-6, [nan, 0.0, 1.0])]))
    cases.append(make_case("negative_w", "a negative half-width", spec(Z, {"explicit": [0.01]}, -1e-5),
                           one, one_dev))
    for k, mi in enumerate([0.0, 1.5, -0.1]):
        cases.append(make_case(f"bad_min_inplane_{k}", f"min_inplane {mi}",
                               spec(Z, {"explicit": [0.01]}, 1e-4, {"inplane": mi}), one, one_dev))
    cases.append(make_case("huge_point", "a coordinate above 2^480",
                           spec(Z, {"explicit": [0.01]}, 1e-4), [[1e200, 0.0, 0.01]], one_dev))
    cases.append(make_case("huge_position", "a position above 2^480",
                           spec(Z, {"explicit": [0.01, 1e200]}, 1e-4), one, one_dev))
    # The exact path refuses a non-zero coordinate below 2^-480; the float filter decides
    # far from a bound without it.
    pts = [[0.0, 0.0, s7 + w14], [1e-150, 0.0, 0.5], [1e-150, 0.0, s7 + w14]]
    cases.append(make_case("exact_domain", "q_x = 1e-150 is refused only where the exact sign is needed",
                           spec(Z, {"explicit": [s7]}, w14), pts, [plain(1e-6), plain(2e-6), plain(3e-6)],
                           needs_exact_out_of_domain={"2": True}))
    # The documented bound of the rounded m holds on a seeded sweep of unit normals.
    rng = SplitMix(0x6D62)
    for _ in range(20000):
        v = [rng.uniform(-1.0, 1.0) for _ in range(3)]
        ln = math.sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])
        if ln < 0.25:
            continue
        ns = [c / ln for c in v]
        for nn in (Z, [0.0, 0.6, 0.8], [0.48, 0.36, 0.8]):
            check_m_bound(ns, nn)
    return {"cases": cases}


# ------------------------------------------------------------------ AC-6


class SplitMix:
    def __init__(self, seed):
        self.s = seed & 0xFFFFFFFFFFFFFFFF

    def next(self):
        self.s = (self.s + 0x9E3779B97F4A7C15) & 0xFFFFFFFFFFFFFFFF
        z = self.s
        z = ((z ^ (z >> 30)) * 0xBF58476D1CE4E5B9) & 0xFFFFFFFFFFFFFFFF
        z = ((z ^ (z >> 27)) * 0x94D049BB133111EB) & 0xFFFFFFFFFFFFFFFF
        return z ^ (z >> 31)

    def uniform(self, lo, hi):
        u = (self.next() >> 11) * (1.0 / 9007199254740992.0)
        return lo + u * (hi - lo)


AC6 = {
    "seed": 0x5EC7,
    "count": 1_000_000,
    "pose": {"r": [[0.6, -0.8, 0.0], [0.8, 0.6, 0.0], [0.0, 0.0, 1.0]], "t": [0.001, -0.002, 0.003]},
    "normal": [0.0, 0.6, 0.8],
    "start": -0.04,
    "step": 0.0016,
    "sections": 100,
    "w": 0.0004,
    "min_inplane": 0.3,
}

SCALE = 400  # every product of the AC-6 data is an integer multiple of 2^-400 (asserted)


def to_int(x: float, e: int) -> int:
    num, den = x.as_integer_ratio()
    assert (1 << e) % den == 0, x
    return num * ((1 << e) // den)


def ac6_points():
    rng = SplitMix(AC6["seed"])
    out = []
    for _ in range(AC6["count"]):
        p = [rng.uniform(-0.05, 0.05), rng.uniform(-0.05, 0.05), rng.uniform(0.0, 0.1)]
        kind = rng.next() % 16
        d = rng.uniform(-1e-4, 1e-4)
        v = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)]
        ln = math.sqrt((v[0] * v[0] + v[1] * v[1]) + v[2] * v[2])
        ns = [0.0, 0.0, 1.0] if ln < 0.25 else [v[0] / ln, v[1] / ln, v[2] / ln]
        out.append((p, kind, d, ns))
    return out


def ac6_digest():
    pts = ac6_points()
    n = AC6["normal"]
    w = AC6["w"]
    pos = [AC6["start"] + float(k) * AC6["step"] for k in range(AC6["sections"])]
    assert all(b - a > 2 * w for a, b in zip(pos, pos[1:]))
    half = SCALE // 2
    n_int = [to_int(c, half) for c in n]
    lo = [to_int(s, SCALE) - to_int(w, SCALE) for s in pos]  # ascending
    hi = [to_int(s, SCALE) + to_int(w, SCALE) for s in pos]
    runs = {"normal3d": None, "inplane": AC6["min_inplane"]}
    secs = {r: [{"members": [], "withheld": [], "oblique": [], "noproj": 0} for _ in pos] for r in runs}
    for i, (p, kind, d, ns) in enumerate(pts):
        q = apply_pose(AC6["pose"], p)
        v = sum(a * to_int(b, half) for a, b in zip(n_int, q))
        # Bands k with lo_k <= v <= hi_k.
        first = bisect_left(hi, v)
        last = bisect_right(lo, v)
        hits = range(first, last)
        if not hits:
            continue
        m = None
        if kind >= 3:
            c = (ns[0] * n[0] + ns[1] * n[1]) + ns[2] * n[2]
            vv = [ns[0] - c * n[0], ns[1] - c * n[1], ns[2] - c * n[2]]
            m = math.sqrt((vv[0] * vv[0] + vv[1] * vv[1]) + vv[2] * vv[2])
        for k in hits:
            for run, mi in runs.items():
                sec = secs[run][k]
                if kind == 0:
                    sec["noproj"] += 1
                elif kind == 1:
                    sec["withheld"].append((i, 0))
                elif run == "normal3d":
                    sec["members"].append((i, d))
                elif kind == 2:
                    sec["withheld"].append((i, 1))
                elif m < mi:
                    sec["oblique"].append(i)
                else:
                    sec["members"].append((i, d / m))
    buf = bytearray()
    stats = {}
    for run in runs:
        total = 0
        for k, sec in enumerate(secs[run]):
            buf += struct.pack("<I", k)
            buf += struct.pack("<d", pos[k])
            buf += struct.pack("<I", len(sec["members"]))
            for i, dv in sec["members"]:
                buf += struct.pack("<Id", i, dv)
            buf += struct.pack("<I", len(sec["withheld"]))
            for i, r in sec["withheld"]:
                buf += struct.pack("<IB", i, r)
            buf += struct.pack("<I", len(sec["oblique"]))
            for i in sec["oblique"]:
                buf += struct.pack("<I", i)
            buf += struct.pack("<I", sec["noproj"])
            flags = 0
            if not sec["members"]:
                flags |= 1
            if sec["oblique"]:
                flags |= 2
            if sec["noproj"]:
                flags |= 4
            if sec["withheld"]:
                flags |= 8
            buf += struct.pack("<B", flags)
            total += len(sec["members"])
        stats[run] = total
    return {
        "note": "AC-6: SplitMix64 points (seed 0x5ec7); per point: p = (U(-0.05,0.05), U(-0.05,0.05), "
                "U(0,0.1)), kind = next() % 16 (0 no projection, 1 no sign, 2 no normal, else both), "
                "d = U(-1e-4,1e-4), v = 3 x U(-1,1), normal = v / |v| (or z when |v| < 0.25). "
                "U(lo,hi) = lo + ((next() >> 11) * 2^-53) * (hi - lo). The digest is SHA-256 over, per run "
                "(normal3d then inplane), per section in chain order: u32 index, f64 position, u32 count and "
                "(u32 index, f64 dev) per member, u32 count and (u32 index, u8 reason 0 no_sign 1 no_normal) per "
                "withheld point, u32 count and u32 index per oblique point, u32 no-projection count, u8 flags "
                "(1 empty, 2 oblique, 4 no_projection, 8 withheld); little-endian.",
        "seed": AC6["seed"],
        "count": AC6["count"],
        "pose": pose_json(AC6["pose"]),
        "normal": hx3(AC6["normal"]),
        "start": hx(AC6["start"]),
        "step": hx(AC6["step"]),
        "sections": AC6["sections"],
        "w": hx(AC6["w"]),
        "min_inplane": hx(AC6["min_inplane"]),
        "members_normal3d": stats["normal3d"],
        "members_inplane": stats["inplane"],
        "digest": "sha256:" + hashlib.sha256(bytes(buf)).hexdigest(),
    }


# ------------------------------------------------------------------ output


def render(obj) -> str:
    return json.dumps(obj, indent=1, sort_keys=False) + "\n"


def render_compact(obj) -> str:
    return json.dumps(obj, separators=(",", ":"), sort_keys=False) + "\n"


def main():
    outs = {
        "cyl_sections.json": render_compact(cyl_fixture()),
        "cone_inplane.json": render_compact(cone_fixture()),
        "cases.json": render(cases_fixture()),
        "det_pin.json": render(ac6_digest()),
    }
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, text in outs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                print(f"{name}: differs from the generator output", file=sys.stderr)
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
            print(f"wrote {name}")
    sys.exit(1 if bad else 0)


if __name__ == "__main__":
    main()
