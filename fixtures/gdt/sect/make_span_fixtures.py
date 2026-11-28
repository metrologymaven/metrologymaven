#!/usr/bin/env python3
"""Independent reference for GDT.DIM.SECT.SPAN (2D elements from section-curve spans), stdlib only.

Writes three files next to this script:

- `rect_with_fillet.json`: a closed section curve (format `fid.scan.sect.curve/1`) of a
  20 x 10 mm rectangle whose four corners are 2 mm fillets, vertex spacing 0.05 mm on the
  straight edges and 63 equal-angle steps per quarter-circle fillet (chord about 0.0499 mm).
  The curve starts in the middle of the bottom edge (x = 10 mm, y = 0) and runs
  counter-clockwise, so a span over the bottom edge wraps past vertex 0.
- `open_arc.json`: an open section curve, 41 vertices on a 120 degree arc of radius 5 mm
  about the plane origin.
- `cases.json`: spans over these curves with the expected selection (first vertex index and
  vertex count) and the planted geometry the fitted element must recover within 1e-12 m
  (AC-1), or the expected error (AC-2, AC-3 and the other refusals).

This script does not run or mirror the Rust code. Expected values come from the planted
geometry and the leaf's rules:

- Coordinates are exact rationals (`Fraction`) rounded once to binary64 (`float(Fraction)` is
  correctly rounded). Fillet points use the rational parametrisation
  ((1 - t^2), 2 t) / (1 + t^2) of the unit circle, placed per quadrant by exact swaps and sign
  changes, so every rounded fillet vertex lies within 1e-18 m of the planted circle. The
  parameter t of step j is tan(j * (pi / 2) / 63 / 2) computed in `decimal` at 60 digits
  (Taylor series, no libm) and rounded to a multiple of 1e-15, which keeps it exact and the
  same on every platform; any t gives a point exactly on the circle.
- The curve digest is SHA-256 of the RFC 8785 canonical JSON of every field but `digest`
  (`json.dumps(sort_keys=True, separators=(",", ":"))`, ASCII strings only, so it is RFC 8785).
- Arc-length parameters follow the leaf's pinned rule (vertex 0 at 0; each segment
  sqrt(du * du + dv * dv) in binary64, summed in vertex order; Python floats are IEEE-754
  binary64 with correctly rounded `math.sqrt`, so the parameters are bit-exact on every
  platform). Span bounds are vertex parameters, so the inclusive selection is exact; each
  expected count is also asserted against the count known from the construction.
- Errors are hand-labelled from REQ-GDT.DIM.SECT.SPAN-003 and -004 and the procedure in the
  leaf's Design (bounds checked before order; collinear points refused by the circle fit).

With `--check` the files are rebuilt in memory and compared byte for byte with the committed
ones. All geometry is synthetic, in metres; no standards text; CC0.
"""

import hashlib
import json
import math
import struct
import sys
from decimal import Decimal, getcontext
from fractions import Fraction as F
from pathlib import Path

getcontext().prec = 60
HERE = Path(__file__).resolve().parent
FORMAT = "fid.scan.sect.curve/1"
PI = Decimal("3.14159265358979323846264338327950288419716939937510582097494459")
MM = F(1, 1000)


def hexf(x):
    """16 lowercase hex digits of the binary64 bits; refuses negative zero."""
    assert not (x == 0.0 and math.copysign(1.0, x) < 0)
    return struct.pack(">d", x).hex()


def dec_sin_cos(x):
    """sin and cos of a Decimal by Taylor series (|x| <= pi/3), 60 digits."""
    s, c = Decimal(0), Decimal(0)
    term = Decimal(1)
    for k in range(60):
        if k % 2 == 0:
            c += term if k % 4 == 0 else -term
        else:
            s += term if k % 4 == 1 else -term
        term = term * x / Decimal(k + 1)
    return s, c


def tan_half_rational(phi_num, phi_den):
    """tan(theta / 2) for theta = pi * phi_num / phi_den, as an exact multiple of 1e-15."""
    half = PI * Decimal(phi_num) / Decimal(phi_den) / Decimal(2)
    s, c = dec_sin_cos(half)
    t = s / c
    return F(int((t * Decimal(10) ** 15).to_integral_value()), 10**15)


def unit_circle(t):
    """(cos theta, sin theta) of t = tan(theta / 2), exact."""
    d = 1 + t * t
    return (1 - t * t) / d, 2 * t / d


