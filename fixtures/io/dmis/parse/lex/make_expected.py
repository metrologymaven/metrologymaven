#!/usr/bin/env python3
"""Independent reference lexer for spec leaf IO.DMIS.PARSE.LEX (stdlib only).

Lexes every hand-written ``*.dmi`` fixture in this folder with the lexical rules of the
leaf spec (as written down in the crate docs of ``fid-io-dmis::lex``, strict options) and
writes the expected token stream next to it as ``<name>.tokens``. ``--check`` compares
instead of writing. The ``.dmi`` fixtures are short snippets written by the project
(CC-BY-4.0), not copied from any standard. This script shares no code with the Rust lexer; it uses regular
expressions where the Rust code walks bytes.

Output format, one record per line, fields separated by one TAB:

    statement <first_line> <last_line>
    <line>:<col> <kind> <lexeme> [<extra>...]
    error malformed <reason> <line> <col>
    error limit_exceeded <bound> <line>

Extras: word -> upper-case form; label -> prefix, name; string -> decoded value;
comment -> text after ``$$``. Byte strings are escaped: printable ASCII except ``\\`` as
is, ``\\`` as ``\\\\``, every other byte as ``\\xHH``.
"""

import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent
MAX_LINE = 64 * 1024
MAX_CHAIN = 10_000
MAX_TOKENS = 100_000

NUMBER = re.compile(rb"[+-]?(?:[0-9]+(?:\.[0-9]*)?|\.[0-9]+)(?:[eE][+-]?[0-9]+)?")
WORD = re.compile(rb"[A-Za-z][A-Za-z0-9_]*")
LABEL = re.compile(rb"([A-Za-z]+)\(([A-Za-z0-9_]+)\)")
PUNCT = {ord("/"): "slash", ord(","): "comma", ord("="): "equals", ord("("): "lparen", ord(")"): "rparen"}
NAME_BYTES = set(b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789_")


class Fail(Exception):
    def __init__(self, *fields):
        super().__init__(fields)
        self.fields = fields


def esc(b):
    out = []
    for c in b:
        if c == 0x5C:
            out.append("\\\\")
        elif 0x20 <= c <= 0x7E:
            out.append(chr(c))
        else:
            out.append("\\x%02x" % c)
    return "".join(out)


class Statement:
    def __init__(self, first):
        self.first = first
        self.last = first
        self.lines = 0
        self.tokens = []  # (line, col, kind, lexeme bytes, [extra bytes])
        self.marker = None
        self.commas = 0
        self.raw_next = False

    def add(self, line, col, kind, lexeme, extra=()):
        if len(self.tokens) >= MAX_TOKENS:
            raise Fail("limit_exceeded", "statement_tokens", self.first)
        self.tokens.append((line, col, kind, lexeme, list(extra)))

    def is_text(self):
        return bool(self.tokens) and self.tokens[0][2] == "word" and self.tokens[0][4][0] == b"TEXT"


def lex_line(raw, n, st):
    """Adds the tokens of one non-comment line; True if the line is continued."""
    i, end = 0, len(raw)
    while i < end:
        c = raw[i]
        col = i + 1
        if c in (0x20, 0x09):
            i += 1
            continue
        if st.raw_next:
            st.raw_next = False
            if c != 0x27:
                st.add(n, col, "raw_text", raw[i:])
                return False
        if c == 0x27:
            j, value = i + 1, bytearray()
            while True:
                if j >= end:
                    raise Fail("malformed", "unterminated_string", n, col)
                if raw[j] == 0x27:
                    if j + 1 < end and raw[j + 1] == 0x27:
                        value.append(0x27)
                        j += 2
                        continue
                    j += 1
                    break
                value.append(raw[j])
                j += 1
            st.add(n, col, "string", raw[i:j], [bytes(value)])
            i = j
            continue
        if c in PUNCT:
            st.add(n, col, PUNCT[c], raw[i : i + 1])
            if c == ord(","):
                if st.commas == 0 and st.is_text():
                    st.raw_next = True
                st.commas += 1
            i += 1
            continue
        m = LABEL.match(raw, i)
        if m:
            st.add(n, col, "label", m.group(0), [m.group(1).upper(), m.group(2)])
            i = m.end()
            continue
        m = WORD.match(raw, i)
        if m:
            st.add(n, col, "word", m.group(0), [m.group(0).upper()])
            i = m.end()
            continue
        if c in b"0123456789.+-":
            m = NUMBER.match(raw, i)
            if not m or (m.end() < end and (raw[m.end()] in NAME_BYTES or raw[m.end()] == ord("."))):
                raise Fail("malformed", "bad_number", n, col)
            st.add(n, col, "number", m.group(0))
            i = m.end()
            continue
        if c == ord("$"):
            if raw[i + 1 :].strip(b" \t") == b"":
                st.marker = (n, col)
                return True
            raise Fail("malformed", "unexpected_byte", n, col)
        if c >= 0x80:
            raise Fail("malformed", "non_ascii", n, col)
        raise Fail("malformed", "unexpected_byte", n, col)
    return False


def lex(data):
    if data.startswith(b"\xef\xbb\xbf"):
        data = data[3:]
    lines = data.split(b"\n")
    if lines and lines[-1] == b"":
        lines.pop()
    out, st = [], None
    for n, raw in enumerate(lines, 1):
        if raw.endswith(b"\r"):
            raw = raw[:-1]
        if len(raw) > MAX_LINE:
            raise Fail("limit_exceeded", "line_length", n)
        body = raw.lstrip(b" \t")
        if st is None and body == b"":
            continue
        if st is None:
            st = Statement(n)
        st.lines += 1
        if st.lines > MAX_CHAIN:
            raise Fail("limit_exceeded", "continuation_lines", st.first)
        st.last = n
        if body == b"":
            continue
        if body.startswith(b"$$"):
            st.add(n, len(raw) - len(body) + 1, "comment", body, [body[2:]])
            if st.lines == 1:
                out.append(st)
                st = None
            continue
        if not lex_line(raw, n, st):
            out.append(st)
            st = None
    if st is not None:
        raise Fail("malformed", "dangling_continuation", *st.marker)
    return out


def render(data):
    try:
        stmts = lex(data)
    except Fail as e:
        return "\t".join(["error"] + [str(f) for f in e.fields]) + "\n"
    rows = []
    for st in stmts:
        rows.append("statement\t%d\t%d" % (st.first, st.last))
        for line, col, kind, lexeme, extra in st.tokens:
            rows.append("\t".join(["%d:%d" % (line, col), kind, esc(lexeme)] + [esc(x) for x in extra]))
    return "".join(r + "\n" for r in rows)


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for dmi in sorted(HERE.glob("*.dmi")):
        want = render(dmi.read_bytes()).encode("ascii")
        target = dmi.with_suffix(".tokens")
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
