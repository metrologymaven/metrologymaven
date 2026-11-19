#!/usr/bin/env python3
"""Independent text-extraction check for RPT.OUTPUT.PDF.TEXT (AC-4, REQ-006). Stdlib only.

Parses each golden PDF named in expected-text.json (the Rust tests require the crate's output
to equal these files byte for byte), walks catalog -> page tree -> page -> fonts, reads each
Type0 font's /ToUnicode CMap, decodes the hex strings of every Tj/TJ in the content stream
and compares the extracted text with the hand-labelled string. It also checks the file
structure: header, every cross-reference offset pointing at its object, startxref, %%EOF,
and every stream's /Length.

Usage: python check_pdf_text.py
"""

import json
import pathlib
import re
import sys

HERE = pathlib.Path(__file__).resolve().parent

DELIM = b"()<>[]{}/%"
WS = b" \t\r\n\f\0"


class Ref:
    def __init__(self, num):
        self.num = num


class Name(str):
    pass


class Op(str):
    pass


def tokens(data, i=0, end=None):
    end = len(data) if end is None else end
    while i < end:
        c = data[i:i + 1]
        if c in (b" ", b"\t", b"\r", b"\n", b"\f", b"\0"):
            i += 1
        elif c == b"%":
            while i < end and data[i:i + 1] not in (b"\r", b"\n"):
                i += 1
        elif data[i:i + 2] in (b"<<", b">>"):
            yield data[i:i + 2].decode(), i + 2
            i += 2
        elif c in (b"[", b"]", b"{", b"}"):
            yield c.decode(), i + 1
            i += 1
        elif c == b"<":
            j = data.index(b">", i)
            h = re.sub(rb"\s", b"", data[i + 1:j])
            if len(h) % 2:
                h += b"0"
            yield bytes.fromhex(h.decode()), j + 1
            i = j + 1
        elif c == b"(":
            depth = 0
            out = bytearray()
            j = i + 1
            while True:
                b = data[j:j + 1]
                if b == b"\\":
                    nb = data[j + 1:j + 2]
                    m = re.match(rb"[0-7]{1,3}", data[j + 1:j + 4])
                    if m:
                        out.append(int(m.group(0), 8) & 0xFF)
                        j += 1 + len(m.group(0))
                        continue
                    out += {b"n": b"\n", b"r": b"\r", b"t": b"\t", b"b": b"\b", b"f": b"\f"}.get(nb, nb)
                    j += 2
                    continue
                if b == b"(":
                    depth += 1
                elif b == b")":
                    if depth == 0:
                        break
                    depth -= 1
                out += b
                j += 1
            yield bytes(out), j + 1
            i = j + 1
        elif c == b"/":
            j = i + 1
            while j < end and data[j:j + 1] not in (b" ", b"\t", b"\r", b"\n", b"\f", b"\0") and data[j] not in DELIM:
                j += 1
            raw = data[i + 1:j]
            name = re.sub(rb"#([0-9A-Fa-f]{2})", lambda m: bytes([int(m.group(1), 16)]), raw)
            yield Name(name.decode("latin-1")), j
            i = j
        else:
            j = i
            while j < end and data[j] not in WS and data[j] not in DELIM:
                j += 1
            word = data[i:j].decode("latin-1")
            if re.fullmatch(r"[+-]?(\d+\.?\d*|\.\d+)", word):
                yield (float(word) if "." in word else int(word)), j
            else:
                yield Op(word), j
            i = j


def read_objects(data):
    if not data.startswith(b"%PDF-1."):
        raise AssertionError("bad header")
    tail = data[-64:]
    m = re.search(rb"startxref\n(\d+)\n%%EOF\n$", tail)
    assert m, "startxref / %%EOF missing"
    xref = int(m.group(1))
    assert data[xref:xref + 5] == b"xref\n", "startxref does not point at xref"
    lines = data[xref:].split(b"\n")
    count = int(lines[1].split()[1])
    offsets = {}
    for n in range(1, count):
        entry = lines[2 + n] + b"\n"
        assert len(entry) == 20, "xref entry length"
        offsets[n] = int(entry[:10])
    objs = {}
    for n, off in offsets.items():
        head = ("%d 0 obj\n" % n).encode()
        assert data[off:off + len(head)] == head, "xref offset of object %d" % n
        p = off + len(head)
        objs[n] = parse_object(data, p)
    return objs


