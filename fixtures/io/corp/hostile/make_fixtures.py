#!/usr/bin/env python3
"""Independent reference for IO.CORP.HOSTILE (CC-BY-4.0 project data).

Writes the small valid inputs of fixtures/io/corp/hostile/ (cube_bin.stl, cube_mm.3mf,
min.qif, a copy of fixtures/io/e57/phys/ok_64.e57), their corpus manifests, and
expected.txt: for seed 42 and plan/hostile.toml, every hostile variant the generator must
emit, in order, as `<format> <path> <sha256> <expected error>`.

The mutators are re-implemented here from the spec leaf, independently of the Rust code;
the Rust tests compare the generator's output against expected.txt. The ZIP bomb is
checked here with zlib (it inflates to its declared size and CRC, and its declared sizes
sum past the 64 GiB default byte limit).

Run:  python fixtures/io/corp/hostile/make_fixtures.py
"""

import hashlib
import struct
import tomllib
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
SEED = 42
DEFAULT_BYTES = 64 * 1024 ** 3
MASK32 = 0xFFFFFFFF
MASK64 = (1 << 64) - 1

# ---------------------------------------------------------------- inputs


def stl_cube() -> bytes:
    header = b"fid hostile corpus fixture: unit cube in mm, binary STL, CC-BY-4.0".ljust(80, b" ")
    v = [(0, 0, 0), (10, 0, 0), (10, 10, 0), (0, 10, 0), (0, 0, 10), (10, 0, 10), (10, 10, 10), (0, 10, 10)]
    faces = [  # (normal, triangle)
        ((0, 0, -1), (0, 2, 1)), ((0, 0, -1), (0, 3, 2)), ((0, 0, 1), (4, 5, 6)), ((0, 0, 1), (4, 6, 7)),
        ((0, -1, 0), (0, 1, 5)), ((0, -1, 0), (0, 5, 4)), ((1, 0, 0), (1, 2, 6)), ((1, 0, 0), (1, 6, 5)),
        ((0, 1, 0), (2, 3, 7)), ((0, 1, 0), (2, 7, 6)), ((-1, 0, 0), (3, 0, 4)), ((-1, 0, 0), (3, 4, 7)),
    ]
    out = bytearray(header + struct.pack("<I", len(faces)))
    for n, tri in faces:
        out += struct.pack("<3f", *n)
        for i in tri:
            out += struct.pack("<3f", *v[i])
        out += b"\0\0"
    return bytes(out)


def stored_zip(files) -> bytes:
    """A ZIP with stored (uncompressed) entries, fixed DOS date 1980-01-01 00:00."""
    out, central = bytearray(), bytearray()
    for name, data in files:
        name = name.encode()
        crc = zlib.crc32(data)
        off = len(out)
        out += struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, 0, 0, 0, 0x21, crc, len(data), len(data), len(name), 0)
        out += name + data
        central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 20, 0, 0, 0, 0x21, crc, len(data), len(data),
                               len(name), 0, 0, 0, 0, 0, off)
        central += name
    cd_off = len(out)
    out += central
    out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, len(files), len(files), len(central), cd_off, 0)
    return bytes(out)


def tmf_cube() -> bytes:
    ct = (b'<?xml version="1.0" encoding="UTF-8"?>\n'
          b'<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
          b'<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
          b'<Default Extension="model" ContentType="application/vnd.ms-package.3dmanufacturing-3dmodel+xml"/>'
          b'</Types>\n')
    rels = (b'<?xml version="1.0" encoding="UTF-8"?>\n'
            b'<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            b'<Relationship Target="/3D/3dmodel.model" Id="rel0" '
            b'Type="http://schemas.microsoft.com/3dmanufacturing/2013/01/3dmodel"/>'
            b'</Relationships>\n')
    verts = [(0, 0, 0), (10, 0, 0), (10, 10, 0), (0, 10, 0), (0, 0, 10), (10, 0, 10), (10, 10, 10), (0, 10, 10)]
    tris = [(0, 2, 1), (0, 3, 2), (4, 5, 6), (4, 6, 7), (0, 1, 5), (0, 5, 4), (1, 2, 6), (1, 6, 5), (2, 3, 7),
            (2, 7, 6), (3, 0, 4), (3, 4, 7)]
    model = ['<?xml version="1.0" encoding="UTF-8"?>\n',
             '<model unit="millimeter" xmlns="http://schemas.microsoft.com/3dmanufacturing/core/2015/02">\n',
             ' <resources>\n  <object id="1" type="model">\n   <mesh>\n    <vertices>\n']
    model += [f'     <vertex x="{x}" y="{y}" z="{z}"/>\n' for x, y, z in verts]
    model += ['    </vertices>\n    <triangles>\n']
    model += [f'     <triangle v1="{a}" v2="{b}" v3="{c}"/>\n' for a, b, c in tris]
    model += ['    </triangles>\n   </mesh>\n  </object>\n </resources>\n',
              ' <build>\n  <item objectid="1"/>\n </build>\n</model>\n']
    return stored_zip([("[Content_Types].xml", ct), ("_rels/.rels", rels),
                       ("3D/3dmodel.model", "".join(model).encode())])


