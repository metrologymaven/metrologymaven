#!/usr/bin/env python3
"""Fixture generator and independent oracle for spec node IO.CAD.STEP.HDR (Metrology Maven).

CC-BY-4.0 project data. Every fixture is synthetic: written by this script, not copied
from any exporter, dataset or standard. Schema identifiers are names only; no standards
text is reproduced.

The oracle below is written separately from the Rust crate fid-io-step, from the leaf's
rules (spec/io/cad/step/hdr/NODE.md, including its "Pinned at build" section):

- FILE_SCHEMA must hold exactly one identifier. After trimming ASCII whitespace and
  upper-casing ASCII, it is accepted when it starts with one of the table prefixes and the
  next character is the end, a space, '{' or '_'.
- A context is every data instance with a GEOMETRIC_REPRESENTATION_CONTEXT part. It owns
  geometry when another instance refers to it.
- Length units: SI metre with prefix none/centi/milli/micro/nano; conversion-based INCH or
  FOOT (any ASCII case) over an SI length unit, with the exact factor 0.0254 / 0.3048 and a
  warning when value * base factor differs by more than 1e-12 relative. Plane angle:
  SI radian, or conversion-based DEGREE over SI radian (warning beyond 1e-6 relative).
- Uncertainty: the first GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT entry whose unit is a length
  unit; its authored lexeme and its unit's exact factor are recorded.
- Effective unit per context: the IO.REG.PROV resolution table (declared / caller /
  override, unit_required, unit_conflict). Under an override, an uncertainty written in the
  context's declared length unit moves to the effective unit (same number) and the report
  gets a step_uncertainty_reexpressed warning.
- A part the reader reads (SI_UNIT, CONVERSION_BASED_UNIT, LENGTH_UNIT, PLANE_ANGLE_UNIT,
  GEOMETRIC_REPRESENTATION_CONTEXT, the two GLOBAL_*_ASSIGNED_CONTEXT parts, a
  *MEASURE_WITH_UNIT part, or a second value-carrying measure part) appearing twice in one
  instance is malformed (duplicate_part). A zero uncertainty of -0. reads as +0.
- Provenance: the record's unit, its source and unit_overridden (true when any context
  owning geometry, or the primary context, got its unit from the caller), and the unit
  items step.contexts, step.unitless_contexts, step.units.mixed, step.units.group_<k>
  (grouped on the parsed uncertainty value and unit, never the authored token) and
  step.units.not_declared[_<k>].

It writes the fixtures *.stp and expected.txt (one block per case).

Run (stdlib only, Python 3.8+):
  python fixtures/io/cad/step/hdr/make_cases.py           # rewrite the fixtures
  python fixtures/io/cad/step/hdr/make_cases.py --check   # fail if any file is stale
"""

import re
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent

# ---------------------------------------------------------------- fixture text

NOTE = "/* Synthetic fixture for spec node IO.CAD.STEP.HDR (Metrology Maven), CC-BY-4.0. {} */"

SCHEMA_242 = "AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }"
SCHEMA_214 = "AUTOMOTIVE_DESIGN { 1 0 10303 214 1 1 1 1 }"
SCHEMA_203 = "CONFIG_CONTROL_DESIGN"
SCHEMA_203E2 = "AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES_MIM_LF"
SCHEMA_209 = "AP209_MULTIDISCIPLINARY_ANALYSIS_AND_DESIGN_MIM_LF"


def step_file(name, note, schema_param, body, system="fid synthetic generator", file_name=True):
    lines = [
        "ISO-10303-21;",
        "HEADER;",
        NOTE.format(note),
        "FILE_DESCRIPTION(('synthetic header fixture'),'2;1');",
    ]
    if file_name:
        lines.append(
            "FILE_NAME('{}','2026-11-20T00:00:00',('Fixture Author','Second Author'),('Metrology Maven'),"
            "'make_cases.py','{}','');".format(name, system)
        )
    if schema_param is not None:
        lines.append("FILE_SCHEMA({});".format(schema_param))
    lines += ["ENDSEC;", "DATA;"]
    lines += body
    lines += ["ENDSEC;", "END-ISO-10303-21;", ""]
    return "\n".join(lines)


def one(schema):
    return "('{}')".format(schema)


