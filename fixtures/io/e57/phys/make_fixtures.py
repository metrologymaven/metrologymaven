#!/usr/bin/env python3
"""Independent reference generator for the IO.CLOUD.E57.PHYS fixtures (crate fid-io-e57).

Written separately from the Rust code, from the leaf's rules (spec/io/cloud/e57/phys/NODE.md):
a 48-byte little-endian file header at the start of the logical stream (8-byte signature,
u32 major and minor version, u64 physical file length, u64 XML physical offset, u64 XML
logical length, u64 page size), a fixed page size, and in every page a payload of
page_size - 4 bytes followed by the CRC-32C of that payload. Logical offset l maps to
physical (l // payload) * page_size + l % payload. The trailing zero bytes of the last page
are padding. [unverified: field layout, and the big-endian byte order of the stored
checksum, are to be confirmed against ASTM E2807 and a corpus file (AC-4).]

CRC-32C here is the plain bitwise definition (reflected polynomial 0x82F63B78, initial value
and final XOR 0xFFFFFFFF), not a table, so it shares no code shape with the Rust table version.

Every file is tiny and synthetic: the payload comes from a fixed linear congruential rule
below, the XML text is a two-line placeholder. No third-party data, no randomness.

Run (stdlib only):
  python fixtures/io/e57/phys/make_fixtures.py           # rewrite the fixtures
  python fixtures/io/e57/phys/make_fixtures.py --check   # fail if any fixture is stale
"""

import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

SIGNATURE = b"ASTM-E57"
HEADER_LEN = 48
CHECKSUM_LEN = 4
MIN_PAGE_SIZE = HEADER_LEN + CHECKSUM_LEN  # page 0's payload holds the whole header
MAX_PAGE_SIZE = 1 << 20
XML = b'<?xml version="1.0" encoding="UTF-8"?>\n<e57Root/>\n'


def crc32c(data: bytes) -> int:
    crc = 0xFFFFFFFF
    for byte in data:
        crc ^= byte
        for _ in range(8):
            crc = (crc >> 1) ^ 0x82F63B78 if crc & 1 else crc >> 1
    return crc ^ 0xFFFFFFFF


def lcg_bytes(n: int, seed: int) -> bytes:
    out = bytearray()
    x = seed
    for _ in range(n):
        x = (x * 1103515245 + 12345) % (1 << 31)
        out.append((x >> 16) & 0xFF)
    return bytes(out)


def pack_header(major, minor, file_length, xml_offset, xml_length, page_size) -> bytes:
    return SIGNATURE + struct.pack("<IIQQQQ", major, minor, file_length, xml_offset, xml_length, page_size)


