#!/usr/bin/env python3
"""Independent reference for spec leaf IO.DMIS.PARSE.GDEF (stdlib only).

For every hand-written ``*.dmi`` and ``*.dmo`` fixture in this folder, classifies each logical
statement with the GDEF rules as written down in the crate docs of ``fid-io-dmis::gdef`` and
writes the expected typed views next to it as ``<name>.views``. ``--check`` compares instead
of writing.

The fixtures are synthetic files written by the project (CC-BY-4.0); no line is taken from
the DMIS standard, the NIST test suite or a vendor program, and the rules below are the
project's own reading of the statement structure, not text from the standard. Lexing reuses
the project's independent Python lexer in ``../lex/make_expected.py``. This script shares no
code with the Rust crate: it checks each statement family with straight-line code over the
field list, where the Rust crate compiles shape descriptors into a backtracking matcher.

Output format, one record per line, fields separated by one TAB; a list is its items joined
by one space, ``-`` when empty or absent; tokens are written with their authored spelling:

    comment <line>:<col>
    unknown <line>:<col> <KEYWORD>
    header  <line>:<col> <class> <name> <version> <extra>
    text    <line>:<col> text <channel> <text>
    units   <line>:<col> units <length> <angle> <temp>
    datum   <line>:<col> <class> <feature label> <datum label>
    feat    <line>:<col> <class> <label> def|actual <KIND> CART|POL|- INNER|OUTER|- <minor> <values> <refs>
    tol     <line>:<col> <class> <label> def|actual <KIND> <category> <values> <words> <refs> INTOL|OUTOL|-
"""

import importlib.util
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("dmis_lex_ref", HERE.parent / "lex" / "make_expected.py")
LEX = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(LEX)

# Feature kinds: word -> (class stem, needs INNER/OUTER, optional form words, min numbers, max numbers).
GEOMETRIC = {
    "POINT": ("point", False, (), 3, 6),
    "LINE": ("line", False, ("BND", "UNBND"), 6, 12),
    "PLANE": ("plane", False, (), 6, 6),
    "CIRCLE": ("circle", True, (), 7, 7),
    "ARC": ("arc", True, (), 7, 10),
    "CYLNDR": ("cylndr", True, (), 7, 8),
    "CONE": ("cone", True, (), 7, 8),
    "SPHERE": ("sphere", True, (), 4, 7),
    "ELLIPS": ("ellips", True, (), 8, 13),
    "CPARLN": ("cparln", True, ("ROUND", "SQUARE", "FLAT"), 8, 13),
    "TORUS": ("torus", True, (), 8, 8),
    "EDGEPT": ("edgept", False, (), 6, 9),
    "GCURVE": ("gcurve", False, (), 3, 9),
    "GSURF": ("gsurf", False, (), 3, 9),
}
# Member kinds: word -> (class stem, optional leading word).
MEMBERS = {"PATERN": ("patern", False), "COMPOUND": ("compound", True)}
TOLERANCE = {
    "DIAM": "size", "RAD": "size", "WIDTH": "size",
    "DISTB": "location", "ANGLB": "location", "POS": "location", "CONCEN": "location",
    "FLAT": "form", "STRGHT": "form", "CIRLTY": "form", "CYLCTY": "form",
    "PARLEL": "orientation", "PERPND": "orientation", "ANGLR": "orientation",
    "PROFL": "profile", "PROFP": "profile", "PROFS": "profile",
    "CRNOUT": "runout", "TRNOUT": "runout",
}
TOL_LEAD_MAX = 4
TOL_TAIL_MAX = 11
MODIFIERS = {"MMC", "LMC", "RFS", "AVG", "MIN", "MAX"}
LENGTHS = {"MM", "CM", "METER", "INCH", "FEET"}
ANGLES = {"ANGDEC", "ANGDMS", "ANGRAD"}
TEMPS = {"TEMPC", "TEMPF"}
FAMILY_MAJORS = {"DMISMN", "DMISMD", "FILNAM", "ENDFIL", "TEXT", "UNITS", "DATDEF", "FEAT", "TOL"}


class NoMatch(Exception):
    pass


def kind(t):
    return t[2]


def up(t):
    return t[4][0].decode("ascii") if kind(t) == "word" else None