MM = "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));"
RAD = "#11=(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.));"
SR = "#12=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT());"
DIMS = "#22=DIMENSIONAL_EXPONENTS(1.,0.,0.,0.,0.,0.,0.);"
POINT = "#1=CARTESIAN_POINT('origin',(0.,0.,0.));"


def ctx(n, units, unc=None):
    u = ",".join("#{}".format(x) for x in units)
    if unc is None:
        return "#{}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT(({}))" \
               "REPRESENTATION_CONTEXT('ctx {}','synthetic'));".format(n, u, n)
    c = ",".join("#{}".format(x) for x in unc)
    return "#{}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({}))" \
           "GLOBAL_UNIT_ASSIGNED_CONTEXT(({}))REPRESENTATION_CONTEXT('ctx {}','synthetic'));".format(n, c, u, n)


def shape(n, ctx_n):
    return "#{}=SHAPE_REPRESENTATION('shape',(#1),#{});".format(n, ctx_n)


FIXTURES = {}

FIXTURES["ap242_mm.stp"] = step_file("ap242_mm.stp", "AC-1: AP242, millimetre, uncertainty 1e-6.", one(SCHEMA_242), [
    POINT, MM, RAD, SR,
    "#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10,'distance_accuracy_value','synthetic');",
    ctx(14, [10, 11, 12], [13]),
    shape(20, 14),
])

FIXTURES["ap214_inch.stp"] = step_file("ap214_inch.stp", "AC-2: AP214, conversion-based inch over mm, degree.",
                                       one(SCHEMA_214), [
    POINT, MM, RAD, SR,
    "#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0001),#23,'distance_accuracy_value','synthetic');",
    "#21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#10);",
    DIMS,
    "#23=(CONVERSION_BASED_UNIT('INCH',#21)LENGTH_UNIT()NAMED_UNIT(#22));",
    "#24=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.0174532925199433),#11);",
    "#25=(CONVERSION_BASED_UNIT('DEGREE',#24)NAMED_UNIT(#22)PLANE_ANGLE_UNIT());",
    ctx(30, [23, 25, 12], [13]),
    shape(40, 30),
])

FIXTURES["ap209.stp"] = step_file("ap209.stp", "AC-3: schema outside the accepted list.", one(SCHEMA_209), [
    POINT, MM, RAD, ctx(14, [10, 11]), shape(20, 14),
])

FIXTURES["no_unit.stp"] = step_file("no_unit.stp", "AC-4: a context owning geometry declares no length unit.",
                                    one(SCHEMA_242), [
    POINT, RAD, SR, ctx(14, [11, 12]), shape(20, 14),
])

FIXTURES["ap203_cm_deg.stp"] = step_file("ap203_cm_deg.stp", "AP203 first edition, centimetre, simple-form SI units.",
                                         "('  config_control_design  ')", [
    POINT,
    "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.CENTI.,.METRE.));",
    "#11=SI_UNIT(*,$,.RADIAN.);",
    "#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(5),#10,'distance_accuracy_value','integer value');",
    ctx(14, [10, 11], [13]),
    shape(20, 14),
])

FIXTURES["ap203e2_foot_warn.stp"] = step_file(
    "ap203e2_foot_warn.stp", "AP203 second edition, foot whose file factor is off, degree off.", one(SCHEMA_203E2), [
        POINT,
        "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));",
        RAD,
        "#21=(LENGTH_MEASURE_WITH_UNIT()MEASURE_WITH_UNIT(LENGTH_MEASURE(0.30481),#10));",
        DIMS,
        "#23=(CONVERSION_BASED_UNIT('foot',#21)LENGTH_UNIT()NAMED_UNIT(#22));",
        "#24=PLANE_ANGLE_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(0.01745),#11);",
        "#25=(CONVERSION_BASED_UNIT('Degree',#24)NAMED_UNIT(#22)PLANE_ANGLE_UNIT());",
        "#26=(LENGTH_MEASURE_WITH_UNIT()MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-05),#23)"
        "UNCERTAINTY_MEASURE_WITH_UNIT('distance_accuracy_value','complex form'));",
        ctx(30, [23, 25], [26]),
        shape(40, 30),
    ])

