#!/usr/bin/env python3
"""Independent reference for RPT.OUTPUT.PDF.TEXT (stdlib only; never runs the Rust code).

Re-implements, from the spec node's rules, the font reading, shaping, subsetting, ToUnicode
CMap and one-page PDF of crate fid-report-pdf, and writes the expected outputs the Rust tests
compare byte for byte:

  expected-shapes.txt   AC-1: glyph IDs, advances (um), clusters, runs and warnings of
                        every case of strings.json (bundled fonts + script-test.ttf)
  expected-subsets.txt  AC-2: 120 distinct glyphs of sans regular -> 121-glyph subset
                        (tag, SHA-256, size); composite-test.ttf subset of "C"
  ac4-value.pdf         AC-4: "-0,050" in sans regular on an A4 page
  mixed.pdf             two subsets (sans bold + script-test) on one page

It also checks that the generated AC-4 PDF contains the hand-labelled text object
(ac4-text-object.txt) and ToUnicode CMap (ac4-tounicode.cmap) byte for byte; those two files
are written by hand from the spec's rules, not by this script.

Usage: python make_fixtures.py [--check]
"""

import hashlib
import json
import pathlib
import struct
import sys

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
FONTS = ROOT / "crates" / "fid-report-pdf" / "assets" / "fonts"

BUNDLED = [
    ("sans-regular", "MavenReportSans-Regular"),
    ("sans-bold", "MavenReportSans-Bold"),
    ("mono-regular", "MavenReportMono-Regular"),
    ("mono-bold", "MavenReportMono-Bold"),
]


def rhe(n, d):
    """n / d rounded half to even (d > 0)."""
    q, r = divmod(n, d)
    if 2 * r > d:
        return q + 1
    if 2 * r == d:
        return q + (q & 1)
    return q


# ---------------------------------------------------------------------------------------
# Font reading


class Font:
    def __init__(self, fid, ps_name, data):
        self.id = fid
        self.ps_name = ps_name
        self.data = data
        n = struct.unpack(">H", data[4:6])[0]
        self.tables = {}
        for i in range(n):
            tag, _, off, ln = struct.unpack(">4sIII", data[12 + 16 * i: 28 + 16 * i])
            self.tables[tag] = data[off:off + ln]
        head = self.tables[b"head"]
        self.upem = struct.unpack(">H", head[18:20])[0]
        self.bbox = struct.unpack(">hhhh", head[36:44])
        loca_long = struct.unpack(">h", head[50:52])[0] == 1
        self.num_glyphs = struct.unpack(">H", self.tables[b"maxp"][4:6])[0]
        hhea = self.tables[b"hhea"]
        self.ascender, self.descender = struct.unpack(">hh", hhea[4:8])
        nhm = struct.unpack(">H", hhea[34:36])[0]
        hmtx = self.tables[b"hmtx"]
        self.metrics = []
        for g in range(self.num_glyphs):
            if g < nhm:
                self.metrics.append(struct.unpack(">Hh", hmtx[4 * g: 4 * g + 4]))
            else:
                lsb = struct.unpack(">h", hmtx[4 * nhm + 2 * (g - nhm): 4 * nhm + 2 * (g - nhm) + 2])[0]
                self.metrics.append((self.metrics[nhm - 1][0], lsb))
        loca = self.tables[b"loca"]
        if loca_long:
            self.loca = [struct.unpack(">I", loca[4 * i: 4 * i + 4])[0] for i in range(self.num_glyphs + 1)]
        else:
            self.loca = [2 * struct.unpack(">H", loca[2 * i: 2 * i + 2])[0] for i in range(self.num_glyphs + 1)]
        self.cmap = self._read_cmap(self.tables[b"cmap"])

    def _read_cmap(self, c):
        n = struct.unpack(">H", c[2:4])[0]
        best = None
        for i in range(n):
            plat, enc, off = struct.unpack(">HHI", c[4 + 8 * i: 12 + 8 * i])
            fmt = struct.unpack(">H", c[off:off + 2])[0]
            rank = {(3, 10, 12): 0, (0, 4, 12): 1, (0, 6, 12): 1, (3, 1, 4): 2, (0, 3, 4): 3}.get((plat, enc, fmt))
            if rank is None and plat == 0 and enc <= 2 and fmt == 4:
                rank = 4
            if rank is not None and (best is None or rank < best[0]):
                best = (rank, off)
        rank, off = best
        m = {}
        if rank <= 1:
            ngroups = struct.unpack(">I", c[off + 12: off + 16])[0]
            for g in range(ngroups):
                s, e, gid = struct.unpack(">III", c[off + 16 + 12 * g: off + 28 + 12 * g])
                for cp in range(s, e + 1):
                    m[cp] = gid + cp - s
        else:
            seg = struct.unpack(">H", c[off + 6: off + 8])[0] // 2
            ends = off + 14
            starts = ends + 2 * seg + 2
            deltas = starts + 2 * seg
            ranges = deltas + 2 * seg
            for i in range(seg):
                e = struct.unpack(">H", c[ends + 2 * i: ends + 2 * i + 2])[0]
                s = struct.unpack(">H", c[starts + 2 * i: starts + 2 * i + 2])[0]
                dl = struct.unpack(">H", c[deltas + 2 * i: deltas + 2 * i + 2])[0]
                ro = struct.unpack(">H", c[ranges + 2 * i: ranges + 2 * i + 2])[0]
                for cp in range(s, e + 1):
                    if ro == 0:
                        g = (cp + dl) & 0xFFFF
                    else:
                        a = ranges + 2 * i + ro + 2 * (cp - s)
                        g = struct.unpack(">H", c[a:a + 2])[0]
                        g = 0 if g == 0 else (g + dl) & 0xFFFF
                    m[cp] = g
        return {cp: g for cp, g in m.items() if 0 < g < self.num_glyphs}

    def glyph(self, cp):
        return self.cmap.get(cp, 0)

    def glyph_data(self, g):
        return self.tables[b"glyf"][self.loca[g]:self.loca[g + 1]]


