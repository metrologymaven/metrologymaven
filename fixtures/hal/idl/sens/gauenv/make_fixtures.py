#!/usr/bin/env python3
"""Independent fixture generator for HAL.IDL.SENS.GAUENV (Gauge@1 and Environment@1 packages,
their readings and reading batches, and the Gauge-core@1 and Environment-core@1 profiles).

Writes project-authored fixtures (CC-BY-4.0) under fixtures/hal/idl/sens/gauenv/:
  - caliper_read.json     (AC-1: RFC 8785 gauge-reading/1, value 0.01234 m, resolution 1e-5 m)
  - room_env.json         (AC-4: RFC 8785 env-reading/1, 293.15 K, u 0.05 K)
  - expected.json         (AC-1, AC-4: sizes, digests and resolution steps, computed here)
  - cases.json            (AC-2, AC-3 and the other rules: single-fault readings and batches,
                           each hand-labelled with the expected outcome; this script's own
                           validator must agree with every label)
  - gauge_v1.json         (AC-5: the Gauge@1 package after the streaming template, written from
                           the leaf's rules, not from the Rust code)
  - gauge_v1.sha256       (AC-5: SHA-256 over this script's RFC 8785 canonical form with lists
                           sorted as REQ-HAL.IDL.GEN.FMT-003 says)
  - environment_v1.json   (AC-5: the Environment@1 package, likewise)
  - environment_v1.sha256

All device data is invented (D-038): the gauges, sensors, serials and calibration records do not
exist, and every identity says so. Nothing here is a measurement. Stdlib only; shares no code
with crates/fid-hal-idl.
Run:  python fixtures/hal/idl/sens/gauenv/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import math
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

MAX_IDENTITY = 256
MAX_READING_BYTES = 4096
MAX_BATCH_READINGS = 50_000
MAX_BATCH_BYTES = 16 * 1024 * 1024
TOLERANCE = 1e-9
MAX_STEPS = float(2 ** 53)
UNITS = ["m", "rad", "s", "K", "Pa", "1"]
I64_MIN, I64_MAX = -(1 << 63), (1 << 63) - 1

GAUGE_TAG, GAUGE_BATCH = "gauge-reading/1", "gauge-batch/1"
ENV_TAG, ENV_BATCH = "env-reading/1", "env-batch/1"


def jcs(value) -> bytes:
    """RFC 8785 canonical JSON for the values used here (strings, booleans, small integers,
    arrays, objects with keys sorted by UTF-16 code units)."""
    if value is None:
        return b"null"
    if isinstance(value, bool):
        return b"true" if value else b"false"
    if isinstance(value, int):
        return str(value).encode("ascii")
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if isinstance(value, list):
        return b"[" + b",".join(jcs(v) for v in value) + b"]"
    if isinstance(value, dict):
        items = sorted(value.items(), key=lambda kv: kv[0].encode("utf-16-be"))
        return b"{" + b",".join(jcs(k) + b":" + jcs(v) for k, v in items) + b"}"
    raise TypeError(f"unsupported type: {type(value)}")


def sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def f64_hex(x: float) -> str:
    return f"0x{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def hex_f64(s: str) -> float:
    return struct.unpack("<d", struct.pack("<Q", int(s[2:], 16)))[0]


def q(value: float, unit: str):
    return {"unit": unit, "value": f64_hex(value)}


# ------------------------------------------------------------------ the packages, from the spec

def ref(name):
    return {"$ref": "#/$defs/" + name}


def string(max_len):
    return {"type": "string", "maxLength": max_len}


def closed(props, required):
    return {"type": "object", "additionalProperties": False, "properties": props, "required": required}


def enum(values):
    return {"type": "string", "enum": values}


# The HAL.IDL.SENS.STREAM template, restated from that leaf's rules.
START_ERRORS = [
    "TRANSIENT", "COMM_LOST", "CONFIG_INVALID", "NOT_QUALIFIED", "CALIBRATION_EXPIRED", "LIMIT",
    "SAFETY_STOP_OBSERVED", "VENDOR_LICENSE", "UNSUPPORTED", "INTERNAL",
]
READ_ERRORS = ["TRANSIENT", "COMM_LOST", "INTERNAL"]
STREAM_OPERATIONS = [
    ("start_stream", "D1", START_ERRORS),
    ("stop_stream", "D1", READ_ERRORS),
    ("stream_status", "R0", READ_ERRORS),
]
STREAM_DEFS = {
    "SessionId": string(36),
    "StopReason": enum(["requested", "device_error", "limit", "safety_stop", "end_of_data"]),
    "StreamEvent": closed(
        {"kind": enum(["started", "stopped", "gap"]), "reason": ref("StopReason"), "from": string(20), "to": string(20)},
        ["kind"],
    ),
    "StreamState": enum(["idle", "streaming"]),
    "StreamStatus": closed({"state": ref("StreamState"), "next_seq": string(20)}, ["next_seq", "state"]),
}
STREAM_COMMON = [
    ("SC-01", "Batch size within the recommendation", [
        "over a test stream of at least 60 s, at least 95 % of batches hold 1 MB to 8 MB or span at "
        "most 50 ms of device time",
    ]),
    ("SC-02", "Start and stop idempotence", [
        "start_stream on a running stream changes nothing and emits no second started event",
        "stop_stream on a stopped stream changes nothing and emits no second stopped event",
    ]),
    ("SC-03", "Sequence contiguity", [
        "each batch starts one after the last sequence number of the batch before it",
        "no batch range is reversed, overlapping or missing",
    ]),
    ("SC-04", "Extension declaration and digesting", [
        "every vendor-extension setting is named under a vendor namespace listed in describe",
        "every extension value is in the configuration digest the device reports",
    ]),
]

QUANTITY_DEFS = {
    "Quantity": closed({"unit": ref("SiUnit"), "value": string(18)}, ["unit", "value"]),
    "SiUnit": enum(UNITS),
}

# REQ-HAL.IDL.SENS.GAUENV-005: the Gauge-core@1 items.
GAUGE_CORE = [
    ("GA-01", "Quantisation", [
        "every reading has a positive resolution",
        "every value is an integer multiple of its resolution within 1e-9 relative",
    ]),
    ("GA-02", "Unit consistency", [
        "value and resolution share one unit, m or rad",
        "every reading of a stream has the unit and resolution of the first and of the gauge info in describe",
        "every value lies within the range the gauge info declares",
    ]),
    ("GA-03", "Zero behaviour", [
        "after a successful zero with the gauge held still the next reading is zero",
        "zero changes neither the gauge identity, unit and resolution nor the calibration reference",
    ]),
    ("GA-04", "Timestamp monotonicity", [
        "t_host never decreases from one reading to the next",
        "t_dev is present in every reading or in none and increases strictly",
        "no reading repeats an earlier one and the gauge identity and calibration reference do not change "
        "within a stream",
    ]),
]

# REQ-HAL.IDL.SENS.GAUENV-005: the Environment-core@1 items.
ENVIRONMENT_CORE = [
    ("EN-01", "Units", [
        "temperature is in K and positive, humidity a fraction in [0, 1] and pressure in Pa and positive",
        "every reading of a stream carries the same optional quantities",
    ]),
    ("EN-02", "Uncertainty presence", [
        "every reading states a positive standard uncertainty of its temperature in K",
    ]),
    ("EN-03", "Sampling-time consistency", [
        "every reading states a positive sampling time in s",
        "the sampling time is the same in every reading of a stream and equals the configured sampling_time "
        "when the device has one",
    ]),
    ("EN-04", "Timestamp monotonicity", [
        "t_host never decreases from one reading to the next",
        "t_dev is present in every reading or in none and increases strictly",
        "no reading repeats an earlier one and the sensor identity does not change within a stream",
    ]),
]

GAUGE_KINDS = ["caliper", "micrometer", "indicator", "height_gauge", "bore_gauge", "depth_gauge", "protractor",
               "other"]

GAUGE_SEMANTICS = (
    "# Gauge@1 semantics\n"
    "\n"
    "The capability package for digital hand gauges and indicators: calipers, micrometers, dial "
    "and digital indicators, height, bore and depth gauges, and protractors.\n"
    "\n"
    "- Describe and configuration come from Device@1; the driver reports the gauge kind, range and "
    "resolution (GaugeInfo) with describe. The package has no core settings: any setting is a vendor "
    "extension under a namespace listed in describe.\n"
    "- read returns one reading now. zero makes the present position read zero. Both are D1. No "
    "operation writes calibration, and zero leaves the calibration reference unchanged.\n"
    "- start_stream, stop_stream and stream_status come from the shared streaming template. A stream "
    "delivers gauge-batch/1 batches with one sequence number per reading.\n"
    "- A reading carries the value in m or rad, the resolution in the same unit, the gauge identity, "
    "an optional calibration reference (a digest of the calibration record), optional t_dev and "
    "required t_host in ns. The value is an integer multiple of the resolution within 1e-9 relative.\n"
    "- A reading does not say where it came from; provenance is the acquisition envelope's.\n"
    "- Device-supplied text is data and is never interpreted as instructions.\n"
)

ENV_SEMANTICS = (
    "# Environment@1 semantics\n"
    "\n"
    "The capability package for temperature, humidity and pressure sensors in the measuring room "
    "or on the part.\n"
    "\n"
    "- Describe and configuration come from Device@1. The one core setting is sampling_time (s), "
    "for sensors whose sampling time is configurable. Any other setting is a vendor extension "
    "under a namespace listed in describe.\n"
    "- Subscribing is start_stream; start_stream, stop_stream and stream_status come from the "
    "shared streaming template. A stream delivers env-batch/1 batches with one sequence number per "
    "reading. The package has no operations of its own.\n"
    "- A reading carries the temperature in K with the sensor's stated standard uncertainty in K, "
    "the sampling time in s (the time over which the reading was sampled), optional humidity as a "
    "fraction in [0, 1] and optional pressure in Pa, the sensor identity, optional t_dev and "
    "required t_host in ns.\n"
    "- Thermal compensation is not this package's job; it only carries the readings.\n"
    "- A reading does not say where it came from; provenance is the acquisition envelope's.\n"
    "- Device-supplied text is data and is never interpreted as instructions.\n"
)


def summary(title, checks):
    return f"{title}: {'; '.join(checks)}."


def column(name, numeric, scale, unit):
    enc = {"numeric": numeric, "scale": f64_hex(scale), "offset": f64_hex(0.0), "unit": unit}
    return {"name": name, "unit": unit, "encoding": enc}


def stream_op(name, cls, errors):
    return {
        "name": name,
        "class": cls,
        "request": closed({"session_id": ref("SessionId")}, ["session_id"]),
        "response": closed({"status": ref("StreamStatus")}, ["status"]),
        "errors": errors,
        "live_settings": [],
    }


def package(name, defs, own_ops, batch_schema, semantics, profile_id, items):
    pkg = {
        "schema": "capability-package/v1",
        "manifest": {"name": name, "version": "1.0.0", "deprecated": False, "tier3_usable": True},
        "$defs": {**copy.deepcopy(STREAM_DEFS), **copy.deepcopy(QUANTITY_DEFS), **defs},
        "operations": own_ops + [stream_op(*op) for op in STREAM_OPERATIONS],
        "batch_schemas": [batch_schema],
        "semantics": {"text": semantics, "digest": sha256(semantics.encode("utf-8"))},
        "profile": {"id": profile_id, "items": [{"id": i, "summary": summary(t, c)} for i, t, c in items + STREAM_COMMON]},
    }
    return pkg


def gauge_package():
    defs = {
        "GaugeKind": enum(GAUGE_KINDS),
        "GaugeInfo": closed({
            "kind": ref("GaugeKind"),
            "range": closed({"min": ref("Quantity"), "max": ref("Quantity")}, ["max", "min"]),
            "resolution": ref("Quantity"),
        }, ["kind", "range", "resolution"]),
        "GaugeReading": closed({
            "schema": enum([GAUGE_TAG]),
            "value": ref("Quantity"),
            "resolution": ref("Quantity"),
            "gauge_identity": string(MAX_IDENTITY),
            "calibration_ref": string(71),
            "t_dev": string(20),
            "t_host": string(20),
        }, ["gauge_identity", "resolution", "schema", "t_host", "value"]),
    }
    session = closed({"session_id": ref("SessionId")}, ["session_id"])
    ops = [
        {"name": "read", "class": "D1", "request": session,
         "response": closed({"reading": ref("GaugeReading")}, ["reading"]),
         "errors": ["TRANSIENT", "COMM_LOST", "NOT_QUALIFIED", "CALIBRATION_EXPIRED", "LIMIT", "UNSUPPORTED",
                    "INTERNAL"],
         "live_settings": []},
        {"name": "zero", "class": "D1", "request": copy.deepcopy(session), "response": closed({}, []),
         "errors": ["TRANSIENT", "COMM_LOST", "NOT_QUALIFIED", "LIMIT", "UNSUPPORTED", "INTERNAL"],
         "live_settings": []},
    ]
    batch = {
        "id": "gauge.readings",
        # Reference encodings: value and resolution f64 in m, times i64 ns.
        "columns": [
            column("value", "f64", 1.0, "m"),
            column("resolution", "f64", 1.0, "m"),
            column("t_host", "i64", 1e-9, "s"),
            column("t_dev", "i64", 1e-9, "s"),
        ],
        "validity_mask": False,
        "seq_range": True,
    }
    return package("Gauge", defs, ops, batch, GAUGE_SEMANTICS, "Gauge-core@1", GAUGE_CORE)


def environment_package():
    defs = {
        "EnvReading": closed({
            "schema": enum([ENV_TAG]),
            "temperature": ref("Quantity"),
            "temperature_u": ref("Quantity"),
            "humidity": ref("Quantity"),
            "pressure": ref("Quantity"),
            "sampling_time": ref("Quantity"),
            "sensor_identity": string(MAX_IDENTITY),
            "t_dev": string(20),
            "t_host": string(20),
        }, ["sampling_time", "schema", "sensor_identity", "t_host", "temperature", "temperature_u"]),
    }
    batch = {
        "id": "environment.readings",
        "columns": [
            column("temperature", "f64", 1.0, "K"),
            column("temperature_u", "f64", 1.0, "K"),
            column("humidity", "f64", 1.0, "1"),
            column("pressure", "f64", 1.0, "Pa"),
            column("sampling_time", "f64", 1.0, "s"),
            column("t_host", "i64", 1e-9, "s"),
            column("t_dev", "i64", 1e-9, "s"),
        ],
        "validity_mask": False,
        "seq_range": True,
    }
    return package("Environment", defs, [], batch, ENV_SEMANTICS, "Environment-core@1", ENVIRONMENT_CORE)


def canonical_digest(pkg) -> str:
    out = copy.deepcopy(pkg)
    out["operations"] = sorted(out["operations"], key=lambda o: o["name"])
    for op in out["operations"]:
        op["errors"] = sorted(op["errors"])
        op["live_settings"] = sorted(op["live_settings"])
    out["batch_schemas"] = sorted(out["batch_schemas"], key=lambda b: b["id"])
    out["profile"]["items"] = sorted(out["profile"]["items"], key=lambda i: i["id"])
    return sha256(jcs(out))


# ------------------------------------------------------------------ the independent validator

class Fail(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def bad():
    return Fail("reading_invalid")


def no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise bad()
        out[k] = v
    return out


def closed_keys(obj, allowed):
    if not isinstance(obj, dict) or any(k not in allowed for k in obj):
        raise bad()
    return obj


def req_str(obj, key):
    s = obj.get(key)
    if not isinstance(s, str):
        raise bad()
    return s


def u64(obj, key):
    s = req_str(obj, key)
    if not s or len(s) > 20 or not s.isascii() or not s.isdigit() or (s != "0" and s.startswith("0")):
        raise bad()
    v = int(s)
    if v >= 1 << 64:
        raise bad()
    return v


def i64(obj, key):
    s = req_str(obj, key)
    digits = s[1:] if s.startswith("-") else s
    if not digits or not digits.isascii() or not digits.isdigit() or (digits != "0" and digits.startswith("0")) \
            or s == "-0":
        raise bad()
    v = int(s)
    if not I64_MIN <= v <= I64_MAX:
        raise bad()
    return v


def quantity(obj, key):
    """A Quantity as HAL.IDL.UNITS reads it: closed keys, the unit before the value."""
    if key not in obj:
        raise bad()
    qv = obj[key]
    if not isinstance(qv, dict) or any(k not in ("unit", "value") for k in qv) or "unit" not in qv:
        raise bad()
    unit = qv["unit"]
    if not isinstance(unit, str):
        raise bad()
    if unit not in UNITS:
        raise Fail("unit_unsupported")
    s = qv.get("value")
    if not isinstance(s, str) or len(s) != 18 or not s.startswith("0x") or any(c not in "0123456789abcdef" for c in s[2:]):
        raise bad()
    v = hex_f64(s)
    if not math.isfinite(v):
        raise Fail("value_out_of_range")
    return (v, unit)


def identity(obj, key):
    s = req_str(obj, key)
    if not 1 <= len(s.encode("utf-8")) <= MAX_IDENTITY:
        raise bad()
    return s


def digest(obj, key):
    if key not in obj:
        return None
    s = req_str(obj, key)
    if len(s) != 71 or not s.startswith("sha256:") or any(c not in "0123456789abcdef" for c in s[7:]):
        raise bad()
    return s


def opt(obj, key, fn):
    return fn(obj, key) if key in obj else None


def read_gauge(o):
    closed_keys(o, {"calibration_ref", "gauge_identity", "resolution", "schema", "t_dev", "t_host", "value"})
    if req_str(o, "schema") != GAUGE_TAG:
        raise bad()
    return {
        "value": quantity(o, "value"),
        "resolution": quantity(o, "resolution"),
        "identity": identity(o, "gauge_identity"),
        "calibration_ref": digest(o, "calibration_ref"),
        "t_dev": opt(o, "t_dev", i64),
        "t_host": i64(o, "t_host"),
    }


def nearest_distance(r):
    """|r - n| for the integer n nearest r; exact for |r| <= 2^53."""
    f = math.floor(r)
    d = r - f
    return d if d < 0.5 else 1.0 - d


def check_gauge(g):
    """REQ-HAL.IDL.SENS.GAUENV-001, -002 in the leaf's order."""
    (v, unit), (res, res_unit) = g["value"], g["resolution"]
    if unit not in ("m", "rad"):
        raise Fail("unit_unsupported")
    if res_unit != unit:
        raise Fail("unit_unsupported")
    if not res > 0.0:
        raise Fail("value_out_of_range")
    r = v / res
    if not math.isfinite(r) or abs(r) > MAX_STEPS:
        raise Fail("value_out_of_range")
    scale = abs(r) if abs(r) > 1.0 else 1.0
    if not nearest_distance(r) <= TOLERANCE * scale:
        raise Fail("reading_not_quantised")