FIXTURES["multi_context.stp"] = step_file(
    "multi_context.stp", "Three contexts: mm, micrometre, and an unreferenced one without units.",
    "('ap242_managed_model_based_3d_engineering_mim_lf')", [
        POINT, MM, RAD,
        "#15=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));",
        "#16=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.NANO.,.METRE.));",
        "#17=UNCERTAINTY_MEASURE_WITH_UNIT(PLANE_ANGLE_MEASURE(1.E-03),#11,'angle','not a length');",
        "#18=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(2.5E-02),#16,'distance_accuracy_value','in nm');",
        ctx(30, [10, 11]),
        ctx(31, [15, 11], [17, 18]),
        "#32=GEOMETRIC_REPRESENTATION_CONTEXT('unused','no units',3);",
        shape(40, 30),
        shape(41, 31),
    ])

# ----- error fixtures

FIXTURES["evil_schema.stp"] = step_file(
    "evil_schema.stp", "Schema identifier with a right-to-left override and a control character.",
    "('AP242\\X2\\202E\\X0\\EVIL\\X\\07')",[POINT, MM, ctx(14, [10]), shape(20, 14)],
    system="sys\\X2\\202E\\X0\\tem")

FIXTURES["prefix_trap.stp"] = step_file(
    "prefix_trap.stp", "An identifier that only shares a prefix with an accepted one.",
    one("AUTOMOTIVE_DESIGNER_SCHEMA"), [POINT, MM, ctx(14, [10]), shape(20, 14)])

FIXTURES["two_schemas.stp"] = step_file(
    "two_schemas.stp", "FILE_SCHEMA with two identifiers.",
    "('{}','{}')".format(SCHEMA_214, SCHEMA_242), [POINT, MM, ctx(14, [10]), shape(20, 14)])

FIXTURES["no_schema.stp"] = step_file("no_schema.stp", "No FILE_SCHEMA record.", None,
                                      [POINT, MM, ctx(14, [10]), shape(20, 14)])

FIXTURES["no_file_name.stp"] = step_file("no_file_name.stp", "No FILE_NAME record.", one(SCHEMA_242),
                                         [POINT, MM, ctx(14, [10]), shape(20, 14)], file_name=False)

FIXTURES["yard.stp"] = step_file("yard.stp", "Conversion-based length unit with an unsupported name.", one(SCHEMA_242), [
    POINT, MM,
    "#21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(914.4),#10);",
    DIMS,
    "#23=(CONVERSION_BASED_UNIT('YARD',#21)LENGTH_UNIT()NAMED_UNIT(#22));",
    ctx(30, [23]), shape(40, 30),
])

FIXTURES["kilo.stp"] = step_file("kilo.stp", "SI length unit with an unsupported prefix.", one(SCHEMA_242), [
    POINT, "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.KILO.,.METRE.));", ctx(14, [10]), shape(20, 14),
])

FIXTURES["nested_conversion.stp"] = step_file(
    "nested_conversion.stp", "Inch defined over another conversion-based unit (and a cycle).", one(SCHEMA_242), [
        POINT,
        "#21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),#23);",
        DIMS,
        "#23=(CONVERSION_BASED_UNIT('INCH',#21)LENGTH_UNIT()NAMED_UNIT(#22));",
        ctx(30, [23]), shape(40, 30),
    ])

FIXTURES["dangling_unit.stp"] = step_file("dangling_unit.stp", "A unit reference to an undefined instance.",
                                          one(SCHEMA_242), [POINT, MM, ctx(14, [10, 99]), shape(20, 14)])

FIXTURES["two_lengths.stp"] = step_file("two_lengths.stp", "Two length units in one context.", one(SCHEMA_242), [
    POINT, MM, "#15=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));", ctx(14, [10, 15]), shape(20, 14),
])

FIXTURES["negative_factor.stp"] = step_file("negative_factor.stp", "Inch with a negative conversion factor.",
                                            one(SCHEMA_242), [
    POINT, MM,
    "#21=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(-25.4),#10);",
    DIMS,
    "#23=(CONVERSION_BASED_UNIT('INCH',#21)LENGTH_UNIT()NAMED_UNIT(#22));",
    ctx(30, [23]), shape(40, 30),
])

FIXTURES["wrong_types.stp"] = step_file("wrong_types.stp", "SI_UNIT whose name is a string, not an enumeration.",
                                        one(SCHEMA_242), [
    POINT, "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,'METRE'));", ctx(14, [10]), shape(20, 14),
])

