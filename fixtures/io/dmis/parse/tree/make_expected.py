#!/usr/bin/env python3
"""Independent reference for spec leaf IO.DMIS.PARSE.TREE (stdlib only).

For every hand-written ``*.dmi`` fixture in this folder, builds the generic statement tree
with the rules written down in the crate docs of ``fid-io-dmis::tree`` and the test registry
below, and writes the expected result next to it as ``<name>.tree`` (strict mode, then
lenient mode). It also writes ``classes.json``, the statement-class list of the test
registry. ``--check`` compares instead of writing.

The ``.dmi`` fixtures are short snippets written by the project (CC-BY-4.0); none is copied
from a standard or a vendor program, and the test descriptors are made up for the tests (they
are not the DMIS grammar). Lexing reuses the project's independent Python lexer in
``../lex/make_expected.py``. This script shares no code with the Rust crate: shapes are
matched here with recursive generators, where the Rust code runs a compiled backtracking
program on an explicit stack.

Output format, one record per line, fields separated by one TAB:

    mode strict|lenient
    statement <line>:<col> <class> <label or -> <MAJOR> <fields>
    unknown <line>:<col> <keyword or -> <reason or ->
    comment <line>:<col>
    tokens <spelling>...            (after every node: all of its tokens, input order)
    error malformed <line> <col> <class>
    error limit_exceeded <bound> <line>

``<fields>`` is ``-`` without a slash, else ``/`` and the fields joined by ``,``; a field is
its items joined by one space; an item is ``<kind>:<spelling>`` or ``(`` fields ``)``.
"""

import importlib.util
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent
_spec = importlib.util.spec_from_file_location("dmis_lex_ref", HERE.parent / "lex" / "make_expected.py")
LEX = importlib.util.module_from_spec(_spec)
_spec.loader.exec_module(LEX)

MAX_GROUP_DEPTH = 32
FAMILY = "TEST"
PHASE = "P1a"


def kw(w):
    return ("kw", w)


NUM = ("number",)
WORD = ("anyword",)
STR = ("str",)
RAW = ("raw",)
ANY = ("any",)


def seq(*s):
    return ("seq", list(s))


def alt(*s):
    return ("alt", list(s))


def opt(s):
    return ("opt", s)


def rep(s, lo, hi):
    return ("rep", s, lo, hi)


def label(p=None):
    return ("label", p)


def group(s):
    return ("group", s)


# The test registry, in registration order: (class, major, label kind, shape).
# Mirrored by `test_registry()` in crates/fid-io-dmis/tests/tree.rs.
REGISTRY = [
    ("feat_point", "FEAT", ("required", "F"), seq(kw("POINT"), alt(kw("CART"), kw("POL")), rep(NUM, 6, 6))),
    (
        "feat_circle",
        "FEAT",
        ("required", "F"),
        seq(kw("CIRCLE"), alt(kw("INNER"), kw("OUTER")), alt(kw("CART"), kw("POL")), rep(NUM, 6, 6), NUM),
    ),
    ("tol_diam", "TOL", ("required", "T"), seq(kw("DIAM"), NUM, NUM, opt(alt(kw("MMC"), kw("LMC"))))),
    ("units", "UNITS", ("none", None), seq(alt(kw("MM"), kw("INCH")), opt(alt(kw("ANGDEC"), kw("ANGDMS"))))),
    ("text", "TEXT", ("none", None), seq(alt(kw("OPER"), kw("OUTFIL")), alt(RAW, STR))),
    ("output", "OUTPUT", ("none", None), rep(label(), 1, None)),
    ("nest", "NESTT", ("optional", "V"), seq(WORD, opt(group(rep(alt(NUM, group(rep(NUM, 1, 3))), 1, 3))), opt(ANY))),
    ("endfil", "ENDFIL", ("none", None), seq()),
]


def label_json(k):
    kind, p = k
    return "none" if kind == "none" else "%s:%s" % (kind, p)