def same_gauge_stream(first, g):
    return (g["identity"] == first["identity"] and g["value"][1] == first["value"][1]
            and g["resolution"] == first["resolution"] and g["calibration_ref"] == first["calibration_ref"])


def read_env(o):
    closed_keys(o, {"humidity", "pressure", "sampling_time", "schema", "sensor_identity", "t_dev", "t_host",
                    "temperature", "temperature_u"})
    if req_str(o, "schema") != ENV_TAG:
        raise bad()
    return {
        "temperature": quantity(o, "temperature"),
        "temperature_u": quantity(o, "temperature_u"),
        "humidity": opt(o, "humidity", quantity),
        "pressure": opt(o, "pressure", quantity),
        "sampling_time": quantity(o, "sampling_time"),
        "identity": identity(o, "sensor_identity"),
        "t_dev": opt(o, "t_dev", i64),
        "t_host": i64(o, "t_host"),
    }


def check_env(e):
    """REQ-HAL.IDL.SENS.GAUENV-003: units first, then the domains."""
    want = [("temperature", "K"), ("temperature_u", "K"), ("humidity", "1"), ("pressure", "Pa"),
            ("sampling_time", "s")]
    for k, unit in want:
        if e[k] is not None and e[k][1] != unit:
            raise Fail("unit_unsupported")
    for k in ("temperature", "temperature_u"):
        if not e[k][0] > 0.0:
            raise Fail("value_out_of_range")
    if e["humidity"] is not None and not 0.0 <= e["humidity"][0] <= 1.0:
        raise Fail("value_out_of_range")
    if e["pressure"] is not None and not e["pressure"][0] > 0.0:
        raise Fail("value_out_of_range")
    if not e["sampling_time"][0] > 0.0:
        raise Fail("value_out_of_range")