FIXTURES["partial_unit.stp"] = step_file(
    "partial_unit.stp", "Two geometry contexts: #30 declares mm, #31 declares no length unit.", one(SCHEMA_242), [
        POINT, MM, RAD,
        "#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-03),#10,'distance_accuracy_value','synthetic');",
        ctx(30, [10, 11], [13]),
        ctx(31, [11]),
        shape(40, 30),
        shape(41, 31),
    ])

FIXTURES["uncertainty_tokens.stp"] = step_file(
    "uncertainty_tokens.stp", "Four mm contexts whose uncertainties differ only in how they are written.",
    one(SCHEMA_242), [
        POINT, MM,
        "#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10,'distance_accuracy_value','a');",
        "#14=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0000010),#10,'distance_accuracy_value','b');",
        "#15=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(-0.),#10,'distance_accuracy_value','c');",
        "#16=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0),#10,'distance_accuracy_value','d');",
        ctx(30, [10], [13]),
        ctx(31, [10], [14]),
        ctx(32, [10], [15]),
        ctx(33, [10], [16]),
        shape(40, 30), shape(41, 31), shape(42, 32), shape(43, 33),
    ])

FIXTURES["dup_si_part.stp"] = step_file("dup_si_part.stp", "A length unit with two SI_UNIT parts.", one(SCHEMA_242), [
    POINT, "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.)SI_UNIT($,.METRE.));", ctx(14, [10]), shape(20, 14),
])

FIXTURES["dup_unit_context.stp"] = step_file(
    "dup_unit_context.stp", "A context with two GLOBAL_UNIT_ASSIGNED_CONTEXT parts.", one(SCHEMA_242), [
        POINT, MM, "#15=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));",
        "#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))"
        "GLOBAL_UNIT_ASSIGNED_CONTEXT((#15))REPRESENTATION_CONTEXT('ctx 14','synthetic'));",
        shape(20, 14),
    ])

FIXTURES["dup_measure.stp"] = step_file(
    "dup_measure.stp", "An uncertainty with two value-carrying measure parts.", one(SCHEMA_242), [
        POINT, MM,
        "#13=(LENGTH_MEASURE_WITH_UNIT()MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10)"
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-03),#10,'distance_accuracy_value','second value'));",
        ctx(14, [10], [13]), shape(20, 14),
    ])

# (fixture, caller unit or None, override_unit)
CASES = [
    ("ap242_mm.stp", None, False),
    ("ap242_mm.stp", "mm", False),
    ("ap242_mm.stp", "in", False),
    ("ap242_mm.stp", "in", True),
    ("ap214_inch.stp", None, False),
    ("ap214_inch.stp", "mm", True),
    ("ap209.stp", None, False),
    ("no_unit.stp", None, False),
    ("no_unit.stp", "mm", False),
    ("no_unit.stp", None, True),
    ("ap203_cm_deg.stp", None, False),
    ("ap203e2_foot_warn.stp", None, False),
    ("multi_context.stp", None, False),
    ("multi_context.stp", "ft", False),
    ("multi_context.stp", "mm", True),
    ("partial_unit.stp", None, False),
    ("partial_unit.stp", "mm", False),
    ("partial_unit.stp", "in", True),
    ("uncertainty_tokens.stp", None, False),
    ("dup_si_part.stp", None, False),
    ("dup_unit_context.stp", None, False),
    ("dup_measure.stp", None, False),
    ("evil_schema.stp", None, False),
    ("prefix_trap.stp", None, False),
    ("two_schemas.stp", None, False),
    ("no_schema.stp", None, False),
    ("no_file_name.stp", None, False),
    ("yard.stp", None, False),
    ("kilo.stp", None, False),
    ("nested_conversion.stp", None, False),
    ("dangling_unit.stp", None, False),
    ("two_lengths.stp", None, False),
    ("negative_factor.stp", None, False),
    ("wrong_types.stp", None, False),
]

# ---------------------------------------------------------------- reader

class Bad(Exception):
    def __init__(self, code, entity=None, raw=None):
        super().__init__(code)
        self.code = code
        self.entity = entity
        self.raw = raw


def decode_string(body):
    out = []
    i = 0
    while i < len(body):
        if body.startswith("''", i):
            out.append("'")
            i += 2
        elif body.startswith("\\\\", i):
            out.append("\\")
            i += 2
        elif body.startswith("\\X2\\", i):
            j = body.index("\\X0\\", i)
            hexs = body[i + 4:j]
            out.append("".join(chr(int(hexs[k:k + 4], 16)) for k in range(0, len(hexs), 4)))
            i = j + 4
        elif body.startswith("\\X\\", i):
            out.append(chr(int(body[i + 3:i + 5], 16)))
            i += 5
        else:
            out.append(body[i])
            i += 1
    return "".join(out)