def canonical(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def curve_doc(section_id, frame, plane, verts, closed):
    content = {
        "closed": closed,
        "format": FORMAT,
        "frame": frame,
        "plane": {k: [hexf(x) for x in v] for k, v in plane.items()},
        "section_id": section_id,
        "vertices": [[hexf(u), hexf(v)] for (u, v) in verts],
    }
    digest = "sha256:" + hashlib.sha256(canonical(content)).hexdigest()
    doc = dict(content)
    doc["digest"] = digest
    return doc, digest


def arc_lengths(verts, closed):
    """The leaf's pinned arc-length rule, in binary64."""
    s = [0.0]
    for (a, b) in zip(verts, verts[1:]):
        du, dv = b[0] - a[0], b[1] - a[1]
        seg = math.sqrt(du * du + dv * dv)
        assert seg > 0.0 and s[-1] + seg > s[-1]
        s.append(s[-1] + seg)
    if closed:
        a, b = verts[-1], verts[0]
        du, dv = b[0] - a[0], b[1] - a[1]
        length = s[-1] + math.sqrt(du * du + dv * dv)
    else:
        length = s[-1]
    return s, length


# ------------------------------------------------------------------ rectangle with fillets

R = 2 * MM
STEP = F(5, 100) * MM
ARC_STEPS = 63


def rect_vertices():
    """Exact vertices and the index where each piece starts."""
    ts = [tan_half_rational(j, 2 * ARC_STEPS) for j in range(ARC_STEPS)]
    arcs = [unit_circle(t) for t in ts]  # (cos, sin) for theta in [0, pi/2)
    pieces = []

    def piece(name, pts):
        pieces.append((name, pts))

    piece("bottom1", [(10 * MM + k * STEP, F(0)) for k in range(160)])
    piece("arc_br", [(18 * MM + R * s, 2 * MM - R * c) for (c, s) in arcs])
    piece("right", [(20 * MM, 2 * MM + k * STEP) for k in range(120)])
    piece("arc_tr", [(18 * MM + R * c, 8 * MM + R * s) for (c, s) in arcs])
    piece("top", [(18 * MM - k * STEP, 10 * MM) for k in range(320)])
    piece("arc_tl", [(2 * MM - R * s, 8 * MM + R * c) for (c, s) in arcs])
    piece("left", [(F(0), 8 * MM - k * STEP) for k in range(120)])
    piece("arc_bl", [(2 * MM - R * c, 2 * MM - R * s) for (c, s) in arcs])
    piece("bottom2", [(2 * MM + k * STEP, F(0)) for k in range(160)])
    exact, start = [], {}
    for name, pts in pieces:
        start[name] = len(exact)
        exact.extend(pts)
    return exact, start


def max_circle_error(exact_pts, cx, cy, r):
    """Largest | |p - c| - r | of the rounded points, exactly (60-digit decimal sqrt)."""
    worst = Decimal(0)
    for (u, v) in exact_pts:
        du = F(float(u)) - cx
        dv = F(float(v)) - cy
        d2 = du * du + dv * dv
        dist = (Decimal(d2.numerator) / Decimal(d2.denominator)).sqrt()
        err = abs(dist - Decimal(r.numerator) / Decimal(r.denominator))
        worst = max(worst, err)
    return worst


def build():
    files = {}
    cases = []

    # Rectangle.
    exact, start = rect_vertices()
    verts = [(float(u), float(v)) for (u, v) in exact]
    n = len(verts)
    assert n == 1132
    plane = {"origin": (0.1, -0.2, 0.05), "x_axis": (0.6, 0.8, 0.0), "y_axis": (0.0, 0.0, 1.0)}
    rect_id = "sect:rect-fillet-1"
    rect_doc, rect_digest = curve_doc(rect_id, "5f2c0e8a9b1d4c3e8f7a6b5c4d3e2f10", plane, verts, True)
    files["rect_with_fillet.json"] = canonical(rect_doc) + b"\n"
    s, length = arc_lengths(verts, True)
    fil_err = max_circle_error(exact[start["arc_br"] : start["right"] + 1], 18 * MM, 2 * MM, R)
    assert fil_err < Decimal("1e-17"), fil_err

    def sel(lo, hi_incl, wrap=False):
        """Expected selection by the inclusive rule on the parameters, independently."""
        a, b = s[lo], s[hi_incl]
        if wrap:
            idx = [i for i in range(n) if s[i] >= a] + [i for i in range(n) if s[i] <= b]
        else:
            idx = [i for i in range(n) if a <= s[i] <= b]
        return idx

    def case(cid, curve, kind, alg, start_v, end_v, wrap=False, section=None, expect=None, chain=None):
        c = {
            "id": cid,
            "curve": curve,
            "kind": kind,
            "chain": chain if chain is not None else [["association", alg]],
            "span": {
                "section_id": section,
                "start": hexf(start_v),
                "end": hexf(end_v),
                "wrap": wrap,
            },
            "expect": expect,
        }
        cases.append(c)

    def ok(idx, built_count, truth):
        assert len(idx) == built_count, (len(idx), built_count)
        return {"first": idx[0], "count": len(idx), "truth": truth}

    def pt(x, y):
        return [hexf(float(x)), hexf(float(y))]

    L2, C2, P = "fit.line2.ls@1.0.0", "fit.circle2.ls@1.0.0", "fit.point.ls@1.0.0"
    rc = "rect_with_fillet.json"

    # AC-1: fillets (64 vertices: both tangent points and the 62 arc vertices between).
    i0, i1 = start["arc_br"], start["right"]
    case("fillet_br", rc, "circle2d", C2, s[i0], s[i1], section=rect_id,
         expect=ok(sel(i0, i1), 64, {"c": pt(18 * MM, 2 * MM), "r": hexf(float(R))}))
    i0, i1 = start["arc_tl"], start["left"]
    case("fillet_tl", rc, "circle2d", C2, s[i0], s[i1], section=rect_id,
         expect=ok(sel(i0, i1), 64, {"c": pt(2 * MM, 8 * MM), "r": hexf(float(R))}))
    # AC-1: straight edges (tangent point to tangent point).
    i0, i1 = start["right"], start["arc_tr"]
    case("edge_right", rc, "line2d", L2, s[i0], s[i1], section=rect_id,
         expect=ok(sel(i0, i1), 121, {"p": pt(20 * MM, 0), "d": pt(0, 1)}))
    i0, i1 = start["top"], start["arc_tl"]
    case("edge_top", rc, "line2d", L2, s[i0], s[i1], section=rect_id,
         expect=ok(sel(i0, i1), 321, {"p": pt(0, 10 * MM), "d": pt(1, 0)}))
    # A wrapping span over the bottom edge: vertices 972..1131 then 0..160.
    i0, i1 = start["bottom2"], start["arc_br"]
    case("edge_bottom_wrap", rc, "line2d", L2, s[i0], s[i1], wrap=True, section=rect_id,
         expect=ok(sel(i0, i1, wrap=True), 160 + 161, {"p": pt(0, 0), "d": pt(1, 0)}))
    # start = +0.0 selects vertex 0.
    case("edge_bottom_from_zero", rc, "line2d", L2, 0.0, s[100], section=rect_id,
         expect=ok(sel(0, 100), 101, {"p": pt(0, 0), "d": pt(1, 0)}))
    # Points: the centroid of the top edge (symmetric about x = 10 mm), and one vertex.
    i0, i1 = start["top"], start["arc_tl"]
    case("point_top_centroid", rc, "point2d", P, s[i0], s[i1], section=rect_id,
         expect=ok(sel(i0, i1), 321, {"c": pt(10 * MM, 10 * MM)}))
    mid = (s[500] + s[501]) / 2
    assert s[500] < mid < s[501]
    case("point_single_vertex", rc, "point2d", P, s[500], mid, section=rect_id,
         expect={"first": 500, "count": 1, "truth": {"c": [hexf(verts[500][0]), hexf(verts[500][1])]}})

    # AC-2: a circle over two vertices.
    case("circle_two_vertices", rc, "circle2d", C2, s[200], s[201], section=rect_id,
         expect={"error": "span_too_short{needed: 3, got: 2}"})
    # AC-3: end beyond the curve length; a negative start.
    case("end_beyond_length", rc, "line2d", L2, s[10], length + 1e-6, section=rect_id,
         expect={"error": "span_out_of_range{end}"})
    case("start_negative", rc, "line2d", L2, -1e-9, s[10], section=rect_id,
         expect={"error": "span_out_of_range{start}"})
    # Order and wrap rules.
    case("start_after_end", rc, "line2d", L2, s[20], s[10], section=rect_id,
         expect={"error": "bad_span{start_not_before_end}"})
    case("start_equals_end", rc, "line2d", L2, s[20], s[20], section=rect_id,
         expect={"error": "bad_span{start_not_before_end}"})
    case("wrap_forward", rc, "line2d", L2, s[10], s[20], wrap=True, section=rect_id,
         expect={"error": "bad_span{wrap_needs_start_after_end}"})
    case("other_section", rc, "line2d", L2, s[10], s[20], section="sect:other",
         expect={"error": "section_mismatch"})
    # The circle fit refuses collinear points (a straight edge); a line chain for a circle.
    i0, i1 = start["right"], start["arc_tr"]
    case("circle_on_edge", rc, "circle2d", C2, s[i0 + 1], s[i1 - 1], section=rect_id,
         expect={"error": "fit_refused{collinear}"})
    case("chain_line_for_circle", rc, "circle2d", L2, s[160], s[223], section=rect_id,
         expect={"error": "fit_kind_mismatch{kind: circle2d, alg: fit.line2.ls@1.0.0}"})
    case("chain_mz", rc, "circle2d", "fit.circle2.mz@1.0.0", s[160], s[223], section=rect_id,
         expect={"error": "fit_unavailable{fit.circle2.mz@1.0.0}"})
    case("chain_other_version", rc, "circle2d", "fit.circle2.ls@2.0.0", s[160], s[223], section=rect_id,
         expect={"error": "fit_unavailable{fit.circle2.ls@2.0.0}"})
    case("chain_none", rc, "circle2d", None, s[160], s[223], section=rect_id,
         chain=[["extraction", "syn.extract.all@0.1.0"]],
         expect={"error": "chain_without_association"})
    case("chain_two", rc, "circle2d", None, s[160], s[223], section=rect_id,
         chain=[["association", C2], ["association", C2]],
         expect={"error": "ambiguous_association{count: 2}"})

    # Open arc: r = 5 mm about the origin, theta from 0 to about 120 degrees in 40 steps.
    r5 = 5 * MM
    arc_exact = []
    for j in range(41):
        c, sn = unit_circle(tan_half_rational(2 * j, 3 * 40))
        arc_exact.append((r5 * c, r5 * sn))
    arc_verts = [(float(u), float(v)) for (u, v) in arc_exact]
    assert max_circle_error(arc_exact, F(0), F(0), r5) < Decimal("1e-17")
    arc_plane = {"origin": (0.0, 0.0, 0.0), "x_axis": (1.0, 0.0, 0.0), "y_axis": (0.0, 1.0, 0.0)}
    arc_id = "sect:open-arc-1"
    arc_doc, _ = curve_doc(arc_id, "0" * 32, arc_plane, arc_verts, False)
    files["open_arc.json"] = canonical(arc_doc) + b"\n"
    sa, la = arc_lengths(arc_verts, False)
    assert la == sa[-1]
    oa = "open_arc.json"
    case("open_arc_full", oa, "circle2d", C2, 0.0, la, section=arc_id,
         expect={"first": 0, "count": 41, "truth": {"c": pt(0, 0), "r": hexf(float(r5))}})
    case("open_arc_wrap", oa, "circle2d", C2, sa[30], sa[10], wrap=True, section=arc_id,
         expect={"error": "bad_span{wrap_on_open_curve}"})
    case("open_arc_end_beyond", oa, "circle2d", C2, sa[0], la + 1e-9, section=arc_id,
         expect={"error": "span_out_of_range{end}"})

    out = {
        "about": "GDT.DIM.SECT.SPAN cases; generated by make_span_fixtures.py (CC0)",
        "curves": {
            rc: {"digest": rect_digest, "section_id": rect_id, "length": hexf(length), "vertices": n},
            oa: {"digest": arc_doc["digest"], "section_id": arc_id, "length": hexf(la), "vertices": 41},
        },
        "cases": cases,
    }
    files["cases.json"] = (json.dumps(out, sort_keys=True, indent=1, ensure_ascii=True) + "\n").encode("ascii")
    return files


def main():
    files = build()
    if "--check" in sys.argv[1:]:
        bad = [name for name, data in files.items() if (HERE / name).read_bytes() != data]
        if bad:
            print("DIFFERS:", ", ".join(sorted(bad)))
            return 1
        print("OK: GDT.DIM.SECT.SPAN fixtures match")
        return 0
    for name, data in files.items():
        (HERE / name).write_bytes(data)
    print("wrote", ", ".join(sorted(files)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
