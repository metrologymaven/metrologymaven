#!/usr/bin/env python3
"""Fixture generator and independent oracle for spec node IO.CAD.STEP.GEOM (Metrology Maven).

CC-BY-4.0 project data. Every fixture is invented geometry written by this script (D-038): no
exporter output, no third-party STEP file or dataset, no standards text.

The expected values are computed here, separately from the Rust crate fid-io-step, from the
leaf's rules (spec/io/cad/step/geom/NODE.md):

- A length is the file value times the context's length factor, one IEEE 754 multiplication
  (the factor is the f64 nearest the exact one: 0.001 for millimetre). A plane angle is the
  file value times the plane-angle factor (1.0 for radian, the f64 nearest pi/180 for degree).
  Python floats are IEEE 754 binary64, so `x * 0.001` here is bit-identical to the rule.
- A line's GEO parameter is the STEP parameter times the vector magnitude in metres, so a
  trim parameter p on a line becomes p * (magnitude * factor).
- B-spline knots are the distinct knots repeated by their multiplicities; weights are kept;
  control points are converted like any length. The rational B-spline curve and surface are
  evaluated exactly (fractions.Fraction, Cox-de Boor recursion on exact rationals, control
  points as exact decimal / 1000) at 100 seeded parameters each; the expected point is that
  exact value rounded once to binary64. The parameters come from a 64-bit LCG (Knuth's MMIX
  constants), top 20 bits, as (2k + 1) / 2^21, so they are exact dyadic rationals in (0, 1).
- The extrusion of the B-spline curve is evaluated the same way, plus v times the extrusion
  vector, at 50 seeded (u, v).
- Offset of a cylinder of radius r by d is the coaxial cylinder of radius r + d (outward
  normal), hand-derived.

It writes the *.stp fixtures and expected.txt: one record per line, `<file> <entity> <field>
<values...>`, reals as the 16 hex digits of their binary64 bit pattern.

Run (stdlib only, Python 3.8+):
  python fixtures/io/cad/step/geom/make_cases.py           # rewrite the fixtures
  python fixtures/io/cad/step/geom/make_cases.py --check   # fail if any file is stale
"""

import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
MM = 0.001
DEG = 0.017453292519943295  # the f64 nearest pi/180

NOTE = "/* Invented geometry for spec node IO.CAD.STEP.GEOM (Metrology Maven), CC-BY-4.0. {} */"
SCHEMA = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"

UNITS_MM_RAD = """#9001=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#9002=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#9003=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());
#9004=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#9001,'distance_accuracy_value','synthetic');
#9005=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#9004))GLOBAL_UNIT_ASSIGNED_CONTEXT((#9001,#9002,#9003))REPRESENTATION_CONTEXT('ctx','3D'));"""

UNITS_MM_DEG = """#9001=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));
#9002=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));
#9003=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());
#9006=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433),#9002);
#9007=DIMENSIONAL_EXPONENTS(0.,0.,0.,0.,0.,0.,0.);
#9008=(CONVERSION_BASED_UNIT('DEGREE',#9006)NAMED_UNIT(#9007)PLANE_ANGLE_UNIT());
#9005=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#9001,#9008,#9003))REPRESENTATION_CONTEXT('ctx','3D'));"""

INCH_CONTEXT = """#9101=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#9001);
#9102=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);
#9103=(CONVERSION_BASED_UNIT('INCH',#9101)LENGTH_UNIT()NAMED_UNIT(#9102));
#9105=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#9103,#9002,#9003))REPRESENTATION_CONTEXT('ctx in','3D'));"""

CONTEXT_2D = "#9205=(GEOMETRIC_REPRESENTATION_CONTEXT(2)PARAMETRIC_REPRESENTATION_CONTEXT()REPRESENTATION_CONTEXT('2D SPACE',''));"