TOKEN = re.compile(r"""
    (?P<ws>\s+|/\*.*?\*/)
  | (?P<str>'(?:[^']|'')*')
  | (?P<ref>\#[0-9]+)
  | (?P<enum>\.[A-Z_][A-Z0-9_]*\.)
  | (?P<real>[+-]?[0-9]+\.[0-9]*(?:[Ee][+-]?[0-9]+)?)
  | (?P<int>[+-]?[0-9]+)
  | (?P<kw>!?[A-Za-z_][A-Za-z0-9_-]*)
  | (?P<p>[(),;=$*])
""", re.X | re.S)


def tokens(text):
    out = []
    i = 0
    while i < len(text):
        m = TOKEN.match(text, i)
        if not m:
            raise ValueError("lex at {}".format(i))
        i = m.end()
        kind = m.lastgroup
        if kind != "ws":
            out.append((kind, m.group()))
    return out


class P:
    def __init__(self, toks):
        self.t = toks
        self.i = 0

    def peek(self):
        return self.t[self.i]

    def take(self, want=None):
        tok = self.t[self.i]
        self.i += 1
        if want is not None and tok[1] != want:
            raise ValueError("want {} got {}".format(want, tok))
        return tok

    def param(self):
        kind, s = self.take()
        if kind == "str":
            return ("str", decode_string(s[1:-1]))
        if kind == "ref":
            return ("ref", int(s[1:]))
        if kind == "enum":
            return ("enum", s[1:-1])
        if kind == "real":
            return ("real", float(s), s)
        if kind == "int":
            return ("int", int(s))
        if s == "$":
            return ("unset",)
        if s == "*":
            return ("derived",)
        if s == "(":
            items = []
            if self.peek()[1] == ")":
                self.take()
                return ("list", items)
            while True:
                items.append(self.param())
                if self.take()[1] == ")":
                    return ("list", items)
        if kind == "kw":
            self.take("(")
            inner = self.param()
            self.take(")")
            return ("typed", s.upper(), inner)
        raise ValueError("param {}".format(s))

    def record(self):
        name = self.take()[1].upper()
        self.take("(")
        params = []
        if self.peek()[1] == ")":
            self.take()
            return (name, params)
        while True:
            params.append(self.param())
            if self.take()[1] == ")":
                return (name, params)


def read_file(text):
    p = P(tokens(text))
    p.take("ISO-10303-21")
    p.take(";")
    p.take("HEADER")
    p.take(";")
    header = []
    while p.peek()[1] != "ENDSEC":
        header.append(p.record())
        p.take(";")
    p.take("ENDSEC")
    p.take(";")
    p.take("DATA")
    p.take(";")
    data = {}
    while p.peek()[1] != "ENDSEC":
        n = int(p.take()[1][1:])
        p.take("=")
        if p.peek()[1] == "(":
            p.take()
            parts = []
            while p.peek()[1] != ")":
                parts.append(p.record())
            p.take(")")
        else:
            parts = [p.record()]
        p.take(";")
        data[n] = parts
    return header, data

# ---------------------------------------------------------------- oracle

SCHEMAS = [
    ("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING", "ap242"),
    ("AUTOMOTIVE_DESIGN", "ap214"),
    ("CONFIG_CONTROL_DESIGN", "ap203"),
    ("AP203_CONFIGURATION_CONTROLLED_3D_DESIGN_OF_MECHANICAL_PARTS_AND_ASSEMBLIES", "ap203"),
]

# symbol -> exact factor to metres
EXACT = {
    "m": Fraction(1), "cm": Fraction(1, 100), "mm": Fraction(1, 1000), "um": Fraction(1, 10**6),
    "nm": Fraction(1, 10**9), "in": Fraction(254, 10000), "ft": Fraction(3048, 10000),
}
SI_PREFIX = {None: "m", "CENTI": "cm", "MILLI": "mm", "MICRO": "um", "NANO": "nm"}
CONV_LENGTH = {"INCH": "in", "FOOT": "ft"}


def bits(x):
    return struct.pack(">d", x).hex()


