#!/usr/bin/env python3
"""Fixture generator and independent oracle for spec node IO.CAD.PMI.READ (Metrology Maven).

CC-BY-4.0 project data. Every fixture is synthetic: written by this script from invented
instances, not copied from any CAD exporter, test model, dataset or standard. Entity names
are names only; no standards or recommended-practice text is reproduced (D-038).

The oracle below is written separately from the Rust crate fid-io-pmi, from the rules in
the leaf's "Pinned at build" section (spec/io/cad/pmi/read/NODE.md). It works on this
script's own instance model, not on the emitted text, except for the AC-5 completeness
scan, which reads the emitted text with a separate top-level name scanner.

It writes the fixtures *.stp and expected.txt (one block per fixture):

  == <fixture>
  item <n> class=<c> kind=<k> reason=<r|-> types=<A+B> links=<..|-> unresolved=<..|->
       refs=<..|-> presented_by=<..|-> values=<v;v|->          (one line per item)
  count <items in the graph>
  scan <PMI-related instances found by the text scan>

A value is <measure>:<measure type|->:<lexeme|->:<unit entity|->:<unit context|->:<unit>.

Run (stdlib only, Python 3.8+):
  python fixtures/io/cad/pmi/read/make_cases.py           # rewrite the fixtures
  python fixtures/io/cad/pmi/read/make_cases.py --check   # fail if any file is stale
"""

import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------- instance model


class Ref:
    def __init__(self, n):
        self.n = n


class Enum:
    def __init__(self, v):
        self.v = v


class Real:
    def __init__(self, lexeme):
        self.lexeme = lexeme


class Int:
    def __init__(self, v):
        self.v = v


class Typed:
    def __init__(self, name, inner):
        self.name = name
        self.inner = inner


class Unset:
    pass


class Derived:
    pass


UNSET = Unset()
DERIVED = Derived()


class Inst:
    def __init__(self, parts, complex_form):
        self.parts = parts  # [(name, [params])]
        self.complex_form = complex_form

    def names(self):
        return [p[0] for p in self.parts]


def simple(name, *params):
    return Inst([(name, list(params))], False)


def cplx(*parts):
    return Inst([(p[0], list(p[1:])) for p in parts], True)


def emit_param(p):
    if isinstance(p, str):
        return "'" + p.replace("'", "''") + "'"
    if isinstance(p, Ref):
        return "#{}".format(p.n)
    if isinstance(p, Enum):
        return ".{}.".format(p.v)
    if isinstance(p, Real):
        return p.lexeme
    if isinstance(p, Int):
        return str(p.v)
    if isinstance(p, Typed):
        return "{}({})".format(p.name, emit_param(p.inner))
    if isinstance(p, Unset):
        return "$"
    if isinstance(p, Derived):
        return "*"
    if isinstance(p, list):
        return "(" + ",".join(emit_param(x) for x in p) + ")"
    raise TypeError(p)


def emit_inst(n, inst):
    body = "".join("{}({})".format(name, ",".join(emit_param(x) for x in params)) for name, params in inst.parts)
    if inst.complex_form:
        body = "(" + body + ")"
    return "#{}={};".format(n, body)


NOTE = "/* Synthetic fixture for spec node IO.CAD.PMI.READ (Metrology Maven), CC-BY-4.0. {} */"
SCHEMA_242 = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"


def step_file(name, note, data):
    lines = [
        "ISO-10303-21;",
        "HEADER;",
        NOTE.format(note),
        "FILE_DESCRIPTION(('synthetic PMI fixture'),'2;1');",
        "FILE_NAME('{}','2026-11-24T00:00:00',('Fixture Author'),('Metrology Maven'),"
        "'make_cases.py','fid synthetic generator','');".format(name),
        "FILE_SCHEMA(('{}'));".format(SCHEMA_242),
        "ENDSEC;",
        "DATA;",
    ]
    for n in sorted(data):
        lines.append(emit_inst(n, data[n]))
    lines += ["ENDSEC;", "END-ISO-10303-21;", ""]
    return "\n".join(lines)


