#!/usr/bin/env python3
"""Independent reference reader for the IO.CAD.STEP.P21 fixtures (Metrology Maven).

CC-BY-4.0 project data. Written separately from the Rust crate fid-io-step, from the
leaf's rules and the conventions stated in crates/fid-io-step/src/{text,real}.rs docs:

- entity, type and enumeration names are upper-cased;
- strings decode '' \\ \\S\\c \\X\\hh \\X2\\...\\X0\\ (UTF-16, pairs combined)
  \\X4\\...\\X0\\ \\PA\\, line breaks inside strings are dropped, raw UTF-8 is kept;
- the canonical print has the header records in order, one DATA section with the
  instances in ascending number, '\\n' line ends; strings re-encoded with printable
  ASCII kept ('' and \\\\ doubled) and every maximal run of other characters as one
  \\X2\\ group (\\X4\\ if the run holds a character above U+FFFF), uppercase hex;
- reals use the shortest round-trip digits (Python's repr) in positional form when the
  decimal exponent of the first digit is in -5..=15 ("123.", "0.001"), else "d.dddE<exp>";
  zero is "0." or "-0.".

For each accepted fixture it writes expected/<stem>.canon.stp (canonical print) and
expected/<stem>.summary.txt (header count, instance count, dangling references).

Run (stdlib only, Python 3.8+):
  python fixtures/io/cad/step/p21/make_expected.py           # rewrite expected/
  python fixtures/io/cad/step/p21/make_expected.py --check   # fail if expected/ is stale
"""

import decimal
import math
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "expected"
ACCEPT = ["minimal", "dangling", "escapes", "reals"]

WORD = re.compile(rb"!?[A-Za-z_][A-Za-z0-9_]*")
WORD_DASH = re.compile(rb"!?[A-Za-z_][A-Za-z0-9_-]*")
DIGITS = re.compile(rb"[0-9]+")
NUMBER = re.compile(rb"[+-]?[0-9]+(\.[0-9]*([Ee][+-]?[0-9]+)?)?")


class Bad(Exception):
    pass