def stp(name, note, data):
    head = [
        "ISO-10303-21;",
        "HEADER;",
        NOTE.format(note),
        "FILE_DESCRIPTION(('invented geometry fixture'),'2;1');",
        "FILE_NAME('{}','2026-11-25T00:00:00',('Fixture Author'),('Metrology Maven'),'make_cases.py','fid synthetic generator','');".format(name),
        "FILE_SCHEMA(('{}'));".format(SCHEMA),
        "ENDSEC;",
        "DATA;",
    ]
    tail = ["ENDSEC;", "END-ISO-10303-21;", ""]
    return "\n".join(head + [l for l in data.strip().splitlines()] + tail)


def hx(x):
    return struct.pack(">d", float(x)).hex()


class Expect:
    def __init__(self):
        self.lines = []

    def add(self, file, entity, field, *values):
        parts = [file, str(entity), field]
        for v in values:
            parts.append(v if isinstance(v, str) else hx(v))
        self.lines.append(" ".join(parts))


def lcg(seed):
    x = seed
    while True:
        x = (6364136223846793005 * x + 1442695040888963407) % (1 << 64)
        k = x >> 44  # top 20 bits
        yield (2 * k + 1) / float(1 << 21)


# ---------------------------------------------------------------- exact B-spline evaluation


def expand(knots, mults):
    out = []
    for k, m in zip(knots, mults):
        out.extend([k] * m)
    return out


def basis(i, p, t, kv):
    """Cox-de Boor N_{i,p}(t) on exact rationals (half-open spans; t is inside the domain)."""
    if p == 0:
        return Fraction(1) if kv[i] <= t < kv[i + 1] else Fraction(0)
    out = Fraction(0)
    d1 = kv[i + p] - kv[i]
    if d1 != 0:
        out += (t - kv[i]) / d1 * basis(i, p - 1, t, kv)
    d2 = kv[i + p + 1] - kv[i + 1]
    if d2 != 0:
        out += (kv[i + p + 1] - t) / d2 * basis(i + 1, p - 1, t, kv)
    return out


def eval_curve(p, kv, ctrl, w, t):
    t = Fraction(t)
    num = [Fraction(0)] * 3
    den = Fraction(0)
    for i, c in enumerate(ctrl):
        b = basis(i, p, t, kv) * w[i]
        den += b
        for k in range(3):
            num[k] += b * c[k]
    return [x / den for x in num]


def eval_surface(p, q, ku, kv, net, w, u, v):
    u, v = Fraction(u), Fraction(v)
    nu, nv = len(net), len(net[0])
    bu = [basis(i, p, u, ku) for i in range(nu)]
    bv = [basis(j, q, v, kv) for j in range(nv)]
    num = [Fraction(0)] * 3
    den = Fraction(0)
    for i in range(nu):
        for j in range(nv):
            b = bu[i] * bv[j] * w[i][j]
            den += b
            for k in range(3):
                num[k] += b * net[i][j][k]
    return [x / den for x in num]


def mm_exact(s):
    return Fraction(s) / 1000


def real_text(x):
    """A STEP real token for the decimal string x."""
    return x if "." in x else x + "."


# ---------------------------------------------------------------- analytic_set (AC-1)


