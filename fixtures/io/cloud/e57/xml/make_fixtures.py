#!/usr/bin/env python3
"""Independent reference generator for the IO.CLOUD.E57.XML fixtures (crate fid-io-e57).

Written separately from the Rust code, from the leaf's rules (spec/io/cloud/e57/xml/NODE.md)
and the byte form documented for the writer (crates/fid-io-e57/src/xml/write.rs). Stdlib
only, no third-party data, no randomness. Every file is synthetic.

What it writes:

- two_scans.xml and two_scans.expected (AC-1): an XML section with two data3D scans, one
  with Cartesian double fields, one with spherical scaled-integer fields, written by hand
  below with varied lexical forms; the expected summary comes from the Python model of the
  same scans, not from parsing the file.
- cases.txt and the error-path files (AC-2, AC-3 and the other refusals): each file with
  the outcome the leaf's rules give it, labelled by hand here.
- writer_scans.txt and writer_three_scans.xml (AC-4): three scans as plain parameter lines,
  and the bytes the writer must produce for them, from this script's own implementation of
  the documented byte form. Reals are written like Rust's `{:e}` (shortest round-trip digits
  in exponent form), derived here from Python's repr.

Reals in the expected files are IEEE-754 binary64 bit patterns in hex, so nothing printed
depends on a platform's libm. Line ends are LF.

Run:
  python fixtures/io/cloud/e57/xml/make_fixtures.py           # rewrite the fixtures
  python fixtures/io/cloud/e57/xml/make_fixtures.py --check   # fail if any fixture is stale
"""

import math
import struct
import sys
from decimal import Decimal
from pathlib import Path

HERE = Path(__file__).resolve().parent
NS = "http://www.astm.org/COMMIT/E57/2010-e57-v1.0"
EXT = "https://example.invalid/e57/ext-1"
DECL = '<?xml version="1.0" encoding="UTF-8"?>\n'
FORMAT_NAME = "ASTM E57 3D Imaging Data File"


def hx(x: float) -> str:
    return struct.pack(">d", x).hex()


def rust_exp(x: float) -> str:
    """Rust's `format!("{:e}", x)` for a finite f64: shortest round-trip digits."""
    if x == 0.0:
        return "-0e0" if math.copysign(1.0, x) < 0 else "0e0"
    sign, digits, exp = Decimal(repr(x)).as_tuple()
    digits = list(digits)
    while len(digits) > 1 and digits[-1] == 0:
        digits.pop()
        exp += 1
    e = exp + len(digits) - 1
    mant = str(digits[0])
    if len(digits) > 1:
        mant += "." + "".join(str(d) for d in digits[1:])
    return ("-" if sign else "") + mant + "e" + str(e)


# ---------------------------------------------------------------- AC-1: two scans

# Each Float literal is written into the XML as is; the expected bits are float(literal).
SCAN0_Q = ["0.5", "+5.0E-1", "0.50", ".5"]
SCAN0_T = ["1.25e3", "-0.1", "2.0E-3"]
SCAN0_CB = ["-1.5", "1.5", "-2", "2.", "0", "3.75e0"]
SCAN1_Q = ["1", "0", "0.0", "0.002"]
SCAN1_T = ["0", "0", "1.5"]
SCAN1_SB = ["0.5", "4.194303E2", "-1.5708", "1.5708", "3.0", "-3.0"]


def floats(indent, names, lits, precision=None):
    p = f' precision="{precision}"' if precision else ""
    return "".join(f'{indent}<{n} type="Float"{p}>{v}</{n}>\n' for n, v in zip(names, lits))


