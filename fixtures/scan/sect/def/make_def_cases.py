#!/usr/bin/env python3
"""Independent reference for SCAN.SECT.DEF (section plane definitions and series). CC0-1.0 data.

Stdlib only. Writes, from the rules of spec node SCAN.SECT.DEF and the pinned rules in the
`fid-scan-sect` `def` module documentation (nothing here was produced by running the Rust
crate):

* `fixtures/scan/sect/def/cases.txt`: definitions with their expected canonical digest
  (AC-5), expected planes (AC-1 exact, AC-2 and other radial series within 1e-15) or
  expected refusal reason (AC-4 and REQ-004);
* `fixtures/scan/cpt_box_offset/cad_box.txt`: the CAD box of the shared SCAN fixture
  `cpt_box_offset` (synthetic, invented): an axis-aligned 100 x 60 x 40 mm box whose corner
  is offset to (10, 20, 5) mm, as a B-rep builder script (8 vertices, 12 line edges, 24
  coedges, 6 loops counter-clockwise about the outward normal, 6 plane faces, one closed
  shell). Two faces (x0 and y1) carry their placement axis pointing into the material with
  sense `reversed`, so their out-of-material normal is the negated axis;
* `fixtures/scan/sect/def/box_faces.txt`: for each box face, the plane a `FromElement`
  definition must resolve to (AC-3): the placement origin and the outward unit normal, stated
  from the box geometry (the negation of an axis is written as `0.0 - x`, so zeros stay
  `+0.0`).

`--check` regenerates everything in memory and fails on any difference with the committed
files.

Rules reproduced (binary64; Python floats are IEEE binary64 and never fused):

* `-0.0` is stored as `+0.0` (`v + 0.0`) for every input coordinate and scalar;
* finite inputs; unit normals and directions: `abs(sqrt(x*x + y*y + z*z) - 1) <= 1e-12`;
  radial `abs(d . r) <= 1e-12`; spacing and step `> 0`; count in 1..10000;
* parallel plane i: point `p0[c] + t * n[c]` with `t = float(i) * s`, normal `n` as given;
  with an extent e >= 0 the count is the first k with `float(k) * s > e + e * 1e-12`
  (closed range), refused above 10000;
* radial plane i: `phi = float(i) * step`, `rot[c] = r[c]*cos + (d x r)[c]*sin +
  d[c]*(d . r)*(1 - cos)`, normal `(d x rot) / |d x rot|`, point = axis point; with a sweep
  w > 0 the count is the first k >= 1 with `float(k) * step >= w - 1e-12` (half-open
  range); refused when `float(k - 1) * step >= 2 pi - 1e-12` (full turn);
* canonical JSON (format `fid.scan.sect.def/1`): RFC 8785 with every value a string, keys
  sorted, f64 as the 16 lowercase hex digits of its bits, counts in decimal, IDs as
  hyphenated lowercase UUIDs, an imported face label as its text; digest = sha256.

The radial cosines and sines come from this platform's `math`, which may differ from the
`fid-num` vendored libm by an ulp, so radial planes are compared within 1e-15 (AC-2's
tolerance); parallel planes and every digest are exact.

File format of `cases.txt` ('#' lines are comments), one block per case:

    case <name>
    def single <px py pz> <nx ny nz>
    def parallel <px py pz> <nx ny nz> <spacing> count <k> | extent <e>
    def radial <ax ay az> <dx dy dz> <rx ry rz> <step> count <k> | sweep <w>
    def cad_face <body 32 hex> imported <label UTF-8 as hex> <sha256:digest>
    def cad_face <body 32 hex> derived <face 32 hex> <sha256:digest>
    def constructed <element 32 hex> <sha256:digest>
    digest <sha256:...>              (valid definitions)
    count <n> exact|within_1e-15     (number of planes and how to compare them)
    plane <i> <px py pz> <nx ny nz>  (selected planes; all of them when n <= 40)
    error <reason code>              (refused definitions)
    end

Floats are Python `repr` (round-trip exact), or `nan` / `inf` / `-inf`.

Usage: python make_def_cases.py [--check]
"""