def analytic_set(files, ex):
    name = "analytic_set.stp"
    pts = {
        1: ("12.5", "-3.25", "7."),
        5: ("-101.125", "2.75", "0.3"),
        30: ("0.", "0.", "0."),
        31: ("10.", "5.5", "0."),
        32: ("20.25", "-4.", "1.5"),
        33: ("30.", "0.125", "-2."),
    }
    data = """
#1=CARTESIAN_POINT('o1',(12.5,-3.25,7.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p1',#1,#2,#3);
#5=CARTESIAN_POINT('o2',(-101.125,2.75,0.3));
#6=DIRECTION('y',(0.,1.,0.));
#7=AXIS2_PLACEMENT_3D('p2',#5,#6,#2);
#8=AXIS2_PLACEMENT_3D('p3',#1,$,$);
#9=DIRECTION('skew',(1.,0.,0.5));
#10=PLANE('plane',#4);
#11=CYLINDRICAL_SURFACE('cyl',#7,15.75);
#12=CONICAL_SURFACE('cone',#4,4.5,0.3);
#13=SPHERICAL_SURFACE('sph',#7,33.3);
#14=TOROIDAL_SURFACE('tor',#8,40.1,6.2);
#15=AXIS2_PLACEMENT_3D('p4',#5,#2,#9);
#16=PLANE('skew plane',#15);
#20=VECTOR('v',#3,2.5);
#21=LINE('line',#5,#20);
#22=CIRCLE('circle',#4,9.9);
#23=ELLIPSE('ellipse',#7,11.1,4.4);
#24=TRIMMED_CURVE('arc',#22,(PARAMETER_VALUE(0.25)),(PARAMETER_VALUE(1.5)),.T.,.PARAMETER.);
#25=TRIMMED_CURVE('seg',#21,(#1,PARAMETER_VALUE(-2.)),(PARAMETER_VALUE(4.)),.T.,.CARTESIAN.);
#30=CARTESIAN_POINT('',(0.,0.,0.));
#31=CARTESIAN_POINT('',(10.,5.5,0.));
#32=CARTESIAN_POINT('',(20.25,-4.,1.5));
#33=CARTESIAN_POINT('',(30.,0.125,-2.));
#34=POLYLINE('poly',(#30,#31,#32));
#35=B_SPLINE_CURVE_WITH_KNOTS('bs',2,(#30,#31,#32,#33),.UNSPECIFIED.,.F.,.F.,(3,1,3),(0.,0.5,1.),.UNSPECIFIED.);
#40=SHAPE_REPRESENTATION('shape',(#10,#11,#12,#13,#14,#16,#21,#22,#23,#24,#25,#34,#35),#9005);
""" + UNITS_MM_RAD
    files[name] = stp(name, "AC-1: one of each analytic surface and curve, millimetre, radian.", data)
    for n, (x, y, z) in pts.items():
        ex.add(name, n, "point", float(x) * MM, float(y) * MM, float(z) * MM)
    o1 = [float(v) * MM for v in pts[1]]
    o2 = [float(v) * MM for v in pts[5]]
    ex.add(name, 10, "plane", *o1)
    ex.add(name, 10, "frame", 0.0, 0.0, 1.0, 1.0, 0.0, 0.0)
    ex.add(name, 11, "cylinder", *o2, 15.75 * MM)
    ex.add(name, 11, "frame", 0.0, 1.0, 0.0, 0.0, 0.0, 1.0)
    ex.add(name, 12, "cone", *o1, 4.5 * MM, 0.3 * 1.0)
    ex.add(name, 13, "sphere", *o2, 33.3 * MM)
    ex.add(name, 14, "torus", *o1, 40.1 * MM, 6.2 * MM)
    # Defaulted frame: axis (0,0,1), reference (1,0,0) (first_proj_axis).
    ex.add(name, 14, "frame", 0.0, 0.0, 1.0, 1.0, 0.0, 0.0)
    # The skew reference (1, 0, 0.5) normalised has |ref . axis| = 0.5 / sqrt(1.25); the frame
    # keeps X = (1, 0, 0).
    ex.add(name, 16, "frame", 0.0, 0.0, 1.0, 1.0, 0.0, 0.0)
    ex.add(name, 15, "warning", "frame_reorthonormalised")
    mag = 2.5 * MM
    ex.add(name, 21, "line", *o2, mag)
    ex.add(name, 22, "circle", *o1, 9.9 * MM)
    ex.add(name, 23, "ellipse", *o2, 11.1 * MM, 4.4 * MM)
    ex.add(name, 24, "trim", 0.25 * 1.0, 1.5 * 1.0)
    ex.add(name, 25, "trim", -2.0 * mag, 4.0 * mag)
    ex.add(name, 25, "trim1_point", *o1)
    ctrl = [[float(v) * MM for v in pts[k]] for k in (30, 31, 32, 33)]
    ex.add(name, 34, "knots", 0.0, 0.0, 1.0, 2.0, 2.0)
    ex.add(name, 34, "ctrl", *[c for p in ctrl[:3] for c in p])
    ex.add(name, 35, "knots", 0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0)
    ex.add(name, 35, "ctrl", *[c for p in ctrl for c in p])