QIF = b"""<?xml version="1.0" encoding="UTF-8"?>
<QIFDocument xmlns="http://qifstandards.org/xsd/qif3" versionQIF="3.0.0" idMax="2">
  <FileUnits>
    <PrimaryUnits>
      <LinearUnit><SIUnitName>meter</SIUnitName><UnitName>mm</UnitName><SIUnitConversion>0.001</SIUnitConversion></LinearUnit>
    </PrimaryUnits>
  </FileUnits>
  <Features>
    <FeatureItems n="1">
      <PointFeatureItem id="2"><FeatureNominalId>1</FeatureNominalId></PointFeatureItem>
    </FeatureItems>
  </Features>
</QIFDocument>
"""

INPUTS = [  # format, file, counts
    ("cloud.e57", "ok_64.e57", {}),
    ("mesh.3mf", "cube_mm.3mf", {"triangles": 12}),
    ("mesh.stl", "cube_bin.stl", {"facets": 12}),
    ("qif.xml", "min.qif", {"features": 1}),
]

# ---------------------------------------------------------------- RNG (Philox4x32-10, as KRN.NUM.RNG)

M0, M1, W0, W1 = 0xD2511F53, 0xCD9E8D57, 0x9E3779B9, 0xBB67AE85


def philox(ctr, key):
    c0, c1, c2, c3 = ctr
    k0, k1 = key
    for r in range(10):
        if r:
            k0, k1 = (k0 + W0) & MASK32, (k1 + W1) & MASK32
        p0, p1 = M0 * c0, M1 * c2
        c0, c1, c2, c3 = (p1 >> 32) ^ c1 ^ k0, p1 & MASK32, (p0 >> 32) ^ c3 ^ k1, p0 & MASK32
    return c0, c1, c2, c3


assert philox((0, 0, 0, 0), (0, 0)) == (0x6627E8D5, 0xE169C58D, 0xBC57AC4C, 0x9B00DBD8)


class Cursor:
    def __init__(self, seed, stream):
        self.seed, self.stream, self.index = seed, stream, 0

    def next_u64(self):
        i, s, k = self.index, self.stream, self.seed
        w = philox((i & MASK32, i >> 32, s & MASK32, s >> 32), (k & MASK32, k >> 32))
        self.index += 1
        return (w[1] << 32) | w[0]

    def below(self, n):
        threshold = ((1 << 64) - n) % n
        while True:
            x = self.next_u64()
            if x >= threshold:
                return x % n


def stream_of(digest: str, mutator: str, index: int) -> int:
    h = hashlib.sha256(f"{digest}\0{mutator}\0{index}".encode()).digest()
    return int.from_bytes(h[:8], "big")

# ---------------------------------------------------------------- mutators


def find_root(data: bytes):
    """(index of the root '<', end of its name, index of the '>' closing its start tag)."""
    i = 3 if data.startswith(b"\xef\xbb\xbf") else 0
    while True:
        while i < len(data) and data[i] in b" \t\r\n":
            i += 1
        if data.startswith(b"<?", i):
            j = data.find(b"?>", i + 2)
            assert j >= 0
            i = j + 2
        elif data.startswith(b"<!--", i):
            j = data.find(b"-->", i + 4)
            assert j >= 0
            i = j + 3
        elif data.startswith(b"<!", i):
            gt = data.find(b">", i + 2)
            br = data.find(b"[", i + 2)
            j = data.find(b"]>", br) + 1 if 0 <= br < gt else gt
            assert j >= 1
            i = j + 1
        elif i < len(data) and data[i] == ord("<"):
            break
        else:
            raise ValueError("no root element")
    j = i + 1
    while j < len(data) and data[j] not in b" \t\r\n/>":
        j += 1
    assert j > i + 1
    k, quote = j, None
    while k < len(data):
        c = data[k]
        if quote is not None:
            if c == quote:
                quote = None
        elif c in b"\"'":
            quote = c
        elif c == ord(">"):
            break
        k += 1
    assert k < len(data) and data[k - 1] != ord("/")
    return i, j, k


