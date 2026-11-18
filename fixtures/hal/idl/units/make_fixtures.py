#!/usr/bin/env python3
"""Independent reference for HAL.IDL.UNITS fixtures (CC-BY-4.0 project data).

Writes, next to this script:
  quantities.json          valid quantities in every unit (AC-1)
  quantities_invalid.json  rejected quantities with the expected error code (AC-2)
  encodings.json           encoding descriptors and single-value decodes (AC-3, REQ-002)
  time_sources.json        shuffled time sources and the order the leaf states (AC-5)
  directions.json          direction vectors labelled by |norm - 1| <= 1e-9 (AC-6, REQ-006)
  frames_arm.json          sensor -> mount -> device -> machine -> part (AC-4)
  frames_cycle.json        sensor -> mount -> sensor (AC-4)
  frames_invalid.json      further graphs that must fail with frame_graph_invalid (REQ-005)

Expected values come only from the rules in spec/hal/idl/units/NODE.md and from
Python's own float64 arithmetic and uuid module, never from the Rust crate.

Wire conventions checked here (see crates/fid-hal-idl/src/units/mod.rs):
  f64 values are "0x" + 16 lowercase hex digits of the IEEE 754 bits (fid-canon F64Bits);
  i64 values are decimal strings (fid-canon I64);
  frame IDs are UUIDv5 of the session namespace and the UTF-8 frame name.

Python floats are IEEE 754 binary64 and CPython evaluates `offset + scale * raw` as a
separate multiply and add (no fused multiply-add), which is the definition in REQ-002.

Run:  python fixtures/hal/idl/units/make_fixtures.py
"""

import json
import math
import random
import struct
import uuid
from pathlib import Path

HERE = Path(__file__).resolve().parent
UNITS = ["m", "rad", "s", "K", "Pa", "1"]
TOL = 1e-9