def same_env_stream(first, e):
    return (e["identity"] == first["identity"] and (e["humidity"] is None) == (first["humidity"] is None)
            and (e["pressure"] is None) == (first["pressure"] is None)
            and e["sampling_time"] == first["sampling_time"])


KINDS = {
    "gauge": (GAUGE_TAG, GAUGE_BATCH, read_gauge, check_gauge, same_gauge_stream),
    "env": (ENV_TAG, ENV_BATCH, read_env, check_env, same_env_stream),
}


def series(kind, readings, raws):
    """The series rules: t_dev presence, t_host, t_dev, the same stream, no exact repeat."""
    same = KINDS[kind][4]
    first = readings[0]
    run = {jcs(raws[0])}
    for i in range(1, len(readings)):
        prev, r = readings[i - 1], readings[i]
        if (r["t_dev"] is None) != (first["t_dev"] is None):
            raise Fail("reading_inconsistent")
        if r["t_host"] < prev["t_host"]:
            raise Fail("timestamps_not_monotonic")
        if r["t_dev"] is not None and r["t_dev"] <= prev["t_dev"]:
            raise Fail("timestamps_not_monotonic")
        if not same(first, r):
            raise Fail("reading_inconsistent")
        if r["t_host"] != prev["t_host"]:
            run = set()
        b = jcs(raws[i])
        if b in run:
            raise Fail("reading_duplicate")
        run.add(b)