def truncate_lengths(n, header_len):
    out = []
    for cut in [n * 1 // 4, n * 2 // 4, n * 3 // 4] + ([header_len] if header_len is not None else []):
        if 0 < cut < n and cut not in out:
            out.append(cut)
    return out


def bitflip(data, plan, digest, index):
    flip = plan.get("flip", {})
    if flip.get("xml_root_name"):
        lt, end, _ = find_root(data)
        start = lt + 1
    else:
        start = flip.get("start", 0)
        end = min(flip.get("end", len(data)), len(data))
    assert start < end
    c = Cursor(SEED, stream_of(digest, "bitflip", index))
    pos = start + c.below(end - start)
    bit = c.below(8)
    out = bytearray(data)
    out[pos] ^= 1 << bit
    return bytes(out)


def inflate_count(data, plan):
    count = plan["count"]
    out = bytearray(data)
    if "le" in count:
        at, w = count["at"], count["le"]
        assert at + w <= len(data)
        out[at:at + w] = b"\xff" * w
    elif count.get("zip_entries"):
        lo = max(0, len(data) - 22 - 65535)
        p = len(data) - 22
        while p >= lo and data[p:p + 4] != b"PK\x05\x06":
            p -= 1
        assert p >= lo
        out[p + 8:p + 12] = b"\xfe\xff\xfe\xff"  # 0xFFFE twice; 0xFFFF is the ZIP64 marker
    else:
        pat = count["after"].encode()
        p = data.find(pat)
        assert p >= 0
        s = p + len(pat)
        e = s
        while e < len(data) and data[e:e + 1].isdigit():
            e += 1
        assert e > s
        out[s:e] = b"18446744073709551615"
    return bytes(out)


NEST_DEPTH = 256


def nest(data):
    _, _, k = find_root(data)
    return data[:k + 1] + b"<x>" * NEST_DEPTH + b"</x>" * NEST_DEPTH + data[k + 1:]


def billion_laughs(data):
    i, j, k = find_root(data)
    name = data[i + 1:j]
    dt = b"<!DOCTYPE " + name + b" [\n<!ENTITY a0 \"lol\">\n"
    for n in range(1, 10):
        dt += b"<!ENTITY a%d \"" % n + (b"&a%d;" % (n - 1)) * 10 + b"\">\n"
    dt += b"]>\n"
    return data[:i] + dt + data[i:k + 1] + b"&a9;" + data[k + 1:]


BOMB_RUNS, BOMB_ENTRIES = 40_000, 7_000


class Bits:
    def __init__(self):
        self.out, self.acc, self.n = bytearray(), 0, 0

    def put(self, v, n):  # LSB first
        for k in range(n):
            self.acc |= ((v >> k) & 1) << self.n
            self.n += 1
            if self.n == 8:
                self.out.append(self.acc)
                self.acc, self.n = 0, 0

    def code(self, c, n):  # Huffman code, MSB first
        for k in range(n - 1, -1, -1):
            self.put((c >> k) & 1, 1)

    def finish(self):
        if self.n:
            self.out.append(self.acc)
        return bytes(self.out)


def zip_bomb():
    b = Bits()
    b.put(1, 1)  # BFINAL
    b.put(1, 2)  # BTYPE = 01, fixed Huffman
    b.code(0x30, 8)  # literal 0
    for _ in range(BOMB_RUNS):
        b.code(0b11000101, 8)  # length code 285 = 258
        b.code(0, 5)  # distance code 0 = 1
    b.code(0, 7)  # end of block
    stream = b.finish()
    size = 1 + 258 * BOMB_RUNS
    crc = zlib.crc32(bytes(size))
    # independent check of the kernel
    d = zlib.decompressobj(-15)
    got = d.decompress(stream) + d.flush()
    assert len(got) == size and zlib.crc32(got) == crc and not d.unused_data
    name = b"3D/3dmodel.model"
    out = bytearray(struct.pack("<IHHHHHIIIHH", 0x04034B50, 20, 0, 8, 0, 0x21, crc, len(stream), size, len(name), 0))
    out += name + stream
    cd_off = len(out)
    for i in range(BOMB_ENTRIES):
        nm = b"b%05d" % i
        out += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 20, 20, 0, 8, 0, 0x21, crc, len(stream), size, len(nm),
                           0, 0, 0, 0, 0, 0) + nm
    cd_len = len(out) - cd_off
    out += struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, BOMB_ENTRIES, BOMB_ENTRIES, cd_len, cd_off, 0)
    assert len(out) <= 1 << 20 and size * BOMB_ENTRIES > DEFAULT_BYTES
    return bytes(out)