TWO_SCANS = (
    DECL
    + "<!-- Synthetic E57 XML section for IO.CLOUD.E57.XML AC-1 (make_fixtures.py). -->\n"
    + f'<e57Root type="Structure" xmlns="{NS}" xmlns:ext="{EXT}">\n'
    + f"  <formatName type=\"String\"><![CDATA[{FORMAT_NAME}]]></formatName>\n"
    + '  <guid type="String">{F0000000-0000-4000-8000-000000000001}</guid>\n'
    + '  <versionMajor type="Integer">1</versionMajor>\n'
    + '  <versionMinor type="Integer"> 0 </versionMinor>\n'
    + '  <data3D type="Vector" allowHeterogeneousChildren="0">\n'
    + '    <vectorChild type="Structure">\n'
    + '      <guid type="String">{F0000000-0000-4000-8000-000000000002}</guid>\n'
    + '      <name type="String">Station A &amp; B</name>\n'
    + '      <description type="String">cartesian double scan</description>\n'
    + '      <pose type="Structure">\n'
    + '        <rotation type="Structure">\n'
    + floats("          ", ["w", "x", "y", "z"], SCAN0_Q)
    + "        </rotation>\n"
    + '        <translation type="Structure">\n'
    + floats("          ", ["x", "y", "z"], SCAN0_T, "double")
    + "        </translation>\n"
    + "      </pose>\n"
    + '      <cartesianBounds type="Structure">\n'
    + floats("        ", ["xMinimum", "xMaximum", "yMinimum", "yMaximum", "zMinimum", "zMaximum"], SCAN0_CB)
    + "      </cartesianBounds>\n"
    + '      <intensityLimits type="Structure">\n'
    + '        <intensityMinimum type="Float" precision="single">0</intensityMinimum>\n'
    + '        <intensityMaximum type="Float" precision="single">1</intensityMaximum>\n'
    + "      </intensityLimits>\n"
    + '      <points type="CompressedVector" fileOffset="4096" recordCount="1000">\n'
    + '        <prototype type="Structure">\n'
    + '          <cartesianX type="Float" precision="double"/>\n'
    + '          <cartesianY type="Float"/>\n'
    + '          <cartesianZ type="Float" precision="double" minimum="-1e3" maximum="1E3"></cartesianZ>\n'
    + '          <intensity type="Float" precision="single" minimum="0" maximum="1"/>\n'
    + '          <ext:normalX type="Float" precision="single"/>\n'
    + "        </prototype>\n"
    + '        <codecs type="Vector" allowHeterogeneousChildren="1"/>\n'
    + "      </points>\n"
    + "      <!-- an extension element: kept as counted, untrusted metadata -->\n"
    + '      <ext:vendorNote type="String">untrusted &#x202E;text</ext:vendorNote>\n'
    + "    </vectorChild>\n"
    + '    <vectorChild type="Structure">\n'
    + '      <guid type="String">{F0000000-0000-4000-8000-000000000003}</guid>\n'
    + '      <name type="String">spherical</name>\n'
    + '      <pose type="Structure">\n'
    + '        <translation type="Structure">\n'
    + floats("          ", ["x", "y", "z"], SCAN1_T)
    + "        </translation>\n"
    + '        <rotation type="Structure">\n'
    + floats("          ", ["w", "x", "y", "z"], SCAN1_Q)
    + "        </rotation>\n"
    + "      </pose>\n"
    + '      <points type="CompressedVector" fileOffset="1048576" recordCount="12345">\n'
    + '        <prototype type="Structure">\n'
    + '          <sphericalRange type="ScaledInteger" minimum="0" maximum="4194303" scale="1e-4" offset="0"/>\n'
    + '          <sphericalAzimuth type="ScaledInteger" minimum="-31416" maximum="31416" scale="1.0E-4"/>\n'
    + '          <sphericalElevation type="ScaledInteger" minimum="-15708" maximum="+15708" scale="0.0001" offset="0.0"/>\n'
    + '          <colorRed type="Integer" minimum="0" maximum="255"/>\n'
    + '          <rowIndex type="Integer" minimum="0" maximum="1999"/>\n'
    + "        </prototype>\n"
    + "      </points>\n"
    + '      <sphericalBounds type="Structure">\n'
    + floats(
        "        ",
        ["rangeMinimum", "rangeMaximum", "elevationMinimum", "elevationMaximum", "azimuthStart", "azimuthEnd"],
        SCAN1_SB,
    )
    + "      </sphericalBounds>\n"
    + '      <colorLimits type="Structure">\n'
    + '        <colorRedMinimum type="Integer">0</colorRedMinimum>\n'
    + '        <colorRedMaximum type="Integer">255</colorRedMaximum>\n'
    + '        <colorGreenMinimum type="Integer">0</colorGreenMinimum>\n'
    + '        <colorGreenMaximum type="Integer">255</colorGreenMaximum>\n'
    + '        <colorBlueMinimum type="Integer">0</colorBlueMinimum>\n'
    + '        <colorBlueMaximum type="Integer">255</colorBlueMaximum>\n'
    + "      </colorLimits>\n"
    + "    </vectorChild>\n"
    + "  </data3D>\n"
    + '  <images2D type="Vector">\n'
    + '    <vectorChild type="Structure">\n'
    + '      <guid type="String">{F0000000-0000-4000-8000-000000000004}</guid>\n'
    + "    </vectorChild>\n"
    + "  </images2D>\n"
    + "</e57Root>\n"
)