def prelude():
    d = {
        1: simple("CARTESIAN_POINT", "origin", [Real("0."), Real("0."), Real("0.")]),
        2: simple("DIRECTION", "z", [Real("0."), Real("0."), Real("1.")]),
        3: simple("DIRECTION", "x", [Real("1."), Real("0."), Real("0.")]),
        4: simple("AXIS2_PLACEMENT_3D", "", Ref(1), Ref(2), Ref(3)),
        5: simple("PLANE", "top", Ref(4)),
        10: cplx(("LENGTH_UNIT",), ("NAMED_UNIT", DERIVED), ("SI_UNIT", Enum("MILLI"), Enum("METRE"))),
        11: cplx(("NAMED_UNIT", DERIVED), ("PLANE_ANGLE_UNIT",), ("SI_UNIT", UNSET, Enum("RADIAN"))),
        12: cplx(("NAMED_UNIT", DERIVED), ("SI_UNIT", UNSET, Enum("STERADIAN")), ("SOLID_ANGLE_UNIT",)),
        13: simple("UNCERTAINTY_MEASURE_WITH_UNIT", Typed("LENGTH_MEASURE", Real("1.E-06")), Ref(10),
                   "distance_accuracy_value", "synthetic"),
        14: cplx(("GEOMETRIC_REPRESENTATION_CONTEXT", Int(3)),
                 ("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", [Ref(13)]),
                 ("GLOBAL_UNIT_ASSIGNED_CONTEXT", [Ref(10), Ref(11), Ref(12)]),
                 ("REPRESENTATION_CONTEXT", "ctx", "synthetic")),
        20: simple("ADVANCED_BREP_SHAPE_REPRESENTATION", "part", [Ref(4)], Ref(14)),
        30: simple("ADVANCED_FACE", "top", [], Ref(5), Enum("T")),
        40: simple("PRODUCT_DEFINITION_SHAPE", "shape", "", UNSET),
    }
    return d


def polyline_presentation(d):
    d[70] = simple("CARTESIAN_POINT", "", [Real("0."), Real("10."), Real("0.")])
    d[71] = simple("CARTESIAN_POINT", "", [Real("20."), Real("10."), Real("0.")])
    d[72] = simple("POLYLINE", "leader", [Ref(70), Ref(71)])
    d[73] = simple("PRESENTATION_STYLE_ASSIGNMENT", [])
    d[74] = simple("ANNOTATION_CURVE_OCCURRENCE", "frame", [Ref(73)], Ref(72))
    d[75] = simple("DRAUGHTING_MODEL", "pmi", [Ref(74)], Ref(14))


def length(value, unit=10):
    return simple("LENGTH_MEASURE_WITH_UNIT", Typed("LENGTH_MEASURE", value), Ref(unit))


def gisu(aspect, item):
    return simple("GEOMETRIC_ITEM_SPECIFIC_USAGE", "", "", Ref(aspect), Ref(20), item)


def dmia(definition, item):
    return simple("DRAUGHTING_MODEL_ITEM_ASSOCIATION", "", "", Ref(definition), Ref(75), Ref(item))


def sar(relating, related):
    return simple("SHAPE_ASPECT_RELATIONSHIP", "", "", Ref(relating), Ref(related))


FIXTURES = {}

# AC-1: one flatness tolerance on one face, with a linked polyline presentation.
d = prelude()
polyline_presentation(d)
d[60] = simple("SHAPE_ASPECT", "top face", "", Ref(40), Enum("T"))
d[61] = gisu(60, Ref(30))
d[62] = length(Real("0.05"))
d[63] = simple("FLATNESS_TOLERANCE", "flatness", "", Ref(62), Ref(60))
d[76] = dmia(63, 74)
FIXTURES["flatness_semantic.stp"] = ("AC-1: flatness 0.05 mm on face #30, presentation #74 linked.", d)

# AC-2: a polyline annotation with no semantics.
d = prelude()
polyline_presentation(d)
FIXTURES["graphic_only.stp"] = ("AC-2: polyline annotation #74, no semantic PMI.", d)

# AC-3: a tolerance whose complex instance has a leaf the reader does not model.
d = prelude()
d[60] = simple("SHAPE_ASPECT", "edge", "", Ref(40), Enum("T"))
d[61] = gisu(60, Ref(30))
d[62] = length(Real("0.05"))
d[64] = length(Real("0.2"))
d[63] = cplx(("GEOMETRIC_TOLERANCE", "straightness", "", Ref(62), Ref(60)),
             ("GEOMETRIC_TOLERANCE_WITH_MAXIMUM_TOLERANCE", Ref(64)),
             ("GEOMETRIC_TOLERANCE_WITH_MODIFIERS", [Enum("MAXIMUM_MATERIAL_REQUIREMENT")]),
             ("STRAIGHTNESS_TOLERANCE",))
