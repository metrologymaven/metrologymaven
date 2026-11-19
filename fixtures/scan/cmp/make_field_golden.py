#!/usr/bin/env python3
"""Independent reference for SCAN.CMP.DEV.FIELD; writes field_golden.json and field_numbers.txt.

Stdlib only (hashlib, struct, decimal, json). Shares no code with the Rust crate
fid-scan-cmp: it formats numbers from Python's own shortest repr, writes canonical JSON
with the json module, builds each layout as one bytes object and hashes it with hashlib.
The rules below are the ones pinned in spec/scan/cmp/dev/field/NODE.md (Design).

Layout version 1 (all integers little-endian):

    "FIDDEV" | 0x01 | prov_len u64 | provenance (canonical JSON, prov_len bytes)
    | n u64 | n x (status u8 | value f64)

Status bytes: VALID 1, NO_MATCH 2, EXCLUDED 3. A point that is not VALID stores +0.0.
A VALID value of -0.0 is stored as +0.0; every other value keeps its IEEE 754 bits.

Provenance canonical JSON: RFC 8785 restricted to strings (kernel convention: every
number is a string), keys sorted, no whitespace. Members:

    actual_digest, actual_selection (omitted when absent), algorithm_id,
    alignment_digest, alignment_id, kernel_version, max_angle_rad, max_distance_m,
    method ("normal" | "shortest"), nominal_digest, nominal_kind (GEO.TYPES.ENV wire
    name), nominal_selection (omitted when absent)

max_angle_rad and max_distance_m are written as the ECMAScript Number::toString of the
f64 (the RFC 8785 number form), inside a JSON string; -0.0 is written "0".

Inputs per case are written into the fixture (points as tokens: "V:<f64 hex bits>" for
VALID, "N" for NO_MATCH, "X" for EXCLUDED); the Rust test builds the same field from them.
Digests in the provenance are SHA-256 of short ASCII labels.

Statistics (stat_*): counts per status, and over the VALID values only (-0.0 read as +0.0)
the minimum, maximum and mean; the mean here is the exact rational sum rounded once to f64,
then divided by the count (one more rounding). Empty strings when no value is VALID.

Run:  python fixtures/scan/cmp/make_field_golden.py [--check]
With --check nothing is written; the script exits non-zero if a file differs.
"""

import hashlib
import json
import math
import os
import struct
import sys
from decimal import Decimal
from fractions import Fraction

HERE = os.path.dirname(os.path.abspath(__file__))
GOLDEN = os.path.join(HERE, "field_golden.json")
NUMBERS = os.path.join(HERE, "field_numbers.txt")

STATUS = {"V": 1, "N": 2, "X": 3}


def es_number(x):
    """ECMAScript Number::toString for a finite float (RFC 8785 section 3.2.2.3)."""
    if not math.isfinite(x):
        raise ValueError("non-finite")
    if x == 0:
        return "0"
    sign, digits, exp = Decimal(repr(x)).as_tuple()
    digits = list(digits)
    while len(digits) > 1 and digits[-1] == 0:
        digits.pop()
        exp += 1
    s = "".join(str(d) for d in digits)
    k = len(s)
    n = exp + k  # value = 0.s * 10**n
    if k <= n <= 21:
        out = s + "0" * (n - k)
    elif 0 < n <= 21:
        out = s[:n] + "." + s[n:]
    elif -6 < n <= 0:
        out = "0." + "0" * (-n) + s
    else:
        e = n - 1
        out = s[0] + ("." + s[1:] if k > 1 else "") + "e" + ("+" if e >= 0 else "-") + str(abs(e))
    return ("-" if sign else "") + out


def f64_hex(x):
    return "%016x" % struct.unpack("<Q", struct.pack("<d", x))[0]


def label_digest(label):
    return "sha256:" + hashlib.sha256(label.encode("ascii")).hexdigest()