import hashlib
import json
import math
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.normpath(os.path.join(HERE, "..", "..", "..", ".."))
CASES = os.path.join(HERE, "cases.txt")
BOX = os.path.join(ROOT, "fixtures", "scan", "cpt_box_offset", "cad_box.txt")
FACES = os.path.join(HERE, "box_faces.txt")

FORMAT = "fid.scan.sect.def/1"
MAX_PLANES = 10000
TOL = 1e-12
TAU = 2.0 * math.pi


class Refused(Exception):
    def __init__(self, reason):
        super().__init__(reason)
        self.reason = reason


# ------------------------------------------------------------------ rules


def canon(v):
    return v + 0.0


def finite(v):
    if not math.isfinite(v):
        raise Refused("non_finite")
    return canon(v)


def finite3(v):
    return [finite(x) for x in v]


def unit3(v):
    v = finite3(v)
    if abs(math.sqrt(v[0] * v[0] + v[1] * v[1] + v[2] * v[2]) - 1.0) > TOL:
        raise Refused("not_unit")
    return v


def count(k):
    if k == 0:
        raise Refused("count_zero")
    if k > MAX_PLANES:
        raise Refused("count_too_large")
    return k


def cross(a, b):
    return [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]


def plane(p, n):
    return (finite3(p), unit3(n))


def parallel(p, n, s, mode, value):
    base = plane(p, n)
    s = finite(s)
    if s <= 0.0:
        raise Refused("spacing_not_positive")
    if mode == "count":
        k = count(value)
        span = ("count", value)
    else:
        e = finite(value)
        if e < 0.0:
            raise Refused("extent_negative")
        limit = e + e * 1e-12
        k = None
        for j in range(MAX_PLANES + 1):
            if float(j) * s > limit:
                k = j
                break
        if k is None:
            raise Refused("count_too_large")
        span = ("extent", e)
    (p0, n0) = base
    planes = []
    for i in range(k):
        t = float(i) * s
        q = [p0[0] + t * n0[0], p0[1] + t * n0[1], p0[2] + t * n0[2]]
        if not all(math.isfinite(c) for c in q):
            raise Refused("plane_not_finite")
        planes.append(([canon(c) for c in q], n0))
    doc = {
        "base": {"normal": hx3(n0), "point": hx3(p0)},
        "format": FORMAT,
        "spacing": hx(s),
        "span": {span[0]: str(span[1]) if span[0] == "count" else hx(span[1])},
        "type": "parallel",
    }
    return doc, planes, "exact"


def radial(a, d, r, step, mode, value):
    a = finite3(a)
    d = unit3(d)
    r = unit3(r)
    if abs(d[0] * r[0] + d[1] * r[1] + d[2] * r[2]) > TOL:
        raise Refused("not_perpendicular")
    step = finite(step)
    if step <= 0.0:
        raise Refused("step_not_positive")
    if mode == "count":
        k = count(value)
        span = {"count": str(value)}
    else:
        w = finite(value)
        if w <= 0.0:
            raise Refused("sweep_not_positive")
        limit = w - 1e-12
        k = None
        for j in range(1, MAX_PLANES + 1):
            if float(j) * step >= limit:
                k = j
                break
        if k is None:
            raise Refused("count_too_large")
        span = {"sweep": hx(w)}
    if float(k - 1) * step >= TAU - 1e-12:
        raise Refused("radial_full_turn")
    dxr = cross(d, r)
    ddr = d[0] * r[0] + d[1] * r[1] + d[2] * r[2]
    planes = []
    for i in range(k):
        phi = float(i) * step
        c, s = math.cos(phi), math.sin(phi)
        rot = [r[j] * c + dxr[j] * s + d[j] * ddr * (1.0 - c) for j in range(3)]
        m = cross(d, rot)
        ln = math.sqrt(m[0] * m[0] + m[1] * m[1] + m[2] * m[2])
        planes.append((a, [canon(x / ln) for x in m]))
    doc = {
        "axis_dir": hx3(d),
        "axis_point": hx3(a),
        "format": FORMAT,
        "ref_dir": hx3(r),
        "span": span,
        "step": hx(step),
        "type": "radial",
    }
    return doc, planes, "within_1e-15"