FIXTURES["unknown_complex.stp"] = ("AC-3: tolerance #63 with an unmodelled complex-entity leaf.", d)

# AC-4: the toleranced shape aspect is not defined.
d = prelude()
d[62] = length(Real("0.05"))
d[63] = simple("FLATNESS_TOLERANCE", "flatness", "", Ref(62), Ref(999))
FIXTURES["dangling_aspect.stp"] = ("AC-4: tolerance #63 refers to undefined aspect #999.", d)

# An association whose definition is a bare shape aspect (with a usage to a face): the
# presentation stays graphic-only, because a shape aspect is not a characteristic.
d = prelude()
polyline_presentation(d)
d[60] = simple("SHAPE_ASPECT", "top face", "", Ref(40), Enum("T"))
d[61] = gisu(60, Ref(30))
d[76] = dmia(60, 74)
FIXTURES["aspect_association.stp"] = ("Association of presentation #74 to bare shape aspect #60: graphic-only.", d)

# Coverage: datums, datum systems, composite aspects, dimensions with values, callouts,
# graphic-only and orphan presentations, unsupported constructs, unresolved units.
d = prelude()
polyline_presentation(d)
d[22] = simple("DIMENSIONAL_EXPONENTS", Real("1."), Real("0."), Real("0."), Real("0."), Real("0."), Real("0."),
               Real("0."))
d[31] = simple("ADVANCED_FACE", "side", [], Ref(5), Enum("T"))
d[32] = simple("ADVANCED_FACE", "bore", [], Ref(5), Enum("F"))
d[33] = simple("EDGE_CURVE", "edge", Ref(34), Ref(34), Ref(5), Enum("T"))
d[34] = simple("VERTEX_POINT", "", Ref(1))
# datum feature A on #31, datum A, compartment, datum system
d[100] = simple("DATUM_FEATURE", "A", "", Ref(40), Enum("T"))
d[101] = gisu(100, Ref(31))
d[102] = simple("DATUM", "A", "", Ref(40), Enum("F"), "A")
d[103] = sar(100, 102)
d[104] = simple("DATUM_REFERENCE_COMPARTMENT", "", "", Ref(40), Enum("F"), Ref(102), UNSET)
d[105] = simple("DATUM_SYSTEM", "DRF", "", Ref(40), Enum("F"), [Ref(104)])
# hole aspect on a face and an edge (list-valued identified item)
d[106] = simple("SHAPE_ASPECT", "hole", "", Ref(40), Enum("T"))
d[107] = gisu(106, [Ref(32), Ref(33)])
d[108] = length(Real("0.1"))
d[109] = cplx(("GEOMETRIC_TOLERANCE", "position", "", Ref(108), Ref(106)),
              ("GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE", [Ref(105)]),
              ("GEOMETRIC_TOLERANCE_WITH_MODIFIERS", [Enum("MAXIMUM_MATERIAL_REQUIREMENT")]),
              ("POSITION_TOLERANCE",))
# composite aspect with two members, one member's geometry dangling
d[110] = simple("SHAPE_ASPECT", "m1", "", Ref(40), Enum("T"))
d[111] = gisu(110, Ref(30))
d[112] = simple("SHAPE_ASPECT", "m2", "", Ref(40), Enum("T"))
d[113] = gisu(112, Ref(998))
d[114] = simple("COMPOSITE_SHAPE_ASPECT", "pattern", "", Ref(40), Enum("T"))
d[115] = sar(114, 110)
d[116] = sar(114, 112)
d[117] = length(Real("0.02"))
d[118] = simple("SURFACE_PROFILE_TOLERANCE", "profile", "", Ref(117), Ref(114))
# size dimension with nominal and plus-minus values
d[120] = simple("DIMENSIONAL_SIZE", Ref(106), "diameter")
d[121] = cplx(("LENGTH_MEASURE_WITH_UNIT",), ("MEASURE_REPRESENTATION_ITEM",),
              ("MEASURE_WITH_UNIT", Typed("LENGTH_MEASURE", Real("12.5")), Ref(10)),
              ("REPRESENTATION_ITEM", "nominal value"))