def load_fonts():
    fonts = [Font(fid, ps, (FONTS / (ps + ".ttf")).read_bytes()) for fid, ps in BUNDLED]
    spec = json.loads((HERE / "strings.json").read_text(encoding="utf-8"))
    sf = spec["script_font"]
    fonts.append(Font(sf["id"], sf["postscript_name"], (HERE / sf["file"]).read_bytes()))
    return fonts, spec


# ---------------------------------------------------------------------------------------
# Shaping: one glyph per character; selected font, then fallback order, then .notdef.


def shape(fonts, fallback, sel_id, text, size):
    sel = [f.id for f in fonts].index(sel_id)
    runs = []
    warns = []
    pos = 0
    for ch in text:
        cp = ord(ch)
        pick = None
        if fonts[sel].glyph(cp):
            pick = (sel, fonts[sel].glyph(cp))
        else:
            for i in fallback:
                if i != sel and fonts[i].glyph(cp):
                    pick = (i, fonts[i].glyph(cp))
                    break
        if pick is None:
            warns.append((cp, pos))
            pick = (sel, 0)
        fi, g = pick
        f = fonts[fi]
        adv = rhe(f.metrics[g][0] * size, f.upem)
        if runs and runs[-1]["font"] == fi:
            runs[-1]["glyphs"].append((g, adv, pos))
            runs[-1]["text"] += ch
        else:
            runs.append({"font": fi, "start": pos, "text": ch, "glyphs": [(g, adv, pos)], "size": size})
        pos += len(ch.encode("utf-8"))
    return runs, warns


# ---------------------------------------------------------------------------------------
# Subsetting


def components(g):
    if len(g) < 10 or struct.unpack(">h", g[0:2])[0] >= 0:
        return []
    out = []
    p = 10
    while True:
        flags, gid = struct.unpack(">HH", g[p:p + 4])
        out.append((p + 2, gid))
        p += 4 + (4 if flags & 1 else 2)
        if flags & 0x08:
            p += 2
        elif flags & 0x40:
            p += 4
        elif flags & 0x80:
            p += 8
        if not flags & 0x20:
            return out


