#!/usr/bin/env python3
"""Independent reference fixtures for TRUST.PKG.READER (crate fid-pkg, module reader).

Writes, without running any Rust code:

- malformed/<case>/package.zip   hostile or broken packages, one failure class each;
- malformed/<case>/expect.json    the error code (and reason tag) the reader must return,
                                  hand-labelled from the leaf's rules;
- malformed/<case>/inspect.json   for cases whose container opens but a member fails, the
                                  expected `verify.inspect_package@1` result with statuses;
- reader/valid/<name>.zip         packages the reader must accept, with
- reader/valid/<name>.inspect.json  the expected `verify.inspect_package@1` result;
- reader/inflate/<name>.deflate   raw deflate streams made once by Python's zlib, used as
                                  member data of reader/valid/zlib.zip.

Containers are assembled byte by byte with `struct`; deflate data comes from two small
encoders written here (fixed-Huffman LZ77 and a hand-built dynamic-Huffman zip bomb), from
RFC 1951 stored blocks, or from the committed zlib vectors. Every generated deflate stream is
checked against Python's zlib inflater before it is used. The zlib vectors are created only
when missing, so `--check` does not depend on the zlib build: it checks that each committed
vector still inflates to its plaintext.

CC-BY-4.0 project data. Stdlib only.

Run:  python fixtures/pkg/make_reader_fixtures.py          (writes the fixtures)
      python fixtures/pkg/make_reader_fixtures.py --check  (fails if anything would change)
"""

import hashlib
import struct
import sys
import zlib
from pathlib import Path

import make_fixtures as mf

HERE = Path(__file__).resolve().parent

U32 = 0xFFFFFFFF
U16 = 0xFFFF
SIG_LOCAL = 0x04034B50
SIG_CENTRAL = 0x02014B50
SIG_EOCD = 0x06054B50
SIG_EOCD64 = 0x06064B50
SIG_LOC64 = 0x07064B50
FLAG_UTF8 = 0x0800
DOS_TIME = mf.DOS_TIME
DOS_DATE = mf.DOS_DATE
MADE_BY = mf.MADE_BY
EXTERNAL_ATTR = mf.EXTERNAL_ATTR
MAX_MANIFEST_BYTES = 256 * 1024 * 1024


def sha(b):
    return hashlib.sha256(b).hexdigest()


def crc(b):
    return zlib.crc32(b) & 0xFFFFFFFF


# --------------------------------------------------------------- canonical JSON (+ integers)