def bits(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def bits_raw(u: int) -> str:
    return "0x%016x" % u


def f32_bits(x: float) -> str:
    return "0x" + struct.pack(">f", x).hex()


def f32_value(hexstr: str) -> float:
    return struct.unpack(">f", bytes.fromhex(hexstr[2:]))[0]


def f64_value(hexstr: str) -> float:
    return struct.unpack(">d", bytes.fromhex(hexstr[2:]))[0]


def write(name: str, data) -> None:
    text = json.dumps(data, indent=1, sort_keys=True, allow_nan=False, ensure_ascii=False)
    (HERE / name).write_text(text + "\n", encoding="utf-8", newline="\n")


# ---------------------------------------------------------------- quantities (AC-1, AC-2)

VALUES = {
    "m": [0.001, -0.0254, 1e-9, 12.5, 0.0, -0.0],
    "rad": [math.pi, -math.pi / 2, 1.5e-6],
    "s": [0.5, 86400.0, 5e-324],
    "K": [293.15, 0.001, 1.7976931348623157e308],
    "Pa": [101325.0, -2.5e-3],
    "1": [0.45, 1.0, -1.0, 2.2250738585072014e-308],
}
quantities = []
for unit in UNITS:
    for v in VALUES[unit]:
        assert math.isfinite(v)
        # "decimal" is the same value as a plain JSON number: the Rust test checks that the
        # bit string decodes to it. -0.0 prints as 0 in canonical JSON, so the sign is
        # carried separately.
        quantities.append({"quantity": {"value": bits(v), "unit": unit}, "decimal": abs(v),
                           "negative": math.copysign(1.0, v) < 0})
write("quantities.json", quantities)

NAN = bits_raw(0x7FF8000000000000)
invalid_q = [
    {"name": "unit_mm", "input": {"value": bits(1.0), "unit": "mm"}, "expect": "unit_unsupported"},
    {"name": "unit_upper_M", "input": {"value": bits(1.0), "unit": "M"}, "expect": "unit_unsupported"},
    {"name": "unit_deg", "input": {"value": bits(1.0), "unit": "deg"}, "expect": "unit_unsupported"},
    {"name": "unit_degC", "input": {"value": bits(20.0), "unit": "degC"}, "expect": "unit_unsupported"},
    {"name": "unit_empty", "input": {"value": bits(1.0), "unit": ""}, "expect": "unit_unsupported"},
    {"name": "value_nan", "input": {"value": NAN, "unit": "m"}, "expect": "quantity_not_finite"},
    {"name": "value_nan_payload", "input": {"value": bits_raw(0xFFF0000000000001), "unit": "s"},
     "expect": "quantity_not_finite"},
    {"name": "value_pos_inf", "input": {"value": bits_raw(0x7FF0000000000000), "unit": "K"},
     "expect": "quantity_not_finite"},
    {"name": "value_neg_inf", "input": {"value": bits_raw(0xFFF0000000000000), "unit": "Pa"},
     "expect": "quantity_not_finite"},
    {"name": "value_plain_number", "input": {"value": 1.5, "unit": "m"}, "expect": "bad_type"},
    {"name": "value_upper_hex", "input": {"value": "0x3FF0000000000000", "unit": "m"}, "expect": "bad_value"},
    {"name": "value_short_hex", "input": {"value": "0x3ff", "unit": "m"}, "expect": "bad_value"},
    {"name": "missing_unit", "input": {"value": bits(1.0)}, "expect": "missing_field"},
    {"name": "extra_field", "input": {"value": bits(1.0), "unit": "m", "display": "mm"},
     "expect": "unknown_field"},
    {"name": "not_object", "input": [bits(1.0), "m"], "expect": "bad_type"},
]
write("quantities_invalid.json", invalid_q)

# ---------------------------------------------------------------- encodings (AC-3, REQ-002)

INT_RANGES = {
    "i16": (-(1 << 15), (1 << 15) - 1),
    "i32": (-(1 << 31), (1 << 31) - 1),
    "i64": (-(1 << 63), (1 << 63) - 1),
    "u16": (0, (1 << 16) - 1),
    "u32": (0, (1 << 32) - 1),
}


def desc(numeric, scale, offset, unit):
    return {"numeric": numeric, "scale": scale, "offset": offset, "unit": unit}


descriptors = [
    {"name": "i32_nanometres", "descriptor": desc("i32", bits(1e-9), bits(0.0), "m"), "expect": "ok"},
    {"name": "i16_tenth_kelvin", "descriptor": desc("i16", bits(0.1), bits(273.15), "K"), "expect": "ok"},
    {"name": "i64_ns_seconds", "descriptor": desc("i64", bits(1e-9), bits(0.0), "s"), "expect": "ok"},
    {"name": "u16_pascal", "descriptor": desc("u16", bits(2.0), bits(-100.0), "Pa"), "expect": "ok"},
    {"name": "u32_encoder_rad", "descriptor": desc("u32", bits(2 * math.pi / 2**32), bits(-math.pi), "rad"),
     "expect": "ok"},
    {"name": "f32_metres", "descriptor": desc("f32", bits(1.0), bits(0.0), "m"), "expect": "ok"},
    {"name": "f64_millimetres", "descriptor": desc("f64", bits(0.001), bits(0.0), "m"), "expect": "ok"},
    {"name": "f64_negative_scale", "descriptor": desc("f64", bits(-1.0), bits(5.0), "1"), "expect": "ok"},
    {"name": "scale_zero", "descriptor": desc("i32", bits(0.0), bits(0.0), "m"), "expect": "encoding_invalid"},
    {"name": "scale_negative_zero", "descriptor": desc("i32", bits(-0.0), bits(0.0), "m"),
     "expect": "encoding_invalid"},
    {"name": "scale_nan", "descriptor": desc("i32", NAN, bits(0.0), "m"), "expect": "encoding_invalid"},
    {"name": "scale_inf", "descriptor": desc("i32", bits_raw(0x7FF0000000000000), bits(0.0), "m"),
     "expect": "encoding_invalid"},
    {"name": "offset_nan", "descriptor": desc("i32", bits(1.0), NAN, "m"), "expect": "encoding_invalid"},
    {"name": "offset_neg_inf", "descriptor": desc("i32", bits(1.0), bits_raw(0xFFF0000000000000), "m"),
     "expect": "encoding_invalid"},
    {"name": "unit_px", "descriptor": desc("u16", bits(1.0), bits(0.0), "px"), "expect": "encoding_invalid"},
    {"name": "unit_mm", "descriptor": desc("i32", bits(1.0), bits(0.0), "mm"), "expect": "encoding_invalid"},
    {"name": "numeric_u64", "descriptor": desc("u64", bits(1.0), bits(0.0), "m"), "expect": "encoding_invalid"},
    {"name": "numeric_i8", "descriptor": desc("i8", bits(1.0), bits(0.0), "m"), "expect": "encoding_invalid"},
    {"name": "missing_offset", "descriptor": {"numeric": "i32", "scale": bits(1.0), "unit": "m"},
     "expect": "missing_field"},
]


def raw_float(numeric, raw):
    """Raw wire value -> exact Python float (int -> float rounds to nearest even)."""
    if numeric == "f32":
        return f32_value(raw)
    if numeric == "f64":
        return f64_value(raw)
    n = int(raw)
    lo, hi = INT_RANGES[numeric]
    assert lo <= n <= hi
    return float(n)


by_name = {d["name"]: d["descriptor"] for d in descriptors}
DECODE_INPUTS = [
    ("i32_nanometres", "123456789"),
    ("i32_nanometres", "-2147483648"),
    ("i32_nanometres", "2147483647"),
    ("i16_tenth_kelvin", "-32768"),
    ("i16_tenth_kelvin", "200"),
    ("i16_tenth_kelvin", "3"),
    ("i64_ns_seconds", "9007199254740993"),  # 2^53 + 1: int -> float rounds
    ("i64_ns_seconds", "-9223372036854775808"),
    ("i64_ns_seconds", "1700000000123456789"),
    ("u16_pascal", "65535"),
    ("u16_pascal", "0"),
    ("u32_encoder_rad", "4294967295"),
    ("u32_encoder_rad", "1073741824"),
    ("f32_metres", f32_bits(0.1)),
    ("f32_metres", f32_bits(-3.4028234663852886e38)),
    ("f64_millimetres", bits(25.4)),
    ("f64_millimetres", bits(0.3)),
    ("f64_negative_scale", bits(5.0)),  # 5 + (-1 * 5) = 0
]
decodes = []
for name, raw in DECODE_INPUTS:
    d = by_name[name]
    scale, offset = f64_value(d["scale"]), f64_value(d["offset"])
    product = scale * raw_float(d["numeric"], raw)
    value = offset + product
    assert math.isfinite(value)
    decodes.append({"descriptor": name, "raw": raw, "expect": bits(value), "unit": d["unit"]})

# Overflow to infinity and a NaN raw float are not finite quantities.
huge = desc("f64", bits(1e300), bits(0.0), "m")
by_name["f64_huge_scale"] = huge
descriptors.append({"name": "f64_huge_scale", "descriptor": huge, "expect": "ok"})
assert not math.isfinite(1e300 * 1e10)
decodes.append({"descriptor": "f64_huge_scale", "raw": bits(1e10), "expect": "quantity_not_finite", "unit": "m"})
decodes.append({"descriptor": "f32_metres", "raw": "0x7fc00000", "expect": "quantity_not_finite", "unit": "m"})

write("encodings.json", {"descriptors": descriptors, "decodes": decodes})

# ---------------------------------------------------------------- time sources (AC-5)

ORDER = ["HW_TRIGGER", "PTP", "DEVICE_CLOCK", "NTP", "HOST_RECEIVE"]  # best first, from the leaf
rng = random.Random(20261117)
shuffles = []
for _ in range(6):
    s = ORDER[:]
    rng.shuffle(s)
    shuffles.append(s)
shuffles.append(list(reversed(ORDER)))
shuffles.append(["NTP", "PTP", "NTP", "HOST_RECEIVE", "HW_TRIGGER", "PTP"])  # duplicates
write("time_sources.json", [
    {"shuffled": s, "sorted": sorted(s, key=ORDER.index)} for s in shuffles
])

# ---------------------------------------------------------------- directions (AC-6, REQ-006)


def norm_dev(v):
    x, y, z = v
    n2 = x * x + y * y + z * z  # left to right, separate multiply and add
    return abs(math.sqrt(n2) - 1.0)


s3 = 1.0 / math.sqrt(3.0)
VECTORS = [
    ("x_axis", (1.0, 0.0, 0.0)),
    ("neg_z", (0.0, 0.0, -1.0)),
    ("three_four_five", (0.6, 0.8, 0.0)),
    ("diagonal", (s3, s3, s3)),
    ("ac6_leaf_example", (1.0, 0.0, 1e-6)),  # |norm - 1| ~ 5e-13: within 1e-9 (see PR)
    ("tilted_1e-4", (1.0, 0.0, 1e-4)),        # |norm - 1| ~ 5e-9
    ("long_by_2e-9", (1.0 + 2e-9, 0.0, 0.0)),
    ("long_by_5e-10", (1.0 + 5e-10, 0.0, 0.0)),
    ("short_by_2e-9", (0.0, 1.0 - 2e-9, 0.0)),
    ("zero", (0.0, 0.0, 0.0)),
    ("two_x", (2.0, 0.0, 0.0)),
    ("huge", (1e200, 0.0, 0.0)),
]
directions = []
for name, v in VECTORS:
    ok = norm_dev(v) <= TOL
    directions.append({"name": name, "vector": [bits(c) for c in v], "expect": "ok" if ok else "direction_not_unit"})
assert [d["expect"] for d in directions if d["name"] == "ac6_leaf_example"] == ["ok"]
assert [d["expect"] for d in directions if d["name"] == "tilted_1e-4"] == ["direction_not_unit"]
for name, comps in [("nan", [NAN, bits(0.0), bits(0.0)]), ("inf", [bits_raw(0x7FF0000000000000), bits(0.0), bits(0.0)])]:
    directions.append({"name": name, "vector": comps, "expect": "direction_not_unit"})
write("directions.json", directions)

# ---------------------------------------------------------------- frames (AC-4, REQ-005)

SESSION_NS = uuid.UUID("6f1c2a3b-4d5e-4f60-8a71-92b3c4d5e6f7")


def fid(name):
    return str(uuid.uuid5(SESSION_NS, name))


def frame(name, kind):
    return {"id": fid(name), "kind": kind}


def edge(parent, child, tref, time_varying=False, pose=None):
    e = {"parent": fid(parent), "child": fid(child), "transform": tref, "time_varying": time_varying}
    if pose is not None:
        e["pose"] = pose
    return e


ids = {n: fid(n) for n in ["sensor", "mount", "device", "machine", "part", "sensor2", "a", "b", "c"]}

# Arrow a -> b means "a is located in b": b is the parent of a.
arm = {
    "frames": [frame("sensor", "sensor"), frame("mount", "mount"), frame("device", "device"),
               frame("machine", "machine"), frame("part", "part")],
    "edges": [edge("mount", "sensor", "xfm:sensor-in-mount"),
              edge("device", "mount", "xfm:mount-in-device"),
              edge("machine", "device", "xfm:device-in-machine", True, "pose:arm-joint-stream"),
              edge("part", "machine", "xfm:machine-in-part", False, "pose:alignment-1")],
}
write("frames_arm.json", {"session_namespace": str(SESSION_NS), "names": ids, "graph": arm})

cycle = {
    "frames": [frame("sensor", "sensor"), frame("mount", "mount")],
    "edges": [edge("mount", "sensor", "xfm:sensor-in-mount"), edge("sensor", "mount", "xfm:mount-in-sensor")],
}
write("frames_cycle.json", {"graph": cycle})

invalid_graphs = [
    {"name": "two_parents", "graph": {
        "frames": [frame("sensor", "sensor"), frame("mount", "mount"), frame("device", "device")],
        "edges": [edge("mount", "sensor", "xfm:1"), edge("device", "sensor", "xfm:2")]}},
    {"name": "duplicate_edge", "graph": {
        "frames": [frame("sensor", "sensor"), frame("mount", "mount")],
        "edges": [edge("mount", "sensor", "xfm:1"), edge("mount", "sensor", "xfm:1")]}},
    {"name": "self_loop", "graph": {
        "frames": [frame("sensor", "sensor")],
        "edges": [edge("sensor", "sensor", "xfm:1")]}},
    {"name": "three_cycle_with_tail", "graph": {
        "frames": [frame("a", "device"), frame("b", "device"), frame("c", "device"), frame("part", "part"),
                   frame("sensor", "sensor")],
        "edges": [edge("a", "b", "xfm:1"), edge("b", "c", "xfm:2"), edge("c", "a", "xfm:3"),
                  edge("a", "sensor", "xfm:4")]}},
    {"name": "undeclared_frame", "graph": {
        "frames": [frame("sensor", "sensor")],
        "edges": [edge("mount", "sensor", "xfm:1")]}},
    {"name": "duplicate_frame", "graph": {
        "frames": [frame("sensor", "sensor"), frame("sensor", "mount")],
        "edges": []}},
]
write("frames_invalid.json", invalid_graphs)

print("wrote fixtures to", HERE)