def single(p, n):
    (p0, n0) = plane(p, n)
    doc = {"format": FORMAT, "plane": {"normal": hx3(n0), "point": hx3(p0)}, "type": "single"}
    return doc, [(p0, n0)], "exact"


def uuid_text(h):
    return "%s-%s-%s-%s-%s" % (h[0:8], h[8:12], h[12:16], h[16:20], h[20:32])


def cad_face(body, kind, face, dig):
    if kind == "imported":
        f = {"imported": bytes.fromhex(face).decode("utf-8")}
    else:
        f = {"derived": uuid_text(face)}
    doc = {
        "digest": dig,
        "element": {"body": uuid_text(body), "face": f, "type": "cad_face"},
        "format": FORMAT,
        "type": "from_element",
    }
    return doc, None, None


def constructed(eid, dig):
    doc = {"digest": dig, "element": {"id": uuid_text(eid), "type": "constructed"}, "format": FORMAT,
           "type": "from_element"}
    return doc, None, None


def hx(v):
    return "%016x" % struct.unpack(">Q", struct.pack(">d", v))[0]


def hx3(v):
    return [hx(x) for x in v]


def digest(doc):
    text = json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return "sha256:" + hashlib.sha256(text.encode("utf-8")).hexdigest()


# ------------------------------------------------------------------ cases

HALF_PI = math.pi / 2.0
Z = [0.0, 0.0, 1.0]
O = [0.0, 0.0, 0.0]
D1 = "sha256:" + "11" * 32
D2 = "sha256:" + hashlib.sha256(b"SCAN.SECT.DEF synthetic element digest").hexdigest()
TRICKY = "q\"b\\s/\x01\x08\t\n\x0c\r\x1f\x7fé \U0001f600 face"