def canon(v):
    if isinstance(v, bool):
        return "true" if v else "false"
    if isinstance(v, int):
        assert 0 <= v < 2**53
        return str(v)
    if isinstance(v, str):
        return mf.jstr(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(mf.jstr(k) + ":" + canon(v[k]) for k in keys) + "}"
    raise TypeError(repr(v))


def canon_bytes(v):
    return canon(v).encode("utf-8")


# --------------------------------------------------------------- deflate encoders

LBASE = [3, 4, 5, 6, 7, 8, 9, 10, 11, 13, 15, 17, 19, 23, 27, 31, 35, 43, 51, 59, 67, 83, 99, 115, 131, 163,
         195, 227, 258]
LEXT = [0, 0, 0, 0, 0, 0, 0, 0, 1, 1, 1, 1, 2, 2, 2, 2, 3, 3, 3, 3, 4, 4, 4, 4, 5, 5, 5, 5, 0]
DBASE = [1, 2, 3, 4, 5, 7, 9, 13, 17, 25, 33, 49, 65, 97, 129, 193, 257, 385, 513, 769, 1025, 1537, 2049, 3073,
         4097, 6145, 8193, 12289, 16385, 24577]
DEXT = [0, 0, 0, 0, 1, 1, 2, 2, 3, 3, 4, 4, 5, 5, 6, 6, 7, 7, 8, 8, 9, 9, 10, 10, 11, 11, 12, 12, 13, 13]
CL_ORDER = [16, 17, 18, 0, 8, 7, 9, 6, 10, 5, 11, 4, 12, 3, 13, 2, 14, 1, 15]


class BitWriter:
    """RFC 1951 bit packing: values LSB first, Huffman codes MSB first."""

    def __init__(self):
        self.out = bytearray()
        self.acc = 0
        self.n = 0

    def bits(self, value, count):
        assert 0 <= value < (1 << count) or count == 0
        self.acc |= value << self.n
        self.n += count
        while self.n >= 8:
            self.out.append(self.acc & 0xFF)
            self.acc >>= 8
            self.n -= 8

    def huff(self, code_len):
        code, length = code_len
        rev = 0
        for i in range(length):
            rev = (rev << 1) | ((code >> i) & 1)
        self.bits(rev, length)

    def align(self):
        if self.n:
            self.bits(0, 8 - self.n)

    def raw(self, data):
        assert self.n == 0
        self.out += data

    def finish(self):
        self.align()
        return bytes(self.out)


def canonical_codes(lengths):
    """RFC 1951 section 3.2.2: {symbol: (code, length)}."""
    max_len = max(lengths)
    bl_count = [0] * (max_len + 1)
    for length in lengths:
        if length:
            bl_count[length] += 1
    next_code = [0] * (max_len + 1)
    code = 0
    for b in range(1, max_len + 1):
        code = (code + bl_count[b - 1]) << 1
        next_code[b] = code
    codes = {}
    for s, length in enumerate(lengths):
        if length:
            codes[s] = (next_code[length], length)
            next_code[length] += 1
    return codes


FIXED_LIT = canonical_codes([8] * 144 + [9] * 112 + [7] * 24 + [8] * 8)
FIXED_DIST = canonical_codes([5] * 30)


def length_code(length):
    if length == 258:
        return 28
    i = 0
    while i + 1 < 28 and LBASE[i + 1] <= length:
        i += 1
    return i


def dist_code(d):
    i = 0
    while i + 1 < 30 and DBASE[i + 1] <= d:
        i += 1
    return i


def deflate_fixed(data):
    """One final fixed-Huffman block with greedy LZ77 (window 32 KiB, lengths 3..258)."""
    w = BitWriter()
    w.bits(1, 1)
    w.bits(1, 2)
    table = {}
    i = 0
    n = len(data)

    def insert(pos):
        if pos + 3 <= n:
            table.setdefault(data[pos:pos + 3], []).append(pos)

    while i < n:
        best_len, best_dist = 0, 0
        if i + 3 <= n:
            for j in reversed(table.get(data[i:i + 3], [])[-16:]):
                d = i - j
                if d > 32768:
                    break
                length = 0
                while length < 258 and i + length < n and data[j + length] == data[i + length]:
                    length += 1
                if length > best_len:
                    best_len, best_dist = length, d
        if best_len >= 3:
            lc = length_code(best_len)
            w.huff(FIXED_LIT[257 + lc])
            w.bits(best_len - LBASE[lc], LEXT[lc])
            dc = dist_code(best_dist)
            w.huff(FIXED_DIST[dc])
            w.bits(best_dist - DBASE[dc], DEXT[dc])
            for k in range(best_len):
                insert(i + k)
            i += best_len
        else:
            w.huff(FIXED_LIT[data[i]])
            insert(i)
            i += 1
    w.huff(FIXED_LIT[256])
    return w.finish()


def bomb_block(w, n_matches, final):
    """A dynamic-Huffman block: literal 0, then n_matches copies of (length 258, distance 1),
    each 2 bits, then end of block. Inflates to 1 + 258 * n_matches zero bytes."""
    w.bits(1 if final else 0, 1)
    w.bits(2, 2)
    w.bits(286 - 257, 5)  # HLIT: literal/length codes 0..285
    w.bits(2 - 1, 5)  # HDIST: distance codes 0..1
    cl = {18: 1, 1: 2, 2: 2}
    w.bits(18 - 4, 4)  # HCLEN: the first 18 entries of CL_ORDER (up to symbol 1)
    for s in CL_ORDER[:18]:
        w.bits(cl.get(s, 0), 3)
    clc = canonical_codes([cl.get(s, 0) for s in range(19)])
    # literal/length lengths: 0 -> 2, 1..255 -> 0, 256 -> 2, 257..284 -> 0, 285 -> 1;
    # distance lengths: 0 -> 1, 1 -> 1.
    w.huff(clc[2])
    w.huff(clc[18])
    w.bits(138 - 11, 7)
    w.huff(clc[18])
    w.bits(117 - 11, 7)
    w.huff(clc[2])
    w.huff(clc[18])
    w.bits(28 - 11, 7)
    w.huff(clc[1])
    w.huff(clc[1])
    w.huff(clc[1])
    lit = canonical_codes([2] + [0] * 255 + [2] + [0] * 28 + [1])
    dist = canonical_codes([1, 1])
    w.huff(lit[0])
    for _ in range(n_matches):
        w.huff(lit[285])
        w.huff(dist[0])
    w.huff(lit[256])


def dynamic_block(w, lit_lengths, dist_lengths, body, final):
    """A dynamic-Huffman block with the given code lengths; `body(w, lit, dist)` writes the
    symbols. Code lengths are sent with zero runs as 17/18; the code-length code gives
    length 4 to symbols 0..13, 17 and 18 (a complete code)."""
    lit_lengths = list(lit_lengths) + [0] * (286 - len(lit_lengths))
    seq = []
    lengths = lit_lengths + list(dist_lengths)
    i = 0
    while i < len(lengths):
        if lengths[i] == 0:
            run = 1
            while i + run < len(lengths) and lengths[i + run] == 0 and run < 138:
                run += 1
            if run >= 11:
                seq.append((18, run - 11, 7))
            elif run >= 3:
                seq.append((17, run - 3, 3))
            else:
                seq.extend([(0, 0, 0)] * run)
            i += run
        else:
            assert lengths[i] <= 13
            seq.append((lengths[i], 0, 0))
            i += 1
    cl = {sym: 4 for sym in list(range(14)) + [17, 18]}
    clc = canonical_codes([cl.get(x, 0) for x in range(19)])
    w.bits(1 if final else 0, 1)
    w.bits(2, 2)
    w.bits(286 - 257, 5)
    w.bits(len(dist_lengths) - 1, 5)
    w.bits(19 - 4, 4)
    for x in CL_ORDER:
        w.bits(cl.get(x, 0), 3)
    for sym, extra, nbits in seq:
        w.huff(clc[sym])
        if nbits:
            w.bits(extra, nbits)
    lit = canonical_codes(lit_lengths)
    dist = canonical_codes(list(dist_lengths))
    body(w, lit, dist)
    w.huff(lit[256])


def zeros_array_stream(n):
    """Deflate for the JSON array `[0,0,...,0]` of 2 + 129 * n zeros (5 + 258 * n bytes):
    literals `[0,0`, n copies of (length 258, distance 2), literal `]`. Each copy costs 3 bits,
    so the ratio stays near 688:1, under the 1000:1 cap."""
    lit_lengths = [0] * 286
    for sym, length in ((285, 2), (0x30, 2), (0x2C, 3), (256, 3), (0x5B, 3), (0x5D, 3)):
        lit_lengths[sym] = length

    def body(w, lit, dist):
        for ch in b"[0,0":
            w.huff(lit[ch])
        for _ in range(n):
            w.huff(lit[285])
            w.huff(dist[1])
        w.huff(lit[0x5D])

    w = BitWriter()
    dynamic_block(w, lit_lengths, [1, 1], body, True)
    comp = w.finish()
    plain = b"[0" + b",0" * (1 + 129 * n) + b"]"
    return checked(comp, plain), plain


def stored_blocks(w, data, final):
    chunks = [data[i:i + 65535] for i in range(0, len(data), 65535)] or [b""]
    for k, chunk in enumerate(chunks):
        last = final and k == len(chunks) - 1
        w.bits(1 if last else 0, 1)
        w.bits(0, 2)
        w.align()
        w.raw(struct.pack("<HH", len(chunk), len(chunk) ^ 0xFFFF) + chunk)


def inflate(comp, limit=None):
    d = zlib.decompressobj(-15)
    out = d.decompress(comp) if limit is None else d.decompress(comp, limit)
    return out, d


def checked(comp, plain):
    out, d = inflate(comp)
    assert out == plain and d.eof and not d.unused_data, "deflate stream does not inflate to its plaintext"
    return comp


# --------------------------------------------------------------- ZIP assembly with knobs

def entry(name, data, method=8, comp=None, **kw):
    """An entry: `name` (str or bytes), plaintext `data`, `method`, compressed bytes `comp`
    (default: stored blocks for deflate). Overrides in kw apply to both headers unless
    prefixed `cd_` or `local_`."""
    nb = name.encode("utf-8") if isinstance(name, str) else name
    if comp is None:
        comp = data if method == 0 else mf.stored_deflate(data)
    e = {"name": nb, "data": data, "comp": comp, "method": method, "flags": FLAG_UTF8, "needed": 20,
         "crc": crc(data), "usize": len(data), "csize": len(comp), "time": DOS_TIME, "date": DOS_DATE,
         "made_by": MADE_BY, "ext_attr": EXTERNAL_ATTR, "comment": b"", "zip64": False}
    e.update(kw)
    return e


def get(e, side, key):
    return e.get(side + "_" + key, e[key])


def build(entries, zip64_end=False, comment=b"", prefix=b"", gap_after=None, eocd_patch=None, count=None,
          eocd64_data=b""):
    """Assembles a ZIP. `gap_after` = (index, bytes) hides bytes after that entry's data.
    `eocd_patch` maps EOCD field names to values; `count` overrides the entry count."""
    out = bytearray(prefix)
    central = bytearray()
    for i, e in enumerate(entries):
        offset = e.get("offset", len(out))
        z64 = e["zip64"]
        if not e.get("skip_local", False):
            name = get(e, "local", "name")
            usize, csize = get(e, "local", "usize"), get(e, "local", "csize")
            if z64:
                extra = struct.pack("<HHQQ", 1, 16, usize, csize)
            else:
                extra = b""
            extra = e.get("local_extra", extra)
            out += struct.pack("<IHHHHHIIIHH", e.get("local_sig", SIG_LOCAL),
                               get(e, "local", "needed"), get(e, "local", "flags"), get(e, "local", "method"),
                               get(e, "local", "time"), get(e, "local", "date"), get(e, "local", "crc"),
                               U32 if z64 else csize, U32 if z64 else usize, len(name), len(extra))
            out += name + extra + e["comp"]
            if gap_after is not None and gap_after[0] == i:
                out += gap_after[1]
        usize, csize = get(e, "cd", "usize"), get(e, "cd", "csize")
        cext = b""
        if z64:
            cext = struct.pack("<HHQQ", 1, 16, usize, csize)
        cext = e.get("cd_extra", cext)
        name = get(e, "cd", "name")
        central += struct.pack("<IHHHHHHIIIHHHHHII", SIG_CENTRAL, e["made_by"], get(e, "cd", "needed"),
                               get(e, "cd", "flags"), get(e, "cd", "method"), get(e, "cd", "time"),
                               get(e, "cd", "date"), get(e, "cd", "crc"), U32 if z64 else csize,
                               U32 if z64 else usize, len(name), len(cext), len(e["comment"]),
                               e.get("disk", 0), 0, e["ext_attr"], offset)
        central += name + cext + e["comment"]
    cd_offset = len(out)
    out += central
    cd_size = len(central)
    n = len(entries) if count is None else count
    p = {"disk": 0, "cd_disk": 0, "n_disk": n, "n_total": n, "cd_size": cd_size, "cd_offset": cd_offset,
         "loc_disk": 0, "loc_total": 1}
    p.update(eocd_patch or {})
    if zip64_end:
        rec = len(out)
        out += struct.pack("<IQHHIIQQQQ", SIG_EOCD64, 44 + len(eocd64_data), 45, 45, 0, 0, p["n_disk"],
                           p["n_total"], p["cd_size"], p["cd_offset"]) + eocd64_data
        out += struct.pack("<IIQI", SIG_LOC64, p["loc_disk"], rec, p["loc_total"])
        out += struct.pack("<IHHHHIIH", SIG_EOCD, p["disk"], p["cd_disk"], U16, U16, U32, U32, len(comment))
    else:
        out += struct.pack("<IHHHHIIH", SIG_EOCD, p["disk"], p["cd_disk"], p["n_disk"], p["n_total"],
                           p["cd_size"], p["cd_offset"], len(comment))
    out += comment
    return bytes(out)


# --------------------------------------------------------------- packages

MINIMAL_MEMBERS = sorted(((m["path"], m["media_type"], mf.body_of(m)) for m in mf.MINIMAL["members"]),
                         key=lambda t: t[0].encode("utf-8"))


def manifest_of(members, contents=mf.MINIMAL, version=mf.FORMAT_VERSION):
    """members: (path, media_type, length, sha256 hex)."""
    return {
        "format": mf.FORMAT_NAME,
        "format_version": version,
        "created_by": contents["created_by"],
        "evidence_tier": contents["evidence_tier"],
        "profile_digest": contents["profile_digest"],
        "kernel_manifest_digest": contents["kernel_manifest_digest"],
        "members": [{"path": p, "media_type": mt, "length": str(n), "sha256": h} for p, mt, n, h in members],
    }


def statement_of(mbytes, members, contents=mf.MINIMAL):
    return canon_bytes({
        "_type": mf.STATEMENT_TYPE,
        "subject": [{"name": "manifest.json", "digest": {"sha256": sha(mbytes)}}]
        + [{"name": p, "digest": {"sha256": h}} for p, _, _, h in members],
        "predicateType": mf.PREDICATE_TYPE,
        "predicate": {"manifest_sha256": sha(mbytes), "format_version": mf.FORMAT_VERSION,
                      "result_count": str(contents["result_count"]), "evidence_tier": contents["evidence_tier"]},
    })


def package_entries(members=None, contents=mf.MINIMAL, listed=None, version=mf.FORMAT_VERSION, zip64=False,
                    member_kw=None, manifest_bytes=None):
    """Entries of a well-formed package: manifest.json, members, statement.json.

    members: (path, media_type, body) or (path, media_type, body, entry-kwargs).
    listed:  manifest rows (path, media_type, length, sha) if not derived from members."""
    members = MINIMAL_MEMBERS if members is None else members
    rows = listed if listed is not None else [(m[0], m[1], len(m[2]), sha(m[2])) for m in members]
    mbytes = manifest_bytes if manifest_bytes is not None else canon_bytes(manifest_of(rows, contents, version))
    sbytes = statement_of(mbytes, rows, contents)
    es = [entry("manifest.json", mbytes, zip64=zip64)]
    for m in members:
        kw = dict(m[3]) if len(m) > 3 else {}
        kw.setdefault("method", mf.method_for(m[1]))
        kw.setdefault("zip64", zip64)
        kw.update((member_kw or {}).get(m[0], {}))
        es.append(entry(m[0], m[2], **kw))
    es.append(entry("statement.json", sbytes, zip64=zip64))
    return es, rows


def inventory(rows, contents=mf.MINIMAL, signatures=0, statuses=None, version=mf.FORMAT_VERSION):
    statuses = statuses or {}
    return canon_bytes({
        "format_version": version,
        "evidence_tier": contents["evidence_tier"],
        "profile_digest": contents["profile_digest"],
        "members": [{"path": p, "media_type": mt, "length": str(n), "sha256": h, "status": statuses.get(p, "ok")}
                    for p, mt, n, h in rows],
        "signature_count": signatures,
    }) + b"\n"


def with_extra(extra_entries, **kw):
    es, rows = package_entries(**kw)
    return build(es[:-1] + extra_entries + es[-1:]), rows


def replace(es, target, **kw):
    out = []
    for e in es:
        if e["name"] == target.encode("utf-8"):
            e = dict(e)
            e.update(kw)
        out.append(e)
    return out


# --------------------------------------------------------------- zlib vectors

def lcg_bytes(seed, n):
    out = bytearray()
    x = seed
    for _ in range(n):
        x = (x * 6364136223846793005 + 1442695040888963407) & 0xFFFFFFFFFFFFFFFF
        out.append(x >> 56)
    return bytes(out)


WORDS = ["probe", "datum", "plane", "cylinder", "flatness", "tolerance", "nominal", "actual", "deviation",
         "frame", "feature", "point", "sphere", "axis", "profile", "zone", "report", "kernel", "result", "uncertainty"]


def text_plain():
    r = lcg_bytes(7, 6000)
    words = [WORDS[b % len(WORDS)] for b in r]
    lines = [" ".join(words[i:i + 12]) + "\n" for i in range(0, len(words), 12)]
    return "".join(lines).encode("utf-8")


VECTORS = {
    # name: (plaintext function, zlib level)
    "text-l9": (text_plain, 9),
    "random-l6": (lambda: lcg_bytes(11, 5000), 6),
    "lines-l1": (lambda: b"".join(b'{"seq":"%d","value":"0x3ff0000000000000"}\n' % i for i in range(1500)), 1),
}


def vector(name, check, errors):
    plain_fn, level = VECTORS[name]
    plain = plain_fn()
    path = HERE / "reader" / "inflate" / f"{name}.deflate"
    if path.exists():
        comp = path.read_bytes()
    else:
        if check:
            errors.append(f"reader/inflate/{name}.deflate missing")
            return plain, b""
        c = zlib.compressobj(level, zlib.DEFLATED, -15)
        comp = c.compress(plain) + c.flush()
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(comp)
    out, d = inflate(comp)
    if out != plain or not d.eof or d.unused_data:
        errors.append(f"reader/inflate/{name}.deflate does not inflate to its plaintext")
    return plain, comp


# --------------------------------------------------------------- cases

def malformed_cases():
    """Returns {case: (zip bytes, code, reason, note, inspect bytes or None)}."""
    cases = {}

    def add(name, data, code, reason, note, inspect=None):
        assert name not in cases
        cases[name] = (data, code, reason, note, inspect)

    base, rows = package_entries()
    golden = build(base)
    assert golden == (HERE / "golden" / "minimal.zip").read_bytes(), "the base must equal the golden package"

    # ---------------------------------------------------------- limits
    es64, _ = package_entries(zip64=True)
    add("limit_member_count", build(es64, zip64_end=True, eocd_patch={"n_disk": 1_000_001, "n_total": 1_000_001}),
        "pkg.limit", "member_count", "zip64 end record claims 1,000,001 entries (limit 1,000,000)")
    long_name = "results/" + "a" * (4097 - len("results/"))
    add("limit_path_length", with_extra([entry(long_name, b"{}")])[0], "pkg.limit", "path_length",
        "an entry name of 4097 bytes (limit 4096)")
    es = replace(base, "manifest.json", usize=MAX_MANIFEST_BYTES + 1)
    add("limit_manifest_size", build(es), "pkg.limit", "manifest_size",
        "manifest.json declares 268435457 bytes uncompressed (limit 256 MiB)")

    # AC-3 bomb: 42 KB of deflate claiming (in the manifest and both headers) 4096 bytes.
    w = BitWriter()
    bomb_block(w, 171_900, True)
    bomb = w.finish()
    head, d = inflate(bomb, 8192)
    assert len(head) == 8192 and not d.eof
    full, d = inflate(bomb)
    assert full == b"\0" * (1 + 258 * 171_900) and d.eof
    assert 40_000 <= len(bomb) <= 44_000
    claimed = b"\0" * 4096
    listed = [r if r[0] != "results/records.json" else ("results/records.json", "application/json", 4096, sha(claimed))
              for r in rows]
    members = [m if m[0] != "results/records.json" else (m[0], m[1], claimed, {"comp": bomb}) for m in MINIMAL_MEMBERS]
    es, _ = package_entries(members=members, listed=listed)
    add("bomb_member_length", build(es), "pkg.limit", "member_length",
        "AC-3: a 42 KB deflate stream that inflates to 44 MB while the manifest and headers say 4096 bytes",
        inventory(listed, statuses={"results/records.json": "limit"}))

    # Same stream, declared honestly: 44 MB from 42 KB is above 1000:1.
    big = len(full)
    assert big > 1000 * len(bomb)
    listed_b = [r if r[0] != "results/records.json" else ("results/records.json", "application/json", big, sha(full))
                for r in rows]
    members_b = [m if m[0] != "results/records.json" else (m[0], m[1], full, {"comp": bomb}) for m in MINIMAL_MEMBERS]
    es, _ = package_entries(members=members_b, listed=listed_b)
    add("bomb_declared_ratio", build(es), "pkg.limit", "ratio",
        "the central directory declares 44 MB from 42 KB, above the 1000:1 cap")

    # Declared ratio within the cap, but the stream itself runs above 1000:1 at its start.
    w = BitWriter()
    bomb_block(w, 40_000, False)
    part = 1 + 258 * 40_000
    pad = 0
    while True:
        trial = BitWriter()
        bomb_block(trial, 40_000, False)
        stored_blocks(trial, b"\x01" * pad, True)
        comp = trial.finish()
        if part + pad <= 1000 * len(comp):
            break
        pad += 64
    plain = b"\0" * part + b"\x01" * pad
    checked(comp, plain)
    listed_c = [r if r[0] != "results/records.json" else ("results/records.json", "application/json", len(plain),
                                                           sha(plain)) for r in rows]
    members_c = [m if m[0] != "results/records.json" else (m[0], m[1], plain, {"comp": comp})
                 for m in MINIMAL_MEMBERS]
    es, _ = package_entries(members=members_c, listed=listed_c)
    add("bomb_stream_ratio", build(es), "pkg.limit", "ratio",
        "declared size within 1000:1 overall, but the first block inflates at about 1030:1",
        inventory(listed_c, statuses={"results/records.json": "limit"}))

    # ---------------------------------------------------------- end records and directory
    add("truncated_tiny", golden[:10], "pkg.malformed", "truncated", "a 10-byte file")
    add("truncated_tail", golden[: len(golden) * 6 // 10], "pkg.malformed", "no_eocd",
        "the package cut at 60 %: no end record")
    cut = 60
    add("truncated_middle", golden[:cut] + golden[cut + 16:], "pkg.malformed", "cd_bounds",
        "16 bytes cut out of member data: the directory is no longer where the end record says")
    add("archive_comment", build(base, comment=b"hi"), "pkg.malformed", "archive_comment",
        "an archive comment (the format has none)")
    add("multi_disk", build(base, eocd_patch={"disk": 1}), "pkg.malformed", "multi_disk", "disk number 1")
    add("eocd_count_mismatch", build(base, eocd_patch={"n_disk": 4}), "pkg.malformed", "eocd_mismatch",
        "entries on this disk differ from the total")
    es64, _ = package_entries(zip64=True)
    add("zip64_locator", build(es64, zip64_end=True, eocd_patch={"loc_total": 2}), "pkg.malformed",
        "zip64_locator", "the zip64 locator says two disks")
    cd_off = struct.unpack_from("<I", golden, len(golden) - 6)[0]
    add("cd_offset_off_by_one", build(base, eocd_patch={"cd_offset": cd_off + 1}), "pkg.malformed", "cd_bounds",
        "the directory offset is one byte too far")
    add("cd_count_short", build(base, count=2), "pkg.malformed", "cd_size",
        "the end record counts 2 entries but the directory holds 5")

    # ---------------------------------------------------------- entry flags and types
    rec = "results/records.json"
    add("encrypted_flag", build(replace(base, rec, flags=FLAG_UTF8 | 1)), "pkg.malformed", "encrypted",
        "general-purpose bit 0 (traditional encryption)")
    add("encrypted_aes", build(replace(base, rec, method=99)), "pkg.malformed", "encrypted",
        "method 99 (AES)")
    add("data_descriptor", build(replace(base, rec, flags=FLAG_UTF8 | 8)), "pkg.malformed", "data_descriptor",
        "general-purpose bit 3 (sizes in a trailing data descriptor)")
    add("flags_patched", build(replace(base, rec, flags=FLAG_UTF8 | 0x20)), "pkg.malformed", "flags",
        "general-purpose bit 5 (patched data)")
    add("method_bzip2", build(replace(base, rec, method=12)), "pkg.malformed", "method", "method 12 (bzip2)")
    add("version_needed", build(replace(base, rec, needed=63)), "pkg.malformed", "version_needed",
        "version needed 6.3")
    add("symlink", build(replace(base, rec, ext_attr=0o120777 << 16)), "pkg.malformed", "symlink",
        "Unix mode S_IFLNK")
    add("special_dir_mode", build(replace(base, rec, ext_attr=(0o040755 << 16) | 0x10)), "pkg.malformed",
        "special_file", "Unix directory mode")
    add("special_fifo", build(replace(base, rec, ext_attr=0o010644 << 16)), "pkg.malformed", "special_file",
        "Unix FIFO mode")
    add("special_dos_dir", build(replace(base, rec, made_by=20, ext_attr=0x10)), "pkg.malformed", "special_file",
        "MS-DOS host with the directory attribute")
    add("host_ntfs", build(replace(base, rec, made_by=(10 << 8) | 45)), "pkg.malformed", "host",
        "made on host 10 (NTFS): attributes cannot be checked")
    add("entry_comment", build(replace(base, rec, comment=b"x")), "pkg.malformed", "entry_comment",
        "an entry comment")
    add("extra_timestamp", build(replace(base, rec, cd_extra=struct.pack("<HHBI", 0x5455, 5, 1, 0))),
        "pkg.malformed", "extra_field", "an extended-timestamp extra field")
    add("extra_unicode_path", build(replace(base, rec, cd_extra=struct.pack("<HHBI", 0x7075, 5 + 9, 1, 0)
                                            + b"../evil.x")),
        "pkg.malformed", "extra_field", "a Unicode-path extra field naming ../evil.x")
    add("zip64_extra_length", build(replace(base, rec, cd_extra=struct.pack("<HHQ", 1, 8, 1))), "pkg.malformed",
        "zip64_extra", "a zip64 extra field with no field marked 0xFFFFFFFF")
    add("name_not_utf8", with_extra([entry(b"results/\xff\xfe.json", b"{}")])[0], "pkg.malformed",
        "name_encoding", "the UTF-8 flag with bytes that are not UTF-8")
    add("name_cp437", with_extra([entry("results/\u00e9.json".encode("utf-8"), b"{}", flags=0)])[0],
        "pkg.malformed", "name_encoding", "a non-ASCII name without the UTF-8 flag (would be CP437)")

    # ---------------------------------------------------------- names: zip-slip and friends
    for case, name, note in [
        ("slip_dotdot", "../x", "a parent-folder segment"),
        ("slip_inner_dotdot", "results/../../x", "a parent-folder segment inside the path"),
        ("slip_absolute", "/etc/passwd", "an absolute path"),
        ("slip_backslash", "results\\..\\..\\x", "backslash separators"),
        ("slip_drive", "C:/x", "a drive prefix"),
        ("slip_unc", "//server/share/x", "a UNC-style double slash"),
        ("name_not_nfc", "results/e\u0301.json", "a decomposed (non-NFC) name"),
        ("name_dot_segment", "results/./x.json", "a dot segment"),
        ("name_directory", "results/dir/", "a directory entry"),
        ("name_nul", "results/a\u0000.json", "a NUL byte"),
    ]:
        add(case, with_extra([entry(name, b"{}")])[0], "pkg.malformed", "bad_path", note)
    add("name_control", with_extra([entry("results/a\u0001b.json", b"{}")])[0], "pkg.malformed", "control_char",
        "a C0 control character")
    add("name_c1_control", with_extra([entry("results/a\u0085b.json", b"{}")])[0], "pkg.malformed",
        "control_char", "a C1 control character (NEL)")
    for case, name, note in [
        ("lookalike_fullwidth_dots", "results/\uff0e\uff0e/x.json", "fullwidth full stops that NFKC maps to '..'"),
        ("lookalike_division_slash", "results\u2215..\u2215x.json", "division slashes posing as '/'"),
        ("lookalike_fullwidth_solidus", "results\uff0fx.json", "a fullwidth solidus"),
        ("lookalike_small_reverse_solidus", "results/a\ufe68b.json", "a small reverse solidus"),
        ("lookalike_bidi_override", "results/gnp.\u202eexe.json", "a right-to-left override"),
        ("lookalike_zero_width", "results/records\u200b.json", "a zero-width space"),
        ("lookalike_one_dot_leader", "results/\u2024\u2024/x.json", "one-dot leaders that NFKC maps to '.'"),
    ]:
        add(case, with_extra([entry(name, b"{}")])[0], "pkg.malformed", "lookalike", note)

    # ---------------------------------------------------------- duplicates and collisions
    records = MINIMAL_MEMBERS[2]
    assert records[0] == rec
    add("duplicate_entry", with_extra([entry(rec, records[2])])[0], "pkg.malformed", "duplicate_entry",
        "results/records.json twice")
    add("case_collision", with_extra([entry("results/Records.json", records[2])])[0], "pkg.malformed",
        "case_collision", "results/Records.json next to results/records.json")
    add("case_collision_manifest", with_extra([entry("MANIFEST.json", b"{}")])[0], "pkg.malformed",
        "case_collision", "MANIFEST.json next to manifest.json")
    add("path_conflict", with_extra([entry("results/records.json/x", b"{}")])[0], "pkg.malformed", "path_conflict",
        "a member that is also a folder of another")

    # ---------------------------------------------------------- local headers and layout
    add("stored_size", build(replace(base, "profile/profile.json", method=0, comp=MINIMAL_MEMBERS[1][2] + b"x",
                                     csize=len(MINIMAL_MEMBERS[1][2]) + 1)),
        "pkg.malformed", "stored_size", "a stored entry whose compressed and uncompressed sizes differ")
    add("local_signature", build(replace(base, rec, local_sig=0x08074B50)), "pkg.malformed", "local_signature",
        "a data-descriptor signature where the local header belongs")
    # TRUST.PKG.FIXTURE: the one malformed reason a static file can show that had no case yet.
    cd_sig = bytearray(golden)
    at = cd_sig.index(struct.pack("<I", SIG_CENTRAL))
    assert cd_sig.count(struct.pack("<I", SIG_CENTRAL)) == 5 and at > 0
    cd_sig[at + 3] = 0x03
    add("cd_signature", bytes(cd_sig), "pkg.malformed", "cd_signature",
        "the first central directory header's signature is PK\\x01\\x03 instead of PK\\x01\\x02")
    add("local_name_mismatch", build(replace(base, rec, local_name=b"results/evilrecords.json"[:len(rec)])),
        "pkg.malformed", "local_mismatch", "the local header names a different file than the directory")
    add("local_method_mismatch", build(replace(base, rec, local_method=0)), "pkg.malformed", "local_mismatch",
        "the local header says stored, the directory deflate")
    add("local_crc_mismatch", build(replace(base, rec, local_crc=0)), "pkg.malformed", "local_mismatch",
        "the local CRC differs from the directory's")
    add("local_size_mismatch", build(replace(base, rec, local_usize=1)), "pkg.malformed", "local_mismatch",
        "the local uncompressed size differs from the directory's")
    add("local_extra", build(replace(base, rec, local_extra=struct.pack("<HHI", 0x5455, 4, 0))), "pkg.malformed",
        "local_mismatch", "an extra field in the local header only")

    # Overlap: a second directory entry for the same local record (the classic non-recursive bomb).
    es = [dict(e) for e in base]
    golden_offsets = []
    pos = 0
    for e in es:
        golden_offsets.append(pos)
        pos += 30 + len(e["name"]) + len(e["comp"])
    dup = dict(es[3])
    dup.update(name=b"results/z.json", skip_local=True, offset=golden_offsets[3])
    add("overlap_same_offset", build(es[:4] + [dup] + es[4:]), "pkg.malformed", "overlap",
        "two directory entries share one local record")
    dup = dict(es[2])
    dup.update(name=b"profile/q.json", skip_local=True, offset=golden_offsets[2] + 40)
    add("overlap_partial", build(es[:3] + [dup] + es[3:]), "pkg.malformed", "overlap",
        "a directory entry points into the middle of another entry's data")
    shifted = [dict(e, offset=o + 16) for e, o in zip(base, golden_offsets)]
    add("gap_prefix", build(shifted, prefix=b"#!/bin/sh\nexit\n\n"), "pkg.malformed", "gap",
        "16 bytes of data before the first local header")
    add("gap_hidden", build(base, gap_after=(4, b"hidden")), "pkg.malformed", "gap",
        "hidden bytes after the last member's data")

    # ---------------------------------------------------------- manifest and membership
    add("no_manifest", build(replace(base, "manifest.json", name=b"manifest.jsn")), "pkg.malformed", "no_manifest",
        "no manifest.json entry")
    add("manifest_not_json", build(replace(base, "manifest.json", data=b"{not json", comp=mf.stored_deflate(
        b"{not json"), usize=9, csize=len(mf.stored_deflate(b"{not json")), crc=crc(b"{not json"))),
        "pkg.malformed", "manifest", "manifest.json is not JSON")
    mtext = canon(manifest_of(rows))
    dup_key = ('{"format":"x",' + mtext[1:]).encode("utf-8")
    es, _ = package_entries(manifest_bytes=dup_key)
    add("manifest_duplicate_key", build(es), "pkg.malformed", "manifest", "manifest.json repeats the key format")
    es, _ = package_entries(version="2.0.0")
    add("unsupported_version", build(es), "pkg.unsupported_version", "version", "format_version 2.0.0")
    add("unlisted_member", with_extra([entry("results/extra.json", b"{}")])[0], "pkg.unlisted_member", "unlisted",
        "results/extra.json is not in the manifest")
    add("unlisted_cyrillic", with_extra([entry("results/r\u0435cords.json", records[2])])[0],
        "pkg.unlisted_member", "unlisted", "a Cyrillic 'e' in place of the Latin one")
    add("unlisted_signatures_file", with_extra([entry("signatures", b"{}")])[0], "pkg.unlisted_member", "unlisted",
        "a file named signatures (not inside signatures/)")
    es = [e for e in base if e["name"] != b"profile/profile.json"]
    add("missing_member", build(es), "pkg.malformed", "missing_member",
        "profile/profile.json is in the manifest but not in the container")
    body = records[2]
    longer = body + b" "
    es = replace(base, rec, data=longer, comp=mf.stored_deflate(longer), usize=len(longer),
                 csize=len(mf.stored_deflate(longer)), crc=crc(longer))
    add("length_mismatch_declared", build(es), "pkg.digest_mismatch", "length",
        "the directory declares one byte more than the manifest")
    flipped = body[:-2] + b"X" + body[-1:]
    es = replace(base, rec, data=flipped, comp=mf.stored_deflate(flipped), crc=crc(flipped))
    add("digest_mismatch", build(es), "pkg.digest_mismatch", "sha256",
        "one byte of results/records.json changed, CRC updated",
        inventory(rows, statuses={rec: "digest_mismatch"}))
    add("crc_mismatch", build(replace(base, rec, crc=crc(body) ^ 1)), "pkg.malformed", "crc",
        "results/records.json intact but its CRC is wrong in both headers",
        inventory(rows, statuses={rec: "malformed"}))
    # Deflate streams.
    bad_type = bytes([0x07]) + b"\0" * 8  # BFINAL=1, BTYPE=11
    add("deflate_bad_block_type", build(replace(base, rec, comp=bad_type, csize=len(bad_type))), "pkg.malformed",
        "deflate", "a deflate block of reserved type 3", inventory(rows, statuses={rec: "malformed"}))
    nlen = bytes([0x01, 0x05, 0x00, 0x00, 0x00]) + b"abcde"
    add("deflate_bad_nlen", build(replace(base, rec, comp=nlen, csize=len(nlen))), "pkg.malformed", "deflate",
        "a stored block whose NLEN is not the complement of LEN", inventory(rows, statuses={rec: "malformed"}))
    far = BitWriter()
    far.bits(1, 1)
    far.bits(1, 2)
    far.huff(FIXED_LIT[ord("a")])
    far.huff(FIXED_LIT[257])  # length 3
    far.huff(FIXED_DIST[1])  # distance 2, but only 1 byte out
    far.huff(FIXED_LIT[256])
    far = far.finish()
    add("deflate_distance_too_far", build(replace(base, rec, comp=far, csize=len(far))), "pkg.malformed", "deflate",
        "a back-reference before the start of the output", inventory(rows, statuses={rec: "malformed"}))
    comp = mf.stored_deflate(body) + b"\0\0"
    add("deflate_trailing", build(replace(base, rec, comp=comp, csize=len(comp))), "pkg.malformed", "trailing_data",
        "two bytes after the final deflate block", inventory(rows, statuses={rec: "malformed"}))
    w = BitWriter()
    stored_blocks(w, body, False)
    comp = w.finish()
    add("deflate_no_final", build(replace(base, rec, comp=comp, csize=len(comp))), "pkg.malformed",
        "member_truncated", "the deflate stream never sets BFINAL", inventory(rows, statuses={rec: "malformed"}))
    short = body[:-3]
    comp = mf.stored_deflate(short)
    add("deflate_short", build(replace(base, rec, comp=comp, csize=len(comp))), "pkg.digest_mismatch", "length",
        "the stream inflates to 3 bytes fewer than declared", inventory(rows, statuses={rec: "digest_mismatch"}))

    # ---------------------------------------------------------- Windows names
    for case, name, note in [
        ("windows_colon", "results/a:b.json", "a colon (an NTFS alternate data stream)"),
        ("windows_stream", "results/records.json:evil", "a colon after a member name"),
        ("windows_trailing_dot", "results/x.", "a segment ending in a dot"),
        ("windows_trailing_space", "results/x ", "a segment ending in a space"),
        ("windows_folder_trailing_dot", "results/dir./x.json", "a folder ending in a dot"),
        ("windows_device_con", "results/CON", "the device name CON"),
        ("windows_device_nul_ext", "results/nul.json", "the device name NUL with an extension"),
        ("windows_device_com1_space", "results/Com1 .txt", "COM1 with a space before the extension"),
        ("windows_device_lpt9", "results/lpt9", "the device name LPT9"),
        ("windows_device_superscript", "results/COM¹.json", "COM with a superscript one"),
        ("windows_device_aux_folder", "aux/x.json", "the device name AUX as a folder"),
        ("windows_device_prn", "results/prn.tar.gz", "the device name PRN with two extensions"),
    ]:
        add(case, with_extra([entry(name, b"{}")])[0], "pkg.malformed", "windows_name", note)

    # ---------------------------------------------------------- full case folding
    add("casefold_final_sigma", with_extra([entry("results/σ.json", b"{}"), entry("results/ς.json", b"{}")])[0],
        "pkg.malformed", "case_collision", "final sigma and sigma fold together (lowercasing keeps them apart)")
    add("casefold_long_s", with_extra([entry("results/s.json", b"{}"), entry("results/ſ.json", b"{}")])[0],
        "pkg.malformed", "case_collision", "long s folds to s")
    add("casefold_sharp_s", with_extra([entry("results/ss.json", b"{}"), entry("results/ß.json", b"{}")])[0],
        "pkg.malformed", "case_collision", "sharp s folds to ss")
    add("casefold_folder", with_extra([entry("results/ſ", b"{}"), entry("results/s/x.json", b"{}")])[0],
        "pkg.malformed", "path_conflict", "results/ſ folds to the folder results/s")

    # ---------------------------------------------------------- manifest bombs
    comp, plain = zeros_array_stream(260_111)
    assert len(plain) > 64 * 1024 * 1024 - 300 and len(comp) < 120_000
    es = [entry("manifest.json", plain, comp=comp)]
    add("manifest_bomb_size", build(es), "pkg.limit", "manifest_size",
        "a ~100 KB package whose manifest.json inflates to 64 MiB of [0,0,...]: above 64 KiB + 4608 bytes per entry")
    dummies = [entry("results/e%03d" % i, b"") for i in range(300)]
    comp, plain = zeros_array_stream(5400)
    assert len(plain) <= 64 * 1024 + 4608 * 301
    es = [entry("manifest.json", plain, comp=comp)] + dummies
    add("manifest_bomb_values", build(es), "pkg.limit", "manifest_budget",
        "301 entries allow a 1.4 MB manifest, but its 696,603 elements exceed 64 + 9 per entry")
    dummies = [entry("r/%04d" % i, b"", method=0) for i in range(3500)]
    comp, plain = zeros_array_stream(62_000)
    assert len(plain) <= min(16 * 1024 * 1024, 64 * 1024 + 4608 * 3501)
    es = [entry("manifest.json", plain, comp=comp)] + dummies
    add("manifest_bomb_values_16m", build(es), "pkg.limit", "manifest_budget",
        "3501 entries allow a 16 MB manifest of [0,0,...]; the element budget stops the parse early")
    m = manifest_of(rows)
    m["x"] = [0] * 10_000
    es, _ = package_entries(manifest_bytes=canon_bytes(m))
    add("manifest_extra_values", build(es), "pkg.limit", "manifest_budget",
        "an unknown manifest key holding 10,000 numbers: above 64 + 9 per entry")
    m = manifest_of(rows)
    m["x"] = [[[[[[[[0]]]]]]]]
    es, _ = package_entries(manifest_bytes=canon_bytes(m))
    add("manifest_deep", build(es), "pkg.limit", "manifest_budget", "a manifest nested deeper than 8")
    es64, _ = package_entries(zip64=True)
    add("zip64_extensible_data", build(es64, zip64_end=True, eocd64_data=b"\x01\x00\x00\x00"), "pkg.malformed",
        "zip64_record", "a zip64 end record with an extensible data sector")
    return cases


def valid_packages(check, errors):
    """Returns {name: (zip bytes, inspect bytes)}."""
    out = {}
    es, rows = package_entries()
    out["minimal"] = (build(es), inventory(rows))
    es, rows = package_entries(zip64=True)
    out["minimal.zip64"] = (build(es, zip64_end=True), inventory(rows))
    mixed = sorted(((m["path"], m["media_type"], mf.body_of(m)) for m in mf.MIXED["members"]),
                   key=lambda t: t[0].encode("utf-8"))
    es, rows = package_entries(members=mixed, contents=mf.MIXED)
    zbytes = build(es)
    assert sha(zbytes) == (HERE / "golden" / "mixed.zip.sha256").read_text().split()[0]
    out["mixed"] = (zbytes, inventory(rows, contents=mf.MIXED))
    # Two DSSE-envelope stand-ins under signatures/ (not listed in the manifest).
    es, rows = package_entries()
    sigs = [entry("signatures/0.dsse.json", b'{"payloadType":"test-only"}'),
            entry("signatures/1.dsse.json", b'{"payloadType":"test-only","n":"1"}')]
    out["signed"] = (build(es + sigs), inventory(rows, signatures=2))
    # Real deflate: fixed-Huffman LZ77 from this script, and committed zlib vectors.
    members = []
    for path, plain in [("results/records.json", b'[{"id":"r1"},{"id":"r1"},{"id":"r1"},{"id":"r2"}]' * 40),
                        ("journal/segment.jsonl", b''.join(b'{"seq":"%d","event":"synthetic"}\n' % i
                                                          for i in range(400)))]:
        members.append((path, "application/json", plain, {"comp": checked(deflate_fixed(plain), plain)}))
    for name in sorted(VECTORS):
        plain, comp = vector(name, check, errors)
        members.append((f"points/{name}.bin", "application/octet-stream", plain, {"comp": comp}))
    members.sort(key=lambda t: t[0].encode("utf-8"))
    es, rows = package_entries(members=members)
    out["deflate"] = (build(es), inventory(rows))
    # Written the way Python's zipfile writes stored entries (version 2.0, no UTF-8 flag for
    # ASCII names), with an empty member.
    members = [("profile/profile.json", "image/png", b"{}", {"flags": 0, "made_by": (3 << 8) | 20}),
               ("results/empty.json", "image/png", b"", {"flags": 0, "made_by": (3 << 8) | 20})]
    es, rows = package_entries(members=members)
    es = [dict(e, flags=0, made_by=(3 << 8) | 20) for e in es]
    out["plain-ascii"] = (build(es), inventory(rows))
    return out


def write_all(check, errors):
    files = {}
    for case, (data, code, reason, note, inspect) in malformed_cases().items():
        files[f"malformed/{case}/package.zip"] = data
        files[f"malformed/{case}/expect.json"] = canon_bytes(
            {"error_code": code, "reason": reason, "note": note}) + b"\n"
        if inspect is not None:
            files[f"malformed/{case}/inspect.json"] = inspect
    for name, (data, inspect) in valid_packages(check, errors).items():
        files[f"reader/valid/{name}.zip"] = data
        files[f"reader/valid/{name}.inspect.json"] = inspect
    return files


def main():
    check = "--check" in sys.argv[1:]
    errors = []
    files = write_all(check, errors)
    stale = []
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(rel)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    for root in ("malformed", "reader/valid"):
        for p in sorted((HERE / root).rglob("*")):
            if p.is_file() and p.relative_to(HERE).as_posix() not in files:
                stale.append(p.relative_to(HERE).as_posix() + " (not generated)")
    if stale or errors:
        for e in errors:
            print(e)
        if stale:
            print("stale fixtures:", ", ".join(stale))
        return 1
    print(f"{len(files)} reader fixtures {'up to date' if check else 'written'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