def match_schema(s):
    t = s.strip(" \t\r\n").upper()
    for prefix, sch in SCHEMAS:
        if t.startswith(prefix):
            rest = t[len(prefix):]
            if rest == "" or rest[0] in " {_":
                return sch
    return None


def header_record(header, name):
    found = [r for r in header if r[0] == name]
    if not found:
        raise Bad("malformed")
    return found[0][1]


def part(parts, name, entity=None):
    found = [ps for n, ps in parts if n == name]
    if len(found) > 1:
        raise Bad("malformed", entity)
    return found[0] if found else None


def si_of(n, parts):
    ps = part(parts, "SI_UNIT")
    if len(ps) == 3:
        ps = ps[1:]
    prefix, name = ps
    if prefix[0] == "unset":
        prefix = None
    elif prefix[0] == "enum":
        prefix = prefix[1]
    else:
        raise Bad("malformed", n)
    if name[0] != "enum":
        raise Bad("malformed", n)
    return prefix, name[1]


def measure(n, data):
    if n not in data:
        raise Bad("malformed", n)
    names = [name for name, _ in data[n] if name.endswith("MEASURE_WITH_UNIT")]
    if len(set(names)) != len(names):
        raise Bad("malformed", n)
    carrying = [ps for name, ps in data[n]
                if name.endswith("MEASURE_WITH_UNIT") and len(ps) >= 2 and ps[1][0] == "ref"]
    if len(carrying) != 1:
        raise Bad("malformed", n)
    ps = carrying[0]
    v = ps[0]
    if v[0] == "typed":
        v = v[2]
    if v[0] == "real":
        value, lexeme = v[1], v[2]
    elif v[0] == "int":
        value, lexeme = float(v[1]), str(v[1])
    else:
        raise Bad("malformed", n)
    if value == 0:
        value = 0.0  # -0. reads as +0.
    return value, lexeme, ps[1][1]


def classify(n, data, warnings, nested=False):
    """('length', symbol) | ('angle', 'radian'|'degree') | ('other',)"""
    if n not in data:
        raise Bad("malformed", n)
    parts = data[n]
    names = [p[0] for p in parts]
    for read in ("SI_UNIT", "CONVERSION_BASED_UNIT", "LENGTH_UNIT", "PLANE_ANGLE_UNIT"):
        if names.count(read) > 1:
            raise Bad("malformed", n)
    is_len = "LENGTH_UNIT" in names
    is_ang = "PLANE_ANGLE_UNIT" in names
    if "SI_UNIT" in names:
        prefix, name = si_of(n, parts)
        if name == "METRE":
            if prefix not in SI_PREFIX:
                raise Bad("step_unit_unsupported", n)
            return ("length", SI_PREFIX[prefix])
        if name == "RADIAN":
            if prefix is not None:
                raise Bad("step_unit_unsupported", n)
            return ("angle", "radian")
        if is_len or is_ang:
            raise Bad("malformed", n)
        return ("other",)
    ps = part(parts, "CONVERSION_BASED_UNIT")
    if ps is not None:
        if not (is_len or is_ang):
            return ("other",)
        if nested:
            raise Bad("malformed", n)
        if len(ps) == 3:
            ps = ps[1:]
        name, factor = ps
        if name[0] != "str" or factor[0] != "ref":
            raise Bad("malformed", n)
        word = name[1].upper()
        value, lexeme, base_n = measure(factor[1], data)
        base = classify(base_n, data, warnings, nested=True)
        if is_len:
            if word not in CONV_LENGTH:
                raise Bad("step_unit_unsupported", n)
            sym = CONV_LENGTH[word]
            if base[0] != "length":
                raise Bad("malformed", n)
            if not value > 0:
                raise Bad("malformed", factor[1])
            file_factor = value * float(EXACT[base[1]])
            exact = float(EXACT[sym])
            if abs(file_factor - exact) / exact > 1e-12:
                warnings.add((n, "length_factor_mismatch"))
            return ("length", sym)
        if word != "DEGREE":
            raise Bad("step_unit_unsupported", n)
        if base != ("angle", "radian"):
            raise Bad("malformed", n)
        if not value > 0:
            raise Bad("malformed", factor[1])
        exact = 0.017453292519943295
        if abs(value - exact) / exact > 1e-6:
            warnings.add((n, "angle_factor_mismatch"))
        return ("angle", "degree")
    if is_len or is_ang:
        raise Bad("malformed", n)
    return ("other",)