def fl(lits):
    return ",".join(hx(float(v)) for v in lits)


def norm(q):
    w, x, y, z = (float(v) for v in q)
    return math.sqrt(w * w + x * x + y * y + z * z)


def two_scans_expected():
    out = [
        "scan 0 guid={F0000000-0000-4000-8000-000000000002} name=Station A & B "
        "description=cartesian double scan points=4096,1000 codecs=0",
        f"pose 0 q={fl(SCAN0_Q)} t={fl(SCAN0_T)}",
        "field 0 0 cartesianX float precision=double min=- max=-",
        "field 0 1 cartesianY float precision=- min=- max=-",
        f"field 0 2 cartesianZ float precision=double min={hx(-1e3)} max={hx(1e3)}",
        f"field 0 3 intensity float precision=single min={hx(0.0)} max={hx(1.0)}",
        "field 0 4 ext:normalX float precision=single min=- max=-",
        f"cbounds 0 {fl(SCAN0_CB)}",
        f"ilimits 0 float:{hx(0.0)} float:{hx(1.0)}",
        "scan 1 guid={F0000000-0000-4000-8000-000000000003} name=spherical description=- "
        "points=1048576,12345 codecs=0",
        f"pose 1 q={fl(SCAN1_Q)} t={fl(SCAN1_T)}",
        f"field 1 0 sphericalRange scaled min=0 max=4194303 scale={hx(1e-4)} offset={hx(0.0)}",
        f"field 1 1 sphericalAzimuth scaled min=-31416 max=31416 scale={hx(1e-4)} offset=-",
        f"field 1 2 sphericalElevation scaled min=-15708 max=15708 scale={hx(1e-4)} offset={hx(0.0)}",
        "field 1 3 colorRed integer min=0 max=255",
        "field 1 4 rowIndex integer min=0 max=1999",
        f"sbounds 1 {fl(SCAN1_SB)}",
        "climits 1 integer:0 integer:255 integer:0 integer:255 integer:0 integer:255",
        "images2d 1",
        "extensions 2",
    ]
    for i, q in enumerate([SCAN0_Q, SCAN1_Q]):
        n = norm(q)
        if not abs(n - 1.0) <= 1e-6:
            out.append(f"warning pose_quaternion_not_unit scan={i} norm={hx(n)}")
    return "\n".join(out) + "\n"


# ---------------------------------------------------------------- error paths and bounds


def doc(body: str, ns_extra: str = "", decl: str = DECL) -> str:
    return decl + f'<e57Root type="Structure" xmlns="{NS}"{ns_extra}>\n' + body + "</e57Root>\n"


DATA3D = '  <data3D type="Vector"/>\n'


def scan(body: str) -> str:
    return '  <data3D type="Vector">\n    <vectorChild type="Structure">\n' + body + "    </vectorChild>\n  </data3D>\n"


POINTS = (
    '      <points type="CompressedVector" fileOffset="0" recordCount="1">\n'
    '        <prototype type="Structure"><x type="Float"/></prototype>\n'
    "      </points>\n"
)
GUID = '      <guid type="String">g</guid>\n'