def cases():
    """(name, def tokens, maker, args, note). Each refused case has exactly one fault."""
    return [
        ("ac1_parallel_count", "parallel", parallel, (O, Z, 0.01, "count", 5), "AC-1"),
        ("parallel_extent_tilted", "parallel", parallel, ([0.01, 0.02, 0.005], [0.6, 0.0, 0.8], 0.003, "extent", 0.03),
         "extent with a closed end"),
        ("parallel_extent_zero", "parallel", parallel, ([1.0, 2.0, 3.0], [0.0, 1.0, 0.0], 0.5, "extent", 0.0),
         "extent 0: the base plane only"),
        ("parallel_extent_inexact_end", "parallel", parallel, (O, [1.0, 0.0, 0.0], 0.1, "extent", 0.3),
         "3 * 0.1 > 0.3 in binary64, kept by the slack"),
        ("parallel_near_unit_normal", "parallel", parallel, (O, [0.0, 0.0, 1.0 + 5e-13], 0.25, "count", 3),
         "normal 5e-13 off unit: accepted, kept as given"),
        ("parallel_count_max", "parallel", parallel, ([-0.5, 0.0, 0.0], [1.0, 0.0, 0.0], 1e-4, "count", 10000),
         "the largest count"),
        ("parallel_extent_max", "parallel", parallel, (O, Z, 0.001, "extent", 9.999), "derives exactly 10000"),
        ("single", "single", single, ([0.0, 0.0, 0.5], Z), "one plane"),
        ("single_negative_zero", "single", single, ([-0.0, -0.0, 0.5], [-0.0, 0.0, 1.0]),
         "-0.0 stored as +0.0: same digest as `single`"),
        ("ac2_radial_quarter_turns", "radial", radial, (O, Z, [1.0, 0.0, 0.0], HALF_PI, "count", 4), "AC-2"),
        ("radial_sweep_full_circle", "radial", radial, ([0.1, 0.2, 0.0], Z, [0.0, 1.0, 0.0], math.pi / 6.0, "sweep",
                                                        TAU), "sweep 2 pi, half-open: 12 planes"),
        ("radial_sweep_half", "radial", radial, (O, Z, [1.0, 0.0, 0.0], math.pi / 3.0, "sweep", math.pi),
         "pi / 3 steps over [0, pi): 3 planes"),
        ("radial_tilted_axis", "radial", radial, ([0.01, -0.02, 0.03], [0.0, 0.6, 0.8], [1.0, 0.0, 0.0], 0.25, "count",
                                                  7), "tilted axis"),
        ("from_cad_face_imported", "cad_face", cad_face,
         ("00112233445566778899aabbccddeeff", "imported", TRICKY.encode("utf-8").hex(), D1),
         "label needing every RFC 8785 escape"),
        ("from_cad_face_derived", "cad_face", cad_face,
         ("0f0e0d0c0b0a09080706050403020100", "derived", "a1b2c3d4e5f60718293a4b5c6d7e8f90", D2), "derived face ID"),
        ("from_constructed", "constructed", constructed, ("5e0c3a1f2b4d4e6f8a9b0c1d2e3f4051", D2), "constructed plane"),
        # ---- refusals (AC-4 and REQ-004)
        ("ac4_spacing_zero", "parallel", parallel, (O, Z, 0.0, "count", 5), "AC-4"),
        ("ac4_count_20000", "parallel", parallel, (O, Z, 0.01, "count", 20000), "AC-4"),
        ("spacing_negative", "parallel", parallel, (O, Z, -0.01, "count", 5), ""),
        ("count_zero", "parallel", parallel, (O, Z, 0.01, "count", 0), ""),
        ("count_10001", "parallel", parallel, (O, Z, 0.01, "count", 10001), ""),
        ("extent_too_many", "parallel", parallel, (O, Z, 1e-4, "extent", 1.0), "derives 10001"),
        ("extent_negative", "parallel", parallel, (O, Z, 0.01, "extent", -0.01), ""),
        ("extent_nan", "parallel", parallel, (O, Z, 0.01, "extent", float("nan")), ""),
        ("normal_not_unit", "parallel", parallel, (O, [0.0, 0.0, 2.0], 0.01, "count", 2), ""),
        ("normal_just_off_unit", "single", single, (O, [0.0, 0.0, 1.0 + 2e-12]), "2e-12 off unit"),
        ("normal_zero", "single", single, (O, O), ""),
        ("point_infinite", "single", single, ([float("inf"), 0.0, 0.0], Z), ""),
        ("plane_overflow", "parallel", parallel, ([1e308, 0.0, 0.0], [1.0, 0.0, 0.0], 1e308, "count", 3),
         "the second plane's x overflows"),
        ("radial_step_zero", "radial", radial, (O, Z, [1.0, 0.0, 0.0], 0.0, "count", 4), ""),
        ("radial_not_perpendicular", "radial", radial, (O, Z, [1.0, 0.0, 1e-9], 0.5, "count", 4), "d . r = 1e-9"),
        ("radial_axis_not_unit", "radial", radial, (O, [0.0, 0.0, 0.5], [1.0, 0.0, 0.0], 0.5, "count", 4), ""),
        ("radial_full_turn", "radial", radial, (O, Z, [1.0, 0.0, 0.0], HALF_PI, "count", 5), "5th plane repeats"),
        ("radial_sweep_past_turn", "radial", radial, (O, Z, [1.0, 0.0, 0.0], HALF_PI, "sweep", 7.0), ""),
        ("radial_sweep_zero", "radial", radial, (O, Z, [1.0, 0.0, 0.0], HALF_PI, "sweep", 0.0), ""),
        ("radial_count_zero", "radial", radial, (O, Z, [1.0, 0.0, 0.0], HALF_PI, "count", 0), ""),
    ]


def ftok(v):
    if math.isnan(v):
        return "nan"
    if math.isinf(v):
        return "inf" if v > 0 else "-inf"
    return repr(float(v))