# ---------------------------------------------------------------- degree units


def degree_units(files, ex):
    name = "degree_units.stp"
    data = """
#1=CARTESIAN_POINT('o',(1.,2.,3.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p',#1,#2,#3);
#10=CONICAL_SURFACE('cone',#4,2.,30.);
#11=CIRCLE('circle',#4,5.);
#12=TRIMMED_CURVE('arc',#11,(PARAMETER_VALUE(90.)),(PARAMETER_VALUE(180.)),.T.,.PARAMETER.);
#40=SHAPE_REPRESENTATION('shape',(#10,#12),#9005);
""" + UNITS_MM_DEG
    files[name] = stp(name, "Plane angles in degrees: cone semi-angle and conic trim parameters.", data)
    ex.add(name, 10, "cone", 1.0 * MM, 2.0 * MM, 3.0 * MM, 2.0 * MM, 30.0 * DEG)
    ex.add(name, 12, "trim", 90.0 * DEG, 180.0 * DEG)


# ---------------------------------------------------------------- rational_bspline (AC-2)

SURF_NET = [
    [("0.", "0.", "0."), ("10.", "0.", "2."), ("20.", "0.", "1."), ("30.", "0.", "0.")],
    [("0.", "12.5", "3."), ("10.", "12.5", "7.25"), ("20.", "12.5", "6."), ("30.", "12.5", "2.5")],
    [("0.", "25.", "0.5"), ("10.", "25.", "1."), ("20.", "25.", "4."), ("30.", "25.", "-1.")],
]
SURF_W = [["1.", "0.8", "1.25", "1."], ["0.9", "1.5", "0.7", "1.1"], ["1.", "1.2", "0.95", "1."]]
CURVE_PTS = [("0.", "0.", "0."), ("5.", "10.", "1."), ("15.", "12.", "-2."), ("25.", "4.", "0.5"), ("30.", "-3.", "3.")]
CURVE_W = ["1.", "0.6", "1.4", "0.85", "1."]


