#!/usr/bin/env python3
"""Independent reference generator for the IO.CLOUD.LAS.HDR fixtures (crate fid-io-las).

Written separately from the Rust code, from the leaf's rules (spec/io/cloud/las/hdr/NODE.md)
and the public header, VLR and point record layouts of the ASPRS LAS specification 1.0 to
1.4 (cited by designator only; field positions below are written from the layout, not
copied text). Every file is tiny, synthetic and built byte by byte here with `struct`; the
point values come from fixed formulas, so the expected values in expected.txt are the
values this script packed, hand-checkable against the formulas. No third-party data, no
randomness. Fixtures are CC-BY-4.0 (project-generated).

Layout used (all little-endian):
- public header: signature at 0; file source ID 4; global encoding 6; GUID 8..24; version
  major 24, minor 25; system identifier 26..58; generating software 58..90; creation day
  90, year 92; header size 94; point data offset 96 (u32); VLR count 100 (u32); point
  format 104 (u8); record length 105 (u16); legacy point count 107 (u32); five legacy
  per-return counts 111..131; scale x/y/z 131; offset x/y/z 155; then max x, min x, max y,
  min y, max z, min z from 179 to 227 (doubles). 1.3 adds the waveform record offset at
  227 (u64), size 235. 1.4 adds the first EVLR offset 235 (u64), EVLR count 243 (u32),
  64-bit point count 247 and fifteen 64-bit per-return counts 255..375.
- VLR: reserved u16, user ID 16 bytes, record ID u16, payload length u16, description 32
  bytes (54 bytes), then the payload. EVLR: same with a u64 length (60 bytes).
- point formats 0..5: x, y, z (i32), intensity (u16), return byte (return number bits 0..2,
  number of returns bits 3..5, scan direction bit 6, edge bit 7), class byte (class bits
  0..4, flags bits 5..7), scan angle rank (i8), user data (u8), point source ID (u16);
  then GPS time (f64) for 1/3/4/5, RGB (3 x u16) for 2/3/5, wave packet (29 bytes) for 4/5.
- point formats 6..10: x, y, z, intensity, return byte (return number bits 0..3, number of
  returns bits 4..7), flags byte (class flags bits 0..3, scanner channel bits 4..5, scan
  direction bit 6, edge bit 7), class (u8), user data (u8), scan angle (i16), point source
  ID (u16), GPS time (f64); then RGB for 7/8/10, NIR (u16) for 8/10, wave packet for 9/10.
- extra-bytes descriptor (record 4 of user "LASF_Spec"): 192 bytes; data type at 2,
  options at 3, name 4..36. Types 1..10 are u8 i8 u16 i16 u32 i32 u64 i64 f32 f64; 11..20
  and 21..30 are 2- and 3-element arrays of them; type 0 is `options` undocumented bytes.

Run (stdlib only):
  python fixtures/io/cloud/las/hdr/make_fixtures.py           # rewrite the fixtures
  python fixtures/io/cloud/las/hdr/make_fixtures.py --check   # fail if any fixture is stale
"""

import struct
import sys
import zlib
from pathlib import Path

HERE = Path(__file__).resolve().parent

STD_LEN = {0: 227, 1: 227, 2: 227, 3: 235, 4: 375}
BASE_LEN = {0: 20, 1: 28, 2: 26, 3: 34, 4: 57, 5: 63, 6: 30, 7: 36, 8: 38, 9: 59, 10: 67}
HAS_GPS = {1, 3, 4, 5, 6, 7, 8, 9, 10}
HAS_RGB = {2, 3, 5, 7, 8, 10}
HAS_NIR = {8, 10}
HAS_WAVE = {4, 5, 9, 10}
# descriptor scalar type code -> (struct code, name, size)
SCALAR = {
    1: ("B", "u8", 1), 2: ("b", "i8", 1), 3: ("H", "u16", 2), 4: ("h", "i16", 2),
    5: ("I", "u32", 4), 6: ("i", "i32", 4), 7: ("Q", "u64", 8), 8: ("q", "i64", 8),
    9: ("I", "f32", 4), 10: ("Q", "f64", 8),  # floats are generated and printed as bits
}