def parse_object(data, p):
    # Collect raw tokens up to `stream` or `endobj`, then build the value with references.
    toks = tokens(data, p)
    raw = []
    for t, pos in toks:
        if t in ("stream", "endobj"):
            break
        raw.append((t, pos))
    value = build(raw)
    if t == "stream":
        length = value["Length"]
        start = pos + 1  # after "stream\n"
        assert data[pos:pos + 1] == b"\n"
        s = data[start:start + length]
        assert data[start + length:start + length + 10] == b"\nendstream", "stream /Length"
        return (value, s)
    return (value, None)


def build(raw):
    vals = [t for t, _ in raw]
    out, i = _build(vals, 0)
    return out


def _build(v, i):
    t = v[i]
    if t == "<<":
        d = {}
        i += 1
        while v[i] != ">>":
            k = v[i]
            val, i = _build(v, i + 1)
            d[k] = val
        return d, i + 1
    if t == "[":
        arr = []
        i += 1
        while v[i] != "]":
            val, i = _build(v, i)
            arr.append(val)
        return arr, i + 1
    if isinstance(t, int) and i + 2 < len(v) and isinstance(v[i + 1], int) and isinstance(v[i + 2], Op) and v[i + 2] == "R":
        return Ref(t), i + 3
    return t, i + 1


def deref(objs, v):
    while isinstance(v, Ref):
        v = objs[v.num][0]
    return v


def tounicode(stream):
    m = {}
    toks = [t for t, _ in tokens(stream)]
    i = 0
    while i < len(toks):
        if toks[i] == "beginbfchar":
            i += 1
            while toks[i] != "endbfchar":
                code, uni = toks[i], toks[i + 1]
                m[int.from_bytes(code, "big")] = uni.decode("utf-16-be")
                i += 2
        elif toks[i] == "beginbfrange":
            i += 1
            while toks[i] != "endbfrange":
                lo, hi, dst = toks[i], toks[i + 1], toks[i + 2]
                a, b = int.from_bytes(lo, "big"), int.from_bytes(hi, "big")
                if isinstance(dst, bytes):
                    base = int.from_bytes(dst, "big")
                    for k in range(a, b + 1):
                        m[k] = (base + k - a).to_bytes(len(dst), "big").decode("utf-16-be")
                i += 3
        i += 1
    return m


def extract(data):
    objs = read_objects(data)
    m = re.search(rb"trailer\n<< /Size (\d+) /Root (\d+) 0 R >>", data)
    assert m, "trailer"
    trailer_root = int(m.group(2))
    cat = objs[trailer_root][0]
    assert cat["Type"] == "Catalog"
    pages = deref(objs, cat["Pages"])
    text = []
    for kid in pages["Kids"]:
        page = deref(objs, kid)
        assert page["Type"] == "Page"
        fonts = deref(objs, deref(objs, page["Resources"])["Font"])
        maps = {}
        for name, ref in fonts.items():
            f = deref(objs, ref)
            assert f["Subtype"] == "Type0" and f["Encoding"] == "Identity-H"
            maps[name] = tounicode(objs[f["ToUnicode"].num][1])
        content = objs[page["Contents"].num][1]
        cur = None
        stack = []
        for t, _ in tokens(content):
            if isinstance(t, Op):
                if t == "Tf":
                    cur = stack[-2]
                elif t == "Tj":
                    text.append(decode(maps[cur], stack[-1]))
                elif t == "TJ":
                    text.append("".join(decode(maps[cur], s) for s in stack if isinstance(s, bytes)))
                stack = []
            else:
                stack.append(t)
    return "".join(text)


def decode(cmap, b):
    return "".join(cmap.get(int.from_bytes(b[i:i + 2], "big"), "�") for i in range(0, len(b), 2))


def main():
    expected = json.loads((HERE / "expected-text.json").read_text(encoding="utf-8"))
    bad = 0
    for name, want in sorted(expected.items()):
        got = extract((HERE / name).read_bytes())
        if got != want:
            print("%s: extracted %a, expected %a" % (name, got, want))
            bad += 1
        else:
            print("%s: %a ok" % (name, got))
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