def rational_bspline(files, ex):
    name = "rational_bspline.stp"
    lines = []
    n = 100
    ids = []
    for row in SURF_NET:
        rid = []
        for p in row:
            lines.append("#{}=CARTESIAN_POINT('',({},{},{}));".format(n, *p))
            rid.append(n)
            n += 1
        ids.append(rid)
    cid = []
    for p in CURVE_PTS:
        lines.append("#{}=CARTESIAN_POINT('',({},{},{}));".format(n, *p))
        cid.append(n)
        n += 1
    net_txt = ",".join("(" + ",".join("#%d" % i for i in r) + ")" for r in ids)
    w_txt = ",".join("(" + ",".join(r) + ")" for r in SURF_W)
    lines.append(
        "#10=(BOUNDED_SURFACE()B_SPLINE_SURFACE(2,2,({}),.UNSPECIFIED.,.F.,.F.,.F.)"
        "B_SPLINE_SURFACE_WITH_KNOTS((3,3),(3,1,3),(0.,1.),(0.,0.5,1.),.UNSPECIFIED.)"
        "GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_SURFACE(({}))REPRESENTATION_ITEM('rsurf')SURFACE());".format(net_txt, w_txt)
    )
    lines.append(
        "#11=(BOUNDED_CURVE()B_SPLINE_CURVE(3,({}),.UNSPECIFIED.,.F.,.F.)B_SPLINE_CURVE_WITH_KNOTS((4,1,4),(0.,0.4,1.),.UNSPECIFIED.)"
        "CURVE()GEOMETRIC_REPRESENTATION_ITEM()RATIONAL_B_SPLINE_CURVE(({}))REPRESENTATION_ITEM('rcurve'));".format(
            ",".join("#%d" % i for i in cid), ",".join(CURVE_W)
        )
    )
    lines.append(
        "#12=B_SPLINE_SURFACE_WITH_KNOTS('plain',2,2,({}),.UNSPECIFIED.,.F.,.F.,.F.,(3,3),(3,1,3),(0.,1.),(0.,0.5,1.),.UNSPECIFIED.);".format(net_txt)
    )
    lines.append("#40=SHAPE_REPRESENTATION('shape',(#10,#11,#12),#9005);")
    files[name] = stp(name, "AC-2: rational B-spline surface and curve (complex form) and a plain B-spline surface, millimetre.", "\n".join(lines) + "\n" + UNITS_MM_RAD)

    ku = [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]
    kv = [0.0, 0.0, 0.0, 0.5, 1.0, 1.0, 1.0]
    ex.add(name, 10, "knots_u", *ku)
    ex.add(name, 10, "knots_v", *kv)
    ex.add(name, 10, "weights", *[float(w) for r in SURF_W for w in r])
    ex.add(name, 10, "ctrl", *[float(c) * MM for r in SURF_NET for p in r for c in p])
    ex.add(name, 12, "ctrl", *[float(c) * MM for r in SURF_NET for p in r for c in p])
    kc = [0.0] * 4 + [0.4] + [1.0] * 4
    ex.add(name, 11, "knots", *kc)
    ex.add(name, 11, "weights", *[float(w) for w in CURVE_W])
    ex.add(name, 11, "ctrl", *[float(c) * MM for p in CURVE_PTS for c in p])

    fku = [Fraction(x) for x in ku]
    fkv = [Fraction(x) for x in kv]
    net = [[[mm_exact(c) for c in p] for p in r] for r in SURF_NET]
    w = [[Fraction(x) for x in r] for r in SURF_W]
    ones = [[Fraction(1)] * 4 for _ in range(3)]
    g = lcg(20261125)
    for _ in range(100):
        u, v = next(g), next(g)
        x = eval_surface(2, 2, fku, fkv, net, w, u, v)
        ex.add(name, 10, "eval", u, v, *[float(c) for c in x])
        y = eval_surface(2, 2, fku, fkv, net, ones, u, v)
        ex.add(name, 12, "eval", u, v, *[float(c) for c in y])
    fkc = [Fraction(x) for x in kc]
    cp = [[mm_exact(c) for c in p] for p in CURVE_PTS]
    cw = [Fraction(x) for x in CURVE_W]
    for _ in range(100):
        t = next(g)
        x = eval_curve(3, fkc, cp, cw, t)
        ex.add(name, 11, "eval", t, *[float(c) for c in x])


# ---------------------------------------------------------------- offset_cyl (AC-3)


def offset_cyl(files, ex):
    name = "offset_cyl.stp"
    data = """
#1=CARTESIAN_POINT('o',(5.,-2.,10.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p',#1,#2,#3);
#10=CYLINDRICAL_SURFACE('base',#4,20.);
#11=OFFSET_SURFACE('outer',#10,5.,.F.);
#12=OFFSET_SURFACE('inner',#10,-2.5,.F.);
#13=PLANE('flat',#4);
#14=OFFSET_SURFACE('raised',#13,1.5,.F.);
#15=CONICAL_SURFACE('apex cone',#4,0.,0.5);
#16=OFFSET_SURFACE('cone shell',#15,1.,.F.);
#40=SHAPE_REPRESENTATION('shape',(#11,#12,#14,#16),#9005);
""" + UNITS_MM_RAD
    files[name] = stp(name, "AC-3: offsets of a cylinder (exact cylinders of known radius), of a plane and of a cone at its apex.", data)
    ex.add(name, 11, "axis", 5.0 * MM, -2.0 * MM, 10.0 * MM, 0.0, 0.0, 1.0)
    ex.add(name, 11, "radius", 25.0 * MM)
    ex.add(name, 12, "radius", 17.5 * MM)
    # The plane offset is the plane z = 10 + 1.5 mm.
    ex.add(name, 14, "plane_z", 11.5 * MM)
    # The cone's normal is degenerate at its apex (v = 0), which the fit grid contains.
    ex.add(name, 16, "skipped", "OFFSET_SURFACE", "offset_degenerate_normal")


