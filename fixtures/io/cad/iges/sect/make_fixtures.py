#!/usr/bin/env python3
"""Independent fixture generator and oracle for IO.CAD.IGES.SECT (Metrology Maven).

CC-BY-4.0 project data. Every file here is invented test data written by this script;
no third-party IGES file was used or copied. Written separately from the Rust crate
fid-io-iges, from the leaf's rules (spec/io/cad/iges/sect/NODE.md) and the layout stated
in crates/fid-io-iges/src/write.rs docs. Only clause-level knowledge of the fixed-column
ASCII form is used (IGES 5.3 [unverified: edition], sections S, G, D, P and T).

The script owns the data model: each accepted fixture is built from Python values, laid
out in 80-column lines, and its expected parse result is written from the same values
(never from the Rust parser). It writes:

- <stem>.igs                one fixture per case;
- expected/<stem>.dump      the expected parse result of each accepted fixture;
- iges.toml                 the IGES corpus manifest (IO.CORP.MANIF schema) over the
                            fixtures, with SHA-256 digests and expected outcomes.

Canonical layout (what fid_io_iges::write must reproduce byte for byte for the canonical
fixtures): '\\n' line ends; Start text left-justified in columns 1-72; Global and
Parameter Data as a stream of tokens (each token is one parameter followed by its
delimiter) packed greedily into 72 (Global) or 64 (Parameter Data) columns, a token that
does not fit starting a new line and a token longer than the width being cut into
full-width pieces; Directory Entry fields right-justified in 8 columns (status as four
two-digit numbers); the back-pointer right-justified in columns 65-72; the section letter
in column 73 and a zero-padded 7-digit sequence number in columns 74-80; the Terminate
line 'S'+7 digits, 'G'+7, 'D'+7, 'P'+7.

Reals: the value of a lexeme is Python's float() of the lexeme with D/d replaced by E,
which is correctly rounded (round-half-even). Unit factors are exact fractions; the f64
in the dump is float(Fraction), also correctly rounded.

Dump format (one item per line, text fields last):
  start|<text>
  delims|<param delim>|<record delim>
  g|<global param number>|int|<value>
  g|<n>|real|<16 hex digits of the IEEE-754 bits>|<lexeme>
  g|<n>|str|<text>
  g|<n>|default
  unit|<flag|name|caller>|<unit>|<exact decimal factor to metres>|<f64 bits of the factor>
  de|<seq>|type=..|ptr=..|structure=..|font=..|level=..|view=..|xform=..|labeldisp=..|
     status=<8 digits>|weight=..|color=..|lines=..|form=..|subscript=..|res1=..|res2=..|label=..
     (one line)
  p|<de seq>|<index from 0>|<same kinds as g>

Run (stdlib only, Python 3.8+):
  python fixtures/io/cad/iges/sect/make_fixtures.py           # rewrite the fixtures
  python fixtures/io/cad/iges/sect/make_fixtures.py --check   # fail if anything is stale
"""

import hashlib
import struct
import sys
from fractions import Fraction
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "expected"

# Unit flag -> (name, exact factor to metres). IGES 5.3 Global parameter 14 and 15
# [unverified: table recalled, not checked against the standard text].
UNITS = {
    1: ("INCH", Fraction(254, 10000)),
    2: ("MM", Fraction(1, 1000)),
    4: ("FT", Fraction(3048, 10000)),
    5: ("MI", Fraction(1609344, 1000)),
    6: ("M", Fraction(1)),
    7: ("KM", Fraction(1000)),
    8: ("MIL", Fraction(254, 10000000)),
    9: ("UM", Fraction(1, 1000000)),
    10: ("CM", Fraction(1, 100)),
    11: ("UIN", Fraction(254, 10000000000)),
}
NAMES = {"IN": 1, "INCH": 1, "MM": 2, "FT": 4, "MI": 5, "M": 6, "KM": 7, "MIL": 8, "UM": 9, "CM": 10, "UIN": 11}


# ----- parameter values -------------------------------------------------------------

class I:
    def __init__(self, v):
        self.v = v


class R:
    def __init__(self, lex):
        self.lex = lex
        self.value = float(lex.replace("D", "E").replace("d", "e"))


class S:
    def __init__(self, text):
        assert all(0x20 <= ord(c) <= 0x7E for c in text)
        self.text = text


class Dflt:
    pass


def render(p):
    if isinstance(p, I):
        return str(p.v)
    if isinstance(p, R):
        return p.lex
    if isinstance(p, S):
        return "%dH%s" % (len(p.text), p.text)
    return ""


def bits(x):
    return struct.pack(">d", x).hex()


