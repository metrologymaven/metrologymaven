#!/usr/bin/env python3
"""Independent reference for SCAN.CLOUD.REG.RREP (registration report and registered cloud
versions). CC0-1.0 data.

Stdlib only. Nothing here was produced by running the Rust crate. Reads the shared scan fixture
`fixtures/scan/reg/three_scans_known.txt` (written by make_graph_fixtures.py; never modified
here) and writes `fixtures/scan/reg/rrep_cases.txt`: for each case, the expected content digest
of every input cloud and every registered output cloud, each pose's translation length and
correctly rounded rotation angle, the group ID and the expected canonical report JSON.

`--check` regenerates the file in memory and fails on any difference with the committed file.

# Rules reproduced (spec SCAN.CLOUD.REG.RREP and the pinned rules of the `fid-scan-cloud`
# `reg::report` module documentation)

Python floats are IEEE binary64; `+ - * /` and `math.sqrt` are correctly rounded and never
fused, so every printed value is the same on every platform. No libm function decides a printed
value (see the rotation angle below).

* Input clouds: the points and normals of each scan of `three_scans_known` in frame
  INPUT_FRAME, normal channel only.
* GEO.TYPES.DIGEST layout 1 of a point cloud: `FIDGEO | 0x01 | 0x02 | frame[16] | n u64 | x[n]
  | y[n] | z[n] | 0 u64 (triangles) | 1 u64 (channels) | 6 u16 | "normal" | 8 u8 | n u64 |
  nx[n] | ny[n] | nz[n]`, little endian, f64 as IEEE bits with -0.0 written as +0.0; the digest
  is SHA-256 (`hashlib`).
* GEO.FRAME.APPLY of a rigid pose (R, t) into REFERENCE_FRAME: point `y[i] = dot3(R[i], x) +
  t[i]`, normal `m[i] = dot3(R[i], n)` then `n'[i] = m[i] / sqrt(dot3(m, m))` per component,
  with `dot3(a, b) = (a0 * b0 + a1 * b1) + a2 * b2`.
* Translation length `sqrt(dot3(t, t))`.
* Rotation angle `acos(c)`, `c = clamp(((R00 + R11) + R22 - 1) / 2, -1, 1)`. `c` is computed
  exactly as above; `acos(c)` is computed here with the `decimal` module (80 digits, Newton on
  a Taylor-series cosine; `math.acos` only seeds the iteration) and rounded to the nearest
  double, the script asserting the 80-digit value is far from a rounding boundary. The crate's
  `fid_num::f::acos` is a faithfully rounded libm port, so the test accepts its result within
  one ulp of this value and substitutes its exact bits for the `@ROT<i>@` placeholder of the
  report JSON (every other byte of the report is compared exactly).
* Report JSON: RFC 8785 over strings, arrays and objects only (keys sorted; ASCII content, so
  `json.dumps(sort_keys=True, separators=(",", ":"))` is the canonical form); f64 values as
  `"0x"` plus 16 hex digits of the bits (-0.0 as +0.0); quantities `{"unit", "value"}`; pairs
  sorted by (a, b); scans in input order; `schema` fid.scan.registration-report/1; the required
  `graph`, `accuracy_basis: unverified` and `residual_kind: registration_fit` members.
* Group ID: `uuid.uuid5(NAMESPACE, command | 0x00 | sorted raw input digests)`, NAMESPACE =
  `uuid.uuid5(NAMESPACE_URL, NAMESPACE_URL_NAME)`.
* Poses: case `three_scans_known` uses the fixture's poses; case `identity_fixed` expresses them
  relative to scan 0 (`R0^T Ri`, `R0^T (ti - t0)`, dot3 down the columns), so scan 0 is exactly
  the identity (AC-3) and the scans keep their placement.
* Overlap (CTGT, every point sampled since `max_samples` exceeds the scan size): a point of
  scan a at pose a is within when `sqrt(min dist2) <= d` over the points of scan b at pose b,
  `dist2 = (dx * dx + dy * dy) + dz * dz`; fraction `within / sampled`. The design overlaps keep
  a margin of about 3 mm against d = 1 mm, so no count sits at a boundary.
* Pair graph (GRAPH): a cell is disjoint when the boxes of the two posed scans have, on some
  axis, a gap g > 0 with `sqrt(g * g) > d`; an edge (a, b), a < b, when `min(f(a, b), f(b, a))
  >= min_overlap`; components by union-find; status `edge`, `disjoint` (both cells disjoint)
  or `below_min`.
* The residual statistics and no-residual counts of each pair are invented test inputs
  (rms = 1e-5 (a + 1) + 2e-6 b, max = 4 rms, n = within, no_hit = 3a + 1, no_normal = b,
  rejected = 2); the report only records them.

# File format ('#' lines are comments; floats are Python `repr`, round-trip exact)

    namespace <uuid>
    input_frame <32 hex>
    reference_frame <32 hex>
    case <name> command <cmd> algorithm <alg> verdict <verdict text> global_rms <float>
         d <overlap distance m> min_overlap <float> max_samples <int>        (one line)
    pose <i> <R row-major 9> <t 3>                       (scanner frame to reference frame)
    graph_pair <a> <b> edge|below_min|disjoint            (every a < b)
    pair <a> <b> within <w> sampled <s> rms <m> max <m> n <k> no_hit <k> no_normal <k> rejected <k>
                                                          (in the order given to the report)
    scan <i> input <sha256 hex> output <sha256 hex> translation <hex64> rotation <hex64>
    group <uuid>
    report <canonical JSON with @ROT<i>@ for the rotation value of scan i>
    end

Usage: python make_rrep_fixtures.py [--check]
"""