def nested(depth: int) -> str:
    # root (depth 1), chain (2), `depth - 3` nested `deep` structures, then the leaf at `depth`.
    inner = "<leaf type=\"Integer\">1</leaf>"
    for _ in range(depth - 3):
        inner = f'<deep type="Structure">{inner}</deep>'
    return f'  <chain type="Structure">{inner}</chain>\n'


CASES = [
    # (file, content, expected outcome)
    ("doctype.xml", DECL + '<!DOCTYPE e57Root [<!ENTITY x "y">]>\n' + doc(DATA3D, decl=""),
     "malformed xml_forbidden_construct"),
    ("entity_ref.xml", doc(DATA3D + '  <note type="String">&custom;</note>\n'), "malformed xml_forbidden_construct"),
    ("encoding.xml", doc(DATA3D, decl='<?xml version="1.0" encoding="ISO-8859-1"?>\n'),
     "malformed xml_encoding_unsupported"),
    ("bom.xml", "﻿" + doc(DATA3D), "malformed xml_bom"),
    ("truncated.xml", doc(DATA3D)[:-12], "truncated_file"),
    # depth: root (1), chain (2), 62 deep (3 .. 64), leaf (65) on one path
    ("depth65.xml", doc(DATA3D + nested(65)), "limit_exceeded depth"),
    ("depth64.xml", doc(DATA3D + nested(64)), "ok scans=0 warning=no_scans"),
    ("not_e57_root.xml", DECL + f'<e57root type="Structure" xmlns="{NS}">\n{DATA3D}</e57root>\n',
     "malformed not_e57_root"),
    ("wrong_namespace.xml", DECL + '<e57Root type="Structure" xmlns="urn:x">\n' + DATA3D + "</e57Root>\n",
     "malformed not_e57_root"),
    ("e57_prefixed.xml", doc(DATA3D, f' xmlns:e="{NS}"'), "malformed e57_namespace_prefixed"),
    ("ns_not_on_root.xml", doc(DATA3D + '  <x type="Structure" xmlns:q="urn:q"/>\n'),
     "malformed namespace_declaration_not_on_root"),
    ("ns_xml_prefix.xml", doc(DATA3D, ' xmlns:xml="http://www.w3.org/XML/1998/namespace"'),
     "malformed namespace_reserved"),
    ("ns_uri_duplicate.xml", doc(DATA3D, ' xmlns:a="urn:a" xmlns:b="urn:a"'), "malformed namespace_uri_duplicate"),
    ("element_name.xml", doc(DATA3D + '  <a:1x type="Integer">1</a:1x>\n', ' xmlns:a="urn:a"'),
     "malformed element_name"),
    ("namespace_prefix.xml", doc(DATA3D, ' xmlns:1a="urn:a"'), "malformed namespace_prefix"),
    ("type_missing.xml", doc(DATA3D + "  <x>1</x>\n"), "malformed type_missing"),
    ("type_unknown.xml", doc(DATA3D + '  <x type="Double">1</x>\n'), "malformed type_unknown"),
    ("attr_not_applicable.xml", doc(DATA3D + '  <x type="Integer" scale="2">1</x>\n'),
     "malformed attribute_not_applicable"),
    ("attr_namespaced.xml", doc(DATA3D + '  <x type="Integer" a:m="1">1</x>\n', ' xmlns:a="urn:a"'),
     "malformed attribute_not_applicable"),
    ("duplicate_key.xml", doc(DATA3D + '  <x type="Integer">1</x>\n  <x type="Integer">2</x>\n'),
     "malformed duplicate_key"),
    ("integer_syntax.xml", doc(DATA3D + '  <x type="Integer">1.0</x>\n'), "malformed integer_syntax"),
    ("integer_overflow.xml", doc(DATA3D + '  <x type="Integer">9223372036854775808</x>\n'),
     "malformed integer_out_of_range"),
    ("value_out_of_range.xml", doc(DATA3D + '  <x type="Integer" minimum="0" maximum="10">11</x>\n'),
     "malformed value_out_of_range"),
    ("minimum_above_maximum.xml", doc(DATA3D + '  <x type="Float" minimum="2" maximum="1"/>\n'),
     "malformed minimum_above_maximum"),
    ("float_inf.xml", doc(DATA3D + '  <x type="Float">INF</x>\n'), "malformed float_syntax"),
    ("float_nan.xml", doc(DATA3D + '  <x type="Float">NaN</x>\n'), "malformed float_syntax"),
    ("float_overflow.xml", doc(DATA3D + '  <x type="Float">1e400</x>\n'), "malformed float_not_finite"),
    ("float_single_range.xml", doc(DATA3D + '  <x type="Float" precision="single">1e39</x>\n'),
     "malformed float_out_of_range"),
    ("precision_unknown.xml", doc(DATA3D + '  <x type="Float" precision="half">1</x>\n'),
     "malformed precision_unknown"),
    ("scale_zero.xml", doc(DATA3D + '  <x type="ScaledInteger" scale="0">1</x>\n'), "malformed scale_zero"),
    ("vector_child_name.xml", doc('  <data3D type="Vector">\n    <item type="Structure"/>\n  </data3D>\n'),
     "malformed vector_child_name"),
    ("vector_heterogeneous.xml",
     doc(DATA3D + '  <v type="Vector">\n    <vectorChild type="Integer">1</vectorChild>\n'
         '    <vectorChild type="Float">1</vectorChild>\n  </v>\n'),
     "malformed vector_heterogeneous"),
    ("vector_heterogeneous_allowed.xml",
     doc(DATA3D + '  <v type="Vector" allowHeterogeneousChildren="1">\n'
         '    <vectorChild type="Integer">1</vectorChild>\n    <vectorChild type="Float">1</vectorChild>\n  </v>\n'),
     "ok scans=0 warning=no_scans"),
    ("text_in_structure.xml", doc(DATA3D + '  <s type="Structure">text</s>\n'), "malformed unexpected_text"),
    ("child_in_leaf.xml", doc(DATA3D + '  <i type="Integer"><j type="Integer">1</j></i>\n'),
     "malformed unexpected_child"),
    ("control_character.xml", doc(DATA3D + '  <s type="String">a&#x85;b</s>\n'), "malformed control_character"),
    ("cv_attribute_missing.xml",
     doc(scan(GUID + '      <points type="CompressedVector" fileOffset="0">\n'
                     '        <prototype type="Structure"><x type="Float"/></prototype>\n      </points>\n')),
     "malformed attribute_missing"),
    ("cv_prototype_missing.xml",
     doc(scan(GUID + '      <points type="CompressedVector" fileOffset="0" recordCount="1"/>\n')),
     "malformed prototype_missing"),
    ("cv_child.xml",
     doc(scan(GUID + '      <points type="CompressedVector" fileOffset="0" recordCount="1">\n'
                     '        <prototype type="Structure"><x type="Float"/></prototype>\n'
                     '        <extra type="Integer">1</extra>\n      </points>\n')),
     "malformed compressed_vector_child"),
    ("codecs_not_vector.xml",
     doc(scan(GUID + '      <points type="CompressedVector" fileOffset="0" recordCount="1">\n'
                     '        <prototype type="Structure"><x type="Float"/></prototype>\n'
                     '        <codecs type="Structure"/>\n      </points>\n')),
     "malformed codecs_not_vector"),
    ("data3d_missing.xml", doc('  <images2D type="Vector"/>\n'), "malformed data3d_missing"),
    ("guid_missing.xml", doc(scan(POINTS)), "malformed scan_guid_missing scan=0"),
    ("points_missing.xml", doc(scan(GUID)), "malformed scan_points_missing scan=0"),
    ("prototype_empty.xml",
     doc(scan(GUID + '      <points type="CompressedVector" fileOffset="0" recordCount="1">\n'
                     '        <prototype type="Structure"/>\n      </points>\n')),
     "malformed prototype_empty scan=0"),
    ("pose_incomplete.xml",
     doc(scan(GUID + POINTS + '      <pose type="Structure">\n        <rotation type="Structure">\n'
              + floats("          ", ["w", "x", "y", "z"], ["1", "0", "0", "0"])
              + "        </rotation>\n      </pose>\n")),
     "malformed pose_incomplete scan=0"),
    ("bounds_inverted.xml",
     doc(scan(GUID + POINTS + '      <cartesianBounds type="Structure">\n'
              + floats("        ", ["xMinimum", "xMaximum", "yMinimum", "yMaximum", "zMinimum", "zMaximum"],
                       ["0", "1", "2", "1", "0", "1"])
              + "      </cartesianBounds>\n")),
     "malformed bounds_inverted scan=0"),
    ("limits_kind_mismatch.xml",
     doc(scan(GUID + POINTS + '      <intensityLimits type="Structure">\n'
              '        <intensityMinimum type="Integer">0</intensityMinimum>\n'
              '        <intensityMaximum type="Float">1</intensityMaximum>\n      </intensityLimits>\n')),
     "malformed limits_kind_mismatch scan=0"),
    ("quaternion_unnormalised.xml",
     doc(scan(GUID + POINTS + '      <pose type="Structure">\n        <rotation type="Structure">\n'
              + floats("          ", ["w", "x", "y", "z"], ["2", "0", "0", "0"])
              + '        </rotation>\n        <translation type="Structure">\n'
              + floats("          ", ["x", "y", "z"], ["0", "0", "0"])
              + "        </translation>\n      </pose>\n")),
     f"ok scans=1 warning=pose_quaternion_not_unit:0:{hx(2.0)}"),
]