d[122] = simple("SHAPE_DIMENSION_REPRESENTATION", "", [Ref(121)], Ref(14))
d[123] = simple("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION", Ref(120), Ref(122))
d[124] = length(Real("-0.1"))
d[125] = length(Real("0.15"))
d[126] = simple("TOLERANCE_VALUE", Ref(124), Ref(125))
d[127] = simple("PLUS_MINUS_TOLERANCE", Ref(126), Ref(120))
d[128] = simple("LIMITS_AND_FITS", "H", "7", "", "")
# location dimension with an integer nominal
d[130] = simple("DIMENSIONAL_LOCATION", "linear distance", "", Ref(100), Ref(106))
d[131] = cplx(("LENGTH_MEASURE_WITH_UNIT",), ("MEASURE_REPRESENTATION_ITEM",),
              ("MEASURE_WITH_UNIT", Typed("LENGTH_MEASURE", Int(40)), Ref(10)),
              ("REPRESENTATION_ITEM", "nominal value"))
d[132] = simple("SHAPE_DIMENSION_REPRESENTATION", "", [Ref(131)], Ref(14))
d[133] = simple("DIMENSIONAL_CHARACTERISTIC_REPRESENTATION", Ref(130), Ref(132))
# unsupported constructs
d[135] = simple("ANGULAR_SIZE", Ref(106), "angle", Enum("SMALL"))
d[136] = simple("PLANE_ANGLE_MEASURE_WITH_UNIT", Typed("PLANE_ANGLE_MEASURE", Real("0.5")), Ref(11))
d[137] = simple("ANGULARITY_TOLERANCE", "angularity", "", Ref(136), Ref(106))
d[138] = simple("TOLERANCE_ZONE", "", "", Ref(40), Enum("F"), [Ref(109)], Ref(139))
d[139] = simple("TOLERANCE_ZONE_FORM", "cylindrical")
# datum target on a vertex
d[140] = simple("DATUM_TARGET", "A1", "", Ref(40), Enum("T"), "1")
d[141] = gisu(140, Ref(34))
# composite datum feature B over member #110
d[142] = cplx(("COMPOSITE_SHAPE_ASPECT",), ("DATUM_FEATURE",), ("SHAPE_ASPECT", "B", "", Ref(40), Enum("T")))
d[143] = sar(142, 110)
# two tolerance types in one instance; two kinds in one instance
d[144] = cplx(("FLATNESS_TOLERANCE",), ("GEOMETRIC_TOLERANCE", "x", "", Ref(117), Ref(110)),
              ("STRAIGHTNESS_TOLERANCE",))
d[145] = cplx(("DATUM", "C"), ("DATUM_FEATURE",), ("SHAPE_ASPECT", "C", "", Ref(40), Enum("F")))
# presentations: tessellated occurrence linked directly and through a callout
d[150] = simple("TESSELLATED_ANNOTATION_OCCURRENCE", "pos", [Ref(73)], Ref(151))
d[151] = simple("TESSELLATED_GEOMETRIC_SET", "", [])
d[152] = simple("ANNOTATION_CURVE_OCCURRENCE", "dim", [Ref(73)], Ref(72))
d[153] = simple("DRAUGHTING_CALLOUT", "callout", [Ref(150), Ref(152)])
d[154] = dmia(109, 150)
d[155] = dmia(120, 153)
d[156] = simple("ANNOTATION_CURVE_OCCURRENCE", "note", [Ref(73)], Ref(72))
d[157] = simple("ANNOTATION_OCCURRENCE", "orphan", [Ref(73)], Ref(72))
d[158] = dmia(997, 157)
d[159] = dmia(118, 996)
d[160] = cplx(("ANNOTATION_OCCURRENCE",), ("DATUM_FEATURE",), ("SHAPE_ASPECT", "D", "", Ref(40), Enum("T")),
              ("STYLED_ITEM", "", [Ref(73)], Ref(72)))
d[161] = cplx(("ANNOTATION_CURVE_OCCURRENCE",), ("ANNOTATION_OCCURRENCE",), ("GEOMETRIC_REPRESENTATION_ITEM",),
              ("REPRESENTATION_ITEM", ""), ("STYLED_ITEM", "", [Ref(73)], Ref(72)))
# unit not defined; unit in no context
d[162] = length(Real("0.3"), unit=995)
d[163] = simple("STRAIGHTNESS_TOLERANCE", "s", "", Ref(162), Ref(110))
d[164] = cplx(("LENGTH_UNIT",), ("NAMED_UNIT", DERIVED), ("SI_UNIT", Enum("MICRO"), Enum("METRE")))
d[165] = length(Real("5.E-01"), unit=164)
d[166] = simple("ROUNDNESS_TOLERANCE", "r", "", Ref(165), Ref(110))
FIXTURES["datums_dims.stp"] = ("Coverage: datums, dimensions, callouts, unsupported and unresolved cases.", d)

