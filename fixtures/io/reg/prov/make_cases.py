#!/usr/bin/env python3
"""Fixtures and expected values for the IO.REG.PROV tests (crates/fid-io-reg/tests/prov.rs).

Independent of the Rust code: every expected value is computed here from the rules written
in spec/io/reg/prov/NODE.md (Design), with Python's hashlib, fractions and json. Run from the
repository root:

    python fixtures/io/reg/prov/make_cases.py          # rewrite cube_mm.xyz and expected.txt
    python fixtures/io/reg/prov/make_cases.py --check  # fail if either file differs

All data is synthetic and hand-made (no third-party samples, D-038). Licence: CC-BY-4.0.

cube_mm.xyz: the 8 corners of a 25.4 mm cube, one point per line, `x y z` separated by one
space, LF line ends, zeros written `0` and 25.4 written `25.40` (so the file is 96 bytes).
Corner i has x = bit 0, y = bit 1, z = bit 2 of i.

expected.txt, one record per line, fields separated by one space:

- `cube <bytes> <sha256 hex>`: size and SHA-256 of cube_mm.xyz.
- `cube_si <i> <x bits> <y bits> <z bits>`: corner i converted from mm to metres, each value
  the f64 nearest to (decimal text) * 1/1000, ties to even, as 16 lowercase hex digits of its
  IEEE 754 bits. The text is first read as the nearest f64 (as any reader does), then that
  f64 is scaled with one rounding.
- `conv <unit> <in bits> <out bits>`: `out` is the f64 nearest to the exact product of the
  f64 `in` and the unit's exact factor (round half to even, subnormals included); the sign of
  a zero result is the sign of `in`.
- `frame <32 hex> <name>`: the synthesized source frame of cube_mm.xyz: ID = first 16 bytes
  of SHA-256(b"fid.io.reg.prov.source_frame@1" + b"\\x00" + the 32 digest bytes), name =
  `source:` + the first 12 hex digits of the digest.
- `provenance <json>`: the RFC 8785 canonical JSON of the AC-1 provenance record (built by
  the test with the inputs listed in `ac1_record` below, the principal committed with
  AC1_SALT and AC1_IDENTITY). Every value is a string, a bool, null or an object, so JCS
  equals sorted keys with no whitespace.
"""

import hashlib
import json
import math
import os
import random
import struct
import sys
from fractions import Fraction

# AC-1 principal: an agent with identity "agent-7", identity salt bytes 0x00..0x0f, no
# on_behalf_of. The test builds the same commitment through fid-journal.
AC1_SALT = bytes(range(16))
AC1_IDENTITY = "agent-7"

HERE = os.path.dirname(os.path.abspath(__file__))

# Exact factors to metres (GEO.FRAME.UNITS table) and their decimal text (pinned in NODE.md).
UNITS = {
    "m": (Fraction(1), "1"),
    "mm": (Fraction(1, 1000), "0.001"),
    "um": (Fraction(1, 10**6), "0.000001"),
    "nm": (Fraction(1, 10**9), "0.000000001"),
    "in": (Fraction(127, 5000), "0.0254"),
    "ft": (Fraction(381, 1250), "0.3048"),
    "thou": (Fraction(127, 5000000), "0.0000254"),
}