def refs_in(params):
    stack = list(params)
    while stack:
        p = stack.pop()
        if p[0] == "ref":
            yield p[1]
        elif p[0] == "list":
            stack.extend(p[1])
        elif p[0] == "typed":
            stack.append(p[2])


def resolve(text, given, override):
    header, data = read_file(text)
    fs = header_record(header, "FILE_SCHEMA")
    ids = fs[0][1]
    if len(ids) != 1:
        raise Bad("malformed")
    raw = ids[0][1]
    schema = match_schema(raw)
    if schema is None:
        raise Bad("step_schema_unsupported", raw=raw)
    header_record(header, "FILE_DESCRIPTION")
    fname = header_record(header, "FILE_NAME")
    system = fname[5][1]
    if override and given is None:
        raise Bad("unit_required")
    contexts = []
    for n in sorted(data):
        if part(data[n], "GEOMETRIC_REPRESENTATION_CONTEXT", n) is not None:
            contexts.append(n)
    owned = set()
    for n, parts in data.items():
        for _, ps in parts:
            for r in refs_in(ps):
                if r in contexts and r != n:
                    owned.add(r)
    warnings = set()
    out = []
    unitless = []
    for c in contexts:
        parts = data[c]
        length = angle = None
        gu = part(parts, "GLOBAL_UNIT_ASSIGNED_CONTEXT", c)
        gc = part(parts, "GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT", c)
        for item in (gu[0][1] if gu else []):
            k = classify(item[1], data, warnings)
            if k[0] == "length":
                if length is not None:
                    raise Bad("malformed", c)
                length = k[1]
            elif k[0] == "angle":
                if angle is not None:
                    raise Bad("malformed", c)
                angle = k[1]
        unc = None
        for item in (gc[0][1] if gc else []):
            value, lexeme, un = measure(item[1], data)
            k = classify(un, data, warnings)
            if k[0] == "length":
                if value < 0:
                    raise Bad("malformed", item[1])
                unc = (lexeme, value, k[1])
                break
        geometry = c in owned
        if length is None and given is None:
            if geometry:
                raise Bad("unit_required", c)
            unitless.append(c)
            continue
        if length is None:
            eff, source = given, "caller"
        elif given is None or given == length:
            eff, source = length, "declared"
        elif not override:
            raise Bad("unit_conflict", c)
        else:
            eff, source = given, "override"
        if source == "override" and unc is not None and unc[2] == length:
            unc = (unc[0], unc[1], eff)
            warnings.add((c, "step_uncertainty_reexpressed"))
        out.append((c, eff, angle, unc, source, geometry))
    lines = ["schema {}".format(schema), "system {}".format(system.encode("utf-8").hex())]
    for c, eff, angle, unc, source, geometry in out:
        u = "none" if unc is None else "{} {}".format(unc[0], bits(float(EXACT[unc[2]])))
        lines.append("context {} unit {} factor {} angle {} uncertainty {} source {} geometry {}".format(
            c, eff, bits(float(EXACT[eff])), angle or "none", u, source, "true" if geometry else "false"))
    for c in unitless:
        lines.append("unitless {}".format(c))
    for n, code in sorted(warnings):
        lines.append("warning {} {}".format(n, code))
    lines += provenance(out, unitless, given, override)
    return lines


# ---------------------------------------------------------------- provenance oracle

DECIMAL = {"m": "1", "cm": "0.01", "mm": "0.001", "um": "0.000001", "nm": "0.000000001",
           "in": "0.0254", "ft": "0.3048", "thou": "0.0000254"}
MAX_DECLARED = 256
MAX_TEXT_BYTES = 4096
HEADER_ITEMS = 11  # step.schema .. step.implementation_level


def shortest(x):
    """The shortest round-trip decimal of a non-negative finite float, written the way the
    leaf pins it: fixed with at least one fractional digit for 1e-4 <= x < 1e16 (and 0),
    else '<mantissa>e<exponent>' with no '+' and no leading zeros in the exponent."""
    r = repr(x)
    if "e" in r:
        mant, exp = r.split("e")
        return "{}e{}".format(mant, int(exp))
    return r