def pad(b: bytes, n: int) -> bytes:
    return (b + bytes(n))[:n]


# ---------------------------------------------------------------- records


def vlr(user: bytes, record_id: int, payload: bytes, desc: bytes = b"", length=None) -> bytes:
    n = len(payload) if length is None else length
    return struct.pack("<H16sHH32s", 0, pad(user, 16), record_id, n, pad(desc, 32)) + payload


def evlr(user: bytes, record_id: int, payload: bytes, desc: bytes = b"", length=None) -> bytes:
    n = len(payload) if length is None else length
    return struct.pack("<H16sHQ32s", 0, pad(user, 16), record_id, n, pad(desc, 32)) + payload


def descriptor(data_type: int, name: bytes, options: int = 0) -> bytes:
    d = struct.pack("<2sBB32s4s", bytes(2), data_type, options, pad(name, 32), bytes(4))
    d += bytes(24 * 5)  # no_data, min, max, scale, offset (unused here)
    d += pad(b"fixture attribute", 32)
    assert len(d) == 192
    return d


def extra_columns(descs, extra_len):
    """(name, type name, count, struct code, element size) per attribute, in record order."""
    cols = []
    used = 0
    for data_type, name, options in descs:
        if data_type == 0:
            cols.append((name, "raw", options, "B", 1))
            used += options
            continue
        count = 1 if data_type <= 10 else (2 if data_type <= 20 else 3)
        code, tname, size = SCALAR[(data_type - 1) % 10 + 1]
        cols.append((name, tname, count, code, size))
        used += size * count
    assert used <= extra_len
    if used < extra_len:
        cols.append((b"", "raw", extra_len - used, "B", 1))
    return cols


# ---------------------------------------------------------------- points