# ---------------------------------------------------------------- AC-4: writer

# Three scans. Reals are Python floats; the parameter file carries their bits.
WRITER_GUID = "{F0000000-0000-4000-8000-0000000000aa}"
WRITER_SCANS = [
    {
        "guid": "{F0000000-0000-4000-8000-0000000000a1}",
        "name": "Scan one",
        "description": "f64 cartesian with intensity",
        "pose": ([0.5, 0.5, 0.5, 0.5], [1234.5678, -0.1, 1e-7]),
        "points": (48, 1000000),
        "fields": [
            ("cartesianX", ("float", "double", None, None)),
            ("cartesianY", ("float", "double", None, None)),
            ("cartesianZ", ("float", "double", -1e3, 1e3)),
            ("intensity", ("float", "single", 0.0, 1.0)),
            ("ext:normalX", ("float", None, None, None)),
        ],
        "cbounds": [-1.5, 1.5, -0.0, 0.30000000000000004, 6.02214076e23, 1.7976931348623157e308],
        "ilimits": [("float", 0.0), ("float", 1.0)],
    },
    {
        "guid": "{F0000000-0000-4000-8000-0000000000a2}",
        "name": None,
        "description": None,
        "pose": None,
        "points": (1048576, 7),
        "fields": [
            ("sphericalRange", ("scaled", 0, 4194303, 1e-4, 0.0)),
            ("sphericalAzimuth", ("scaled", -31416, 31416, 1e-4, None)),
            ("colorRed", ("integer", 0, 255)),
            ("rowIndex", ("integer", None, None)),
            ("label", ("string",)),
        ],
        "sbounds": [0.5, 419.4303, -1.5708, 1.5708, 3.0, -3.0],
        "climits": [("integer", 0), ("integer", 255), ("scaled", 0, 1e-2, 0.5), ("scaled", 100, 1e-2, 0.5),
                    ("integer", 1), ("integer", 1)],
    },
    {
        "guid": "{F0000000-0000-4000-8000-0000000000a3}",
        "name": 'a<b & c>d "q"',
        "description": "  leading spaces",
        "pose": ([0.7071067811865476, 0.0, 0.0, -0.7071067811865476], [5e-324, -2.5e-3, 100.0]),
        "points": (0, 0),
        "fields": [("cartesianX", ("float", "double", None, None))],
    },
]