def provenance(case):
    p = {
        "actual_digest": case["actual_digest"],
        "algorithm_id": case["algorithm_id"],
        "alignment_digest": case["alignment_digest"],
        "alignment_id": case["alignment_id"],
        "kernel_version": case["kernel_version"],
        "max_angle_rad": es_number(case["max_angle_rad"]),
        "max_distance_m": es_number(case["max_distance_m"]),
        "method": case["method"],
        "nominal_digest": case["nominal_digest"],
        "nominal_kind": case["nominal_kind"],
    }
    if case["actual_selection"]:
        p["actual_selection"] = case["actual_selection"]
    if case["nominal_selection"]:
        p["nominal_selection"] = case["nominal_selection"]
    text = json.dumps(p, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return text.encode("utf-8")


def layout(case):
    prov = provenance(case)
    out = bytearray(b"FIDDEV")
    out.append(1)
    out += struct.pack("<Q", len(prov))
    out += prov
    out += struct.pack("<Q", len(case["points"]))
    for status, value in case["points"]:
        out.append(STATUS[status])
        if status == "V":
            bits = struct.unpack("<Q", struct.pack("<d", value))[0]
            if bits == 1 << 63:
                bits = 0
            out += struct.pack("<Q", bits)
        else:
            out += struct.pack("<Q", 0)
    return bytes(out), prov


def base(name, **kw):
    case = {
        "name": name,
        "method": "normal",
        "max_distance_m": 0.002,
        "max_angle_rad": math.pi / 6,
        "alignment_id": "aln-rps-0001",
        "alignment_digest": label_digest("alignment v3"),
        "actual_digest": label_digest("actual scan 17"),
        "nominal_digest": label_digest("nominal cad body"),
        "nominal_kind": "cad_body_ref",
        "actual_selection": "",
        "nominal_selection": "",
        "algorithm_id": "scan.cmp_dev.normal@1.0.0",
        "kernel_version": "0.0.1",
        "points": [],
    }
    case.update(kw)
    return case


CASES = [
    # AC-1: five points; the Rust test fills them in order 4, 2, 0, 1, 3 and 0..4.
    base(
        "ac1_five_points",
        points=[("V", 0.000125), ("V", -0.00025), ("V", 1e-07), ("N", None), ("V", -0.0)],
    ),
    # AC-4: values, one NO_MATCH and one EXCLUDED point.
    base(
        "ac4_golden",
        points=[("V", 0.0003), ("N", None), ("V", -0.0011), ("X", None), ("V", 0.0019999999999999996)],
    ),
    # Shortest method, mesh nominal, both selections, zero angle cutoff, exponent forms.
    base(
        "selections_mesh",
        method="shortest",
        max_distance_m=1e21,
        max_angle_rad=0.0,
        nominal_kind="mesh",
        nominal_digest=label_digest("nominal stl"),
        actual_selection=label_digest("actual selection"),
        nominal_selection=label_digest("nominal selection"),
        algorithm_id="scan.cmp_dev.shortest@1.2.3",
        points=[("X", None), ("V", 5e-324), ("V", -1.7976931348623157e308), ("N", None)],
    ),
    # Empty actual: no points.
    base("empty", max_distance_m=1e-07, max_angle_rad=math.pi, points=[]),
    # Every point NO_MATCH or EXCLUDED: no statistic has a value.
    base("no_valid", points=[("N", None), ("X", None), ("N", None)]),
]

NUMBER_CASES = [
    0.0, -0.0, 1.0, 100.0, 0.1, 0.001, 0.002, 0.0005, 0.000001, 0.0000015, 1e-07, 1.5e-07,
    123.456, 1e20, 1e21, 1.5e21, 123456789012345680000.0, 5e-324, 1.7976931348623157e308,
    math.pi, math.pi / 6, math.pi / 4, 2.5e-05, 0.30000000000000004, -0.25, -1e-09,
]


def stats(points):
    """Counts, totalOrder min and max, and the mean (exact sum, rounded once, divided by n)
    over the VALID values only, -0.0 stored as +0.0. Empty strings when there is no VALID
    value."""
    vals = [0.0 if (v == 0.0) else v for s, v in points if s == "V"]
    out = {
        "stat_valid": sum(1 for s, _ in points if s == "V"),
        "stat_no_match": sum(1 for s, _ in points if s == "N"),
        "stat_excluded": sum(1 for s, _ in points if s == "X"),
        "stat_min_hex": "",
        "stat_max_hex": "",
        "stat_mean_hex": "",
    }
    if vals:
        out["stat_min_hex"] = f64_hex(min(vals))
        out["stat_max_hex"] = f64_hex(max(vals))
        total = float(sum((Fraction(v) for v in vals), Fraction(0)))
        out["stat_mean_hex"] = f64_hex(total / len(vals))
    return out


def points_tokens(points):
    out = []
    for status, value in points:
        out.append("V:" + f64_hex(value) if status == "V" else status)
    return " ".join(out)


def render():
    cases = []
    for c in CASES:
        lay, prov = layout(c)
        cases.append(
            {
                "name": c["name"],
                "method": c["method"],
                "max_distance_m_hex": f64_hex(c["max_distance_m"]),
                "max_angle_rad_hex": f64_hex(c["max_angle_rad"]),
                "alignment_id": c["alignment_id"],
                "alignment_digest": c["alignment_digest"],
                "actual_digest": c["actual_digest"],
                "nominal_digest": c["nominal_digest"],
                "nominal_kind": c["nominal_kind"],
                "actual_selection": c["actual_selection"],
                "nominal_selection": c["nominal_selection"],
                "algorithm_id": c["algorithm_id"],
                "kernel_version": c["kernel_version"],
                "points": points_tokens(c["points"]),
                "provenance_canonical": prov.decode("utf-8"),
                "provenance_sha256": hashlib.sha256(prov).hexdigest(),
                "layout_len": len(lay),
                "sha256": hashlib.sha256(lay).hexdigest(),
            }
        )
        cases[-1].update(stats(c["points"]))
    doc = {
        "about": "SCAN.CMP.DEV.FIELD golden vectors, layout version 1; written by make_field_golden.py (CC0)",
        "cases": cases,
    }
    golden = json.dumps(doc, indent=1, sort_keys=True) + "\n"
    lines = [
        "# SCAN.CMP.DEV.FIELD number strings: f64 hex bits, ECMAScript Number::toString.",
        "# Written by make_field_golden.py (CC0).",
    ]
    for x in NUMBER_CASES:
        lines.append(f64_hex(x) + " " + es_number(x))
    numbers = "\n".join(lines) + "\n"
    return {GOLDEN: golden, NUMBERS: numbers}


def main():
    files = render()
    if "--check" in sys.argv[1:]:
        bad = False
        for path, text in files.items():
            try:
                with open(path, encoding="utf-8", newline="") as f:
                    have = f.read()
            except FileNotFoundError:
                have = None
            if have != text:
                print("stale: " + os.path.relpath(path), file=sys.stderr)
                bad = True
        return 1 if bad else 0
    for path, text in files.items():
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