def make_points(fmt: int, n: int, cols):
    """Deterministic point values for format `fmt` (simple formulas, varied bits)."""
    pts = []
    for i in range(n):
        k = fmt * 16 + i
        p = {
            "xi": -2_000_000_000 + 123_457 * k + i,
            "yi": 7 * k - 1000,
            "zi": 2_147_483_647 - 3 * k,
            "intensity": (4099 * k) % 65536,
            "user_data": (37 * k) % 256,
            "point_source_id": 65535 - k,
            "scan_direction": i % 2,
            "edge_of_flight_line": (i // 2) % 2,
        }
        if fmt >= 6:
            p["return_number"] = (k + 1) % 16
            p["number_of_returns"] = (3 * k + 2) % 16
            p["classification"] = (29 * k) % 256
            p["class_flags"] = k % 16
            p["scanner_channel"] = i % 4
            p["scan_angle"] = (1111 * k) % 60001 - 30000
        else:
            p["return_number"] = (k + 1) % 8
            p["number_of_returns"] = (3 * k + 2) % 8
            p["classification"] = (5 * k) % 32
            p["class_flags"] = k % 8
            p["scan_angle_rank"] = (7 * k) % 181 - 90
        if fmt in HAS_GPS:
            p["gps_time"] = 0x4110_0000_0000_0000 + 0x1_0000_0001 * k  # f64 bits
        if fmt in HAS_RGB:
            p["rgb"] = ((257 * k) % 65536, (65535 - 3 * k), (1000 * k) % 65536)
        if fmt in HAS_NIR:
            p["nir"] = (31 * k + 5) % 65536
        if fmt in HAS_WAVE:
            index = 0 if i == 1 else 1 + i
            p["wave_packet"] = struct.pack("<BQIIIII", index, 60 + 40 * i, 40, 0x3F80_0000, k, 2 * k, 3 * k)
        extras = []
        for c, (_name, tname, count, code, size) in enumerate(cols):
            vals = []
            for e in range(count):
                v = (k * 1_000_003 + c * 7919 + e * 104_729) % (1 << (8 * size))
                if code.islower() and v >= 1 << (8 * size - 1):  # signed: wrap into range
                    v -= 1 << (8 * size)
                vals.append(v)
            extras.append(vals)
        p["extra"] = extras
        pts.append(p)
    return pts


def pack_point(fmt: int, p, cols) -> bytes:
    sd, ef = p["scan_direction"], p["edge_of_flight_line"]
    if fmt >= 6:
        b14 = p["return_number"] | (p["number_of_returns"] << 4)
        b15 = p["class_flags"] | (p["scanner_channel"] << 4) | (sd << 6) | (ef << 7)
        rec = struct.pack(
            "<iiiHBBBBhHQ", p["xi"], p["yi"], p["zi"], p["intensity"], b14, b15,
            p["classification"], p["user_data"], p["scan_angle"], p["point_source_id"], p["gps_time"],
        )
    else:
        b14 = p["return_number"] | (p["number_of_returns"] << 3) | (sd << 6) | (ef << 7)
        b15 = p["classification"] | (p["class_flags"] << 5)
        rec = struct.pack(
            "<iiiHBBbBH", p["xi"], p["yi"], p["zi"], p["intensity"], b14, b15,
            p["scan_angle_rank"], p["user_data"], p["point_source_id"],
        )
        if fmt in HAS_GPS:
            rec += struct.pack("<Q", p["gps_time"])
    if fmt in HAS_RGB:
        rec += struct.pack("<HHH", *p["rgb"])
    if fmt in HAS_NIR:
        rec += struct.pack("<H", p["nir"])
    if fmt in HAS_WAVE:
        rec += p["wave_packet"]
    assert len(rec) == BASE_LEN[fmt], (fmt, len(rec))
    for (_name, _t, count, code, _size), vals in zip(cols, p["extra"]):
        rec += struct.pack("<" + code * count, *vals)
    return rec


def describe_point(fmt: int, i: int, p, cols) -> str:
    parts = [f"point {i}"]
    for key in ("xi", "yi", "zi", "intensity", "return_number", "number_of_returns", "scan_direction",
                "edge_of_flight_line", "classification", "class_flags"):
        parts.append(f"{key}={p[key]}")
    if fmt >= 6:
        parts.append(f"scanner_channel={p['scanner_channel']}")
        parts.append(f"scan_angle={p['scan_angle']}")
    else:
        parts.append(f"scan_angle_rank={p['scan_angle_rank']}")
    parts.append(f"user_data={p['user_data']}")
    parts.append(f"point_source_id={p['point_source_id']}")
    if fmt in HAS_GPS:
        parts.append(f"gps_time={p['gps_time']:016x}")
    if fmt in HAS_RGB:
        parts.append("rgb=" + "/".join(str(v) for v in p["rgb"]))
    if fmt in HAS_NIR:
        parts.append(f"nir={p['nir']}")
    if fmt in HAS_WAVE:
        parts.append("wave_packet=" + p["wave_packet"].hex())
    for (name, tname, _count, _code, size), vals in zip(cols, p["extra"]):
        if tname == "raw":
            text = bytes(vals).hex()
        elif tname in ("f32", "f64"):
            text = "/".join(f"{v:0{2 * size}x}" for v in vals)
        else:
            text = "/".join(str(v) for v in vals)
        parts.append(f"extra:{name.decode()}={text}")
    return " ".join(parts)


def attr_names(fmt: int, cols) -> str:
    names = ["intensity", "return_number", "number_of_returns", "scan_direction", "edge_of_flight_line",
             "classification", "class_flags"]
    if fmt >= 6:
        names += ["scanner_channel", "scan_angle"]
    else:
        names += ["scan_angle_rank"]
    names += ["user_data", "point_source_id"]
    if fmt in HAS_GPS:
        names.append("gps_time")
    if fmt in HAS_RGB:
        names.append("rgb")
    if fmt in HAS_NIR:
        names.append("nir")
    if fmt in HAS_WAVE:
        names.append("wave_packet")
    for name, tname, count, _code, _size in cols:
        names.append(f"extra:{name.decode()}:{tname}[{count}]")
    return ",".join(names)


# ---------------------------------------------------------------- files


def build(minor, fmt, records, *, extra_len=0, vlrs=(), padding=b"", header_extra=b"", evlrs=(),
          point_count=None, waveform_evlr_index=None, patch=None):
    """A whole LAS file. `records` is the list of packed point records (all written);
    `point_count` overrides the declared count; `patch` maps a header byte offset to
    (struct code, value) applied last, to break one field."""
    std = STD_LEN[minor]
    header_size = std + len(header_extra)
    vlr_bytes = b"".join(vlrs)
    offset = header_size + len(vlr_bytes) + len(padding)
    reclen = BASE_LEN[fmt] + extra_len
    n = len(records) if point_count is None else point_count
    data = b"".join(records)
    point_end = offset + len(data)
    h = bytearray(std)
    h[0:4] = b"LASF"
    struct.pack_into("<HH", h, 4, 0x0102 if minor > 0 else 0, 0x0001 if minor >= 2 else 0)
    h[8:24] = bytes(range(16))
    h[24], h[25] = 1, minor
    h[26:58] = pad(b"fid-io-las fixture", 32)
    h[58:90] = pad(b"make_fixtures.py", 32)
    struct.pack_into("<HHHIIBHI", h, 90, 321, 2026, header_size, offset, len(vlrs), fmt, reclen,
                     n if (minor < 4 or (fmt < 6 and n < 1 << 32)) else 0)
    struct.pack_into("<5I", h, 111, n, 0, 0, 0, 0)
    struct.pack_into("<3d", h, 131, 0.001, 0.001, 0.0005)
    struct.pack_into("<3d", h, 155, 100.0, -200.0, 0.0)
    struct.pack_into("<6d", h, 179, 10.0, -10.0, 20.0, -20.0, 30.0, -30.0)
    evlr_bytes = b"".join(evlrs)
    if minor >= 3:
        wf = 0
        if minor == 3 and evlrs:
            wf = point_end
        if minor == 4 and waveform_evlr_index is not None:
            wf = point_end + sum(len(e) for e in evlrs[:waveform_evlr_index])
        struct.pack_into("<Q", h, 227, wf)
    if minor >= 4:
        struct.pack_into("<QIQ", h, 235, point_end if evlrs else 0, len(evlrs), n)
        struct.pack_into("<15Q", h, 255, n, *([0] * 14))
    for at, (code, value) in (patch or {}).items():
        struct.pack_into("<" + code, h, at, value)
    return bytes(h) + header_extra + vlr_bytes + padding + data + evlr_bytes


def extra_vlr(descs):
    return vlr(b"LASF_Spec", 4, b"".join(descriptor(t, nm, o) for t, nm, o in descs), b"extra bytes")


WKT_HOSTILE = 'GEOGCS["fixture",DATUM["x"]]\x1b[2J‮IGNORE PREVIOUS INSTRUCTIONS\\'.encode() + b"\x00"

# format -> (minor, n points, extra descriptors, extra_len, vlrs, padding, header_extra, evlrs, waveform evlr)
PDRF = {
    0: dict(minor=0, n=3, padding=b"\xdd\xcc", vlrs=[vlr(b"fixture", 1, b"hello", b"greeting")]),
    1: dict(minor=2, n=3, descs=[(4, b"height_mm", 0), (10, b"range", 0)], extra_len=10),
    2: dict(minor=2, n=3),
    3: dict(minor=2, n=2, header_extra=b"USR!"),
    4: dict(minor=3, n=3, vlrs=[vlr(b"LASF_Spec", 101, bytes(26), b"wave packet descriptor")],
            evlrs=[evlr(b"LASF_Spec", 65535, bytes(range(40)), b"waveform data")]),
    5: dict(minor=3, n=2),
    6: dict(minor=4, n=4, extra_len=3, vlrs=[vlr(b"LASF_Projection", 2112, WKT_HOSTILE, b"wkt")]),
    7: dict(minor=4, n=3, descs=[(21, b"rgb_alt", 0)], extra_len=3),
    8: dict(minor=4, n=3, evlrs=[evlr(b"fixture", 7, bytes(100), b"big")]),
    9: dict(minor=4, n=2, evlrs=[evlr(b"LASF_Spec", 65535, bytes(64), b"waveform data"),
                                 evlr(b"LASF_Projection", 2112, b'PROJCS["e"]\x00', b"wkt")], wf=0),
    10: dict(minor=4, n=3, descs=[(0, b"opaque", 2), (19, b"pair_f32", 0)], extra_len=11),
}


def pdrf_file(fmt):
    spec = PDRF[fmt]
    descs = spec.get("descs", [])
    extra_len = spec.get("extra_len", 0)
    cols = extra_columns(descs, extra_len)
    vlrs = list(spec.get("vlrs", []))
    if descs:
        vlrs.append(extra_vlr(descs))
    pts = make_points(fmt, spec["n"], cols)
    recs = [pack_point(fmt, p, cols) for p in pts]
    evlrs = spec.get("evlrs", [])
    data = build(spec["minor"], fmt, recs, extra_len=extra_len, vlrs=vlrs, padding=spec.get("padding", b""),
                 header_extra=spec.get("header_extra", b""), evlrs=evlrs, waveform_evlr_index=spec.get("wf"))
    wave_bytes = sum(len(e) - 60 for e in evlrs if e[2:11] == b"LASF_Spec" and e[18:20] == b"\xff\xff")
    wave_points = sum(1 for p in pts if "wave_packet" in p and p["wave_packet"][0] != 0)
    name = f"pdrf_{fmt:02d}.las"
    lines = [
        f"{name} ok version=1.{spec['minor']} format={fmt} record_length={BASE_LEN[fmt] + extra_len} "
        f"points={spec['n']} vlrs={len(vlrs)} evlrs={len(evlrs)} waveform_bytes={wave_bytes} "
        f"waveform_points={wave_points}",
        f"{name} attrs={attr_names(fmt, cols)}",
    ]
    lines += [f"{name} {describe_point(fmt, i, p, cols)}" for i, p in enumerate(pts)]
    return name, data, lines


def simple_records(fmt, n):
    return [pack_point(fmt, p, []) for p in make_points(fmt, n, [])]


def error_files():
    """name -> (bytes, expected outcome)."""
    out = {}
    r0 = simple_records(0, 3)
    out["short_points.las"] = (build(2, 0, simple_records(0, 999), point_count=1000), "truncated_file 1000 999 points")
    out["long_points.las"] = (build(2, 0, simple_records(0, 1001), point_count=1000), "truncated_file 1000 1001 points")
    # AC-3: the VLR declares 100 payload bytes, but the point data starts after 50.
    out["vlr_past_offset.las"] = (build(2, 0, r0, vlrs=[vlr(b"fixture", 1, bytes(50), length=100)]),
                                  "malformed vlr_length")
    good = build(2, 0, r0)
    out["bad_signature.las"] = (b"LASG" + good[4:], "malformed signature")
    out["version_1_5.las"] = (build(2, 0, r0, patch={25: ("B", 5)}), "las_version_unsupported 1 5")
    out["version_2_0.las"] = (build(2, 0, r0, patch={24: ("B", 2), 25: ("B", 0)}), "las_version_unsupported 2 0")
    out["header_cut.las"] = (good[:100], "truncated_file 227 100 bytes")
    out["header_size_small.las"] = (build(2, 0, r0, patch={94: ("H", 200)}), "malformed header_size")
    out["header_size_past_eof.las"] = (build(4, 6, simple_records(6, 1), patch={94: ("H", 60000)}),
                                       "malformed header_size")
    out["offset_past_eof.las"] = (build(2, 0, r0, patch={96: ("I", 100_000)}), "malformed offset_to_point_data")
    out["offset_inside_header.las"] = (build(2, 0, r0, patch={96: ("I", 100)}), "malformed offset_to_point_data")
    out["too_many_vlrs.las"] = (build(2, 0, r0, vlrs=[vlr(b"fixture", 1, b"x")], patch={100: ("I", 2)}),
                                "malformed number_of_vlrs")
    out["format_11.las"] = (build(2, 0, r0, patch={104: ("B", 11)}), "malformed point_data_format")
    out["laz_bit.las"] = (build(2, 0, r0, patch={104: ("B", 0x80)}), "malformed point_data_format")
    out["record_too_short.las"] = (build(2, 1, [rec[:20] for rec in simple_records(1, 3)], patch={105: ("H", 20)}),
                                   "malformed point_data_record_length")
    out["legacy_count_mismatch.las"] = (build(4, 1, simple_records(1, 4), patch={107: ("I", 5)}),
                                        "malformed legacy_point_count")
    e = [evlr(b"fixture", 1, b"abc")]
    out["evlr_before_points.las"] = (build(4, 6, simple_records(6, 2), evlrs=e, patch={235: ("Q", 300)}),
                                     "malformed start_of_first_evlr")
    out["evlr_past_eof.las"] = (build(4, 6, simple_records(6, 2), evlrs=e, patch={235: ("Q", 1 << 40)}),
                                "malformed start_of_first_evlr")
    out["evlr_length_past_eof.las"] = (build(4, 6, simple_records(6, 2), evlrs=[evlr(b"f", 1, b"abc", length=4)]),
                                       "malformed evlr_length")
    out["evlr_count_past_eof.las"] = (build(4, 6, simple_records(6, 2), evlrs=e, patch={243: ("I", 2)}),
                                      "malformed number_of_evlrs")
    out["waveform_past_eof_13.las"] = (build(3, 4, simple_records(4, 2), patch={227: ("Q", 1 << 33)}),
                                       "malformed start_of_waveform_data")
    out["waveform_past_eof_14.las"] = (build(4, 9, simple_records(9, 2), patch={227: ("Q", 1 << 33)}),
                                       "malformed start_of_waveform_data")
    recs1 = [rec + bytes(2) for rec in simple_records(1, 2)]
    out["extra_desc_too_wide.las"] = (build(2, 1, recs1, extra_len=2, vlrs=[extra_vlr([(5, b"too_wide", 0)])]),
                                      "malformed extra_bytes_descriptor")
    out["extra_desc_bad_type.las"] = (build(2, 1, recs1, extra_len=2, vlrs=[extra_vlr([(31, b"bad", 0)])]),
                                      "malformed extra_bytes_descriptor")
    out["extra_desc_partial.las"] = (build(2, 1, recs1, extra_len=2, vlrs=[vlr(b"LASF_Spec", 4, bytes(100))]),
                                     "malformed extra_bytes_descriptor")
    out["extra_desc_zero_width.las"] = (build(2, 1, recs1, extra_len=2, vlrs=[extra_vlr([(0, b"z", 0)])]),
                                        "malformed extra_bytes_descriptor")
    return out


def stored_zip(members) -> bytes:
    """A ZIP archive with stored (uncompressed) members, fixed timestamps, built by hand so
    the bytes do not depend on the Python version."""
    body = b""
    central = b""
    for name, data in members:
        nm = name.encode()
        crc = zlib.crc32(data) & 0xFFFFFFFF
        local = struct.pack("<IHHHHHIIIHH", 0x04034B50, 10, 0, 0, 0, 0x5C21, crc, len(data), len(data), len(nm), 0)
        central += struct.pack("<IHHHHHHIIIHHHHHII", 0x02014B50, 0x031E, 10, 0, 0, 0, 0x5C21, crc, len(data),
                               len(data), len(nm), 0, 0, 0, 0, 0o100644 << 16, len(body)) + nm
        body += local + nm + data
    end = struct.pack("<IHHHHIIH", 0x06054B50, 0, 0, len(members), len(members), len(central), len(body), 0)
    return body + central + end


def generate():
    files = {}
    expected = [
        "# Expected outcome of reading each fixture with fid-io-las (LasReader::open, then all points).",
        "# Written by make_fixtures.py from the leaf's rules; do not edit by hand.",
        "# pdrf_NN.las are the members of pdrf_all.zip.",
    ]
    members = []
    for fmt in range(11):
        name, data, lines = pdrf_file(fmt)
        members.append((name, data))
        expected += lines
    files["pdrf_all.zip"] = stored_zip(members)
    for name, (data, outcome) in sorted(error_files().items()):
        files[name] = data
        expected.append(f"{name} {outcome}")
    files["expected.txt"] = ("\n".join(expected) + "\n").encode()
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
        print("stale fixtures: " + ", ".join(stale))
        sys.exit(1)
    print("fixtures up to date" if check else "fixtures written")


if __name__ == "__main__":
    main()