class Reader:
    def __init__(self, data):
        self.s = data
        self.i = 0

    def at(self, n=1):
        return self.s[self.i:self.i + n]

    def ws(self):
        while True:
            c = self.at()
            if c in (b" ", b"\t", b"\r", b"\n"):
                self.i += 1
            elif self.at(2) == b"/*":
                j = self.s.find(b"*/", self.i + 2)
                if j < 0:
                    raise Bad("unterminated comment")
                self.i = j + 2
            else:
                return

    def lit(self, b):
        self.ws()
        if self.at(len(b)) != b:
            raise Bad("expected %r at %d" % (b, self.i))
        self.i += len(b)

    def match(self, pattern):
        m = pattern.match(self.s, self.i)
        if not m:
            raise Bad("token expected at %d" % self.i)
        self.i = m.end()
        return m.group(0).decode("ascii")

    def word(self, dash=False):
        self.ws()
        return self.match(WORD_DASH if dash else WORD)

    def items(self):
        """Parameters after an opening parenthesis, through the closing one."""
        out = []
        self.ws()
        if self.at() == b")":
            self.i += 1
            return out
        while True:
            out.append(self.value())
            self.ws()
            c = self.at()
            self.i += 1
            if c == b")":
                return out
            if c != b",":
                raise Bad("expected , or ) at %d" % (self.i - 1))

    def value(self):
        self.ws()
        c = self.at()
        if c == b"(":
            self.i += 1
            return ("list", self.items())
        if c == b"$":
            self.i += 1
            return ("unset",)
        if c == b"*":
            self.i += 1
            return ("derived",)
        if c == b"#":
            self.i += 1
            return ("ref", int(self.match(DIGITS)))
        if c == b"'":
            return ("str", self.string())
        if c == b'"':
            j = self.s.index(b'"', self.i + 1)
            digits = self.s[self.i + 1:j].decode("ascii").upper()
            self.i = j + 1
            return ("bin", digits)
        if c == b".":
            self.i += 1
            name = self.match(WORD).upper()
            if self.at() != b".":
                raise Bad("enum end")
            self.i += 1
            return ("enum", name)
        if c in b"+-0123456789" and c:
            tok = self.match(NUMBER)
            if "." in tok:
                v = float(tok)
                if not math.isfinite(v):
                    raise Bad("real range")
                return ("real", v)
            v = int(tok)
            if not -(2 ** 63) <= v < 2 ** 63:
                raise Bad("int range")
            return ("int", v)
        name = self.word().upper()
        self.lit(b"(")
        inner = self.items()
        if len(inner) != 1:
            raise Bad("typed arity")
        return ("typed", name, inner[0])

    def string(self):
        s = self.s
        self.i += 1
        out = []
        while True:
            if self.i >= len(s):
                raise Bad("unterminated string")
            c = s[self.i]
            t = s[self.i:]
            if c == 0x27:
                if t.startswith(b"''"):
                    out.append("'")
                    self.i += 2
                    continue
                self.i += 1
                return "".join(out)
            if c in (0x0A, 0x0D):
                self.i += 1
            elif t.startswith(b"\\\\"):
                out.append("\\")
                self.i += 2
            elif t.startswith(b"\\S\\"):
                out.append(chr(t[3] + 0x80))
                self.i += 4
            elif t.startswith(b"\\PA\\"):
                self.i += 4
            elif t.startswith(b"\\X2\\") or t.startswith(b"\\X4\\"):
                end = t.index(b"\\X0\\", 4)
                raw = bytes.fromhex(t[4:end].decode("ascii"))
                out.append(raw.decode("utf-16-be" if t[2] == ord("2") else "utf-32-be"))
                self.i += end + 4
            elif t.startswith(b"\\X\\"):
                out.append(chr(int(t[3:5].decode("ascii"), 16)))
                self.i += 5
            elif c == 0x5C:
                raise Bad("bad escape at %d" % self.i)
            elif 0x20 <= c <= 0x7E:
                out.append(chr(c))
                self.i += 1
            elif c >= 0x80:
                n = 2 if c < 0xE0 else 3 if c < 0xF0 else 4
                out.append(s[self.i:self.i + n].decode("utf-8"))
                self.i += n
            else:
                raise Bad("control byte in string")

    def record(self):
        name = self.word().upper()
        self.lit(b"(")
        return (name, self.items())

    def file(self):
        self.lit(b"ISO-10303-21")
        self.lit(b";")
        self.lit(b"HEADER")
        self.lit(b";")
        header = []
        while True:
            self.ws()
            save = self.i
            if self.word() == "ENDSEC":
                self.lit(b";")
                break
            self.i = save
            header.append(self.record())
            self.lit(b";")
        data = {}
        while True:
            w = self.word(dash=True)
            if w == "END-ISO-10303-21":
                self.lit(b";")
                break
            if w != "DATA":
                raise Bad("section " + w)
            self.lit(b";")
            while True:
                self.ws()
                if self.at() != b"#":
                    if self.word() != "ENDSEC":
                        raise Bad("expected ENDSEC")
                    self.lit(b";")
                    break
                self.i += 1
                n = int(self.match(DIGITS))
                self.lit(b"=")
                self.ws()
                parts = []
                if self.at() == b"(":
                    self.i += 1
                    while True:
                        self.ws()
                        if self.at() == b")":
                            self.i += 1
                            break
                        parts.append(self.record())
                else:
                    parts.append(self.record())
                self.lit(b";")
                if n in data:
                    raise Bad("duplicate entity %d" % n)
                data[n] = parts
        self.ws()
        if self.i != len(self.s):
            raise Bad("trailing data")
        return header, data