# ---------------------------------------------------------------- unknown_surface (AC-4)


def unknown_surface(files, ex):
    name = "unknown_surface.stp"
    data = """
#1=CARTESIAN_POINT('o',(0.,0.,0.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p',#1,#2,#3);
#10=PLANE('plane',#4);
#11=RECTANGULAR_TRIMMED_SURFACE('rts',#10,0.,10.,0.,5.,.T.,.T.);
#40=SHAPE_REPRESENTATION('shape',(#10,#11),#9005);
""" + UNITS_MM_RAD
    files[name] = stp(name, "AC-4: one rectangular_trimmed_surface and one plane.", data)
    ex.add(name, 10, "plane", 0.0, 0.0, 0.0)
    ex.add(name, 11, "skipped", "RECTANGULAR_TRIMMED_SURFACE", "unsupported")


# ---------------------------------------------------------------- invalid entities (REQ-002, -005)


def invalid(files, ex):
    name = "invalid.stp"
    data = """
#1=CARTESIAN_POINT('o',(0.,0.,0.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p',#1,#2,#3);
#5=CARTESIAN_POINT('a',(1.,0.,0.));
#6=CARTESIAN_POINT('b',(2.,1.,0.));
#7=CARTESIAN_POINT('c',(3.,0.,0.));
#8=DIRECTION('zero',(0.,0.,0.));
#9=AXIS2_PLACEMENT_3D('bad',#1,#8,#3);
#10=PLANE('good',#4);
#11=CIRCLE('no radius',#4);
#12=B_SPLINE_CURVE_WITH_KNOTS('decreasing',2,(#5,#6,#7),.UNSPECIFIED.,.F.,.F.,(3,3),(1.,0.),.UNSPECIFIED.);
#13=B_SPLINE_CURVE_WITH_KNOTS('count',2,(#5,#6,#7),.UNSPECIFIED.,.F.,.F.,(3,2),(0.,1.),.UNSPECIFIED.);
#14=HYPERBOLA('hyp',#4,1.,2.);
#15=CYLINDRICAL_SURFACE('dangling',#99,3.);
#16=CYLINDRICAL_SURFACE('zero axis',#9,3.);
#17=TOROIDAL_SURFACE('self-intersecting',#4,2.,3.);
#18=CYLINDRICAL_SURFACE('also good',#4,3.);
#40=SHAPE_REPRESENTATION('shape',(#10,#11,#12,#13,#14,#15,#16,#17,#18),#9005);
""" + UNITS_MM_RAD
    files[name] = stp(name, "Invalid and unsupported entities next to valid ones.", data)
    ex.add(name, 11, "skipped", "CIRCLE", "malformed")
    ex.add(name, 12, "skipped", "B_SPLINE_CURVE_WITH_KNOTS", "nurbs_invalid")
    ex.add(name, 13, "skipped", "B_SPLINE_CURVE_WITH_KNOTS", "nurbs_invalid")
    ex.add(name, 14, "skipped", "HYPERBOLA", "unsupported")
    ex.add(name, 15, "skipped", "CYLINDRICAL_SURFACE", "dangling_reference")
    ex.add(name, 16, "skipped", "CYLINDRICAL_SURFACE", "malformed")
    ex.add(name, 17, "skipped", "TOROIDAL_SURFACE", "geo_invalid")
    ex.add(name, 10, "plane", 0.0, 0.0, 0.0)
    ex.add(name, 18, "cylinder", 0.0, 0.0, 0.0, 3.0 * MM)