def opt_hex(v):
    return "-" if v is None else hx(v)


def opt_int(v):
    return "-" if v is None else str(v)


def scalar_param(s):
    if s[0] == "integer":
        return f"integer {s[1]}"
    if s[0] == "float":
        return f"float {hx(s[1])}"
    return f"scaled {s[1]} {opt_hex(s[2])} {opt_hex(s[3])}"


def writer_params():
    out = [f"file_guid {WRITER_GUID}", f"ns ext {EXT}"]
    for s in WRITER_SCANS:
        out.append("scan")
        out.append(f"guid {s['guid']}")
        if s["name"] is not None:
            out.append(f"name {s['name']}")
        if s["description"] is not None:
            out.append(f"description {s['description']}")
        if s["pose"] is not None:
            q, t = s["pose"]
            out.append("pose " + " ".join(hx(v) for v in q + t))
        out.append(f"points {s['points'][0]} {s['points'][1]}")
        for name, k in s["fields"]:
            if k[0] == "float":
                out.append(f"field {name} float {k[1] or '-'} {opt_hex(k[2])} {opt_hex(k[3])}")
            elif k[0] == "scaled":
                out.append(f"field {name} scaled {opt_int(k[1])} {opt_int(k[2])} {opt_hex(k[3])} {opt_hex(k[4])}")
            elif k[0] == "integer":
                out.append(f"field {name} integer {opt_int(k[1])} {opt_int(k[2])}")
            else:
                out.append(f"field {name} string")
        if "cbounds" in s:
            out.append("cbounds " + " ".join(hx(v) for v in s["cbounds"]))
        if "sbounds" in s:
            out.append("sbounds " + " ".join(hx(v) for v in s["sbounds"]))
        if "ilimits" in s:
            out.append("ilimits " + " ".join(scalar_param(v) for v in s["ilimits"]))
        if "climits" in s:
            out.append("climits " + " ".join(scalar_param(v) for v in s["climits"]))
        out.append("end")
    return "\n".join(out) + "\n"