def v3tok(v):
    return " ".join(ftok(x) for x in v)


def def_line(kind, args):
    if kind == "parallel":
        p, n, s, mode, value = args
        last = str(value) if mode == "count" else ftok(value)
        return "def parallel %s %s %s %s %s" % (v3tok(p), v3tok(n), ftok(s), mode, last)
    if kind == "radial":
        a, d, r, step, mode, value = args
        last = str(value) if mode == "count" else ftok(value)
        return "def radial %s %s %s %s %s %s" % (v3tok(a), v3tok(d), v3tok(r), ftok(step), mode, last)
    if kind == "single":
        p, n = args
        return "def single %s %s" % (v3tok(p), v3tok(n))
    if kind == "cad_face":
        return "def cad_face %s %s %s %s" % args
    if kind == "constructed":
        return "def constructed %s %s" % args
    raise SystemExit("unknown kind " + kind)


def hand_checks(results):
    """Acceptance values stated by hand from the spec, checked against the computed ones."""
    _, planes, _ = results["ac1_parallel_count"]
    zs = [p[0][2] for p in planes]
    assert zs == [0.0, 0.01, 0.02, 0.03, 0.04] and all(p[1] == Z for p in planes), zs
    _, planes, _ = results["ac2_radial_quarter_turns"]
    want = [(0.0, 1.0, 0.0), (-1.0, 0.0, 0.0), (0.0, -1.0, 0.0), (1.0, 0.0, 0.0)]
    for (_, n), w in zip(planes, want):
        assert all(abs(a - b) <= 1e-15 for a, b in zip(n, w)), (n, w)
    assert len(results["parallel_extent_inexact_end"][1]) == 4
    assert len(results["parallel_extent_zero"][1]) == 1
    assert len(results["parallel_extent_max"][1]) == MAX_PLANES
    assert len(results["radial_sweep_full_circle"][1]) == 12
    assert len(results["radial_sweep_half"][1]) == 3
    assert digest(results["single"][0]) == digest(results["single_negative_zero"][0])
    for name, reason in [("ac4_spacing_zero", "spacing_not_positive"), ("ac4_count_20000", "count_too_large")]:
        assert results[name] == reason, (name, results[name])