# ---------------------------------------------------------------- oracle rules

TOLERANCE_TYPES = {
    "ANGULARITY_TOLERANCE", "CIRCULAR_RUNOUT_TOLERANCE", "COAXIALITY_TOLERANCE", "CONCENTRICITY_TOLERANCE",
    "CYLINDRICITY_TOLERANCE", "FLATNESS_TOLERANCE", "LINE_PROFILE_TOLERANCE", "PARALLELISM_TOLERANCE",
    "PERPENDICULARITY_TOLERANCE", "POSITION_TOLERANCE", "ROUNDNESS_TOLERANCE", "STRAIGHTNESS_TOLERANCE",
    "SURFACE_PROFILE_TOLERANCE", "SYMMETRY_TOLERANCE", "TOTAL_RUNOUT_TOLERANCE",
}
TOLERANCE_FORMS = {
    "GEOMETRIC_TOLERANCE", "GEOMETRIC_TOLERANCE_WITH_DATUM_REFERENCE", "GEOMETRIC_TOLERANCE_WITH_MODIFIERS",
    "GEOMETRIC_TOLERANCE_WITH_DEFINED_UNIT", "GEOMETRIC_TOLERANCE_WITH_DEFINED_AREA_UNIT",
}
KIND = {}
for _n in TOLERANCE_TYPES | TOLERANCE_FORMS:
    KIND[_n] = "tolerance"
KIND.update({
    "DIMENSIONAL_SIZE": "dimension",
    "DIMENSIONAL_LOCATION": "dimension",
    "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION": "dimension_representation",
    "PLUS_MINUS_TOLERANCE": "plus_minus",
    "TOLERANCE_VALUE": "tolerance_value",
    "LIMITS_AND_FITS": "limits_and_fits",
    "SHAPE_ASPECT": "shape_aspect",
    "COMPOSITE_SHAPE_ASPECT": "shape_aspect",
    "COMPOSITE_GROUP_SHAPE_ASPECT": "shape_aspect",
    "ALL_AROUND_SHAPE_ASPECT": "shape_aspect",
    "DATUM_FEATURE": "datum_feature",
    "DATUM": "datum",
    "DATUM_TARGET": "datum_target",
    "DATUM_SYSTEM": "datum_system",
    "DATUM_REFERENCE_COMPARTMENT": "datum_reference",
    "DATUM_REFERENCE_ELEMENT": "datum_reference",
    "GENERAL_DATUM_REFERENCE": "datum_reference",
})
COMPOSITE = {"COMPOSITE_SHAPE_ASPECT", "COMPOSITE_GROUP_SHAPE_ASPECT", "ALL_AROUND_SHAPE_ASPECT"}
EXTRA_RELATED = {
    "ANGULAR_LOCATION", "ANGULAR_SIZE", "APEX", "CENTRE_OF_SYMMETRY", "EXTENSION", "GEOMETRIC_ALIGNMENT",
    "GEOMETRIC_INTERSECTION", "PARALLEL_OFFSET", "PERPENDICULAR_TO", "TANGENT",
}
GEOMETRY_KINDS = {"tolerance", "dimension", "shape_aspect", "datum_feature", "datum", "datum_target"}
USAGE_NAMES = {"ITEM_IDENTIFIED_REPRESENTATION_USAGE", "GEOMETRIC_ITEM_SPECIFIC_USAGE",
               "DRAUGHTING_MODEL_ITEM_ASSOCIATION", "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER"}
DMIA_NAMES = {"DRAUGHTING_MODEL_ITEM_ASSOCIATION", "DRAUGHTING_MODEL_ITEM_ASSOCIATION_WITH_PLACEHOLDER"}


def is_presentation_name(n):
    return (n == "DRAUGHTING_CALLOUT" or n.endswith("_CALLOUT") or n.endswith("ANNOTATION_OCCURRENCE")
            or (n.startswith("ANNOTATION_") and n.endswith("_OCCURRENCE")))


def is_callout_name(n):
    return n == "DRAUGHTING_CALLOUT" or n.endswith("_CALLOUT")


def is_semantic_name(n):
    if is_presentation_name(n):
        return False
    if n in KIND or n in EXTRA_RELATED:
        return True
    if n == "DIMENSIONAL_EXPONENTS":
        return False
    if n.endswith("SHAPE_ASPECT"):
        return True
    return any(w in ("TOLERANCE", "DATUM", "DIMENSIONAL") for w in n.split("_"))