DEFAULT_ERROR = {"truncate": "truncated_file", "bitflip": "malformed", "inflate_count": "limit_exceeded",
                 "nest": "limit_exceeded", "billion_laughs": "xml_forbidden_construct", "zip_bomb": "limit_exceeded"}
ORDER = ["truncate", "bitflip", "inflate_count", "nest", "billion_laughs", "zip_bomb"]
BITFLIPS = 3


def mutators(tags):
    m = {"truncate", "bitflip", "inflate_count"}
    if "xml" in tags:
        m |= {"nest", "billion_laughs"}
    if "tree" in tags:
        m.add("nest")
    if "zip" in tags or "container" in tags:
        m.add("zip_bomb")
    return [x for x in ORDER if x in m]


def variants(plan, data, digest):
    for mut in mutators(plan["tags"]):
        if mut == "truncate":
            for i, n in enumerate(truncate_lengths(len(data), plan.get("header_len"))):
                yield mut, i, data[:n]
        elif mut == "bitflip":
            for i in range(BITFLIPS):
                yield mut, i, bitflip(data, plan, digest, i)
        elif mut == "inflate_count":
            yield mut, 0, inflate_count(data, plan)
        elif mut == "nest":
            yield mut, 0, nest(data)
        elif mut == "billion_laughs":
            yield mut, 0, billion_laughs(data)
        else:
            yield mut, 0, zip_bomb()


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def main():
    data_of = {
        "cube_bin.stl": stl_cube(),
        "cube_mm.3mf": tmf_cube(),
        "min.qif": QIF,
        "ok_64.e57": (ROOT / "fixtures/io/e57/phys/ok_64.e57").read_bytes(),
    }
    for name, data in data_of.items():
        (HERE / name).write_bytes(data)
    for fmt, name, counts in INPUTS:
        c = ", ".join(f"{k} = {v}" for k, v in counts.items())
        src = ("copy of fixtures/io/e57/phys/ok_64.e57" if name.endswith(".e57")
               else "synthetic, fixtures/io/corp/hostile/make_fixtures.py")
        text = (f"# Corpus manifest (IO.CORP.MANIF) for the IO.CORP.HOSTILE tests. Generated by make_fixtures.py.\n"
                f'format = "{fmt}"\n\n[[entry]]\npath = "{name}"\nsource = "{src}"\nlicence = "CC-BY-4.0"\n'
                f'sha256 = "{sha(data_of[name])}"\nexpect = {{ accept = {{ counts = {{ {c} }} }} }}\n')
        (HERE / f"{fmt}.toml").write_bytes(text.replace("counts = {  }", "counts = {}").encode())
    plans = {p["id"]: p for p in tomllib.loads((HERE / "plan/hostile.toml").read_text())["format"]}
    lines = ["# IO.CORP.HOSTILE expected variants for seed 42 (make_fixtures.py): format path sha256 error"]
    seen = set()
    for fmt, name, _ in sorted(INPUTS):  # formats in name order, as the generator walks manifests
        plan = plans[fmt]
        overrides = {o["mutator"]: o["error"] for o in plan.get("override", [])}
        data = data_of[name]
        digest = sha(data)
        ext = name.rsplit(".", 1)[1]
        for mut, i, out in variants(plan, data, digest):
            d = sha(out)
            if d in seen:
                continue
            seen.add(d)
            path = f"{fmt}/{digest[7:23]}-{mut}-{i}.{ext}"
            lines.append(f"{fmt} {path} {d} {overrides.get(mut, DEFAULT_ERROR[mut])}")
    (HERE / "expected.txt").write_bytes(("\n".join(lines) + "\n").encode())
    print(f"wrote {len(lines) - 1} expected variants")


if __name__ == "__main__":
    main()
