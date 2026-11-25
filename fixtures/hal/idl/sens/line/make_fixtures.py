#!/usr/bin/env python3
"""Independent fixture generator for HAL.IDL.SENS.LINE (LineScan@1 package, columnar profile
batches and the LineScan-core@1 profile).

Writes project-authored fixtures (CC-BY-4.0) under fixtures/hal/idl/sens/line/:
  - profiles_100x2000.header.json  (AC-1: RFC 8785 header of a seeded 100 x 2 000 batch, i32 x
                                    and z, device time, no intensity, no pose)
  - profiles_100x2000.bin          (AC-1: its packed little-endian payload)
  - expected.json                  (AC-1: sizes, digests, valid-sample count, summary, probes of
                                    decoded values and the device-time interval range, all
                                    computed by this script)
  - cases.json                     (AC-2 .. AC-4 and the other rules: small single-fault
                                    batches, each hand-labelled with the expected outcome; this
                                    script's own validator must agree with every label)
  - linescan_v1.json               (AC-5: the LineScan@1 package after the streaming template,
                                    written from the leaf's rules, not from the Rust code)
  - linescan_v1.sha256             (AC-5: SHA-256 over this script's RFC 8785 canonical form
                                    with lists sorted as REQ-HAL.IDL.GEN.FMT-003 says)

All device data is invented (D-038). Stdlib only; shares no code with crates/fid-hal-idl.
Run:  python fixtures/hal/idl/sens/line/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import math
import os
import random
import struct
import sys
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))

SEED = 20261125
HEADER_TAG = "linescan-batch/1"
BUDGET_BYTES = 1_700_000  # REQ-005, MB read as 10^6 bytes
MAX_PROFILES = 100_000
MAX_POINTS = 65_536
MAX_SAMPLES = 1 << 24
TOLERANCE_PERCENT = 5  # LS-05 proposal [unverified]

# An invented session namespace; frame IDs are UUIDv5 under it (HAL.IDL.UNITS).
SESSION_NS = uuid.UUID("3d0b8f5c-9a51-4c2e-8f0e-6b7d2a1c4e90")
SENSOR_FRAME = str(uuid.uuid5(SESSION_NS, "demo-line-sensor"))


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


# ------------------------------------------------------------------ the package, from the spec

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

# REQ-HAL.IDL.SENS.LINE-004: the LineScan-core@1 items.
LINESCAN_CORE = [
    ("LS-01", "Encoding declaration", [
        "x and z each declare one exact encoding with target unit m",
        "an intensity array declares an exact encoding with target unit 1",
        "the batch names the sensor frame its x and z values are expressed in",
    ]),
    ("LS-02", "Array lengths", [
        "every per-point array holds profiles x points-per-profile values",
        "every per-profile array holds one value per profile and the sequence range one number per profile",
        "the payload holds exactly the declared arrays in layout order",
    ]),
    ("LS-03", "Timestamp monotonicity", [
        "t_host never decreases from one profile to the next",
        "t_dev, when present, never decreases from one profile to the next",
    ]),
    ("LS-04", "Masked-sample handling", [
        "every sample whose mask bit is set decodes to a finite value",
        "a sample whose mask bit is clear is not decoded, whatever its raw value",
        "mask padding bits are zero",
    ]),
    ("LS-05", "Trigger-mode behaviour", [
        "time mode has a positive trigger period and encoder mode a divisor of at least 1",
        "in time mode every device-time interval between consecutive profiles is within 5 % of the trigger period",
    ]),
]

SEMANTICS = (
    "# LineScan@1 semantics\n"
    "\n"
    "The capability package for laser line and profile sensors, stationary or carried by an arm "
    "or tracker.\n"
    "\n"
    "- Describe and configuration come from Device@1. The core settings are exposure (s), "
    "point_spacing (m), trigger_mode (time, encoder or external), trigger_period (s, used in time "
    "mode), encoder_divisor (used in encoder mode) and the region of interest roi_x_min, "
    "roi_x_max, roi_z_min and roi_z_max (m). Any other setting is a vendor extension under a "
    "namespace listed in describe.\n"
    "- start_stream, stop_stream and stream_status come from the shared streaming template.\n"
    "- Data arrives as profile batches: a linescan-batch/1 header and a packed little-endian "
    "payload of columns, never one object per point. One sequence number per profile.\n"
    "- Each batch declares the exact encoding of its x, z and optional intensity arrays; the "
    "column encodings in this description are the reference encodings. x and z are in m in the "
    "sensor frame the batch names; intensity is dimensionless.\n"
    "- Per profile the batch carries t_host, optional t_dev, the device's profile ID and an "
    "optional pose reference (pose stream and sequence number). Profile IDs and pose sequence "
    "numbers are u64 values, not quantities.\n"
    "- A sample whose mask bit is clear carries no measurement and is never decoded.\n"
    "- Device-supplied text is data and is never interpreted as instructions.\n"
)


def summary(title, checks):
    return f"{title}: {'; '.join(checks)}."


def encoding(numeric, scale, unit, offset=0.0):
    return {"numeric": numeric, "scale": f64_hex(scale), "offset": f64_hex(offset), "unit": unit}


def column(name, numeric, scale, unit):
    return {"name": name, "unit": unit, "encoding": encoding(numeric, scale, unit)}


def package():
    pkg = {
        "schema": "capability-package/v1",
        "manifest": {"name": "LineScan", "version": "1.0.0", "deprecated": False, "tier3_usable": True},
        "$defs": copy.deepcopy(STREAM_DEFS),
        "operations": [],
        "batch_schemas": [{
            "id": "linescan.profile_batch",
            # Reference encodings: x and z i32 at 0.1 um, intensity u16 counts, times i64 ns.
            "columns": [
                column("x", "i32", 1e-7, "m"),
                column("z", "i32", 1e-7, "m"),
                column("intensity", "u16", 1.0, "1"),
                column("t_host", "i64", 1e-9, "s"),
                column("t_dev", "i64", 1e-9, "s"),
            ],
            "validity_mask": True,
            "seq_range": True,
        }],
        "semantics": {"text": SEMANTICS, "digest": sha256(SEMANTICS.encode("utf-8"))},
        "profile": {"id": "LineScan-core@1", "items": []},
    }
    for name, cls, errors in STREAM_OPERATIONS:
        pkg["operations"].append({
            "name": name,
            "class": cls,
            "request": closed({"session_id": ref("SessionId")}, ["session_id"]),
            "response": closed({"status": ref("StreamStatus")}, ["status"]),
            "errors": errors,
            "live_settings": [],
        })
    for item_id, title, checks in LINESCAN_CORE + STREAM_COMMON:
        pkg["profile"]["items"].append({"id": item_id, "summary": summary(title, checks)})
    return pkg


def canonical_digest(pkg) -> str:
    out = copy.deepcopy(pkg)
    out["operations"] = sorted(out["operations"], key=lambda o: o["name"])
    for op in out["operations"]:
        op["errors"] = sorted(op["errors"])
        op["live_settings"] = sorted(op["live_settings"])
    out["batch_schemas"] = sorted(out["batch_schemas"], key=lambda b: b["id"])
    out["profile"]["items"] = sorted(out["profile"]["items"], key=lambda i: i["id"])
    return sha256(jcs(out))


# ------------------------------------------------------------------ batches, from the spec

WIDTH = {"i16": 2, "u16": 2, "i32": 4, "u32": 4, "f32": 4, "i64": 8, "f64": 8}
FMT = {"i16": "h", "u16": "H", "i32": "i", "u32": "I", "f32": "f", "i64": "q", "f64": "d"}
UNITS = {"m", "rad", "s", "K", "Pa", "1"}


def pack(numeric, values) -> bytes:
    return struct.pack("<" + FMT[numeric] * len(values), *values)


def mask_bytes(valid) -> bytes:
    out = bytearray((len(valid) + 7) // 8)
    for i, v in enumerate(valid):
        if v:
            out[i // 8] |= 1 << (i % 8)
    return bytes(out)


def make_batch(b):
    """Header object and payload of a batch description `b` (plain Python lists)."""
    p, n = len(b["t_host"]), b["ppp"]
    arrays = {
        "t_host": {"len": str(p)},
        "profile_id": {"len": str(p)},
        "mask": {"len": str(len(b["valid"]))},
        "x": {"encoding": b["x_enc"], "len": str(len(b["x"]))},
        "z": {"encoding": b["z_enc"], "len": str(len(b["z"]))},
    }
    payload = pack("i64", b["t_host"])
    if b.get("t_dev") is not None:
        arrays["t_dev"] = {"len": str(p)}
        payload += pack("i64", b["t_dev"])
    payload += struct.pack("<" + "Q" * len(b["profile_id"]), *b["profile_id"])
    header = {
        "schema": HEADER_TAG,
        "first_seq": str(b["first_seq"]),
        "last_seq": str(b["first_seq"] + p - 1),
        "profiles": str(p),
        "points_per_profile": str(n),
        "frame": {"id": SENSOR_FRAME, "kind": "sensor"},
        "arrays": arrays,
    }
    if b.get("pose") is not None:
        stream_id, seqs = b["pose"]
        header["pose_stream"] = stream_id
        arrays["pose_seq"] = {"len": str(len(seqs))}
        payload += struct.pack("<" + "Q" * len(seqs), *seqs)
    payload += mask_bytes(b["valid"])
    payload += pack(b["x_enc"]["numeric"], b["x"]) + pack(b["z_enc"]["numeric"], b["z"])
    if b.get("intensity") is not None:
        enc, values = b["intensity"]
        arrays["intensity"] = {"encoding": enc, "len": str(len(values))}
        payload += pack(enc["numeric"], values)
    return header, payload


# ------------------------------------------------------------------ the independent validator

class Fail(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise Fail("batch_header_invalid")
        out[k] = v
    return out


def closed_keys(obj, allowed):
    if not isinstance(obj, dict) or any(k not in allowed for k in obj):
        raise Fail("batch_header_invalid")
    return obj


def u64(obj, key):
    s = obj.get(key)
    if not isinstance(s, str) or not s or len(s) > 20 or not s.isascii() or not s.isdigit():
        raise Fail("batch_header_invalid")
    if s != "0" and s.startswith("0"):
        raise Fail("batch_header_invalid")
    v = int(s)
    if v >= 1 << 64:
        raise Fail("batch_header_invalid")
    return v


def frame_ok(f):
    if not isinstance(f, dict) or set(f) != {"id", "kind"}:
        return False
    fid, kind = f["id"], f["kind"]
    if not isinstance(fid, str) or kind not in ("sensor", "mount", "device", "machine", "part"):
        return False
    try:
        u = uuid.UUID(fid)
    except ValueError:
        return False
    return str(u) == fid and u.version == 5 and u.variant == uuid.RFC_4122


def stream_id_ok(s):
    allowed = set("abcdefghijklmnopqrstuvwxyz0123456789._-")
    return isinstance(s, str) and 0 < len(s) <= 64 and s[0] in set("abcdefghijklmnopqrstuvwxyz0123456789") \
        and all(c in allowed for c in s)


def read_encoding(e):
    """An EncodingDescriptor as HAL.IDL.UNITS defines it; anything wrong is encoding_invalid."""
    if not isinstance(e, dict) or set(e) != {"numeric", "offset", "scale", "unit"}:
        raise Fail("encoding_invalid")
    numeric = e["numeric"]
    if numeric not in WIDTH:
        raise Fail("encoding_invalid")
    vals = []
    for k in ("scale", "offset"):
        s = e[k]
        if not isinstance(s, str) or len(s) != 18 or not s.startswith("0x") or \
                any(c not in "0123456789abcdef" for c in s[2:]):
            raise Fail("encoding_invalid")
        vals.append(hex_f64(s))
    scale, offset = vals
    if not math.isfinite(scale) or scale == 0.0 or not math.isfinite(offset):
        raise Fail("encoding_invalid")
    if e["unit"] not in UNITS:
        raise Fail("encoding_invalid")
    return {"numeric": numeric, "scale": scale, "offset": offset, "unit": e["unit"]}


def validate(header_bytes: bytes, payload: bytes):
    """The leaf's rules in order. Returns the batch summary or raises Fail(code)."""
    try:
        h = json.loads(header_bytes.decode("utf-8"), object_pairs_hook=no_dupes)
    except (ValueError, UnicodeDecodeError):
        raise Fail("batch_header_invalid")
    closed_keys(h, {"arrays", "first_seq", "frame", "last_seq", "points_per_profile", "pose_stream",
                    "profiles", "schema"})
    if h.get("schema") != HEADER_TAG:
        raise Fail("batch_header_invalid")
    first, last, p, n = u64(h, "first_seq"), u64(h, "last_seq"), u64(h, "profiles"), u64(h, "points_per_profile")
    if not frame_ok(h.get("frame")) or h["frame"]["kind"] != "sensor":
        raise Fail("batch_header_invalid")
    pose_stream = h.get("pose_stream")
    if "pose_stream" in h and not stream_id_ok(pose_stream):
        raise Fail("batch_header_invalid")
    a = closed_keys(h.get("arrays"), {"intensity", "mask", "pose_seq", "profile_id", "t_dev", "t_host", "x", "z"})
    lens, encs = {}, {}
    for k in ("t_host", "t_dev", "profile_id", "pose_seq", "mask", "x", "z", "intensity"):
        if k not in a:
            continue
        if k in ("x", "z", "intensity"):
            entry = closed_keys(a[k], {"encoding", "len"})
            lens[k] = u64(entry, "len")
            if "encoding" not in entry:
                raise Fail("encoding_invalid")
            encs[k] = read_encoding(entry["encoding"])
        else:
            lens[k] = u64(closed_keys(a[k], {"len"}), "len")
    for k in ("t_host", "profile_id", "mask", "x", "z"):
        if k not in lens:
            raise Fail("batch_header_invalid")
    if ("pose_seq" in lens) != (pose_stream is not None):
        raise Fail("batch_header_invalid")

    if last < first:
        raise Fail("batch_header_invalid")
    if last - first + 1 != p:
        raise Fail("array_length_mismatch")
    if not (1 <= p <= MAX_PROFILES) or not (1 <= n <= MAX_POINTS) or p * n > MAX_SAMPLES:
        raise Fail("batch_header_invalid")
    samples = p * n
    for k, unit in (("x", "m"), ("z", "m"), ("intensity", "1")):
        if k in encs and encs[k]["unit"] != unit:
            raise Fail("encoding_invalid")
    for k in ("t_host", "t_dev", "profile_id", "pose_seq"):
        if k in lens and lens[k] != p:
            raise Fail("array_length_mismatch")
    for k in ("mask", "x", "z", "intensity"):
        if k in lens and lens[k] != samples:
            raise Fail("array_length_mismatch")
    per_profile = sum(1 for k in ("t_host", "t_dev", "profile_id", "pose_seq") if k in lens)
    want = 8 * p * per_profile + (samples + 7) // 8 + sum(WIDTH[encs[k]["numeric"]] * samples for k in encs)
    if want != len(payload):
        raise Fail("array_length_mismatch")

    pos = 0

    def take(count, fmt, width):
        nonlocal pos
        out = list(struct.unpack_from("<" + fmt * count, payload, pos))
        pos += count * width
        return out

    t_host = take(p, "q", 8)
    t_dev = take(p, "q", 8) if "t_dev" in lens else None
    take(p, "Q", 8)  # profile IDs: no rule beyond their count
    if "pose_seq" in lens:
        take(p, "Q", 8)
    mask = payload[pos:pos + (samples + 7) // 8]
    pos += len(mask)
    if samples % 8 and mask[-1] >> (samples % 8):
        raise Fail("mask_invalid")
    columns = {}
    for k in ("x", "z", "intensity"):
        if k in encs:
            columns[k] = take(samples, FMT[encs[k]["numeric"]], WIDTH[encs[k]["numeric"]])
    for series in (t_host, t_dev):
        if series is not None and any(b < a_ for a_, b in zip(series, series[1:])):
            raise Fail("timestamps_not_monotonic")
    for k in ("x", "z", "intensity"):
        if k not in columns:
            continue
        e = encs[k]
        for i, raw in enumerate(columns[k]):
            if mask[i // 8] >> (i % 8) & 1:
                value = e["offset"] + e["scale"] * float(raw)
                if not math.isfinite(value):
                    raise Fail("sample_not_finite")
    return {
        "first_seq": str(first),
        "last_seq": str(last),
        "bytes": str(len(header_bytes) + len(payload)),
        "span_ns": None if t_dev is None else str(t_dev[-1] - t_dev[0]),
    }


def verdict(header_bytes, payload):
    try:
        return "ok", validate(header_bytes, payload)
    except Fail as f:
        return f.code, None


# ------------------------------------------------------------------ AC-1: the seeded batch

def big_batch():
    rng = random.Random(SEED)
    p, n = 100, 2000
    t_dev = [1_000_000_000 + k * 100_000 + rng.randrange(-500, 501) for k in range(p)]
    t_host = [5_000_000_000 + (k // 10) * 1_000_000 for k in range(p)]  # received ten at a time
    valid, xs, zs = [], [], []
    for k in range(p):
        for j in range(n):
            ok = rng.randrange(64) != 0
            valid.append(ok)
            if ok:
                xs.append(-100_000 + j * 100 + rng.randrange(-3, 4))
                zs.append(1_500_000 + (j - 1000) ** 2 // 50 + k * 10 + rng.randrange(-200, 201))
            else:
                xs.append(-(1 << 31))
                zs.append(-(1 << 31))
    return {
        "first_seq": 4200,
        "ppp": n,
        "t_host": t_host,
        "t_dev": t_dev,
        "profile_id": [900_000 + k for k in range(p)],
        "valid": valid,
        "x_enc": encoding("i32", 1e-7, "m"),
        "z_enc": encoding("i32", 1e-7, "m"),
        "x": xs,
        "z": zs,
    }


def probes(b, indices):
    out = []
    xe, ze = b["x_enc"], b["z_enc"]
    for i in indices:
        entry = {"index": i, "valid": b["valid"][i]}
        if b["valid"][i]:
            entry["x"] = f64_hex(hex_f64(xe["offset"]) + hex_f64(xe["scale"]) * float(b["x"][i]))
            entry["z"] = f64_hex(hex_f64(ze["offset"]) + hex_f64(ze["scale"]) * float(b["z"][i]))
        out.append(entry)
    return out


# ------------------------------------------------------------------ small single-fault cases

def small():
    return {
        "first_seq": 10,
        "ppp": 4,
        "t_host": [100, 200, 300],
        "t_dev": [1000, 2000, 3000],
        "profile_id": [7, 8, 9],
        "valid": [True] * 12,
        "x_enc": encoding("i32", 1e-6, "m"),
        "z_enc": encoding("i32", 1e-6, "m"),
        "x": [-30, -10, 10, 30] * 3,
        "z": [500, 501, 502, 503, 510, 511, 512, 513, 520, 521, 522, 523],
    }


def case_batch(mutate=None):
    b = small()
    if mutate:
        mutate(b)
    return b


def set_(**kw):
    def f(b):
        b.update(kw)
    return f


def nan_at(i, valid):
    def f(b):
        b["x_enc"] = encoding("f32", 1.0, "m")
        b["x"] = [float(v) * 1e-6 for v in b["x"]]
        b["x"][i] = float("nan")
        b["valid"][i] = valid
    return f


def build_cases():
    cases = []

    def add(name, expect, batch=None, header_edit=None, payload_edit=None, raw_header=None):
        header, payload = make_batch(batch if batch is not None else small())
        if header_edit:
            header_edit(header)
        if payload_edit:
            payload = payload_edit(payload)
        text = raw_header(jcs(header)) if raw_header else jcs(header)
        got, summ = verdict(text, payload)
        assert got == expect, f"{name}: oracle says {got}, label says {expect}"
        entry = {"name": name, "expect": expect, "header": text.decode("utf-8"), "payload_hex": payload.hex()}
        if summ is not None:
            entry["summary"] = summ
        cases.append(entry)

    def arr(key, **kw):
        def f(h):
            h["arrays"][key].update(kw)
        return f

    # Passing batches.
    add("ok_base", "ok")
    add("ok_full", "ok", case_batch(set_(intensity=(encoding("u16", 1.0, "1"), [100 + i for i in range(12)]),
                                         pose=("tracker-1.pose", [50, 51, 52]))))
    add("ok_no_t_dev", "ok", case_batch(set_(t_dev=None)))
    add("ok_t_host_equal", "ok", case_batch(set_(t_host=[100, 100, 300])))
    add("ok_masked_nan", "ok", case_batch(nan_at(5, False)))
    # AC-2: the z array has one point fewer.
    add("z_one_point_short", "array_length_mismatch", case_batch(lambda b: b["z"].pop()))
    # AC-3: an x encoding with target unit rad.
    add("x_unit_rad", "encoding_invalid", case_batch(set_(x_enc=encoding("i32", 1e-6, "rad"))))
    # AC-4: t_host decreasing between profiles.
    add("t_host_decreasing", "timestamps_not_monotonic", case_batch(set_(t_host=[100, 300, 200])))
    add("t_dev_decreasing", "timestamps_not_monotonic", case_batch(set_(t_dev=[1000, 3000, 2999])))
    # Other length rules.
    add("payload_trailing_byte", "array_length_mismatch", payload_edit=lambda pl: pl + b"\x00")
    add("seq_range_mismatch", "array_length_mismatch", header_edit=lambda h: h.update(last_seq="13"))
    add("profile_id_short", "array_length_mismatch", case_batch(lambda b: b["profile_id"].pop()))
    add("mask_len_wrong", "array_length_mismatch", header_edit=arr("mask", len="11"))
    add("x_len_long", "array_length_mismatch", case_batch(lambda b: b["x"].append(0)))
    # Mask and sample rules.
    # The mask follows t_host, t_dev and profile_id (3 x 24 bytes); 12 samples leave 4 padding bits.
    add("mask_padding_set", "mask_invalid",
        payload_edit=lambda pl: pl[:72] + bytes([pl[72], pl[73] | 0x10]) + pl[74:])
    add("x_nan_valid", "sample_not_finite", case_batch(nan_at(5, True)))
    add("z_overflow_valid", "sample_not_finite",
        case_batch(set_(z_enc=encoding("f64", 1e308, "m"), z=[1.0] * 11 + [10.0])))
    add("z_overflow_masked", "ok",
        case_batch(set_(z_enc=encoding("f64", 1e308, "m"), z=[1.0] * 11 + [10.0],
                        valid=[True] * 11 + [False])))
    # Encoding declarations.
    add("intensity_unit_m", "encoding_invalid",
        case_batch(set_(intensity=(encoding("u16", 1.0, "m"), [0] * 12))))
    add("x_numeric_u64", "encoding_invalid", header_edit=lambda h: h["arrays"]["x"]["encoding"].update(numeric="u64"))
    add("z_scale_zero", "encoding_invalid", header_edit=lambda h: h["arrays"]["z"]["encoding"].update(scale=f64_hex(0.0)))
    add("x_unit_mm", "encoding_invalid", header_edit=lambda h: h["arrays"]["x"]["encoding"].update(unit="mm"))
    add("z_encoding_missing", "encoding_invalid", header_edit=lambda h: h["arrays"]["z"].pop("encoding"))
    # Header shape.
    add("frame_not_sensor", "batch_header_invalid", header_edit=lambda h: h["frame"].update(kind="machine"))
    add("frame_id_not_v5", "batch_header_invalid",
        header_edit=lambda h: h["frame"].update(id="3d0b8f5c-9a51-4c2e-8f0e-6b7d2a1c4e90"))
    add("origin_device", "batch_header_invalid", header_edit=lambda h: h.update(origin="device"))
    add("pose_seq_without_stream", "batch_header_invalid",
        case_batch(set_(pose=("tracker-1.pose", [50, 51, 52]))), header_edit=lambda h: h.pop("pose_stream"))
    add("pose_stream_invalid", "batch_header_invalid",
        case_batch(set_(pose=("tracker-1.pose", [50, 51, 52]))), header_edit=lambda h: h.update(pose_stream="Pose"))
    add("range_reversed", "batch_header_invalid", header_edit=lambda h: h.update(last_seq="9"))
    add("u64_leading_zero", "batch_header_invalid", header_edit=lambda h: h.update(first_seq="010"))
    add("schema_unknown", "batch_header_invalid", header_edit=lambda h: h.update(schema="linescan-batch/2"))
    add("array_unknown", "batch_header_invalid", header_edit=lambda h: h["arrays"].update(normals={"len": "12"}))
    add("t_host_missing", "batch_header_invalid", header_edit=lambda h: h["arrays"].pop("t_host"))
    add("duplicate_key", "batch_header_invalid",
        raw_header=lambda t: t.replace(b'"schema":"linescan-batch/1"',
                                       b'"schema":"linescan-batch/1","schema":"linescan-batch/1"'))
    return cases


# ------------------------------------------------------------------ files

def build_files():
    def fmt(obj) -> str:
        return json.dumps(obj, indent=2, ensure_ascii=False) + "\n"

    big = big_batch()
    header, payload = make_batch(big)
    header_bytes = jcs(header)
    code, summ = verdict(header_bytes, payload)
    assert code == "ok", code
    total = len(header_bytes) + len(payload)
    assert total <= BUDGET_BYTES, total
    steps = [b - a for a, b in zip(big["t_dev"], big["t_dev"][1:])]
    expected = {
        "header_bytes": len(header_bytes),
        "payload_bytes": len(payload),
        "total_bytes": total,
        "budget_bytes": BUDGET_BYTES,
        "within_budget": total <= BUDGET_BYTES,
        "header_sha256": sha256(header_bytes),
        "payload_sha256": sha256(payload),
        "valid_samples": sum(big["valid"]),
        "summary": summ,
        "t_dev_step_min_ns": min(steps),
        "t_dev_step_max_ns": max(steps),
        "probes": probes(big, [0, 1, 1999, 2000, 2001, 100_000, 199_998, 199_999]
                         + [i for i, v in enumerate(big["valid"]) if not v][:2]),
    }
    pkg = package()
    return {
        "profiles_100x2000.header.json": header_bytes,
        "profiles_100x2000.bin": payload,
        "expected.json": fmt(expected).encode("utf-8"),
        "cases.json": fmt(build_cases()).encode("utf-8"),
        "linescan_v1.json": fmt(pkg).encode("utf-8"),
        "linescan_v1.sha256": (canonical_digest(pkg) + "\n").encode("ascii"),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_files()
    digest = files["linescan_v1.sha256"].decode("ascii").strip()
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
        print(f"ok ({digest})")
        return 0
    for name, content in sorted(files.items()):
        with open(os.path.join(HERE, name), "wb") as f:
            f.write(content)
    print(f"wrote {len(files)} fixtures under {HERE} ({digest})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