def validate_reading(kind, data: bytes):
    if len(data) > MAX_READING_BYTES:
        raise bad()
    try:
        o = json.loads(data.decode("utf-8"), object_pairs_hook=no_dupes)
    except (ValueError, UnicodeDecodeError):
        raise bad()
    _, _, read, check, _ = KINDS[kind]
    r = read(o)
    check(r)
    if jcs(o) != data:
        raise bad()
    return None


def validate_batch(kind, data: bytes):
    if len(data) > MAX_BATCH_BYTES:
        raise bad()
    try:
        o = json.loads(data.decode("utf-8"), object_pairs_hook=no_dupes)
    except (ValueError, UnicodeDecodeError):
        raise bad()
    _, batch_tag, read, check, _ = KINDS[kind]
    closed_keys(o, {"first_seq", "last_seq", "readings", "schema"})
    if req_str(o, "schema") != batch_tag:
        raise bad()
    first, last = u64(o, "first_seq"), u64(o, "last_seq")
    items = o.get("readings")
    if not isinstance(items, list) or not 1 <= len(items) <= MAX_BATCH_READINGS:
        raise bad()
    if last < first or last - first + 1 != len(items):
        raise bad()
    readings = [read(item) for item in items]
    for r in readings:
        check(r)
    series(kind, readings, items)
    if jcs(o) != data:
        raise bad()
    t0, t1 = readings[0]["t_dev"], readings[-1]["t_dev"]
    return {
        "first_seq": str(first),
        "last_seq": str(last),
        "bytes": str(len(data)),
        "span_ns": None if t0 is None else str(t1 - t0),
    }