def render_cases():
    out = ["# SCAN.SECT.DEF cases; generated by make_def_cases.py (independent stdlib Python). CC0-1.0.",
           "# Synthetic definitions; see the script for the rules and the file format."]
    results = {}
    for name, kind, maker, args, note in cases():
        out.append("case " + name)
        if note:
            out.append("# " + note)
        out.append(def_line(kind, args))
        try:
            doc, planes, how = maker(*args)
        except Refused as e:
            results[name] = e.reason
            out.append("error " + e.reason)
            out.append("end")
            continue
        results[name] = (doc, planes, how)
        out.append("digest " + digest(doc))
        if planes is not None:
            out.append("count %d %s" % (len(planes), how))
            n = len(planes)
            picks = range(n) if n <= 40 else sorted(set([0, 1, 2, n // 2, n - 2, n - 1]))
            for i in picks:
                p, q = planes[i]
                out.append("plane %d %s %s" % (i, v3tok(p), v3tok(q)))
        out.append("end")
    hand_checks(results)
    return "\n".join(out) + "\n"


# ------------------------------------------------------------------ the CAD box (AC-3)

C0 = [0.01, 0.02, 0.005]
SIZE = [0.1, 0.06, 0.04]
BODY_ID = "5ca7b0c5000000000000000000c0ffee"


def unit(k):
    return [1.0 if j == k else 0.0 for j in range(3)]


def neg(v):
    return [0.0 - x for x in v]


def box():
    """Builder script lines and the expected plane per face."""
    corner = [[C0[0] + SIZE[0] * (i & 1), C0[1] + SIZE[1] * ((i >> 1) & 1), C0[2] + SIZE[2] * ((i >> 2) & 1)]
              for i in range(8)]
    lines = ["# CAD box of fixture cpt_box_offset (SCAN.SECT.DEF AC-3; shared with SCAN.CMP.CPT).",
             "# Generated by fixtures/scan/sect/def/make_def_cases.py. Synthetic, CC0-1.0. Metres.",
             "body " + BODY_ID]
    for i, p in enumerate(corner):
        lines.append("vertex v%d%d%d %s" % (i & 1, (i >> 1) & 1, (i >> 2) & 1, v3tok(p)))
    edge_of = {}
    n_edge = 0
    for d in range(3):
        for i in range(8):
            if not (i >> d) & 1:
                j = i | (1 << d)
                length = corner[j][d] - corner[i][d]
                axis = unit((d + 1) % 3)
                lines.append("edge e%d-%d %d %d 0.0 %s line %s %s %s" % (i, j, i, j, ftok(length), v3tok(corner[i]),
                                                                          v3tok(axis), v3tok(unit(d))))
                edge_of[(i, j)] = (n_edge, "same")
                edge_of[(j, i)] = (n_edge, "reversed")
                n_edge += 1
    n_coedge = 0
    faces = []
    loops = []
    names = {(0, 0): "x0", (0, 1): "x1", (1, 0): "y0", (1, 1): "y1", (2, 0): "z0", (2, 1): "z1"}
    for a in range(3):
        b, c = (a + 1) % 3, (a + 2) % 3
        for side in (0, 1):
            order = [(0, 0), (1, 0), (1, 1), (0, 1)] if side == 1 else [(0, 0), (0, 1), (1, 1), (1, 0)]
            ring = [(side << a) | (x << b) | (y << c) for x, y in order]
            cs = []
            for k in range(4):
                e, sense = edge_of[(ring[k], ring[(k + 1) % 4])]
                lines.append("coedge %d %s" % (e, sense))
                cs.append(n_coedge)
                n_coedge += 1
            name = names[(a, side)]
            loops.append("loop l.%s outer %s" % (name, " ".join(str(x) for x in cs)))
            outward = unit(a) if side == 1 else neg(unit(a))
            reversed_face = name in ("x0", "y1")
            axis = neg(outward) if reversed_face else outward
            sense = "reversed" if reversed_face else "same"
            origin = corner[ring[0]]
            faces.append((name, origin, axis, unit(b), sense, len(loops) - 1, outward))
    lines.extend(loops)
    expected = ["# SCAN.SECT.DEF AC-3: plane of each cpt_box_offset CAD face (origin, outward unit normal).",
                "# Generated by make_def_cases.py from the box geometry. CC0-1.0."]
    for name, origin, axis, ref, sense, li, outward in faces:
        lines.append("face f.%s plane %s %s %s %s %d" % (name, v3tok(origin), v3tok(axis), v3tok(ref), sense, li))
        # The out-of-material normal: the axis for `same`, `0.0 - axis` for `reversed`.
        nrm = axis if sense == "same" else neg(axis)
        assert nrm == outward, (name, nrm, outward)
        expected.append("face f.%s %s %s" % (name, v3tok(origin), v3tok(outward)))
    lines.append("shell closed %s" % " ".join(str(i) for i in range(6)))
    # Every edge is used by exactly two coedges with opposite senses (closed shell).
    return "\n".join(lines) + "\n", "\n".join(expected) + "\n"


def main():
    check = "--check" in sys.argv[1:]
    box_text, faces_text = box()
    outputs = [(CASES, render_cases()), (BOX, box_text), (FACES, faces_text)]
    bad = 0
    for target, text in outputs:
        if check:
            try:
                with open(target, encoding="utf-8", newline="") as f:
                    old = f.read()
            except OSError:
                old = None
            if old != text:
                print("DIFFERS: " + target)
                bad += 1
        else:
            os.makedirs(os.path.dirname(target), exist_ok=True)
            with open(target, "w", encoding="utf-8", newline="\n") as f:
                f.write(text)
            print("wrote " + target)
    if bad:
        sys.exit(1)
    if check:
        print("SCAN.SECT.DEF fixtures up to date")


if __name__ == "__main__":
    main()