def esc_text(s):
    return s.replace("&", "&amp;").replace("<", "&lt;").replace(">", "&gt;").replace("\r", "&#xD;")


def leaf(ind, name, ty, attrs, value):
    a = "".join(f' {k}="{v}"' for k, v in attrs)
    if value == "":
        return f'{ind}<{name} type="{ty}"{a}/>\n'
    return f'{ind}<{name} type="{ty}"{a}>{value}</{name}>\n'


def wfloat(ind, name, v):
    return leaf(ind, name, "Float", [("precision", "double")], rust_exp(v))


def wscalar(ind, name, s):
    if s[0] == "integer":
        return leaf(ind, name, "Integer", [], str(s[1]))
    if s[0] == "float":
        return wfloat(ind, name, s[1])
    attrs = []
    if s[2] is not None:
        attrs.append(("scale", rust_exp(s[2])))
    if s[3] is not None:
        attrs.append(("offset", rust_exp(s[3])))
    return leaf(ind, name, "ScaledInteger", attrs, str(s[1]))


def wfield(ind, name, k):
    attrs = []
    if k[0] == "float":
        if k[1]:
            attrs.append(("precision", k[1]))
        for n, v in (("minimum", k[2]), ("maximum", k[3])):
            if v is not None:
                attrs.append((n, rust_exp(v)))
        return leaf(ind, name, "Float", attrs, "")
    if k[0] == "scaled":
        for n, v in (("minimum", k[1]), ("maximum", k[2])):
            if v is not None:
                attrs.append((n, str(v)))
        for n, v in (("scale", k[3]), ("offset", k[4])):
            if v is not None:
                attrs.append((n, rust_exp(v)))
        return leaf(ind, name, "ScaledInteger", attrs, "")
    if k[0] == "integer":
        for n, v in (("minimum", k[1]), ("maximum", k[2])):
            if v is not None:
                attrs.append((n, str(v)))
        return leaf(ind, name, "Integer", attrs, "")
    return leaf(ind, name, "String", [], "")