def fmt_real(v):
    if v == 0:
        return "-0." if math.copysign(1.0, v) < 0 else "0."
    sign, digits, exp = decimal.Decimal(repr(v)).as_tuple()
    digits = list(digits)
    while len(digits) > 1 and digits[-1] == 0:
        digits.pop()
        exp += 1
    while len(digits) > 1 and digits[0] == 0:
        digits.pop(0)
    ds = "".join(str(d) for d in digits)
    e = exp + len(ds) - 1
    if -5 <= e <= 15:
        if e >= 0:
            ip = ds[:e + 1].ljust(e + 1, "0")
            s = ip + "." + ds[e + 1:]
        else:
            s = "0." + "0" * (-e - 1) + ds
    else:
        s = ds[0] + "." + ds[1:] + "E" + str(e)
    return ("-" if sign else "") + s


def fmt_str(text):
    out = ["'"]
    run = []

    def flush():
        if run:
            if any(ord(ch) > 0xFFFF for ch in run):
                out.append("\\X4\\" + "".join("%08X" % ord(ch) for ch in run) + "\\X0\\")
            else:
                out.append("\\X2\\" + "".join("%04X" % ord(ch) for ch in run) + "\\X0\\")
            run.clear()

    for ch in text:
        if 0x20 <= ord(ch) <= 0x7E:
            flush()
            out.append("''" if ch == "'" else "\\\\" if ch == "\\" else ch)
        else:
            run.append(ch)
    flush()
    out.append("'")
    return "".join(out)


def fmt_value(p):
    kind = p[0]
    if kind == "int":
        return str(p[1])
    if kind == "real":
        return fmt_real(p[1])
    if kind == "str":
        return fmt_str(p[1])
    if kind == "enum":
        return "." + p[1] + "."
    if kind == "bin":
        return '"' + p[1] + '"'
    if kind == "ref":
        return "#%d" % p[1]
    if kind == "typed":
        return p[1] + "(" + fmt_value(p[2]) + ")"
    if kind == "list":
        return "(" + ",".join(fmt_value(x) for x in p[1]) + ")"
    if kind == "unset":
        return "$"
    if kind == "derived":
        return "*"
    raise Bad(kind)


def fmt_record(rec):
    return rec[0] + "(" + ",".join(fmt_value(x) for x in rec[1]) + ")"


def canon(header, data):
    lines = ["ISO-10303-21;", "HEADER;"]
    lines += [fmt_record(r) + ";" for r in header]
    lines += ["ENDSEC;", "DATA;"]
    for n in sorted(data):
        parts = data[n]
        body = fmt_record(parts[0]) if len(parts) == 1 else "(" + "".join(fmt_record(r) for r in parts) + ")"
        lines.append("#%d=%s;" % (n, body))
    lines += ["ENDSEC;", "END-ISO-10303-21;"]
    return "\n".join(lines) + "\n"


def refs(p):
    if p[0] == "ref":
        yield p[1]
    elif p[0] == "list":
        for x in p[1]:
            yield from refs(x)
    elif p[0] == "typed":
        yield from refs(p[2])


def summary(header, data):
    lines = ["header %d" % len(header), "instances %d" % (len(header) + len(data))]
    dangling = set()
    for n, parts in data.items():
        for _, params in parts:
            for p in params:
                for t in refs(p):
                    if t not in data:
                        dangling.add((n, t))
    lines += ["dangling %d %d" % d for d in sorted(dangling)]
    return "\n".join(lines) + "\n"


def main(argv):
    check = "--check" in argv
    stale = []
    for stem in ACCEPT:
        header, data = Reader((HERE / (stem + ".stp")).read_bytes()).file()
        for suffix, text in ((".canon.stp", canon(header, data)), (".summary.txt", summary(header, data))):
            path = OUT / (stem + suffix)
            want = text.encode("utf-8")
            if check:
                if not path.exists() or path.read_bytes() != want:
                    stale.append(str(path))
            else:
                OUT.mkdir(exist_ok=True)
                path.write_bytes(want)
    if stale:
        print("stale: " + ", ".join(stale))
        return 1
    print("ok" if check else "wrote " + str(OUT))
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