def to_physical(logical: int, page_size: int) -> int:
    payload = page_size - CHECKSUM_LEN
    return (logical // payload) * page_size + logical % payload


def build(page_size: int, data_len: int, seed: int):
    """Returns (file bytes, logical bytes without padding, header values)."""
    payload = page_size - CHECKSUM_LEN
    data = lcg_bytes(data_len, seed)
    xml_logical = HEADER_LEN + len(data)
    logical_len = xml_logical + len(XML)
    pages = max(1, -(-logical_len // payload))
    file_length = pages * page_size
    hdr = dict(major=1, minor=0, file_length=file_length, xml_offset=to_physical(xml_logical, page_size),
               xml_length=len(XML), page_size=page_size)
    logical = pack_header(**hdr) + data + XML
    assert len(logical) == logical_len
    return paginate(logical, page_size), logical, hdr


def paginate(logical: bytes, page_size: int) -> bytes:
    payload = page_size - CHECKSUM_LEN
    padded = logical + bytes(-len(logical) % payload)
    out = bytearray()
    for i in range(0, len(padded), payload):
        chunk = padded[i:i + payload]
        out += chunk + crc32c(chunk).to_bytes(4, "big")
    return bytes(out)


def repage0(file_bytes: bytes, page_size: int) -> bytes:
    """Recomputes page 0's checksum, so that only the header field under test is wrong."""
    payload = page_size - CHECKSUM_LEN
    b = bytearray(file_bytes)
    b[payload:page_size] = crc32c(bytes(b[:payload])).to_bytes(4, "big")
    return bytes(b)


def with_header(file_bytes: bytes, ps: int, **fields) -> bytes:
    sig = file_bytes[:8]
    vals = list(struct.unpack("<IIQQQQ", file_bytes[8:HEADER_LEN]))
    names = ["major", "minor", "file_length", "xml_offset", "xml_length", "page_size"]
    for k, v in fields.items():
        if k == "signature":
            sig = v
        else:
            vals[names.index(k)] = v
    return repage0(sig + struct.pack("<IIQQQQ", *vals) + file_bytes[HEADER_LEN:], ps)


# ---- the spec's open rules, in the order the crate documents them ----------------------------------

def outcome(file_bytes: bytes) -> str:
    """The expected result of opening the file and then verifying every page."""
    actual = len(file_bytes)
    if actual < HEADER_LEN:
        return f"truncated_file {actual}"
    if file_bytes[:8] != SIGNATURE:
        return "malformed signature"
    major, minor, file_length, xml_offset, xml_length, page_size = struct.unpack("<IIQQQQ", file_bytes[8:HEADER_LEN])
    if major != 1:
        return "malformed major_version"
    if page_size < MIN_PAGE_SIZE:
        return "malformed page_size"
    if page_size > MAX_PAGE_SIZE:
        return f"limit_exceeded alloc {MAX_PAGE_SIZE}"
    if file_length != actual or file_length % page_size != 0:
        return "malformed file_length"
    payload = page_size - CHECKSUM_LEN
    pages = file_length // page_size
    if xml_offset >= file_length or xml_offset % page_size >= payload:
        return "malformed xml_offset"
    xml_logical = (xml_offset // page_size) * payload + xml_offset % page_size
    if xml_logical < HEADER_LEN:
        return "malformed xml_offset"
    if xml_logical + xml_length > pages * payload:
        return "malformed xml_length"
    for p in range(pages):
        page = file_bytes[p * page_size:(p + 1) * page_size]
        if crc32c(page[:payload]) != int.from_bytes(page[payload:], "big"):
            return f"checksum_mismatch {p * page_size}"
    return (f"ok page_size={page_size} file_length={file_length} xml_offset={xml_offset} "
            f"xml_length={xml_length} pages={pages}")


def flip(file_bytes: bytes, at: int) -> bytes:
    b = bytearray(file_bytes)
    b[at] ^= 0x01
    return bytes(b)


def generate():
    files = {}
    ok1024, log1024, h1024 = build(1024, 2500, seed=57)
    ok64, log64, h64 = build(64, 300, seed=2807)
    files["ok_1024.e57"] = ok1024
    files["ok_1024.logical"] = log1024
    files["ok_64.e57"] = ok64
    files["ok_64.logical"] = log64

    ps = 1024
    bad = {
        "bad_signature.e57": with_header(ok1024, ps, signature=b"ASTM-E58"),
        "bad_major_version.e57": with_header(ok1024, ps, major=2),
        "bad_page_size.e57": with_header(ok1024, ps, page_size=4),
        "bad_file_length_big.e57": with_header(ok1024, ps, file_length=len(ok1024) + ps),
        "bad_file_length_small.e57": with_header(ok1024, ps, file_length=len(ok1024) - ps),
        "bad_file_length_unaligned.e57": with_header(ok1024[:-10], ps, file_length=len(ok1024) - 10),
        "bad_xml_offset_beyond.e57": with_header(ok1024, ps, xml_offset=len(ok1024)),
        "bad_xml_offset_checksum.e57": with_header(ok1024, ps, xml_offset=ps - 2),
        "bad_xml_offset_header.e57": with_header(ok1024, ps, xml_offset=10),
        "bad_xml_length.e57": with_header(ok1024, ps, xml_length=h1024["xml_length"] + 3 * ps),
        "bad_checksum_page0.e57": flip(ok1024, 100),
        "bad_checksum_page1.e57": flip(ok1024, ps + 7),
        "bad_checksum_crc_field.e57": flip(ok1024, 3 * ps - 1),
        "bad_checksum_page5_of_64.e57": flip(ok64, 5 * 64 + 3),
        "truncated_header.e57": ok1024[:20],
    }
    files.update(bad)

    lines = [
        "# Expected outcome of LogicalReader::open followed by verify_all, per fixture.",
        "# Written by make_fixtures.py from the leaf's rules; do not edit by hand.",
    ]
    for name in sorted(n for n in files if n.endswith(".e57")):
        lines.append(f"{name} {outcome(files[name])}")
    files["expected.txt"] = ("\n".join(lines) + "\n").encode("ascii")

    vectors = [
        b"",
        b"123456789",
        bytes(32),
        b"\xff" * 32,
        bytes(range(32)),
        bytes(range(31, -1, -1)),
        lcg_bytes(1020, seed=7),
        lcg_bytes(4093, seed=11),
    ]
    assert crc32c(b"123456789") == 0xE3069283  # the published CRC-32C check value
    vlines = ["# CRC-32C test vectors: <input hex or '-' for empty> <crc hex>, from make_fixtures.py."]
    for v in vectors:
        vlines.append(f"{v.hex() or '-'} {crc32c(v):08x}")
    files["crc32c.txt"] = ("\n".join(vlines) + "\n").encode("ascii")

    for name, (fb, hdr) in {"ok_1024.e57": (ok1024, h1024), "ok_64.e57": (ok64, h64)}.items():
        assert outcome(fb).startswith("ok "), name
        assert fb[:HEADER_LEN] == pack_header(**hdr)
    return files


def main():
    check = "--check" in sys.argv[1:]
    stale = []
    for name, data in sorted(generate().items()):
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.write_bytes(data)
    if stale:
        print("stale fixtures (rerun make_fixtures.py):", ", ".join(stale))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