# ---------------------------------------------------------------- swept surfaces and pcurves

SWEEP_PTS = [("0.", "0.", "0."), ("10.", "8.", "0."), ("20.", "-2.", "0."), ("30.", "5.", "0.")]


def swept(files, ex):
    name = "swept.stp"
    data = """
#1=CARTESIAN_POINT('o',(0.,0.,0.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p',#1,#2,#3);
#5=CARTESIAN_POINT('',(0.,0.,0.));
#6=CARTESIAN_POINT('',(10.,8.,0.));
#7=CARTESIAN_POINT('',(20.,-2.,0.));
#8=CARTESIAN_POINT('',(30.,5.,0.));
#9=B_SPLINE_CURVE_WITH_KNOTS('profile',3,(#5,#6,#7,#8),.UNSPECIFIED.,.F.,.F.,(4,4),(0.,1.),.UNSPECIFIED.);
#10=VECTOR('up',#2,12.);
#11=SURFACE_OF_LINEAR_EXTRUSION('extruded profile',#9,#10);
#12=CARTESIAN_POINT('rim',(7.,0.,0.));
#13=VECTOR('along z',#2,1.);
#14=LINE('generator',#12,#13);
#15=TRIMMED_CURVE('generator segment',#14,(PARAMETER_VALUE(0.)),(PARAMETER_VALUE(15.)),.T.,.PARAMETER.);
#16=AXIS1_PLACEMENT('z axis',#1,#2);
#17=SURFACE_OF_REVOLUTION('revolved segment',#15,#16);
#18=CARTESIAN_POINT('tube centre',(30.,0.,0.));
#19=DIRECTION('y',(0.,1.,0.));
#20=AXIS2_PLACEMENT_3D('tube',#18,#19,#3);
#21=CIRCLE('tube circle',#20,5.);
#22=SURFACE_OF_REVOLUTION('revolved circle',#21,#16);
#23=CIRCLE('base circle',#4,9.);
#24=SURFACE_OF_LINEAR_EXTRUSION('extruded circle',#23,#10);
#30=PLANE('ground',#4);
#31=CARTESIAN_POINT('',(1.,2.));
#32=DIRECTION('',(0.6,0.8));
#33=VECTOR('',#32,5.);
#34=LINE('2d line',#31,#33);
#35=DEFINITIONAL_REPRESENTATION('',(#34),#9205);
#36=PCURVE('on ground',#30,#35);
#37=VECTOR('',#3,1.);
#38=LINE('3d line',#1,#37);
#39=SURFACE_CURVE('edge geometry',#38,(#36),.CURVE_3D.);
#40=SHAPE_REPRESENTATION('shape',(#11,#17,#22,#24,#39),#9005);
#41=CARTESIAN_POINT('',(0.,0.));
#42=CARTESIAN_POINT('',(4.,1.));
#43=CARTESIAN_POINT('',(8.,-1.));
#44=B_SPLINE_CURVE_WITH_KNOTS('2d spline',2,(#41,#42,#43),.UNSPECIFIED.,.F.,.F.,(3,3),(0.,1.),.UNSPECIFIED.);
#45=DEFINITIONAL_REPRESENTATION('',(#44),#9205);
#46=PCURVE('spline on ground',#30,#45);
#47=PCURVE('on extrusion',#11,#45);
#48=SHAPE_REPRESENTATION('more',(#46,#47),#9005);
""" + UNITS_MM_RAD + "\n" + CONTEXT_2D
    files[name] = stp(name, "Extrusions, revolutions, a surface curve and pcurves in a unitless 2D context.", data)
    v = [0.0, 0.0, 12.0 * MM]
    ex.add(name, 11, "vector", *v)
    fk = [Fraction(0)] * 4 + [Fraction(1)] * 4
    cp = [[mm_exact(c) for c in p] for p in SWEEP_PTS]
    g = lcg(7)
    for _ in range(50):
        u, s = next(g), next(g)
        c = eval_curve(3, fk, cp, [Fraction(1)] * 4, u)
        x = [c[0], c[1], c[2] + Fraction(s) * 12 / 1000]
        ex.add(name, 11, "eval", u, s, *[float(t) for t in x])
    ex.add(name, 17, "cylinder_radius", 7.0 * MM)
    ex.add(name, 17, "u_range", 0.0, 15.0)
    ex.add(name, 22, "torus", 30.0 * MM, 5.0 * MM)
    ex.add(name, 24, "cylinder_radius", 9.0 * MM)
    # pcurve on a plane: both coordinates in millimetres, scaled by 0.001.
    ex.add(name, 36, "line2", 1.0 * MM, 2.0 * MM, (0.6 * 5.0) * MM, (0.8 * 5.0) * MM)
    ex.add(name, 46, "ctrl2", 0.0 * MM, 0.0 * MM, 4.0 * MM, 1.0 * MM, 8.0 * MM, -1.0 * MM)
    # pcurve on an extrusion of a B-spline: STEP parameters, scale 1.
    ex.add(name, 47, "ctrl2", 0.0, 0.0, 4.0, 1.0, 8.0, -1.0)
    ex.add(name, 39, "surface_curve", "38", "36")