def bits(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def from_bits(h):
    return struct.unpack("<d", struct.pack("<Q", int(h, 16)))[0]


def scale(x, unit):
    """The f64 nearest to x * factor (CPython int / int true division is correctly rounded,
    half to even, subnormals included); the sign of x is kept on a zero result."""
    factor, _ = UNITS[unit]
    exact = abs(Fraction(x)) * factor
    out = exact.numerator / exact.denominator
    return math.copysign(out, x)


def cube_bytes():
    lines = []
    for i in range(8):
        coords = ["25.40" if (i >> axis) & 1 else "0" for axis in range(3)]
        lines.append(" ".join(coords) + "\n")
    return "".join(lines).encode("ascii")


def conv_inputs(rng):
    fixed = [
        1.0, -1.0, 25.4, -25.4, 0.1, 0.0, -0.0, 1e-300, -1e-300, 5e-324, -5e-324,
        2.2250738585072014e-308, 2.225073858507201e-308, 1.7976931348623157e308,
        -1.7976931348623157e308, 9007199254740993.0, 123456.789, 1e-9, 3.0,
    ]
    out = list(fixed)
    while len(out) < len(fixed) + 120:
        x = from_bits("%016x" % rng.getrandbits(64))
        if math.isfinite(x):
            out.append(x)
    # Values in a realistic range too.
    for _ in range(60):
        out.append(rng.uniform(-2000.0, 2000.0))
    return out


def ac1_record(digest_hex, size, frame_id, frame_name):
    return {
        "schema": "source_provenance@1",
        "sha256": "sha256:" + digest_hex,
        "bytes": str(size),
        "source_name": "cube_mm.xyz",
        "format": "fid-test-xyz",
        "format_version": None,
        "format_selected_by": "caller",
        "reader": "fid-test-xyz",
        "reader_version": "0.1.0",
        "principal": {
            "kind": "agent",
            # CORE.JRN.ENTRY REQ-003: SHA-256 over the 16-byte salt then the RFC 8785 value.
            "commit": "sha256:" + hashlib.sha256(AC1_SALT + json.dumps(AC1_IDENTITY).encode("utf-8")).hexdigest(),
            "on_behalf_of_commit": None,
        },
        "options": {"delimiter": "space"},
        "unit": "mm",
        "unit_factor": "0.001",
        "unit_source": "caller",
        "unit_overridden": True,
        "frame": frame_id,
        "frame_name": frame_name,
        "frame_origin": "synthesized",
        "evidence": "uncharacterized",
        "origin_claim": None,
        "declared": {},
    }


def build():
    cube = cube_bytes()
    assert len(cube) == 96, len(cube)
    digest = hashlib.sha256(cube).digest()
    lines = ["cube %d %s" % (len(cube), digest.hex())]
    for i, line in enumerate(cube.decode("ascii").splitlines()):
        xs = [scale(float(t), "mm") for t in line.split(" ")]
        lines.append("cube_si %d %s" % (i, " ".join(bits(v) for v in xs)))
    # AC-1 hand check: (25.4, 0, 0) becomes 0.0254 m; AC-4 hand check: 1 in is 0x3f9a027525460aa6.
    assert bits(scale(25.4, "mm")) == bits(0.0254)
    assert bits(scale(1.0, "in")) == "3f9a027525460aa6"
    rng = random.Random(20261119)
    inputs = conv_inputs(rng)
    for unit in UNITS:
        for x in inputs:
            lines.append("conv %s %s %s" % (unit, bits(x), bits(scale(x, unit))))
    frame_id = hashlib.sha256(b"fid.io.reg.prov.source_frame@1" + b"\x00" + digest).digest()[:16].hex()
    frame_name = "source:" + digest.hex()[:12]
    lines.append("frame %s %s" % (frame_id, frame_name))
    rec = ac1_record(digest.hex(), len(cube), frame_id, frame_name)
    js = json.dumps(rec, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    lines.append("provenance " + js)
    return cube, ("\n".join(lines) + "\n").encode("utf-8")


def main():
    cube, expected = build()
    targets = [(os.path.join(HERE, "cube_mm.xyz"), cube), (os.path.join(HERE, "expected.txt"), expected)]
    if "--check" in sys.argv[1:]:
        bad = False
        for path, data in targets:
            try:
                with open(path, "rb") as f:
                    have = f.read()
            except OSError:
                have = None
            if have != data:
                print("differs: " + os.path.relpath(path))
                bad = True
        sys.exit(1 if bad else 0)
    for path, data in targets:
        with open(path, "wb") as f:
            f.write(data)


if __name__ == "__main__":
    main()