def verdict(form, kind, data):
    try:
        out = validate_batch(kind, data) if form == "batch" else validate_reading(kind, data)
        return "ok", out
    except Fail as f:
        return f.code, None


# ------------------------------------------------------------------ invented device data (D-038)

T0 = 1_764_000_000_000_000_000  # an invented host time, in ns
CAL_REF = sha256(b"invented calibration record for caliper EX-0001 (D-038, not a real certificate)")
CALIPER_ID = "Example Gauges EC-150 caliper, serial EX-0001 (invented, D-038)"
SENSOR_ID = "Example Sensors RT-20 room sensor, serial EX-0002 (invented, D-038)"


def caliper(**kw):
    r = {
        "schema": GAUGE_TAG,
        "value": q(0.01234, "m"),
        "resolution": q(1e-5, "m"),
        "gauge_identity": CALIPER_ID,
        "calibration_ref": CAL_REF,
        "t_host": str(T0),
    }
    r.update(kw)
    return {k: v for k, v in r.items() if v is not None}


def room(**kw):
    r = {
        "schema": ENV_TAG,
        "temperature": q(293.15, "K"),
        "temperature_u": q(0.05, "K"),
        "humidity": q(0.45, "1"),
        "pressure": q(101325.0, "Pa"),
        "sampling_time": q(1.0, "s"),
        "sensor_identity": SENSOR_ID,
        "t_dev": "5000000000",
        "t_host": str(T0),
    }
    r.update(kw)
    return {k: v for k, v in r.items() if v is not None}


def gauge_series(n=3, t_dev=False, **kw):
    out = []
    for k in range(n):
        r = caliper(value=q((1234 + k) * 1e-5, "m"), t_host=str(T0 + k * 100_000_000), **kw)
        if t_dev:
            r["t_dev"] = str(2_000_000_000 + k * 100_000_000)
        out.append(r)
    return out