def kind_of(names):
    kinds = []
    for n in names:
        k = KIND.get(n)
        if k is not None and k not in kinds:
            kinds.append(k)
    if len(kinds) > 1 and "shape_aspect" in kinds:
        kinds.remove("shape_aspect")
    return kinds[0] if len(kinds) == 1 else "unknown"


def classify(inst):
    """None (not PMI), ('presentation',), ('semantic', kind) or ('unsupported', kind, reason)."""
    names = inst.names()
    has_p = any(is_presentation_name(n) for n in names)
    has_s = any(is_semantic_name(n) for n in names)
    if not has_p and not has_s:
        return None
    if has_p and has_s:
        return ("unsupported", kind_of(names), "mixed_presentation")
    if has_p:
        return ("presentation",)
    if any(n not in KIND for n in names):
        return ("unsupported", kind_of(names), "type_not_supported")
    kinds = set(KIND[n] for n in names)
    if len(kinds) > 1:
        kinds.discard("shape_aspect")
    if len(kinds) != 1:
        return ("unsupported", kind_of(names), "mixed_kinds")
    if sum(1 for n in names if n in TOLERANCE_TYPES) > 1:
        return ("unsupported", kind_of(names), "two_tolerance_types")
    return ("semantic", kinds.pop())


def attr(inst, root, i):
    """Attribute i of the part named root (complex form), or of the only part."""
    for name, params in inst.parts:
        if name == root:
            return params[i] if i < len(params) else None
    if len(inst.parts) == 1:
        params = inst.parts[0][1]
        return params[i] if i < len(params) else None
    return None


def walk(params):
    """Depth-first pre-order over parameters."""
    for p in params:
        yield p
        if isinstance(p, list):
            for q in walk(p):
                yield q
        elif isinstance(p, Typed):
            for q in walk([p.inner]):
                yield q


def is_measure(inst):
    return any(n.endswith("MEASURE_WITH_UNIT") for n in inst.names())


def unit_kind(inst):
    names = inst.names()
    if "LENGTH_UNIT" in names:
        return "length"
    if "PLANE_ANGLE_UNIT" in names:
        return "angle"
    for name, params in inst.parts:
        if name == "SI_UNIT" and params and isinstance(params[-1], Enum):
            if params[-1].v == "METRE":
                return "length"
            if params[-1].v == "RADIAN":
                return "angle"
    return None


PREFIX_SYMBOL = {None: "m", "CENTI": "cm", "MILLI": "mm", "MICRO": "um", "NANO": "nm"}


def context_units(data):
    """Per geometric context: (length symbol, angle name), from SI units only (fixtures use no others)."""
    out = {}
    for n, inst in data.items():
        if "GEOMETRIC_REPRESENTATION_CONTEXT" not in inst.names():
            continue
        lu, au = None, None
        for u in unit_list(inst):
            ui = data[u]
            si = [p for name, p in ui.parts if name == "SI_UNIT"][0]
            k = unit_kind(ui)
            if k == "length":
                prefix = si[0].v if isinstance(si[0], Enum) else None
                lu = PREFIX_SYMBOL[prefix]
            elif k == "angle":
                au = "radian"
        out[n] = (lu, au)
    return out


def unit_list(inst):
    for name, params in inst.parts:
        if name == "GLOBAL_UNIT_ASSIGNED_CONTEXT":
            for p in params:
                if isinstance(p, list):
                    return [x.n for x in p if isinstance(x, Ref)]
            return []
    return []