def prefix(t):
    return t[4][0].decode("ascii") if kind(t) == "label" else None


def spell(t):
    return LEX.esc(t[3])


def pos(t):
    return "%d:%d" % (t[0], t[1])


def joined(tokens):
    return " ".join(spell(t) for t in tokens) if tokens else "-"


def need(cond):
    if not cond:
        raise NoMatch()


def split(sig):
    """(label token or None, major word, list of one-token fields)."""
    if len(sig) >= 3 and kind(sig[0]) == "label" and kind(sig[1]) == "equals" and kind(sig[2]) == "word":
        label, rest = sig[0], sig[2:]
    elif sig and kind(sig[0]) == "word":
        label, rest = None, sig
    else:
        return None, None, None
    major, body = up(rest[0]), rest[1:]
    if not body:
        return label, major, []
    need(kind(body[0]) == "slash")
    fields, cur = [], []
    for t in body[1:]:
        need(kind(t) not in ("lparen", "rparen", "slash", "equals"))
        if kind(t) == "comma":
            fields.append(cur)
            cur = []
        else:
            cur.append(t)
    if fields or cur:
        fields.append(cur)
    for f in fields:
        need(len(f) == 1)
    return label, major, [f[0] for f in fields]


def is_word_in(t, words):
    return up(t) in words


def header(cls, label, f):
    need(label is None)
    if cls in ("dmismn", "dmismd"):
        need(len(f) >= 2 and kind(f[0]) == "string" and kind(f[1]) == "number")
        need(len(f) - 2 <= 32 and all(kind(t) in ("word", "number") for t in f[2:]))
        name, version, extra = f[0], f[1], f[2:]
    elif cls == "filnam":
        need(len(f) in (1, 2) and kind(f[0]) == "string")
        need(len(f) == 1 or kind(f[1]) == "number")
        name, version, extra = f[0], (f[1] if len(f) == 2 else None), []
    else:  # endfil
        need(not f)
        name, version, extra = None, None, []
    return ["header", cls, spell(name) if name else "-", spell(version) if version else "-", joined(extra)]


def feature(label, f):
    need(label is not None and prefix(label) in ("F", "FA"))
    actual = prefix(label) == "FA"
    need(f and kind(f[0]) == "word")
    word = up(f[0])
    coord = side = "-"
    minor, values, refs = [], [], []
    if word in GEOMETRIC:
        stem, has_side, forms, lo, hi = GEOMETRIC[word]
        i = 1
        if has_side:
            need(i < len(f) and is_word_in(f[i], ("INNER", "OUTER")))
            side = up(f[i])
            i += 1
        if forms and i < len(f) and is_word_in(f[i], forms):
            minor.append(f[i])
            i += 1
        need(i < len(f) and is_word_in(f[i], ("CART", "POL")))
        coord = up(f[i])
        values = f[i + 1:]
        need(lo <= len(values) <= hi and all(kind(t) == "number" for t in values))
    elif word in MEMBERS:
        stem, lead = MEMBERS[word]
        i = 1
        if lead and i < len(f) and kind(f[i]) == "word":
            minor.append(f[i])
            i += 1
        refs = f[i:]
        need(refs and all(prefix(t) in ("F", "FA") for t in refs))
    else:
        raise NoMatch()
    cls = "feat_" + stem + ("_actual" if actual else "")
    return ["feat", cls, spell(label), "actual" if actual else "def", word, coord, side,
            joined(minor), joined(values), joined(refs)]


def tolerance(label, f):
    need(label is not None and prefix(label) in ("T", "TA"))
    actual = prefix(label) == "TA"
    need(f and kind(f[0]) == "word" and up(f[0]) in TOLERANCE)
    word = up(f[0])
    body, flag, tail_max = f[1:], "-", TOL_TAIL_MAX
    if actual:
        need(body and is_word_in(body[-1], ("INTOL", "OUTOL")))
        flag = up(body[-1])
        body = body[:-1]
        tail_max -= 1
    # Every field is a number, a modifier word or a DAT label; flags are not modifiers.
    for t in body:
        need(kind(t) == "number" or is_word_in(t, MODIFIERS) or prefix(t) == "DAT")
    numbers_at = [i for i, t in enumerate(body) if kind(t) == "number"]
    need(numbers_at)  # at least one value
    first = numbers_at[0]
    need(first <= TOL_LEAD_MAX and len(body) - first - 1 <= tail_max)
    values = [t for t in body if kind(t) == "number"]
    words = [t for t in body if kind(t) == "word"]
    refs = [t for t in body if kind(t) == "label"]
    cls = "tol_" + word.lower() + ("_actual" if actual else "")
    return ["tol", cls, spell(label), "actual" if actual else "def", word, TOLERANCE[word],
            joined(values), joined(words), joined(refs), flag]