def dump_param(p):
    if isinstance(p, I):
        return "int|%d" % p.v
    if isinstance(p, R):
        return "real|%s|%s" % (bits(p.value), p.lex)
    if isinstance(p, S):
        return "str|" + p.text
    return "default"


def fmt_decimal(fr):
    """Exact terminating decimal of a fraction whose denominator is 2^a 5^b."""
    if fr.denominator == 1:
        return str(fr.numerator)
    k = 0
    while (fr * 10 ** k).denominator != 1:
        k += 1
    n = (fr * 10 ** k).numerator
    s = str(n).rjust(k + 1, "0")
    return (s[:-k] + "." + s[-k:]).rstrip("0").rstrip(".")


# ----- layout ---------------------------------------------------------------------------

def pack(tokens, width):
    lines, cur = [], ""
    for tok in tokens:
        if len(cur) + len(tok) <= width:
            cur += tok
            continue
        if cur:
            lines.append(cur)
            cur = ""
        while len(tok) > width:
            lines.append(tok[:width])
            tok = tok[width:]
        cur = tok
    if cur:
        lines.append(cur)
    return lines


def line(data, letter, seq, seqfmt="%07d"):
    assert len(data) <= 72, data
    return data.ljust(72) + letter + (seqfmt % seq)


class Entity:
    def __init__(self, etype, params, label="", form=0, font=0, level=0, structure=0, view=0, xform=0,
                 labeldisp=0, status=(0, 0, 0, 0), weight=0, color=0, subscript=0, res1="", res2=""):
        self.etype = etype
        self.params = params
        self.label = label
        self.form = form
        self.font = font
        self.level = level
        self.structure = structure
        self.view = view
        self.xform = xform
        self.labeldisp = labeldisp
        self.status = status
        self.weight = weight
        self.color = color
        self.subscript = subscript
        self.res1 = res1
        self.res2 = res2


class Model:
    def __init__(self, start, gparams, entities, pd=",", rd=";"):
        self.start = start
        self.gparams = gparams  # global parameters 3.. in order
        self.entities = entities
        self.pd = pd
        self.rd = rd


def record_tokens(values, pd, rd):
    out = []
    for i, v in enumerate(values):
        out.append(render(v) + (pd if i + 1 < len(values) else rd))
    return out


def global_tokens(m):
    rest = record_tokens(m.gparams, m.pd, m.rd)
    return ["1H" + m.pd + m.pd, "1H" + m.rd + (m.pd if rest else m.rd)] + rest


def write(m, legacy=False, mutate=None):
    """Lays the model out; returns (text, layout) where layout maps DE seq -> (ptr, lines)."""
    mutate = mutate or {}
    eol = "\r\n" if legacy else "\n"
    seqfmt = "%7d" if legacy else "%07d"
    s_lines = [line(t, "S", i + 1, seqfmt) for i, t in enumerate(m.start)]
    if legacy:
        # One parameter per line (long ones cut at 72 columns), the rest of the line blank.
        g_data = []
        for t in global_tokens(m):
            g_data.extend(pack([t], 72))
    else:
        g_data = pack(global_tokens(m), 72)
    g_lines = [line(d, "G", i + 1, seqfmt) for i, d in enumerate(g_data)]
    p_lines, d_lines, layout = [], [], {}
    for k, e in enumerate(m.entities):
        seq = 2 * k + 1
        if legacy:
            toks = [t + " " for t in record_tokens([I(e.etype)] + e.params, m.pd, m.rd)]
            data = pack(toks, 40)
        else:
            data = pack(record_tokens([I(e.etype)] + e.params, m.pd, m.rd), 64)
        ptr = len(p_lines) + 1
        layout[seq] = (ptr, len(data))
        back = mutate.get(("back", seq), seq)
        for d in data:
            p_lines.append(d.ljust(64) + ("%8d" % back) + "P" + (seqfmt % (len(p_lines) + 1)))
        ptr_written = mutate.get(("ptr", seq), ptr)

        def f(v):
            if legacy and v == 0:
                return " " * 8
            return "%8d" % v

        def txt(t):
            return t.ljust(8) if legacy else t.rjust(8)

        status = "%02d%02d%02d%02d" % e.status
        l1 = (f(e.etype) + f(ptr_written) + f(e.structure) + f(e.font) + f(e.level) + f(e.view) + f(e.xform)
              + f(e.labeldisp) + status)
        l2 = (f(e.etype) + f(e.weight) + f(e.color) + f(len(data)) + f(e.form) + txt(e.res1) + txt(e.res2)
              + txt(e.label) + f(e.subscript))
        d_lines.append(line(l1, "D", seq, seqfmt))
        d_lines.append(line(l2, "D", seq + 1, seqfmt))
    counts = [len(s_lines), len(g_lines), len(d_lines), len(p_lines)]
    for i, delta in mutate.get("terminate", {}).items():
        counts[i] += delta
    t_line = line("S%07dG%07dD%07dP%07d" % tuple(counts), "T", 1, seqfmt)
    all_lines = s_lines + g_lines + d_lines + p_lines
    if not mutate.get("drop_terminate"):
        all_lines.append(t_line)
    for ln in all_lines:
        assert len(ln) == 80, ln
    return eol.join(all_lines) + eol, layout