def classes_json():
    rows = [
        {"class": c, "major": m, "label_kind": label_json(k), "family": FAMILY, "phase": PHASE}
        for c, m, k, _ in REGISTRY
    ]
    rows.sort(key=lambda r: r["class"])
    doc = {"schema": "fid.dmis-classes", "version": 1, "classes": rows}
    return json.dumps(doc, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


# Tokens from the reference lexer: (line, col, kind, lexeme, extra).
def kind(t):
    return t[2]


def upper(t):
    return t[4][0].decode("ascii") if kind(t) == "word" else None


class Fail(Exception):
    def __init__(self, *fields):
        super().__init__(fields)
        self.fields = fields


def single(field):
    return field["items"][0] if len(field["items"]) == 1 else None


def term_ok(shape, field):
    tag = shape[0]
    if tag == "any":
        return True
    it = single(field)
    if it is None:
        return False
    if isinstance(it, dict):  # group
        return tag == "group" and full_match(shape[1], it["fields"])[0]
    k = kind(it)
    if tag == "kw":
        return k == "word" and upper(it) == shape[1]
    if tag == "anyword":
        return k == "word"
    if tag == "number":
        return k == "number"
    if tag == "str":
        return k == "string"
    if tag == "raw":
        return k == "raw_text"
    if tag == "label":
        return k == "label" and (shape[1] is None or it[4][0].decode("ascii") == shape[1])
    return False


def ends(shape, fields, i, far):
    """Yields every field index where `shape` can stop when it starts at `i`."""
    tag = shape[0]
    if tag == "seq":
        yield from seq_ends(shape[1], fields, i, far)
    elif tag == "alt":
        for s in shape[1]:
            yield from ends(s, fields, i, far)
    elif tag == "opt":
        yield from ends(shape[1], fields, i, far)
        yield i
    elif tag == "rep":
        yield from rep_ends(shape[1], shape[2], shape[3], fields, i, far, 0)
    else:
        if i < len(fields) and term_ok(shape, fields[i]):
            yield i + 1
        else:
            far[0] = max(far[0], i)


def seq_ends(items, fields, i, far):
    if not items:
        yield i
        return
    for j in ends(items[0], fields, i, far):
        yield from seq_ends(items[1:], fields, j, far)


def rep_ends(s, lo, hi, fields, i, far, n):
    if n >= lo:
        yield i
    if hi is None or n < hi:
        for j in ends(s, fields, i, far):
            yield from rep_ends(s, lo, hi, fields, j, far, n + 1)


def full_match(shape, fields):
    far = [0]
    for j in ends(shape, fields, 0, far):
        if j == len(fields):
            return True, j
        far[0] = max(far[0], j)
    return False, far[0]


def shape_fields(sig, start):
    """Fields after the slash; raises ('mismatch', tok) or ('deep',)."""
    stack = [{"open": None, "fields": [], "cur": []}]

    def finish(fr):
        if fr["fields"] or fr["cur"]:
            fr["fields"].append({"items": fr["cur"], "comma": None})
        return fr["fields"]

    for t in sig[start:]:
        k = kind(t)
        top = stack[-1]
        if k == "comma":
            top["fields"].append({"items": top["cur"], "comma": t})
            top["cur"] = []
        elif k == "lparen":
            if len(stack) > MAX_GROUP_DEPTH:
                raise Fail("deep")
            stack.append({"open": t, "fields": [], "cur": []})
        elif k == "rparen":
            if len(stack) < 2:
                raise Fail("mismatch", t)
            fr = stack.pop()
            stack[-1]["cur"].append({"open": fr["open"], "fields": finish(fr), "close": t})
        else:
            top["cur"].append(t)
    if len(stack) > 1:
        raise Fail("mismatch", stack[-1]["open"])
    return finish(stack[0])


def field_pos(fields, i, last):
    if i < len(fields):
        f = fields[i]
        if f["items"]:
            it = f["items"][0]
            t = it["open"] if isinstance(it, dict) else it
            return t[0], t[1]
        if f["comma"] is not None:
            return f["comma"][0], f["comma"][1]
    return last[0], last[1]


def esc(b):
    return LEX.esc(b)


def render_fields(fields):
    out = []
    for f in fields:
        items = []
        for it in f["items"]:
            if isinstance(it, dict):
                items.append("(" + render_fields(it["fields"]) + ")")
            else:
                items.append("%s:%s" % (kind(it), esc(it[3])))
        out.append(" ".join(items))
    return ",".join(out)


def build(st, lenient):
    """One node as a list of output rows; raises Fail on a strict error."""
    toks = st.tokens
    spell = "\t".join(["tokens"] + [esc(t[3]) for t in toks])
    pos = "%d:%d" % (toks[0][0], toks[0][1])
    if len(toks) == 1 and kind(toks[0]) == "comment":
        return ["comment\t" + pos, spell]
    sig = [t for t in toks if kind(t) != "comment"]

    def unknown(keyword, reason):
        return ["\t".join(["unknown", pos, keyword or "-", reason or "-"]), spell]

    if len(sig) >= 3 and kind(sig[0]) == "label" and kind(sig[1]) == "equals" and kind(sig[2]) == "word":
        lab, mi = sig[0], 2
    elif sig and kind(sig[0]) == "word":
        lab, mi = None, 0
    else:
        return unknown(None, "no_major_word")
    major = upper(sig[mi])
    descs = [d for d in REGISTRY if d[1] == major]
    if not descs:
        return unknown(major, None)

    def mismatch(line, col, cls):
        if lenient:
            return unknown(major, "shape_mismatch")
        raise Fail("malformed", line, col, cls)

    rest = sig[mi + 1 :]
    slash = None
    fields = []
    if rest:
        if kind(rest[0]) != "slash":
            return mismatch(rest[0][0], rest[0][1], descs[0][0])
        slash = rest[0]
        try:
            fields = shape_fields(sig, mi + 2)
        except Fail as e:
            if e.fields[0] == "deep":
                raise Fail("limit_exceeded", "group_depth", st.first)
            return mismatch(e.fields[1][0], e.fields[1][1], descs[0][0])
    prefix = lab[4][0].decode("ascii") if lab else None
    best = None
    for cls, _, (lk, lp), shape in descs:
        if lk == "none":
            ok = prefix is None
        elif lk == "optional":
            ok = prefix is None or prefix == lp
        else:
            ok = prefix == lp
        reach = None
        if ok:
            matched, reach = full_match(shape, fields)
            if matched:
                row = [
                    "statement",
                    pos,
                    cls,
                    esc(lab[3]) if lab else "-",
                    major,
                    "/" + render_fields(fields) if slash else "-",
                ]
                return ["\t".join(row), spell]
        key = -1 if reach is None else reach
        if best is None or key > best[0]:
            best = (key, reach, cls)
    _, reach, cls = best
    if reach is None:
        line, col = toks[0][0], toks[0][1]
    else:
        line, col = field_pos(fields, reach, sig[-1])
    return mismatch(line, col, cls)


def render_mode(data, lenient):
    rows = ["mode\t" + ("lenient" if lenient else "strict")]
    try:
        stmts = LEX.lex(data)
    except LEX.Fail as e:
        return rows + ["\t".join(["error", "lex"] + [str(f) for f in e.fields])]
    nodes = []
    try:
        for st in stmts:
            nodes.extend(build(st, lenient))
    except Fail as e:
        # A failed parse yields the error only, never the nodes before it.
        return rows + ["\t".join(["error"] + [str(f) for f in e.fields])]
    return rows + nodes


def render(data):
    rows = render_mode(data, False) + render_mode(data, True)
    return "".join(r + "\n" for r in rows)


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    outputs = [(dmi.with_suffix(".tree"), render(dmi.read_bytes()).encode("ascii")) for dmi in sorted(HERE.glob("*.dmi"))]
    outputs.append((HERE / "classes.json", classes_json()))
    for target, want in outputs:
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
