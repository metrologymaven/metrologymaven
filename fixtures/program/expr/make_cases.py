#!/usr/bin/env python3
"""Generate the expression corpora of spec node AUTO.PROG.EXPR.GRAMMAR.

Independent reference: this script has its own lexer and a classic one-function-per-
precedence-level recursive-descent parser, written from docs/program/expr-grammar.md (the
Rust crate uses a Pratt parser). It writes:

  valid/<name>.txt     one expression, no trailing newline
  valid/<name>.json    expected AST with byte spans (format below)
  invalid/<name>.txt   one malformed expression
  invalid/expected.json  {"<name>": {"error": "expr_syntax", "offset": N}}

The invalid offsets are labelled by hand from the grammar's error rules; the script checks
that its own parser agrees with every label and fails otherwise. Nothing here runs the
Rust code.

AST JSON: {"k": kind, "span": [start, end], ...} with kinds
  int {"v": "<decimal digits>"}, dec {"v": text}, qty {"v": text, "unit": sym},
  str {"v": text}, bool {"v": true|false}, ident {"name"},
  field {"base", "name"}, index {"base", "index"}, unary {"op", "arg"},
  binary {"op", "lhs", "rhs"}, cond {"if", "then", "else"}, call {"name", "args"}.

CC-BY-4.0 project data. Run:  python fixtures/program/expr/make_cases.py [--check]
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

KEYWORDS = {"and", "else", "false", "if", "not", "or", "then", "true"}
UNIT_EXTRA = set("_./^*-%") | {"µ", "μ", "°", "Ω"}
I64_MAX = 2**63 - 1


class SyntaxErr(Exception):
    def __init__(self, offset):
        super().__init__(offset)
        self.offset = offset


def ascii_alnum(c):
    return c.isascii() and c.isalnum()


# --------------------------------------------------------------------- lexer
# Works on characters and keeps a parallel list of UTF-8 byte offsets.


def lex(src):
    chars = list(src)
    offs = []
    pos = 0
    for c in chars:
        offs.append(pos)
        pos += len(c.encode("utf-8"))
    offs.append(pos)
    n = len(chars)
    toks = []  # (type, value, start_byte, end_byte)
    try:
        lex_into(chars, offs, toks)
    except SyntaxErr as e:
        return toks, e, offs[n]
    return toks, None, offs[n]


def lex_into(chars, offs, toks):
    n = len(chars)
    i = 0

    def at(j):
        return chars[j] if j < n else ""

    while i < n:
        c = chars[i]
        if c in " \t\n\r":
            i += 1
            continue
        s = i
        if "0" <= c <= "9":
            while "0" <= at(i) <= "9" and at(i) != "":
                i += 1
            is_dec = False
            if at(i) == "." and at(i + 1) != "" and "0" <= at(i + 1) <= "9":
                is_dec = True
                i += 1
                while at(i) != "" and "0" <= at(i) <= "9":
                    i += 1
            text = "".join(chars[s:i])
            if at(i) == "[":
                lb = i
                i += 1
                us = i
                while at(i) != "]":
                    ch = at(i)
                    if ch == "" or not (ascii_alnum(ch) or ch in UNIT_EXTRA):
                        raise SyntaxErr(offs[lb])
                    i += 1
                if i == us:
                    raise SyntaxErr(offs[lb])
                unit = "".join(chars[us:i])
                i += 1
                toks.append(("qty", (text, unit), offs[s], offs[i]))
            elif is_dec:
                toks.append(("dec", text, offs[s], offs[i]))
            else:
                if int(text) > I64_MAX:
                    raise SyntaxErr(offs[s])
                toks.append(("int", int(text), offs[s], offs[i]))
            continue
        if (c.isascii() and c.isalpha()) or c == "_":
            while at(i) != "" and (ascii_alnum(at(i)) or at(i) == "_"):
                i += 1
            word = "".join(chars[s:i])
            if len(word) > 64:
                raise SyntaxErr(offs[s])
            toks.append(("kw" if word in KEYWORDS else "ident", word, offs[s], offs[i]))
            continue
        if c == '"':
            i += 1
            buf = []
            while True:
                ch = at(i)
                if ch == "":
                    raise SyntaxErr(offs[s])
                if ch == '"':
                    i += 1
                    break
                if ch == "\\":
                    e = i
                    nx = at(i + 1)
                    simple = {'"': '"', "\\": "\\", "n": "\n", "r": "\r", "t": "\t"}
                    if nx in simple and nx != "":
                        buf.append(simple[nx])
                        i += 2
                        continue
                    if nx == "u" and at(i + 2) == "{":
                        j = i + 3
                        hexd = ""
                        while at(j) != "" and at(j) in "0123456789abcdefABCDEF":
                            hexd += at(j)
                            j += 1
                        if 1 <= len(hexd) <= 6 and at(j) == "}":
                            v = int(hexd, 16)
                            if v <= 0x10FFFF and not (0xD800 <= v <= 0xDFFF):
                                buf.append(chr(v))
                                i = j + 1
                                continue
                    raise SyntaxErr(offs[e])
                if ord(ch) < 0x20 or ord(ch) == 0x7F:
                    raise SyntaxErr(offs[i])
                buf.append(ch)
                i += 1
            toks.append(("str", "".join(buf), offs[s], offs[i]))
            continue
        two = c + at(i + 1)
        if two in ("<=", ">=", "==", "!="):
            toks.append(("op", two, offs[s], offs[i + 2]))
            i += 2
            continue
        if c in "()[].,+-*/%<>":
            toks.append(("op", c, offs[s], offs[i + 1]))
            i += 1
            continue
        raise SyntaxErr(offs[s])


# -------------------------------------------------------------------- parser
# One function per level: expr > or > and > cmp > add > mul > unary > postfix > primary.


class Parser:
    def __init__(self, toks, end):
        self.t = toks
        self.p = 0
        self.end = end

    def peek(self):
        return self.t[self.p] if self.p < len(self.t) else None

    def here(self):
        tok = self.peek()
        return tok[2] if tok else self.end

    def is_(self, typ, val):
        tok = self.peek()
        return tok is not None and tok[0] == typ and tok[1] == val

    def want(self, typ, val):
        if not self.is_(typ, val):
            raise SyntaxErr(self.here())
        tok = self.t[self.p]
        self.p += 1
        return tok

    def expr(self):
        if self.is_("kw", "if"):
            s = self.here()
            self.p += 1
            c = self.expr()
            self.want("kw", "then")
            a = self.expr()
            self.want("kw", "else")
            b = self.expr()
            return {"k": "cond", "if": c, "then": a, "else": b, "span": [s, b["span"][1]]}
        return self.level(0)

    LEVELS = [
        [("kw", "or")],
        [("kw", "and")],
        [("op", o) for o in ("<", "<=", ">", ">=", "==", "!=")],
        [("op", "+"), ("op", "-")],
        [("op", "*"), ("op", "/"), ("op", "%")],
    ]

    def level(self, n):
        if n == len(self.LEVELS):
            return self.unary()
        lhs = self.level(n + 1)
        while True:
            tok = self.peek()
            if tok is None or (tok[0], tok[1]) not in self.LEVELS[n]:
                return lhs
            self.p += 1
            rhs = self.level(n + 1)
            lhs = {"k": "binary", "op": tok[1], "lhs": lhs, "rhs": rhs, "span": [lhs["span"][0], rhs["span"][1]]}

    def unary(self):
        if self.is_("op", "-") or self.is_("kw", "not"):
            tok = self.t[self.p]
            self.p += 1
            arg = self.unary()
            return {"k": "unary", "op": tok[1], "arg": arg, "span": [tok[2], arg["span"][1]]}
        return self.postfix()

    def postfix(self):
        e = self.primary()
        while True:
            if self.is_("op", "."):
                self.p += 1
                tok = self.peek()
                if tok is None or tok[0] != "ident":
                    raise SyntaxErr(self.here())
                self.p += 1
                e = {"k": "field", "base": e, "name": tok[1], "span": [e["span"][0], tok[3]]}
            elif self.is_("op", "["):
                self.p += 1
                ix = self.expr()
                close = self.want("op", "]")
                e = {"k": "index", "base": e, "index": ix, "span": [e["span"][0], close[3]]}
            else:
                return e

    def primary(self):
        tok = self.peek()
        if tok is None:
            raise SyntaxErr(self.end)
        typ, val, s, e = tok
        self.p += 1
        span = [s, e]
        if typ == "int":
            return {"k": "int", "v": str(val), "span": span}
        if typ == "dec":
            return {"k": "dec", "v": val, "span": span}
        if typ == "qty":
            return {"k": "qty", "v": val[0], "unit": val[1], "span": span}
        if typ == "str":
            return {"k": "str", "v": val, "span": span}
        if typ == "kw" and val in ("true", "false"):
            return {"k": "bool", "v": val == "true", "span": span}
        if typ == "ident":
            if self.is_("op", "("):
                self.p += 1
                args = []
                if not self.is_("op", ")"):
                    while True:
                        args.append(self.expr())
                        if self.is_("op", ","):
                            self.p += 1
                            continue
                        break
                close = self.want("op", ")")
                return {"k": "call", "name": val, "args": args, "span": [s, close[3]]}
            return {"k": "ident", "name": val, "span": span}
        if typ == "op" and val == "(":
            inner = self.expr()
            close = self.want("op", ")")
            inner["span"] = [s, close[3]]
            return inner
        raise SyntaxErr(s)


def parse(src):
    """Errors are reported in source order: the parser runs over the tokens before a lexical
    error, and the lexical error wins unless the parser fails earlier."""
    toks, lex_err, total = lex(src)
    p = Parser(toks, lex_err.offset if lex_err else total)
    try:
        e = p.expr()
        if p.peek() is not None:
            raise SyntaxErr(p.here())
    except SyntaxErr as perr:
        if lex_err is None or perr.offset < lex_err.offset:
            raise
        raise lex_err from None
    if lex_err is not None:
        raise lex_err
    return e


# --------------------------------------------------------------------- cases
# One valid file per production (several for the comparison operators).

VALID = {
    "int": "42",
    "int-zero": "0",
    "int-max": "9223372036854775807",
    "dec": "12.700",
    "qty-dec": "12.700[mm]",
    "qty-int": "30[deg]",
    "qty-micro": "5[µm]",
    "qty-compound": "9.81[m/s^2]",
    "str": '"probe \\"A\\"\\n"',
    "str-unicode-escape": '"\\u{41}\\u{3bc}"',
    "str-utf8": '"Ø 12"',
    "bool-true": "true",
    "bool-false": "false",
    "ident": "nominal_d1",
    "ident-keyword-prefix": "iffy or notable",
    "field": "r.diameter",
    "field-chain": "r.circle.center.x",
    "index": "pts[0]",
    "index-expr": "pts[i + 1].x",
    "unary-neg": "-x",
    "unary-not": "not done",
    "unary-nested": "- -x",
    "unary-binds-tighter": "not a == b",
    "mul-div-rem": "a * b / c % d",
    "add-sub": "a + b - c",
    "cmp-lt": "a < b",
    "cmp-le": "a <= b",
    "cmp-gt": "a > b",
    "cmp-ge": "a >= b",
    "cmp-eq": "a == b",
    "cmp-ne": "a != b",
    "cmp-left-assoc": "a < b < c",
    "and": "a and b and c",
    "or": "a or b and c",
    "precedence": "-a * b + c < d and not e or f",
    "parens": "(a + b) * c",
    "parens-nested": "((x))",
    "cond": "if r.d > 10.0[mm] then 1 else 2",
    "cond-nested-else": "if a then b else if c then d else e",
    "cond-in-parens": "1 + (if a then 2 else 3)",
    "call-empty": "pi()",
    "call-args": "max(a, b, 3)",
    "call-postfix": "f(x).y[0]",
    "whitespace": "  a\t+\nb  ",
}

# name: (source, hand-labelled byte offset of the first offending token)
INVALID = {
    "unbalanced-open-paren": ("(a + b", 6),
    "unbalanced-close-paren": ("a + b)", 5),
    "unbalanced-index": ("x[1", 3),
    "unbalanced-call": ("f(1, 2", 6),
    "unbalanced-nested": ("((a)", 4),
    "stray-close-bracket": ("]", 0),
    "qty-unclosed": ("12.7[", 4),
    "qty-empty-unit": ("12.7[]", 4),
    "qty-bad-unit-char": ("3[m m]", 1),
    "assign": ("a = 1", 2),
    "increment": ("x++", 2),
    "walrus": ("x := 1", 2),
    "let-declaration": ("let x = 1", 4),
    "for-loop": ("for i in xs", 4),
    "command-name": ("a.b@1", 3),
    "missing-else": ("if a then b", 11),
    "missing-operand": ("1 +", 3),
    "cond-as-operand": ("1 + if a then 2 else 3", 4),
    "call-on-field": ("a.f(1)", 3),
    "field-keyword": ("x.and", 2),
    "field-number": ("a.1", 2),
    "trailing-comma": ("f(1,)", 4),
    "juxtaposed": ("1 2", 2),
    "bang": ("!x", 0),
    "string-unclosed": ('"abc', 0),
    "string-bad-escape": ('"a\\q"', 2),
    "string-raw-tab": ('"a\tb"', 2),
    "int-overflow": ("9223372036854775808", 0),
    "name-too-long": ("a" * 65, 0),
    "unknown-char": ("a # b", 2),
    "non-ascii-operator": ("a × b", 2),
    "empty": ("", 0),
}


def dump(obj):
    return json.dumps(obj, ensure_ascii=False, sort_keys=True, indent=1) + "\n"


def build():
    files = {}
    for name, src in VALID.items():
        files[f"valid/{name}.txt"] = src
        files[f"valid/{name}.json"] = dump(parse(src))
    expected = {}
    for name, (src, offset) in INVALID.items():
        try:
            parse(src)
        except SyntaxErr as e:
            if e.offset != offset:
                raise SystemExit(f"{name}: reference parser fails at {e.offset}, label says {offset}")
        else:
            raise SystemExit(f"{name}: reference parser accepts it")
        files[f"invalid/{name}.txt"] = src
        expected[name] = {"error": "expr_syntax", "offset": offset}
    files["invalid/expected.json"] = dump(expected)
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for rel, text in sorted(build().items()):
        path = HERE / rel
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        if stale:
            raise SystemExit("stale: " + ", ".join(stale))
        print("expression corpora up to date")


if __name__ == "__main__":
    main()