def provenance(out, unitless, given, override):
    # out rows: (context, effective, angle, (lexeme, value, unit) | None, source, geometry)
    primary = next((r for r in out if r[5]), out[0] if out else None)
    affected = [r for r in out if r[4] != "declared" and (r[5] or r is primary)]
    try:
        if affected:
            g = affected[0][1]
            if g == "cm":
                raise Bad("step_unit_unsupported", affected[0][0])
            unit, source = g, ("override" if any(r[4] == "override" for r in affected) else "caller")
        elif primary is None:
            if given is None:
                raise Bad("unit_required")
            unit, source = given, "caller"
        else:
            if primary[1] == "cm":
                raise Bad("step_unit_unsupported", primary[0])
            unit, source = primary[1], "declared"
    except Bad as e:
        line = "prov error {}".format(e.code)
        if e.entity is not None:
            line += " entity {}".format(e.entity)
        return [line]
    lines = ["prov unit {} source {} overridden {}".format(unit, source, "false" if source == "declared" else "true")]
    items = {"step.contexts": str(len(out))}
    if unitless:
        items["step.unitless_contexts"] = str(len(unitless))
    if len({r[1] for r in out if r[5]}) > 1:
        items["step.units.mixed"] = "true"
    groups = []
    index = {}
    for c, eff, angle, unc, src, geometry in out:
        key = (eff, src, angle, None if unc is None else (unc[1], unc[2]), geometry)
        if key in index:
            groups[index[key]][1] += 1
            continue
        u = "none" if unc is None else "{} {}".format(shortest(unc[1]), unc[2])
        sig = "length={} factor_m={} source={} angle={} uncertainty={} geometry={}".format(
            eff, DECIMAL[eff], src, angle or "none", u, "true" if geometry else "false")
        index[key] = len(groups)
        groups.append([sig, 1, c])
    for k, (sig, count, first) in enumerate(groups):
        items["step.units.group_{}".format(k + 1)] = "{} contexts={} first=#{}".format(sig, count, first)
    pieces = []
    if affected:
        ids = {r[0] for r in affected}
        runs = []
        prev_in = False
        for r in out:
            if r[0] in ids:
                if prev_in:
                    runs[-1][1] = r[0]
                else:
                    runs.append([r[0], r[0]])
                prev_in = True
            else:
                prev_in = False
        cur = ""
        for a, b in runs:
            run = "#{}".format(a) if a == b else "#{}..#{}".format(a, b)
            if cur and len(cur) + 1 + len(run) > MAX_TEXT_BYTES:
                pieces.append(cur)
                cur = ""
            cur = run if not cur else cur + "," + run
        if cur:
            pieces.append(cur)
        n_over = sum(1 for r in affected if r[4] == "override")
        items["step.units.not_declared"] = "contexts={} override={} caller={}".format(
            len(affected), n_over, len(affected) - n_over)
        for k, piece in enumerate(pieces):
            items["step.units.not_declared_{}".format(k + 1)] = piece
    if HEADER_ITEMS + len(items) > MAX_DECLARED:
        return lines + ["prov record_error too_large"]
    for key in sorted(items):
        lines.append("declared {} {}".format(key, items[key]))
    return lines


def expected():
    out = ["# Expected results for the IO.CAD.STEP.HDR fixtures, written by make_cases.py.",
           "# Block: '== <fixture> unit=<symbol|none> override=<bool>', then the result lines.",
           "# factor: big-endian hex of the f64 nearest to the exact factor to metres.",
           "# system: hex of the UTF-8 originating-system string.",
           "# prov: the provenance record's unit, source and unit_overridden; declared: its unit items."]
    for name, given, override in CASES:
        out.append("== {} unit={} override={}".format(name, given or "none", "true" if override else "false"))
        try:
            out += resolve(FIXTURES[name], given, override)
        except Bad as e:
            line = "error {}".format(e.code)
            if e.entity is not None:
                line += " entity {}".format(e.entity)
            if e.raw is not None:
                line += " raw {}".format(e.raw.encode("utf-8").hex())
            out.append(line)
    out.append("")
    return "\n".join(out)


def main():
    check = "--check" in sys.argv[1:]
    files = dict(FIXTURES)
    files["expected.txt"] = expected()
    stale = []
    for name, text in sorted(files.items()):
        path = HERE / name
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale: " + ", ".join(stale))
        return 1
    if check:
        print("IO.CAD.STEP.HDR fixtures up to date ({} files)".format(len(files)))
    return 0


if __name__ == "__main__":
    sys.exit(main())