def unit_of(m):
    def gp(n):
        i = n - 3
        return m.gparams[i] if 0 <= i < len(m.gparams) else Dflt()

    flag, name = gp(14), gp(15)
    if isinstance(flag, I) and flag.v != 3 and flag.v in UNITS:
        return "flag", flag.v
    if isinstance(name, S) and name.text.strip().upper() in NAMES:
        return "name", NAMES[name.text.strip().upper()]
    return None


def dump(m, layout):
    out = ["start|" + t.rstrip(" ") for t in m.start]
    out.append("delims|%s|%s" % (m.pd, m.rd))
    for i, p in enumerate(m.gparams):
        out.append("g|%d|%s" % (i + 3, dump_param(p)))
    u = unit_of(m)
    if u is not None:
        name, fr = UNITS[u[1]]
        out.append("unit|%s|%s|%s|%s" % (u[0], name, fmt_decimal(fr), bits(float(fr))))
    for k, e in enumerate(m.entities):
        seq = 2 * k + 1
        ptr, nlines = layout[seq]
        out.append(
            "de|%d|type=%d|ptr=%d|structure=%d|font=%d|level=%d|view=%d|xform=%d|labeldisp=%d|status=%02d%02d%02d%02d"
            "|weight=%d|color=%d|lines=%d|form=%d|subscript=%d|res1=%s|res2=%s|label=%s"
            % ((seq, e.etype, ptr, e.structure, e.font, e.level, e.view, e.xform, e.labeldisp) + tuple(e.status)
               + (e.weight, e.color, nlines, e.form, e.subscript, e.res1, e.res2, e.label)))
    for k, e in enumerate(m.entities):
        for i, p in enumerate(e.params):
            out.append("p|%d|%d|%s" % (2 * k + 1, i, dump_param(p)))
    return "\n".join(out) + "\n"


# ----- the invented models -------------------------------------------------------------

def gparams(name, flag, unit_name, sender="MM-FIXTURE"):
    return [
        S(sender), S(name), S("MM-ORACLE"), S("1.0"),          # 3-6
        I(32), I(38), I(6), I(308), I(15),                     # 7-11
        S("MM-RECEIVER"), R("1.0"), flag, unit_name,          # 12-15
        I(1), R("0.08"), S("20261125.120000"),                 # 16-18
        R("0.0001"), R("100.0"),                               # 19-20
        S("Invented Author"), S("Invented Organisation"),      # 21-22
        I(11), I(0), S("20261125.120000"), Dflt(),             # 23-26
    ]


START = [
    "Metrology Maven test fixture for IO.CAD.IGES.SECT.",
    "Invented data, CC-BY-4.0; not taken from any third-party file.",
]


def three_entities():
    return Model(START, gparams("three_entities.igs", I(2), S("MM")), [
        Entity(116, [R("10.0"), R("20.0"), R("0.0"), I(0)], label="PT1", font=1),
        Entity(110, [R("0.0"), R("0.0"), R("0.0"), R("50.0"), R("25.0"), R("0.0")], label="LN1", font=1, color=3),
        Entity(100, [R("0.0"), R("5.0"), R("5.0"), R("10.0"), R("5.0"), R("5.0"), R("10.0")], label="ARC1",
               font=1, level=7, status=(0, 0, 0, 1), subscript=2),
    ])


def d_exponent():
    return Model(START, gparams("d_exponent.igs", I(6), S("M")), [
        Entity(116, [R("1.5D-3"), R("2.0D0"), R("-3.25D+2"), I(0)], label="PTD"),
    ])


LONG = ("A long invented note that spans more than one Parameter Data line, with the "
        "delimiters / and ; and , inside it, which the Hollerith count protects.")


def hollerith():
    return Model(START, gparams("hollerith.igs", I(10), S("CM"), sender="A,B;C/D"), [
        Entity(212, [I(1), S(LONG), R("1.0"), Dflt(), S(""), I(-7), S("x" * 70)], label="NOTE 1", form=0,
               res1="R1", res2="", status=(1, 2, 3, 4)),
        Entity(116, [R("-1.0"), R("+2."), R(".5"), I(-1)], label="P", xform=-3, view=5, labeldisp=0),
    ], pd="/", rd=";")