def oracle(data):
    classes = {}
    for n in sorted(data):
        c = classify(data[n])
        if c is not None:
            classes[n] = c
    ctx_units = context_units(data)
    unit_ctx = {}
    pairs = set()
    usages = {}
    assocs = []
    fwd, rev = {}, {}
    for n in sorted(data):
        if n in classes:
            continue
        inst = data[n]
        names = inst.names()
        if any(x in USAGE_NAMES for x in names):
            dfn = attr(inst, "ITEM_IDENTIFIED_REPRESENTATION_USAGE", 2).n
            it = attr(inst, "ITEM_IDENTIFIED_REPRESENTATION_USAGE", 4)
            items = [x.n for x in it] if isinstance(it, list) else [it.n]
            if any(x in DMIA_NAMES for x in names):
                assocs.append((dfn, items))
            else:
                usages.setdefault(dfn, []).extend(items)
        elif "SHAPE_ASPECT_RELATIONSHIP" in names:
            a = attr(inst, "SHAPE_ASPECT_RELATIONSHIP", 2).n
            b = attr(inst, "SHAPE_ASPECT_RELATIONSHIP", 3).n
            fwd.setdefault(a, []).append(b)
            rev.setdefault(b, []).append(a)
        if "GLOBAL_UNIT_ASSIGNED_CONTEXT" in names:
            for u in unit_list(inst):
                pairs.add((n, u))
                if u not in unit_ctx or n < unit_ctx[u]:
                    unit_ctx[u] = n

    items = {}
    for n, c in classes.items():
        items[n] = {"class": c[0], "kind": c[1] if len(c) > 1 else "presentation",
                    "reason": c[2] if len(c) > 2 else "-", "types": data[n].names(),
                    "links": set(), "unresolved": set(), "refs": set(), "presented_by": set(), "values": []}

    def value(item, m, ctx_given, ctx):
        if any(v[0] == m for v in item["values"]):
            return
        mi = data[m]
        vp = attr(mi, "MEASURE_WITH_UNIT", 0)
        up = attr(mi, "MEASURE_WITH_UNIT", 1)
        mtype, lex = "-", "-"
        inner = vp
        if isinstance(vp, Typed):
            mtype, inner = vp.name, vp.inner
        if isinstance(inner, Real):
            lex = inner.lexeme
        elif isinstance(inner, Int):
            lex = str(inner.v)
        u = up.n
        if u not in data:
            item["unresolved"].add(u)
        if not ctx_given:
            ctx = unit_ctx.get(u)
        unit = "unresolved"
        if u in data and ctx is not None and (ctx, u) in pairs and ctx in ctx_units:
            k = unit_kind(data[u])
            lu, au = ctx_units[ctx]
            if k == "length" and lu is not None:
                unit = lu
            elif k == "angle" and au is not None:
                unit = au
        item["values"].append((m, mtype, lex, u, ctx, unit))

    for n, c in classes.items():
        if c[0] == "presentation":
            continue
        inst = data[n]
        item = items[n]
        for p in walk([q for _, ps in inst.parts for q in ps]):
            if not isinstance(p, Ref):
                continue
            r = p.n
            if r not in data:
                item["unresolved"].add(r)
            elif r in classes:
                item["refs"].add(r)
            elif c[0] == "semantic" and is_measure(data[r]):
                value(item, r, False, None)
        if c[0] != "semantic":
            continue
        if c[1] == "dimension_representation":
            rep = attr(inst, "DIMENSIONAL_CHARACTERISTIC_REPRESENTATION", 1).n
            if rep in data:
                ri = data[rep]
                ctxp = attr(ri, "REPRESENTATION", 2)
                ctx = ctxp.n if isinstance(ctxp, Ref) else None
                for x in attr(ri, "REPRESENTATION", 1):
                    if x.n not in data:
                        item["unresolved"].add(x.n)
                    elif is_measure(data[x.n]):
                        value(item, x.n, True, ctx)
        if c[1] in GEOMETRY_KINDS:
            seen = set()
            todo = [n]
            while todo:
                a = todo.pop()
                if a in seen:
                    continue
                seen.add(a)
                if a not in data:
                    item["unresolved"].add(a)
                    continue
                for x in usages.get(a, []):
                    (item["links"] if x in data else item["unresolved"]).add(x)
                ca = classes.get(a)
                if ca is None or ca[0] != "semantic":
                    continue
                ai = data[a]
                if ca[1] == "tolerance":
                    todo.append(attr(ai, "GEOMETRIC_TOLERANCE", 3).n)
                elif ca[1] == "dimension":
                    if "DIMENSIONAL_SIZE" in ai.names():
                        todo.append(attr(ai, "DIMENSIONAL_SIZE", 0).n)
                    else:
                        todo.append(attr(ai, "SHAPE_ASPECT_RELATIONSHIP", 2).n)
                        todo.append(attr(ai, "SHAPE_ASPECT_RELATIONSHIP", 3).n)
                if any(x in COMPOSITE for x in ai.names()):
                    todo.extend(fwd.get(a, []))
                if ca[1] == "datum":
                    todo.extend(rev.get(a, []))

    def link(p, dfn):
        items[p]["refs"].add(dfn)
        items[dfn]["presented_by"].add(p)

    for dfn, targets in assocs:
        # A definition counts only when it is a characteristic: a supported or unsupported
        # semantic item whose kind is not shape_aspect or unknown, and not mixed_presentation.
        c = classes.get(dfn)
        d_is_sem = (c is not None and c[0] in ("semantic", "unsupported") and c[1] not in ("shape_aspect", "unknown")
                    and (len(c) < 3 or c[2] != "mixed_presentation"))
        for p in targets:
            if p not in data:
                if d_is_sem:
                    items[dfn]["unresolved"].add(p)
                continue
            if p not in items or classes[p][0] != "presentation":
                continue
            if dfn not in data:
                items[p]["unresolved"].add(dfn)
                continue
            if not d_is_sem:
                continue
            link(p, dfn)
            if any(is_callout_name(x) for x in data[p].names()):
                for m in attr(data[p], "DRAUGHTING_CALLOUT", 1):
                    if m.n not in data:
                        items[p]["unresolved"].add(m.n)
                    elif m.n in items and classes[m.n][0] == "presentation":
                        link(m.n, dfn)

    for n, c in classes.items():
        if c[0] == "presentation":
            items[n]["class"] = "semantic" if items[n]["refs"] else "graphic_only"
    return items