def env_series(n=3, t_dev=True):
    out = []
    for k in range(n):
        r = room(temperature=q(293.15 + k * 0.01, "K"), t_host=str(T0 + k * 1_000_000_000),
                 t_dev=str(5_000_000_000 + k * 1_000_000_000) if t_dev else None)
        out.append(r)
    return out


def batch(tag, readings, first_seq=100, last_seq=None):
    if last_seq is None:
        last_seq = first_seq + len(readings) - 1
    return {"schema": tag, "first_seq": str(first_seq), "last_seq": str(last_seq), "readings": readings}


def build_cases():
    cases = []

    def add(name, form, kind, expect, obj=None, raw=None):
        data = raw if raw is not None else jcs(obj)
        got, summ = verdict(form, kind, data)
        assert got == expect, f"{name}: oracle says {got}, label says {expect}"
        entry = {"name": name, "form": form, "kind": kind, "expect": expect, "json": data.decode("utf-8")}
        if summ is not None:
            entry["summary"] = summ
        cases.append(entry)

    g = lambda name, expect, obj=None, raw=None: add(name, "reading", "gauge", expect, obj, raw)  # noqa: E731
    e = lambda name, expect, obj=None, raw=None: add(name, "reading", "env", expect, obj, raw)  # noqa: E731

    # Gauge readings that pass.
    g("g_ok_caliper", "ok", caliper())
    g("g_ok_negative", "ok", caliper(value=q(-0.01234, "m")))
    g("g_ok_zero", "ok", caliper(value=q(0.0, "m")))
    g("g_ok_negative_zero", "ok", caliper(value=q(-0.0, "m")))
    g("g_ok_protractor_rad", "ok", caliper(value=q(0.5236, "rad"), resolution=q(1e-4, "rad")))
    g("g_ok_t_dev", "ok", caliper(t_dev="-42"))
    g("g_ok_no_calibration_ref", "ok", caliper(calibration_ref=None))
    g("g_ok_many_steps", "ok", caliper(value=q(1.23456789, "m"), resolution=q(1e-8, "m")))
    g("g_ok_identity_non_ascii", "ok", caliper(gauge_identity="Jauge exemple é n° EX-0003 (inventée)"))
    g("g_ok_identity_256_bytes", "ok", caliper(gauge_identity="x" * 256))
    # AC-2 and the quantisation rule.
    g("g_not_quantised", "reading_not_quantised", caliper(value=q(0.012345, "m")))
    g("g_not_quantised_by_1e-5_step", "reading_not_quantised", caliper(value=q(0.0123400001, "m")))
    g("g_resolution_zero", "value_out_of_range", caliper(resolution=q(0.0, "m")))
    g("g_resolution_negative", "value_out_of_range", caliper(resolution=q(-1e-5, "m")))
    g("g_too_many_steps", "value_out_of_range", caliper(value=q(1.0, "m"), resolution=q(1e-300, "m")))
    g("g_value_nan", "value_out_of_range", caliper(value={"unit": "m", "value": "0x7ff8000000000000"}))
    g("g_value_infinite", "value_out_of_range", caliper(value={"unit": "m", "value": "0x7ff0000000000000"}))
    # Units.
    g("g_value_unit_s", "unit_unsupported", caliper(value=q(0.01234, "s"), resolution=q(1e-5, "s")))
    g("g_value_unit_mm", "unit_unsupported", caliper(value={"unit": "mm", "value": f64_hex(12.34)}))
    g("g_resolution_unit_rad", "unit_unsupported", caliper(resolution=q(1e-5, "rad")))
    # Wire shape. D-045: a reading cannot say it came from a device.
    g("g_origin_device", "reading_invalid", caliper(origin="device"))
    g("g_origin_simulated", "reading_invalid", caliper(origin="simulated"))
    g("g_schema_unknown", "reading_invalid", caliper(schema="gauge-reading/2"))
    g("g_t_host_missing", "reading_invalid", caliper(t_host=None))
    g("g_t_host_leading_zero", "reading_invalid", caliper(t_host="0" + str(T0)))
    g("g_t_dev_minus_zero", "reading_invalid", caliper(t_dev="-0"))
    g("g_identity_empty", "reading_invalid", caliper(gauge_identity=""))
    g("g_identity_257_bytes", "reading_invalid", caliper(gauge_identity="x" * 257))
    g("g_calibration_ref_upper", "reading_invalid", caliper(calibration_ref=CAL_REF.upper().replace("SHA256", "sha256")))
    g("g_value_hex_upper", "reading_invalid", caliper(value={"unit": "m", "value": f64_hex(0.01234).upper().replace("0X", "0x")}))
    g("g_quantity_extra_key", "reading_invalid", caliper(value={**q(0.01234, "m"), "u": "0"}))
    g("g_resolution_missing", "reading_invalid", caliper(resolution=None))
    # Non-canonical spellings of a valid reading.
    ok = jcs(caliper())
    g("g_whitespace", "reading_invalid", raw=ok.replace(b'":"', b'": "', 1))
    g("g_key_order", "reading_invalid",
      raw=b'{"value":' + jcs(q(0.01234, "m")) + b"," + ok[1:].replace(b',"value":' + jcs(q(0.01234, "m")), b""))
    g("g_trailing_newline", "reading_invalid", raw=ok + b"\n")
    g("g_duplicate_key", "reading_invalid",
      raw=ok.replace(b'"schema":"gauge-reading/1"', b'"schema":"gauge-reading/1","schema":"gauge-reading/1"'))

    # Environment readings that pass.
    e("e_ok_room", "ok", room())
    e("e_ok_minimal", "ok", room(humidity=None, pressure=None, t_dev=None))
    e("e_ok_humidity_0", "ok", room(humidity=q(0.0, "1")))
    e("e_ok_humidity_1", "ok", room(humidity=q(1.0, "1")))
    # AC-3 and the domains.
    e("e_humidity_1_3", "value_out_of_range", room(humidity=q(1.3, "1")))
    e("e_humidity_negative", "value_out_of_range", room(humidity=q(-0.01, "1")))
    e("e_temperature_zero", "value_out_of_range", room(temperature=q(0.0, "K")))
    e("e_uncertainty_zero", "value_out_of_range", room(temperature_u=q(0.0, "K")))
    e("e_uncertainty_negative", "value_out_of_range", room(temperature_u=q(-0.05, "K")))
    e("e_pressure_negative", "value_out_of_range", room(pressure=q(-1.0, "Pa")))
    e("e_sampling_time_zero", "value_out_of_range", room(sampling_time=q(0.0, "s")))
    # Units.
    e("e_humidity_unit_pa", "unit_unsupported", room(humidity=q(0.45, "Pa")))
    e("e_temperature_unit_m", "unit_unsupported", room(temperature=q(293.15, "m")))
    e("e_temperature_degc", "unit_unsupported", room(temperature={"unit": "degC", "value": f64_hex(20.0)}))
    e("e_uncertainty_unit_1", "unit_unsupported", room(temperature_u=q(0.05, "1")))
    e("e_pressure_unit_k", "unit_unsupported", room(pressure=q(101325.0, "K")))
    e("e_sampling_time_unit_k", "unit_unsupported", room(sampling_time=q(1.0, "K")))
    # Uncertainty presence and the wire shape.
    e("e_uncertainty_missing", "reading_invalid", room(temperature_u=None))
    e("e_sampling_time_missing", "reading_invalid", room(sampling_time=None))
    e("e_origin_device", "reading_invalid", room(origin="device"))
    e("e_schema_gauge", "reading_invalid", room(schema=GAUGE_TAG))
    e("e_trailing_space", "reading_invalid", raw=jcs(room()) + b" ")

    gb = lambda name, expect, obj=None, raw=None: add(name, "batch", "gauge", expect, obj, raw)  # noqa: E731
    eb = lambda name, expect, obj=None, raw=None: add(name, "batch", "env", expect, obj, raw)  # noqa: E731

    # Gauge batches.
    gb("gb_ok", "ok", batch(GAUGE_BATCH, gauge_series()))
    gb("gb_ok_t_dev", "ok", batch(GAUGE_BATCH, gauge_series(t_dev=True)))
    s = gauge_series()
    s[1]["t_host"] = s[0]["t_host"]
    gb("gb_ok_shared_t_host", "ok", batch(GAUGE_BATCH, s))
    gb("gb_ok_one_reading", "ok", batch(GAUGE_BATCH, gauge_series(1), first_seq=0))
    s = gauge_series()
    s[2]["t_host"] = str(T0 + 50_000_000)
    gb("gb_t_host_decreasing", "timestamps_not_monotonic", batch(GAUGE_BATCH, s))
    s = gauge_series(t_dev=True)
    s[2]["t_dev"] = s[1]["t_dev"]
    gb("gb_t_dev_equal", "timestamps_not_monotonic", batch(GAUGE_BATCH, s))
    s = gauge_series(t_dev=True)
    del s[1]["t_dev"]
    gb("gb_t_dev_partial", "reading_inconsistent", batch(GAUGE_BATCH, s))
    s = gauge_series()
    s[1]["gauge_identity"] = "Example Gauges EC-150 caliper, serial EX-0009 (invented, D-038)"
    gb("gb_identity_changed", "reading_inconsistent", batch(GAUGE_BATCH, s))
    s = gauge_series()
    s[2]["resolution"] = q(1e-6, "m")
    gb("gb_resolution_changed", "reading_inconsistent", batch(GAUGE_BATCH, s))
    s = gauge_series()
    s[1]["value"], s[1]["resolution"] = q(0.0123, "rad"), q(1e-4, "rad")
    gb("gb_unit_changed", "reading_inconsistent", batch(GAUGE_BATCH, s))
    s = gauge_series()
    s[2]["calibration_ref"] = sha256(b"another invented calibration record (D-038)")
    gb("gb_calibration_changed", "reading_inconsistent", batch(GAUGE_BATCH, s))
    # A repeated reading: the third copies the second exactly (value and host time).
    s = gauge_series()
    s[2] = copy.deepcopy(s[1])
    gb("gb_repeat_no_t_dev", "reading_duplicate", batch(GAUGE_BATCH, s))
    s = gauge_series(t_dev=True)
    s[2] = copy.deepcopy(s[1])
    gb("gb_repeat_with_t_dev", "timestamps_not_monotonic", batch(GAUGE_BATCH, s))
    s = gauge_series()
    s[2]["value"] = q(0.012345, "m")
    gb("gb_not_quantised", "reading_not_quantised", batch(GAUGE_BATCH, s))
    gb("gb_seq_range_mismatch", "reading_invalid", batch(GAUGE_BATCH, gauge_series(), last_seq=103))
    gb("gb_range_reversed", "reading_invalid", batch(GAUGE_BATCH, gauge_series(), last_seq=99))
    gb("gb_empty", "reading_invalid", batch(GAUGE_BATCH, [], last_seq=100))
    gb("gb_first_seq_leading_zero", "reading_invalid", {**batch(GAUGE_BATCH, gauge_series()), "first_seq": "0100"})
    gb("gb_origin_device", "reading_invalid", {**batch(GAUGE_BATCH, gauge_series()), "origin": "device"})
    s = gauge_series()
    s[0]["origin"] = "device"
    gb("gb_reading_origin_device", "reading_invalid", batch(GAUGE_BATCH, s))
    gb("gb_env_tag", "reading_invalid", batch(ENV_BATCH, gauge_series()))
    ok = jcs(batch(GAUGE_BATCH, gauge_series()))
    gb("gb_whitespace", "reading_invalid", raw=ok.replace(b",", b", ", 1))
    gb("gb_trailing_newline", "reading_invalid", raw=ok + b"\n")

    # Environment batches.
    eb("eb_ok", "ok", batch(ENV_BATCH, env_series()))
    eb("eb_ok_no_t_dev", "ok", batch(ENV_BATCH, env_series(t_dev=False)))
    s = env_series()
    s[1]["sampling_time"] = q(2.0, "s")
    eb("eb_sampling_time_changed", "reading_inconsistent", batch(ENV_BATCH, s))
    s = env_series()
    del s[2]["humidity"]
    eb("eb_humidity_dropped", "reading_inconsistent", batch(ENV_BATCH, s))
    s = env_series()
    s[1]["sensor_identity"] = "Example Sensors RT-20 room sensor, serial EX-0008 (invented, D-038)"
    eb("eb_identity_changed", "reading_inconsistent", batch(ENV_BATCH, s))
    s = env_series()
    s[2]["t_dev"] = "5500000000"
    eb("eb_t_dev_decreasing", "timestamps_not_monotonic", batch(ENV_BATCH, s))
    s = env_series()
    s[1]["humidity"] = q(1.3, "1")
    eb("eb_humidity_out_of_range", "value_out_of_range", batch(ENV_BATCH, s))
    s = env_series(t_dev=False)
    s[2] = copy.deepcopy(s[1])
    eb("eb_repeat_no_t_dev", "reading_duplicate", batch(ENV_BATCH, s))
    eb("eb_gauge_tag", "reading_invalid", batch(GAUGE_BATCH, env_series()))
    return cases