def reals():
    vals = [
        "9007199254740993.0",          # 2^53 + 1: halfway, rounds to even (2^53)
        "0.1",
        "1.7976931348623157D308",      # largest finite double
        "2.2250738585072014D-308",     # smallest normal
        "4.9406564584124654D-324",     # smallest subnormal
        "2.4703282292062328D-324",     # just above half the smallest subnormal
        "-0.0",
        "1.E2",
        "1.00000000000000011102230246251565404236316680908203125",  # 1 + 2^-53: tie, to even (1.0)
        "1.00000000000000011102230246251565404236316680908203126",  # just above the tie
        "123456789012345678901234567890.",
        "6.02214076d23",
    ]
    ents = []
    for k in range(0, len(vals), 3):
        ents.append(Entity(116, [R(v) for v in vals[k:k + 3]] + [I(0)], label="R%d" % (k // 3)))
    ents.append(Entity(0, [I(9223372036854775807), I(-9223372036854775808), I(0)], label="INTS"))
    return Model(START, gparams("reals.igs", I(1), S("INCH")), ents)


def unit_name():
    return Model(START, gparams("unit_name.igs", I(3), S("INCH")), [
        Entity(116, [R("1.0"), R("2.0"), R("3.0"), I(0)], label="PT1"),
    ])


def unit_missing():
    return Model(START, gparams("unit_missing.igs", Dflt(), Dflt()), [
        Entity(116, [R("1.0"), R("2.0"), R("3.0"), I(0)], label="PT1"),
    ])


# (stem, builder, layout, mutation, expected outcome)
ACCEPT = [
    ("three_entities", three_entities, False, {}, 3),
    ("d_exponent", d_exponent, False, {}, 1),
    ("hollerith", hollerith, False, {}, 2),
    ("reals", reals, False, {}, 5),
    ("unit_name", unit_name, False, {}, 1),
    ("legacy_layout", three_entities, True, {}, 3),
]
REJECT = [
    ("bad_pointer", three_entities, {("ptr", 3): 99}, "malformed"),
    ("back_pointer", three_entities, {("back", 3): 1}, "malformed"),
    ("bad_terminate", three_entities, {"terminate": {3: 1}}, "truncated_file"),
    ("no_terminate", three_entities, {"drop_terminate": True}, "truncated_file"),
    ("unit_missing", unit_missing, {}, "unit_required"),
]
# Compressed ASCII form: a flag line ('C' in column 1, 'F' in column 73) [unverified: layout].
COMPRESSED = line("C", "F", 1) + "\n" + line("Compressed form, not supported.", "S", 1) + "\n"


def build():
    files = {}
    manifest = [
        "# IGES corpus manifest (IO.CORP.MANIF schema) over the invented IO.CAD.IGES.SECT fixtures.",
        "# Generated by make_fixtures.py; do not edit.",
        'format = "iges"',  # must equal the file stem (fid_io_corp::Manifest::parse)
    ]

    def entry(name, data, expect, tags):
        manifest.extend([
            "",
            "[[entry]]",
            'path = "%s"' % name,
            'source = "synthetic, fixtures/io/cad/iges/sect/make_fixtures.py"',
            'licence = "CC-BY-4.0"',
            'sha256 = "sha256:%s"' % hashlib.sha256(data).hexdigest(),
            "expect = %s" % expect,
            "tags = [%s]" % ", ".join('"%s"' % t for t in tags),
        ])

    for stem, make, legacy, mutate, n in ACCEPT:
        m = make()
        text, layout = write(m, legacy=legacy, mutate=mutate)
        data = text.encode("ascii")
        files[stem + ".igs"] = data
        files["expected/" + stem + ".dump"] = dump(m, layout).encode("ascii")
        entry(stem + ".igs", data, "{ accept = { counts = { entities = %d } } }" % n,
              ["p0-subset"] + ([] if legacy else ["canonical"]))
    for stem, make, mutate, code in REJECT:
        text, _ = write(make(), mutate=mutate)
        data = text.encode("ascii")
        files[stem + ".igs"] = data
        entry(stem + ".igs", data, '{ reject = { error = "%s" } }' % code, ["hostile"])
    files["compressed.igs"] = COMPRESSED.encode("ascii")
    entry("compressed.igs", files["compressed.igs"], '{ reject = { error = "iges_form_unsupported" } }',
          ["hostile"])
    files["iges.toml"] = ("\n".join(manifest) + "\n").encode("ascii")
    return files


def main():
    check = "--check" in sys.argv[1:]
    files = build()
    stale = []
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        if stale:
            print("stale IGES fixtures: " + ", ".join(stale))
            return 1
        print("IGES fixtures up to date (%d files)" % len(files))
    return 0


if __name__ == "__main__":
    sys.exit(main())