def classify(label, major, f):
    if major in ("DMISMN", "DMISMD", "FILNAM", "ENDFIL"):
        return header(major.lower(), label, f)
    if major == "TEXT":
        need(label is None and len(f) == 2 and kind(f[0]) == "word" and kind(f[1]) in ("raw_text", "string"))
        return ["text", "text", spell(f[0]), spell(f[1])]
    if major == "UNITS":
        need(label is None and len(f) in (2, 3))
        need(is_word_in(f[0], LENGTHS) and is_word_in(f[1], ANGLES))
        need(len(f) == 2 or is_word_in(f[2], TEMPS))
        return ["units", "units", spell(f[0]), spell(f[1]), spell(f[2]) if len(f) == 3 else "-"]
    if major == "DATDEF":
        if label is not None:
            # Only the DA(..) labelled form; D(..)=DATDEF is not part of this family.
            need(prefix(label) == "DA" and len(f) == 1 and prefix(f[0]) in ("F", "FA"))
            return ["datum", "datdef_actual", spell(f[0]), spell(label)]
        need(len(f) == 2)
        need(prefix(f[0]) in ("F", "FA") and prefix(f[1]) in ("DAT", "DA"))
        return ["datum", "datdef", spell(f[0]), spell(f[1])]
    if major == "FEAT":
        return feature(label, f)
    return tolerance(label, f)


def render(data):
    rows = []
    for st in LEX.lex(data):
        first = st.tokens[0]
        sig = [t for t in st.tokens if kind(t) != "comment"]
        if not sig:
            rows.append(["comment", pos(first)])
            continue
        try:
            label, major, fields = split(sig)
        except NoMatch:
            label, major, fields = None, up(sig[0]) or "-", None
        if major is None:
            rows.append(["unknown", pos(first), "-"])
            continue
        if major not in FAMILY_MAJORS:
            rows.append(["unknown", pos(first), major])
            continue
        if fields is None:
            raise SystemExit("fixture statement at %s does not split into fields" % pos(first))
        try:
            row = classify(label, major, fields)
        except NoMatch:
            raise SystemExit("fixture statement at %s matches no GDEF rule" % pos(first))
        rows.append([row[0], pos(first)] + row[1:])
    return "".join("\t".join(r) + "\n" for r in rows)


# Statements the GDEF rules must reject (hand-picked; checked on every run).
REJECTED = [
    b"T(X)=TOL/DIAM,0.1,INTOL\n",  # a flag is not a definition modifier
    b"TA(X)=TOL/FLAT,MMC,OUTOL\n",  # a result needs at least one value
    b"D(A)=DATDEF/FA(P1)\n",  # only DA(..) labels the datum form
    b"F(C1)=FEAT/CIRCLE,CART,1,2,3,0,0,1,10\n",  # side word missing
]


def rejects(data):
    st = LEX.lex(data)[0]
    label, major, fields = split([t for t in st.tokens if kind(t) != "comment"])
    try:
        classify(label, major, fields)
    except NoMatch:
        return True
    return False


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for data in REJECTED:
        if not rejects(data):
            raise SystemExit("rule accepts %r" % data)
    if rejects(b"DA(A)=DATDEF/FA(P1)\n"):
        raise SystemExit("rule rejects the DA(..)=DATDEF form")
    sources = sorted(list(HERE.glob("*.dmi")) + list(HERE.glob("*.dmo")))
    for src in sources:
        target = src.with_suffix(".views")
        want = render(src.read_bytes()).encode("ascii")
        if check:
            if not target.exists() or target.read_bytes() != want:
                stale.append(target.name)
        else:
            target.write_bytes(want)
    if stale:
        print("stale: " + ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