import decimal
import hashlib
import json
import math
import os
import struct
import sys
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))
SOURCE = "three_scans_known.txt"
OUT = "rrep_cases.txt"
INPUT_FRAME = bytes([2, 7, 1, 8, 2, 8, 1, 8, 2, 8, 4, 5, 9, 0, 4, 5])
REFERENCE_FRAME = bytes([0xA0 + i for i in range(16)])
NAMESPACE_URL_NAME = (
    "https://github.com/metrologymaven/metrologymaven/spec/scan/cloud/reg/rrep#registration-group-v1"
)
NAMESPACE = uuid.uuid5(uuid.NAMESPACE_URL, NAMESPACE_URL_NAME)
SCHEMA = "fid.scan.registration-report/1"

HEADER = (
    "# SCAN.CLOUD.REG.RREP fixtures; generated by make_rrep_fixtures.py (independent stdlib Python).\n"
    "# CC0-1.0. Synthetic data; see the script for the rules and the file format.\n"
)


def plus_zero(v):
    return 0.0 if v == 0.0 else v


def bits(v):
    return struct.unpack("<Q", struct.pack("<d", plus_zero(v)))[0]


def hex64(v):
    return "0x%016x" % bits(v)


def dot3(a, b):
    return (a[0] * b[0] + a[1] * b[1]) + a[2] * b[2]


def read_scans():
    with open(os.path.join(HERE, SOURCE), encoding="utf-8", newline="") as f:
        text = f.read()
    assert "\r" not in text
    scans = []
    cur = None
    for line in text.split("\n"):
        if not line or line.startswith("#"):
            continue
        t = line.split()
        if t[0] == "scan":
            assert int(t[1]) == len(scans)
            cur = {"points": [], "normals": []}
        elif t[0] == "pose":
            v = [float(x) for x in t[1:13]]
            cur["rot"] = [v[0:3], v[3:6], v[6:9]]
            cur["t"] = v[9:12]
        elif t[0] == "p":
            v = [float(x) for x in t[1:7]]
            cur["points"].append(v[0:3])
            cur["normals"].append(v[3:6])
        elif t[0] == "end":
            scans.append(cur)
            cur = None
    return scans


def cloud_digest(frame, points, normals):
    n = len(points)
    out = [b"FIDGEO", bytes([1, 2]), frame, struct.pack("<Q", n)]
    for k in range(3):
        out.append(b"".join(struct.pack("<Q", bits(p[k])) for p in points))
    out.append(struct.pack("<Q", 0))
    out.append(struct.pack("<Q", 1))
    out.append(struct.pack("<H", 6) + b"normal" + bytes([8]) + struct.pack("<Q", n))
    for k in range(3):
        out.append(b"".join(struct.pack("<Q", bits(m[k])) for m in normals))
    return hashlib.sha256(b"".join(out)).digest()