# ---------------------------------------------------------------- contexts


def contexts(files, ex):
    name = "contexts.stp"
    data = """
#1=CARTESIAN_POINT('shared',(1.,2.,3.));
#2=DIRECTION('z',(0.,0.,1.));
#3=DIRECTION('x',(1.,0.,0.));
#4=AXIS2_PLACEMENT_3D('p',#1,#2,#3);
#5=CARTESIAN_POINT('mm only',(4.,0.,0.));
#6=CARTESIAN_POINT('in only',(4.,0.,0.));
#7=CARTESIAN_POINT('nobody',(4.,0.,0.));
#10=CYLINDRICAL_SURFACE('mm cyl',#4,2.);
#11=CYLINDRICAL_SURFACE('in cyl',#4,2.);
#40=SHAPE_REPRESENTATION('mm',(#10,#5),#9005);
#41=SHAPE_REPRESENTATION('in',(#11,#6),#9105);
""" + UNITS_MM_RAD + "\n" + INCH_CONTEXT
    files[name] = stp(name, "Two contexts (mm, inch) sharing a placement.", data)
    ex.add(name, 5, "point", 4.0 * MM, 0.0, 0.0)
    ex.add(name, 6, "point", 4.0 * 0.0254, 0.0, 0.0)
    ex.add(name, 10, "cylinder", 1.0 * MM, 2.0 * MM, 3.0 * MM, 2.0 * MM)
    ex.add(name, 11, "cylinder", 1.0 * 0.0254, 2.0 * 0.0254, 3.0 * 0.0254, 2.0 * 0.0254)
    ex.add(name, 7, "skipped", "CARTESIAN_POINT", "no_context")
    ex.add(name, 1, "skipped", "CARTESIAN_POINT", "context_conflict")


def build():
    files = {}
    ex = Expect()
    for f in (analytic_set, degree_units, rational_bspline, offset_cyl, unknown_surface, invalid, swept, contexts):
        f(files, ex)
    files["expected.txt"] = "# Generated by make_cases.py (IO.CAD.STEP.GEOM oracle). <file> <entity> <field> <values>\n" + "\n".join(ex.lines) + "\n"
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    stale = []
    for name, text in sorted(files.items()):
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if check:
        if stale:
            print("stale fixtures: " + ", ".join(stale))
            sys.exit(1)
        print("IO.CAD.STEP.GEOM fixtures up to date ({} files)".format(len(files)))


if __name__ == "__main__":
    main()