def writer_xml():
    i1, i2, i3, i4, i5 = (" " * (2 * n) for n in range(1, 6))
    out = DECL + f'<e57Root type="Structure" xmlns="{NS}" xmlns:ext="{EXT}">\n'
    out += leaf(i1, "formatName", "String", [], esc_text(FORMAT_NAME))
    out += leaf(i1, "guid", "String", [], esc_text(WRITER_GUID))
    out += leaf(i1, "versionMajor", "Integer", [], "1")
    out += leaf(i1, "versionMinor", "Integer", [], "0")
    out += f'{i1}<data3D type="Vector">\n'
    for s in WRITER_SCANS:
        out += f'{i2}<vectorChild type="Structure">\n'
        out += leaf(i3, "guid", "String", [], esc_text(s["guid"]))
        for key in ("name", "description"):
            if s[key] is not None:
                out += leaf(i3, key, "String", [], esc_text(s[key]))
        if s["pose"] is not None:
            q, t = s["pose"]
            out += f'{i3}<pose type="Structure">\n{i4}<rotation type="Structure">\n'
            out += "".join(wfloat(i5, n, v) for n, v in zip("wxyz", q))
            out += f'{i4}</rotation>\n{i4}<translation type="Structure">\n'
            out += "".join(wfloat(i5, n, v) for n, v in zip("xyz", t))
            out += f"{i4}</translation>\n{i3}</pose>\n"
        off, rec = s["points"]
        out += f'{i3}<points type="CompressedVector" fileOffset="{off}" recordCount="{rec}">\n'
        out += f'{i4}<prototype type="Structure">\n'
        out += "".join(wfield(i5, n, k) for n, k in s["fields"])
        out += f'{i4}</prototype>\n{i4}<codecs type="Vector"/>\n{i3}</points>\n'
        if "cbounds" in s:
            names = ["xMinimum", "xMaximum", "yMinimum", "yMaximum", "zMinimum", "zMaximum"]
            out += f'{i3}<cartesianBounds type="Structure">\n'
            out += "".join(wfloat(i4, n, v) for n, v in zip(names, s["cbounds"]))
            out += f"{i3}</cartesianBounds>\n"
        if "sbounds" in s:
            names = ["rangeMinimum", "rangeMaximum", "elevationMinimum", "elevationMaximum", "azimuthStart",
                     "azimuthEnd"]
            out += f'{i3}<sphericalBounds type="Structure">\n'
            out += "".join(wfloat(i4, n, v) for n, v in zip(names, s["sbounds"]))
            out += f"{i3}</sphericalBounds>\n"
        if "ilimits" in s:
            out += f'{i3}<intensityLimits type="Structure">\n'
            out += wscalar(i4, "intensityMinimum", s["ilimits"][0])
            out += wscalar(i4, "intensityMaximum", s["ilimits"][1])
            out += f"{i3}</intensityLimits>\n"
        if "climits" in s:
            names = ["colorRedMinimum", "colorRedMaximum", "colorGreenMinimum", "colorGreenMaximum",
                     "colorBlueMinimum", "colorBlueMaximum"]
            out += f'{i3}<colorLimits type="Structure">\n'
            out += "".join(wscalar(i4, n, v) for n, v in zip(names, s["climits"]))
            out += f"{i3}</colorLimits>\n"
        out += f"{i2}</vectorChild>\n"
    out += f'{i1}</data3D>\n{i1}<images2D type="Vector"/>\n</e57Root>\n'
    return out


def outputs():
    files = {
        "two_scans.xml": TWO_SCANS,
        "two_scans.expected": two_scans_expected(),
        "writer_scans.txt": writer_params(),
        "writer_three_scans.xml": writer_xml(),
    }
    lines = ["# file outcome (hand-labelled from the rules of spec/io/cloud/e57/xml/NODE.md)"]
    for name, content, outcome in CASES:
        files[name] = content
        lines.append(f"{name} {outcome}")
    files["cases.txt"] = "\n".join(lines) + "\n"
    return {k: v.encode("utf-8") for k, v in files.items()}


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in sorted(outputs().items()):
        p = HERE / name
        if check:
            if not p.exists() or p.read_bytes() != data:
                stale.append(name)
        else:
            p.write_bytes(data)
    if stale:
        print("stale fixtures: " + ", ".join(stale))
        return 1
    print("ok" if check else "written")
    return 0


if __name__ == "__main__":
    sys.exit(main())