# ------------------------------------------------------------------ files

def build_files():
    def fmt(obj) -> str:
        return json.dumps(obj, indent=2, ensure_ascii=False) + "\n"

    caliper_bytes, room_bytes = jcs(caliper()), jcs(room())
    assert verdict("reading", "gauge", caliper_bytes)[0] == "ok"
    assert verdict("reading", "env", room_bytes)[0] == "ok"
    r = 0.01234 / 1e-5
    expected = {
        "caliper_read": {
            "bytes": len(caliper_bytes),
            "sha256": sha256(caliper_bytes),
            "value": f64_hex(0.01234),
            "resolution": f64_hex(1e-5),
            "steps": round(r),
            "ratio": f64_hex(r),
            "calibration_ref": CAL_REF,
        },
        "room_env": {
            "bytes": len(room_bytes),
            "sha256": sha256(room_bytes),
            "temperature": f64_hex(293.15),
            "temperature_u": f64_hex(0.05),
        },
    }
    gauge, env = gauge_package(), environment_package()
    return {
        "caliper_read.json": caliper_bytes,
        "room_env.json": room_bytes,
        "expected.json": fmt(expected).encode("utf-8"),
        "cases.json": fmt(build_cases()).encode("utf-8"),
        "gauge_v1.json": fmt(gauge).encode("utf-8"),
        "gauge_v1.sha256": (canonical_digest(gauge) + "\n").encode("ascii"),
        "environment_v1.json": fmt(env).encode("utf-8"),
        "environment_v1.sha256": (canonical_digest(env) + "\n").encode("ascii"),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_files()
    digests = " ".join(files[n].decode("ascii").strip() for n in ("gauge_v1.sha256", "environment_v1.sha256"))
    if check:
        for name, want in sorted(files.items()):
            path = os.path.join(HERE, name)
            if not os.path.isfile(path):
                print(f"missing fixture: {path}", file=sys.stderr)
                return 1
            with open(path, "rb") as f:
                if f.read() != want:
                    print(f"fixture mismatch: {path}", file=sys.stderr)
                    return 1
        print(f"ok ({digests})")
        return 0
    for name, content in sorted(files.items()):
        with open(os.path.join(HERE, name), "wb") as f:
            f.write(content)
    print(f"wrote {len(files)} fixtures under {HERE} ({digests})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