def transform(rot, t, points, normals):
    pts = [[dot3(rot[i], x) + t[i] for i in range(3)] for x in points]
    nrm = []
    for n in normals:
        m = [dot3(rot[i], n) for i in range(3)]
        ln = math.sqrt(dot3(m, m))
        nrm.append([m[i] / ln for i in range(3)])
    return pts, nrm


# ------------------------------------------------------------------ correctly rounded acos

D = decimal.Decimal


def dec_pi():
    """pi to the current precision (the `decimal` module documentation recipe)."""
    with decimal.localcontext() as ctx:
        ctx.prec += 2
        three = D(3)
        lasts, t, s, n, na, d, da = 0, three, 3, 1, 0, 0, 24
        while s != lasts:
            lasts = s
            n, na = n + na, na + 8
            d, da = d + da, da + 32
            t = (t * n) / d
            s += t
    return +s


def dec_cos_sin(x):
    """Taylor series of cos and sin at x (|x| <= 4), to the current precision."""
    eps = D(10) ** -(decimal.getcontext().prec + 5)
    c, s = D(0), D(0)
    term = D(1)
    k = 0
    while True:
        if k % 2 == 0:
            c += term if (k // 2) % 2 == 0 else -term
        else:
            s += term if (k // 2) % 2 == 0 else -term
        k += 1
        term = term * x / k
        if abs(term) < eps and k > 4:
            break
    return c, s


def acos_cr(c):
    """The double nearest to acos(c) for a double c in [-1, 1]."""
    if c == 1.0:
        return 0.0
    with decimal.localcontext() as ctx:
        ctx.prec = 80
        if c == -1.0:
            theta = dec_pi()
        else:
            target = D(c)
            theta = D(repr(math.acos(c)))  # seed only; Newton below decides every digit
            for _ in range(200):
                cs, sn = dec_cos_sin(theta)
                step = (cs - target) / sn
                theta += step
                if abs(step) < D(10) ** -65:
                    break
            else:
                raise AssertionError("Newton did not converge for c = %r" % c)
        f = float(theta)
        lo, hi = math.nextafter(f, -math.inf), math.nextafter(f, math.inf)
        for g in (lo, hi):
            gap = abs(D(g) - D(f)) / 2
            assert abs(theta - D(f)) < gap - D(10) ** -55, "too close to a rounding boundary"
        return f


# ------------------------------------------------------------------ report


def quantity(unit, v):
    return {"unit": unit, "value": hex64(v)}


def apply(rot, t, x):
    """The pinned GEO.FRAME.XFORM image R x + t."""
    return [dot3(rot[k], x) + t[k] for k in range(3)]


def relative_to_first(poses):
    """Every pose expressed relative to pose 0: R' = R0^T Ri, t' = R0^T (ti - t0), so scan 0
    gets exactly the identity and the scans keep their mutual placement (up to rounding,
    far inside the 3 mm overlap margin of the fixture)."""
    r0, t0 = poses[0]
    out = [IDENTITY]
    for rot, t in poses[1:]:
        r = [[dot3((r0[0][i], r0[1][i], r0[2][i]), (rot[0][j], rot[1][j], rot[2][j])) for j in range(3)] for i in range(3)]
        u = [t[k] - t0[k] for k in range(3)]
        tt = [dot3((r0[0][i], r0[1][i], r0[2][i]), u) for i in range(3)]
        out.append((r, tt))
    return out


def overlap(world_a, world_b, d):
    """CTGT overlap of every (sampled = all) point of A against the points of B: within when
    sqrt(min dist2) <= d, dist2 = (dx * dx + dy * dy) + dz * dz (brute force)."""
    within = 0
    for y in world_a:
        best = min((y[0] - q[0]) * (y[0] - q[0]) + (y[1] - q[1]) * (y[1] - q[1]) + (y[2] - q[2]) * (y[2] - q[2])
                   for q in world_b)
        if math.sqrt(best) <= d:
            within += 1
    return within, len(world_a)


def disjoint(world_a, world_b, d):
    """GRAPH prefilter: on some axis the gap g between the boxes has g > 0 and sqrt(g * g) > d."""
    for k in range(3):
        a_lo, a_hi = min(p[k] for p in world_a), max(p[k] for p in world_a)
        b_lo, b_hi = min(p[k] for p in world_b), max(p[k] for p in world_b)
        for g in (a_lo - b_hi, b_lo - a_hi):
            if g > 0 and math.sqrt(g * g) > d:
                return True
    return False


CASES = [
    {
        "name": "three_scans_known",
        "command": "cloud.register_global@1",
        "algorithm": "scan.reg.global_p2plane@1.0.0",
        "verdict": "converged(increment)",
        "status": "converged",
        "global_rms": 1.5e-05,
        "poses": "fixture",
        "d": 0.001,
        # 0-2 overlaps 0.24 < 0.3: below_min; the graph stays connected through scan 1
        "min_overlap": 0.3,
        "max_samples": 1000000,
        # both directions of every edge, given in descending order (the report sorts them)
        "pairs": [(2, 1), (1, 2), (1, 0), (0, 1)],
    },
    {
        "name": "identity_fixed",
        "command": "cloud.register_pair@1",
        "algorithm": "scan.reg.pair_p2plane@1.0.0",
        "verdict": "stopped(max_iterations)",
        "status": "not_converged",
        "global_rms": 2.25e-05,
        "poses": "relative_to_first",
        "d": 0.001,
        "min_overlap": 0.1,
        "max_samples": 1000000,
        # the moving-to-earlier pairs only, still covering every scan
        "pairs": [(1, 0), (2, 0), (2, 1)],
    },
]


def residuals(a, b, shared):
    rms = 1e-05 * (a + 1) + 2e-06 * b
    return {"rms": rms, "max": 4.0 * rms, "n": shared, "no_hit": 3 * a + 1, "no_normal": b, "rejected": 2}


IDENTITY = ([[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]], [0.0, 0.0, 0.0])


def components(n, edges):
    parent = list(range(n))

    def find(x):
        while parent[x] != x:
            x = parent[x]
        return x

    for a, b in edges:
        ra, rb = find(a), find(b)
        if ra != rb:
            parent[max(ra, rb)] = min(ra, rb)
    groups = {}
    for i in range(n):
        groups.setdefault(find(i), []).append(i)
    return sorted(groups.values())


def build():
    scans = read_scans()
    out = [HEADER]
    out.append("# namespace = uuid5(NAMESPACE_URL, %s)\n" % NAMESPACE_URL_NAME)
    out.append("namespace %s\n" % NAMESPACE)
    out.append("input_frame %s\n" % INPUT_FRAME.hex())
    out.append("reference_frame %s\n" % REFERENCE_FRAME.hex())
    for case in CASES:
        n = len(scans)
        d, min_ov, max_s = case["d"], case["min_overlap"], case["max_samples"]
        out.append(
            "case %s command %s algorithm %s verdict %s global_rms %r d %r min_overlap %r max_samples %d\n"
            % (case["name"], case["command"], case["algorithm"], case["verdict"], case["global_rms"], d, min_ov, max_s)
        )
        poses = [(s["rot"], s["t"]) for s in scans]
        if case["poses"] == "relative_to_first":
            poses = relative_to_first(poses)
        for i, (rot, t) in enumerate(poses):
            out.append("pose %d %s\n" % (i, " ".join(repr(v) for row in rot for v in row) + " " + " ".join(repr(v) for v in t)))
        world = [[apply(rot, t, p) for p in s["points"]] for s, (rot, t) in zip(scans, poses)]
        cell = {}
        for a in range(n):
            for b in range(n):
                if a == b:
                    continue
                if disjoint(world[a], world[b], d):
                    cell[(a, b)] = ("disjoint", 0, len(world[a]), 0.0)
                else:
                    w, sm = overlap(world[a], world[b], d)
                    cell[(a, b)] = ("measured", w, sm, w / sm)
        edges = [(a, b) for a in range(n) for b in range(a + 1, n) if min(cell[(a, b)][3], cell[(b, a)][3]) >= min_ov]
        comps = components(n, edges)
        assert len(comps) == 1
        graph_pairs = []
        for a in range(n):
            for b in range(a + 1, n):
                if (a, b) in edges:
                    st = "edge"
                elif cell[(a, b)][0] == "disjoint" and cell[(b, a)][0] == "disjoint":
                    st = "disjoint"
                else:
                    st = "below_min"
                graph_pairs.append({
                    "a": str(a), "b": str(b),
                    "overlap_ab": quantity("1", cell[(a, b)][3]),
                    "overlap_ba": quantity("1", cell[(b, a)][3]),
                    "status": st,
                })
                out.append("graph_pair %d %d %s\n" % (a, b, st))
        pair_rows = []
        for a, b in case["pairs"]:
            kind, w, sm, frac = cell[(a, b)]
            assert kind == "measured"
            assert (min(a, b), max(a, b)) in edges
            r = residuals(a, b, w)
            out.append(
                "pair %d %d within %d sampled %d rms %r max %r n %d no_hit %d no_normal %d rejected %d\n"
                % (a, b, w, sm, r["rms"], r["max"], r["n"], r["no_hit"], r["no_normal"], r["rejected"])
            )
            pair_rows.append(((a, b), {
                "a": str(a), "b": str(b),
                "max_abs": quantity("m", r["max"]),
                "n": str(r["n"]),
                "no_residual": {"no_hit": str(r["no_hit"]), "no_normal": str(r["no_normal"]), "rejected": str(r["rejected"])},
                "overlap": quantity("1", frac),
                "overlap_distance_m": quantity("m", d),
                "rms": quantity("m", r["rms"]),
                "sampled": str(sm),
                "within": str(w),
            }))
        scan_json = []
        digests = []
        for i, (s, (rot, t)) in enumerate(zip(scans, poses)):
            din = cloud_digest(INPUT_FRAME, s["points"], s["normals"])
            pts, nrm = transform(rot, t, s["points"], s["normals"])
            dout = cloud_digest(REFERENCE_FRAME, pts, nrm)
            trans = math.sqrt(dot3(t, t))
            c = (((rot[0][0] + rot[1][1]) + rot[2][2]) - 1.0) / 2.0
            c = -1.0 if c < -1.0 else (1.0 if c > 1.0 else c)
            rotation = acos_cr(c)
            assert 0.0 <= rotation <= 3.5
            if (rot, t) == IDENTITY:
                # AC-3: identical coordinates (bits, -0.0 as +0.0)
                assert all(bits(a) == bits(b) for p, q in zip(pts, s["points"]) for a, b in zip(p, q))
                assert rotation == 0.0 and trans == 0.0
            out.append(
                "scan %d input %s output %s translation %s rotation %s\n"
                % (i, din.hex(), dout.hex(), hex64(trans), hex64(rotation))
            )
            digests.append(din)
            matrix = [rot[0] + [t[0]], rot[1] + [t[1]], rot[2] + [t[2]], [0.0, 0.0, 0.0, 1.0]]
            scan_json.append(
                {
                    "rotation": {"unit": "rad", "value": "@ROT%d@" % i},
                    "scan": str(i),
                    "transform": [hex64(v) for row in matrix for v in row],
                    "translation": quantity("m", trans),
                    "version_digest": "sha256:" + din.hex(),
                }
            )
        assert len(set(digests)) == len(digests)
        name = case["command"].encode("ascii") + b"\x00" + b"".join(sorted(digests))
        group = uuid.uuid5(NAMESPACE, name)
        out.append("group %s\n" % group)
        report = {
            "accuracy_basis": "unverified",
            "algorithm_id": case["algorithm"],
            "command": case["command"],
            "global_rms": quantity("m", case["global_rms"]),
            "graph": {
                "components": [[str(k) for k in c] for c in comps],
                "max_samples": str(max_s),
                "min_overlap": quantity("1", min_ov),
                "overlap_distance_m": quantity("m", d),
                "pairs": graph_pairs,
            },
            "pairs": [row for _, row in sorted(pair_rows)],
            "reference_frame": REFERENCE_FRAME.hex(),
            "residual_kind": "registration_fit",
            "scans": scan_json,
            "schema": SCHEMA,
            "status": case["status"],
            "stop_reason": case["verdict"],
        }
        text = json.dumps(report, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
        assert " " not in text
        out.append("report %s\n" % text)
        out.append("end\n")
    return "".join(out)


def main():
    text = build()
    target = os.path.join(HERE, OUT)
    if "--check" in sys.argv[1:]:
        with open(target, encoding="utf-8", newline="") as f:
            if f.read() != text:
                print("%s DIFFERS from make_rrep_fixtures.py output" % OUT)
                return 1
        print("%s OK" % OUT)
        return 0
    with open(target, "w", encoding="utf-8", newline="\n") as f:
        f.write(text)
    print("wrote %s" % OUT)
    return 0


if __name__ == "__main__":
    sys.exit(main())