def checksum(b):
    b = b + b"\0" * ((4 - len(b) % 4) % 4)
    return sum(struct.unpack(">%dI" % (len(b) // 4), b)) & 0xFFFFFFFF


def assemble(tables):
    tags = sorted(tables)
    n = len(tags)
    es = 0
    while (1 << (es + 1)) <= n:
        es += 1
    sr = (1 << es) * 16
    out = bytearray(struct.pack(">IHHHH", 0x00010000, n, sr, es, n * 16 - sr))
    off = 12 + 16 * n
    head_off = None
    for t in tags:
        d = tables[t]
        out += struct.pack(">4sIII", t, checksum(d), off, len(d))
        if t == b"head":
            head_off = off
        off += (len(d) + 3) // 4 * 4
    for t in tags:
        d = tables[t]
        out += d + b"\0" * ((4 - len(d) % 4) % 4)
    adj = (0xB1B0AFBA - checksum(bytes(out))) & 0xFFFFFFFF
    out[head_off + 8: head_off + 12] = struct.pack(">I", adj)
    return bytes(out)


def subset(font, used):
    """used: dict original gid -> character or None. Returns a dict describing the subset."""
    used = dict(used)
    used.setdefault(0, None)
    used[0] = None
    glyphs = set(used)
    frontier = sorted(glyphs)
    while frontier:
        nxt = []
        for g in frontier:
            for _, c in components(font.glyph_data(g)):
                if c not in glyphs:
                    glyphs.add(c)
                    nxt.append(c)
        frontier = nxt
    originals = sorted(glyphs)
    new = {g: i for i, g in enumerate(originals)}
    glyf = b""
    loca = []
    for g in originals:
        loca.append(len(glyf))
        b = bytearray(font.glyph_data(g))
        for pos, c in components(bytes(b)):
            b[pos:pos + 2] = struct.pack(">H", new[c])
        glyf += bytes(b) + b"\0" * ((4 - len(b) % 4) % 4)
    loca.append(len(glyf))
    n = len(originals)
    head = bytearray(font.tables[b"head"])
    head[8:12] = b"\0\0\0\0"
    head[50:52] = struct.pack(">H", 1)
    hhea = bytearray(font.tables[b"hhea"])
    hhea[34:36] = struct.pack(">H", n)
    maxp = bytearray(font.tables[b"maxp"])
    maxp[4:6] = struct.pack(">H", n)
    tables = {
        b"glyf": glyf,
        b"head": bytes(head),
        b"hhea": bytes(hhea),
        b"hmtx": b"".join(struct.pack(">Hh", *font.metrics[g]) for g in originals),
        b"loca": b"".join(struct.pack(">I", o) for o in loca),
        b"maxp": bytes(maxp),
    }
    for t in (b"cvt ", b"fpgm", b"prep"):
        if t in font.tables:
            tables[t] = font.tables[t]
    data = assemble(tables)
    dig = hashlib.sha256(b"".join(struct.pack(">H", g) for g in originals)).digest()
    tag = "".join(chr(65 + dig[i] % 26) for i in range(6))
    entries = sorted((new[g], c) for g, c in used.items() if c is not None)
    return {
        "font": font,
        "tag": tag,
        "bytes": data,
        "originals": originals,
        "new": new,
        "cmap": tounicode(entries),
        "widths": [rhe(font.metrics[g][0] * 1000, font.upem) for g in originals],
    }


def tounicode(entries):
    s = ("/CIDInit /ProcSet findresource begin\n12 dict begin\nbegincmap\n"
         "/CIDSystemInfo << /Registry (Adobe) /Ordering (UCS) /Supplement 0 >> def\n"
         "/CMapName /Adobe-Identity-UCS def\n/CMapType 2 def\n"
         "1 begincodespacerange\n<0000> <FFFF>\nendcodespacerange\n")
    for i in range(0, len(entries), 100):
        block = entries[i:i + 100]
        s += "%d beginbfchar\n" % len(block)
        for g, ch in block:
            u = ch.encode("utf-16-be").hex().upper()
            s += "<%04X> <%s>\n" % (g, u)
        s += "endbfchar\n"
    s += "endcmap\nCMapName currentdict /CMap defineresource pop\nend\nend\n"
    return s.encode("ascii")


def used_of(runs, font_index):
    used = {}
    for r in runs:
        if r["font"] != font_index:
            continue
        for g, _, cl in r["glyphs"]:
            ch = r["text"].encode("utf-8")[cl - r["start"]:].decode("utf-8")[0]
            if g == 0:
                used.setdefault(0, None)
                continue
            prev = used.get(g)
            used[g] = ch if prev is None else min(prev, ch)
    return used


# ---------------------------------------------------------------------------------------
# PDF


def mpt(um):
    return rhe(um * 360, 127)


def fmt_milli(v):
    sign = "-" if v < 0 else ""
    a = abs(v)
    return "%s%d.%03d" % (sign, a // 1000, a % 1000)


def pdf_name(s):
    out = "/"
    for b in s.encode("utf-8"):
        if 0x21 <= b <= 0x7E and chr(b) not in "#()<>[]{}/%":
            out += chr(b)
        else:
            out += "#%02X" % b
    return out


def text_object(res, sub, run, x, y):
    ids = "".join("%04X" % sub["new"][g] for g, _, _ in run["glyphs"])
    return ("BT\n%s %s Tf\n%s %s Td\n<%s> Tj\nET\n" % (
        pdf_name(res), fmt_milli(mpt(run["size"])), fmt_milli(mpt(x)), fmt_milli(mpt(y)), ids)).encode("ascii")


def single_page(w, h, subs, placed):
    content = b""
    for run, x, y in placed:
        k = [s["font"] for s in subs].index(run["font_obj"])
        content += text_object("F%d" % (k + 1), subs[k], run, x, y)
    out = bytearray(b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n")
    offsets = []

    def obj(body):
        offsets.append(len(out))
        out.extend(("%d 0 obj\n" % len(offsets)).encode("ascii"))
        out.extend(body)
        out.extend(b"\nendobj\n")

    def stream(extra, data):
        obj(("<< /Length %d%s >>\nstream\n" % (len(data), extra)).encode("ascii") + data + b"\nendstream")

    obj(b"<< /Type /Catalog /Pages 2 0 R >>")
    obj(b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>")
    fonts = " ".join("/F%d %d 0 R" % (k + 1, 5 + 5 * k) for k in range(len(subs)))
    obj(("<< /Type /Page /Parent 2 0 R /MediaBox [0 0 %s %s] /Resources << /Font << %s >> >> /Contents 4 0 R >>" % (
        fmt_milli(mpt(w)), fmt_milli(mpt(h)), fonts)).encode("ascii"))
    stream("", content)
    for k, s in enumerate(subs):
        i = 5 + 5 * k
        f = s["font"]
        base = pdf_name(s["tag"] + "+" + f.ps_name)
        obj(("<< /Type /Font /Subtype /Type0 /BaseFont %s /Encoding /Identity-H /DescendantFonts [%d 0 R] /ToUnicode %d 0 R >>" % (base, i + 1, i + 4)).encode("ascii"))
        obj(("<< /Type /Font /Subtype /CIDFontType2 /BaseFont %s /CIDSystemInfo << /Registry (Adobe) /Ordering (Identity) /Supplement 0 >> /FontDescriptor %d 0 R /DW 0 /W [0 [%s]] /CIDToGIDMap /Identity >>" % (
            base, i + 2, " ".join(str(w) for w in s["widths"]))).encode("ascii"))
        sc = lambda v: rhe(v * 1000, f.upem)  # noqa: E731
        bb = [sc(v) for v in f.bbox]
        obj(("<< /Type /FontDescriptor /FontName %s /Flags 4 /FontBBox [%d %d %d %d] /ItalicAngle 0 /Ascent %d /Descent %d /CapHeight %d /StemV 80 /FontFile2 %d 0 R >>" % (
            base, bb[0], bb[1], bb[2], bb[3], sc(f.ascender), sc(f.descender), sc(f.ascender), i + 3)).encode("ascii"))
        stream(" /Length1 %d" % len(s["bytes"]), s["bytes"])
        stream("", s["cmap"])
    xref = len(out)
    n = len(offsets) + 1
    tail = "xref\n0 %d\n0000000000 65535 f \n" % n
    for o in offsets:
        tail += "%010d 00000 n \n" % o
    tail += "trailer\n<< /Size %d /Root 1 0 R >>\nstartxref\n%d\n%%%%EOF\n" % (n, xref)
    out.extend(tail.encode("ascii"))
    return bytes(out)


# ---------------------------------------------------------------------------------------
# Outputs

AC2_TEXT = "".join(chr(c) for c in range(0x21, 0x7F)) + "".join(chr(c) for c in range(0xC0, 0xDA))
A4 = (210000, 297000)


def outputs():
    fonts, spec = load_fonts()
    fallback = list(range(len(fonts)))  # bundled order, then the script font
    size = spec["size_um"]
    out = {}

    lines = ["# Expected shaping (AC-1). Generated by make_fixtures.py from strings.json and the font",
             "# files; never by the Rust code. Hex fields are UTF-8 bytes.",
             "# case/text/font/size, then per run: run <font> <start> <text hex>, g <glyph> <advance um> <cluster>;",
             "# warn <code point hex> <cluster>; end."]
    for case in spec["cases"]:
        runs, warns = shape(fonts, fallback, case["font"], case["text"], size)
        lines.append("case " + case["name"])
        lines.append("text " + case["text"].encode("utf-8").hex())
        lines.append("font " + case["font"])
        lines.append("size %d" % size)
        for r in runs:
            lines.append("run %s %d %s" % (fonts[r["font"]].id, r["start"], r["text"].encode("utf-8").hex()))
            for g, adv, cl in r["glyphs"]:
                lines.append("g %d %d %d" % (g, adv, cl))
        for cp, cl in warns:
            lines.append("warn %04X %d" % (cp, cl))
        lines.append("end")
    out["expected-shapes.txt"] = ("\n".join(lines) + "\n").encode("utf-8")

    sans = fonts[0]
    assert len(set(AC2_TEXT)) == 120
    runs, warns = shape(fonts, fallback, "sans-regular", AC2_TEXT, size)
    assert not warns and len(runs) == 1
    s1 = subset(sans, used_of(runs, 0))
    comp = Font("composite-test", "MavenCompositeTest-Regular", (HERE / "composite-test.ttf").read_bytes())
    s2 = subset(comp, {comp.glyph(ord("C")): "C"})
    sub_lines = ["# Expected subsets. Generated by make_fixtures.py; never by the Rust code.",
                 "ac2-glyphs %d" % len(s1["originals"]),
                 "ac2-tag " + s1["tag"],
                 "ac2-sha256 " + hashlib.sha256(s1["bytes"]).hexdigest(),
                 "ac2-bytes %d" % len(s1["bytes"]),
                 "ac2-cmap-sha256 " + hashlib.sha256(s1["cmap"]).hexdigest(),
                 "composite-originals " + " ".join(str(g) for g in s2["originals"]),
                 "composite-tag " + s2["tag"],
                 "composite-sha256 " + hashlib.sha256(s2["bytes"]).hexdigest()]
    out["expected-subsets.txt"] = ("\n".join(sub_lines) + "\n").encode("ascii")

    # AC-4
    runs, warns = shape(fonts, fallback, "sans-regular", "-0,050", size)
    assert not warns and len(runs) == 1
    sub = subset(sans, used_of(runs, 0))
    run = dict(runs[0], font_obj=sans)
    out["ac4-value.pdf"] = single_page(A4[0], A4[1], [sub], [(run, 25000, 270000)])

    # Two fonts: sans bold for Latin, the script font by fallback for CJK.
    text = "Maß 日本 12,5"
    runs, warns = shape(fonts, fallback, "sans-bold", text, size)
    assert not warns
    subs = []
    for fi in sorted({r["font"] for r in runs}, key=lambda i: [r["font"] for r in runs].index(i)):
        subs.append(subset(fonts[fi], used_of(runs, fi)))
    placed = []
    x = 25000
    for r in runs:
        placed.append((dict(r, font_obj=fonts[r["font"]]), x, 250000))
        x += sum(a for _, a, _ in r["glyphs"])
    out["mixed.pdf"] = single_page(A4[0], A4[1], subs, placed)
    return out, sub


def main(argv):
    check = "--check" in argv[1:]
    out, ac4_sub = outputs()
    bad = 0
    # Hand-labelled AC-4 files must appear verbatim in the reference PDF.
    pdf = out["ac4-value.pdf"]
    for name in ("ac4-text-object.txt", "ac4-tounicode.cmap"):
        want = (HERE / name).read_bytes()
        if want not in pdf:
            print("hand-labelled %s does not match the reference PDF" % name)
            bad += 1
    if (HERE / "ac4-tounicode.cmap").read_bytes() != ac4_sub["cmap"]:
        print("hand-labelled ac4-tounicode.cmap differs from the reference CMap")
        bad += 1
    for name, data in out.items():
        p = HERE / name
        if check:
            if not p.exists() or p.read_bytes() != data:
                print("differs: " + name)
                bad += 1
        else:
            p.write_bytes(data)
            print("wrote %s (%d bytes)" % (name, len(data)))
    if bad:
        return 1
    if check:
        print("rpt/output/text fixtures ok")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