def fmt_set(s):
    return ",".join(str(x) for x in sorted(s)) if s else "-"


def fmt_value(v):
    m, mtype, lex, u, ctx, unit = v
    return "{}:{}:{}:{}:{}:{}".format(m, mtype, lex, "-" if u is None else u, "-" if ctx is None else ctx, unit)


# ---------------------------------------------------------------- independent text scan (AC-5)


def top_level_names(body):
    """Entity names of one instance body (after '='), at the top level only."""
    names = []
    i = 0
    depth = 0
    in_str = False
    start = None
    complex_form = body.startswith("(")
    want = 1 if complex_form else 0
    while i < len(body):
        ch = body[i]
        if in_str:
            if ch == "'":
                if i + 1 < len(body) and body[i + 1] == "'":
                    i += 2
                    continue
                in_str = False
        elif ch == "'":
            in_str = True
        elif ch == "(":
            if depth == want and start is not None:
                names.append(body[start:i])
            start = None
            depth += 1
        elif ch == ")":
            depth -= 1
        elif depth == want and (ch.isalnum() or ch in "_!"):
            if start is None:
                start = i
        i += 1
    return names


def scan_count(text):
    data = text.split("DATA;", 1)[1]
    count = 0
    for m in re.finditer(r"#(\d+)=(.*?);\n", data):
        names = top_level_names(m.group(2))
        if any(is_presentation_name(n) or is_semantic_name(n) for n in names):
            count += 1
    return count


# ---------------------------------------------------------------- output


def expected(name, text, data):
    items = oracle(data)
    out = ["== " + name]
    for n in sorted(items):
        it = items[n]
        vals = ";".join(fmt_value(v) for v in it["values"]) if it["values"] else "-"
        out.append("item {} class={} kind={} reason={} types={} links={} unresolved={} refs={} presented_by={} "
                   "values={}".format(n, it["class"], it["kind"], it["reason"], "+".join(it["types"]),
                                      fmt_set(it["links"]), fmt_set(it["unresolved"]), fmt_set(it["refs"]),
                                      fmt_set(it["presented_by"]), vals))
    out.append("count {}".format(len(items)))
    out.append("scan {}".format(scan_count(text)))
    return out


def main():
    check = "--check" in sys.argv[1:]
    outputs = {}
    lines = [
        "# Expected results for the IO.CAD.PMI.READ fixtures, written by make_cases.py.",
        "# Block: '== <fixture>', one 'item' line per graph item, then 'count' (graph items) and",
        "# 'scan' (PMI-related instances found by an independent top-level name scan of the text).",
        "# value: <measure>:<measure type>:<lexeme>:<unit entity>:<unit context>:<unit>.",
    ]
    for name in sorted(FIXTURES):
        note, data = FIXTURES[name]
        text = step_file(name, note, data)
        outputs[name] = text
        lines += expected(name, text, data)
    outputs["expected.txt"] = "\n".join(lines) + "\n"
    stale = []
    for name, text in outputs.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != text.encode("utf-8"):
                stale.append(name)
        else:
            path.write_bytes(text.encode("utf-8"))
    if check and stale:
        print("stale fixtures (run make_cases.py): " + ", ".join(stale))
        return 1
    if not check:
        print("wrote {} files".format(len(outputs)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
