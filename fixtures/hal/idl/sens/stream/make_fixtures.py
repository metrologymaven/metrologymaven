#!/usr/bin/env python3
"""Independent fixture generator for HAL.IDL.SENS.STREAM (streaming template, vendor-extension
rule and the Stream-common@1 profile).

Writes project-authored fixtures (CC-BY-4.0) under fixtures/hal/idl/sens/stream/:
  - demo_stream_v1.json     (AC-1, AC-4: an invented DemoStream@1 package after the streaming
                             template, written from the leaf's rules, not from the Rust code)
  - demo_stream_v1.sha256   (AC-1: SHA-256 over this script's own RFC 8785 canonical form with
                             lists sorted as REQ-HAL.IDL.GEN.FMT-003 says)
  - sizes_ok.json           (AC-3: a synthetic 60 s batch record, 97 of 100 batches within the
                             1-8 MB or 50 ms recommendation)
  - sizes_bad.json          (AC-3: the same shape, 80 of 100 within)
  - expected.json           (AC-3: this script's own tally and verdict for both records, plus
                             whether their sequence ranges are contiguous)

All data is invented (D-038); the records say `"origin": "synthetic"` so a pass on them is
never read as a statement about a device. Stdlib only; shares no code with crates/fid-hal-idl.
Run:  python fixtures/hal/idl/sens/stream/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

# REQ-HAL.IDL.SENS.STREAM-003 (thresholds are proposals [unverified]; MB read as 10^6 bytes).
MIN_BYTES = 1_000_000
MAX_BYTES = 8_000_000
MAX_SPAN_NS = 50_000_000
PASS_PERCENT = 95
TEST_STREAM_NS = 60_000_000_000


def jcs(value) -> bytes:
    """RFC 8785 canonical JSON for the values used here (ASCII strings, booleans, small
    integers, arrays, objects with keys sorted by UTF-16 code units)."""
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


# ------------------------------------------------------------------ schema helpers

def ref(name):
    return {"$ref": "#/$defs/" + name}


def string(max_len):
    return {"type": "string", "maxLength": max_len}


def closed(props, required):
    return {"type": "object", "additionalProperties": False, "properties": props, "required": required}


def enum(values):
    return {"type": "string", "enum": values}


# ------------------------------------------------------------------ the template, from the spec

# REQ-001: the three operations and their classes. Error lists are this leaf's choice, from the
# Device@1 code set (HAL.IDL.BASE.ERR).
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

STOP_REASONS = ["requested", "device_error", "limit", "safety_stop", "end_of_data"]

# The standard stream events and the status every stream operation returns.
STREAM_DEFS = {
    "SessionId": string(36),
    "StopReason": enum(STOP_REASONS),
    "StreamEvent": closed(
        {
            "kind": enum(["started", "stopped", "gap"]),
            "reason": ref("StopReason"),
            "from": string(20),
            "to": string(20),
        },
        ["kind"],
    ),
    "StreamState": enum(["idle", "streaming"]),
    "StreamStatus": closed({"state": ref("StreamState"), "next_seq": string(20)}, ["next_seq", "state"]),
}

# REQ-003 and REQ-004: the four Stream-common@1 items, with stable IDs.
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


def summary(title, checks):
    return f"{title}: {'; '.join(checks)}."


# ------------------------------------------------------------------ the invented demo package

DEMO_SEMANTICS = (
    "# DemoStream@1 semantics\n"
    "\n"
    "An invented package that exists only to exercise the HAL.IDL.SENS.STREAM template. It "
    "streams one length column and stands for no real device.\n"
)


def demo_base():
    return {
        "schema": "capability-package/v1",
        "manifest": {"name": "DemoStream", "version": "1.0.0", "deprecated": False, "tier3_usable": True},
        "$defs": {},
        "operations": [],
        "batch_schemas": [
            {
                "id": "demo.samples",
                "columns": [
                    {
                        "name": "value",
                        "unit": "m",
                        "encoding": {"numeric": "i32", "scale": f64_hex(1e-6), "offset": f64_hex(0.0), "unit": "m"},
                    }
                ],
                "validity_mask": True,
                "seq_range": True,
            }
        ],
        "semantics": {"text": DEMO_SEMANTICS, "digest": sha256(DEMO_SEMANTICS.encode("utf-8"))},
        "profile": {
            "id": "DemoStream-core@1",
            "items": [{"id": "DS-01", "summary": "Payload encoding: the value column declares its exact encoding."}],
        },
    }


def with_template(pkg):
    """The template applied by hand: append the three operations and the four profile items,
    add the type schemas."""
    out = copy.deepcopy(pkg)
    for name, cls, errors in STREAM_OPERATIONS:
        out["operations"].append({
            "name": name,
            "class": cls,
            "request": closed({"session_id": ref("SessionId")}, ["session_id"]),
            "response": closed({"status": ref("StreamStatus")}, ["status"]),
            "errors": errors,
            "live_settings": [],
        })
    out["$defs"].update(copy.deepcopy(STREAM_DEFS))
    for item_id, title, checks in STREAM_COMMON:
        out["profile"]["items"].append({"id": item_id, "summary": summary(title, checks)})
    return out


def canonical_digest(pkg) -> str:
    out = copy.deepcopy(pkg)
    out["operations"] = sorted(out["operations"], key=lambda o: o["name"])
    for op in out["operations"]:
        op["errors"] = sorted(op["errors"])
        op["live_settings"] = sorted(op["live_settings"])
    out["batch_schemas"] = sorted(out["batch_schemas"], key=lambda b: b["id"])
    out["profile"]["items"] = sorted(out["profile"]["items"], key=lambda i: i["id"])
    return sha256(jcs(out))


# ------------------------------------------------------------------ AC-3 batch records

def batch(i, size, span):
    b = {"first_seq": str(i * 100), "last_seq": str(i * 100 + 99), "bytes": str(size)}
    if span is not None:
        b["span_ns"] = str(span)
    return b


def in_batch(i):
    """A batch within the recommendation; the kind cycles so every way of qualifying, and
    each boundary, appears."""
    kind = i % 6
    if kind == 0:
        return batch(i, 1_000_000, 600_000_000)  # lower size bound, long span
    if kind == 1:
        return batch(i, 8_000_000, 600_000_000)  # upper size bound, long span
    if kind == 2:
        return batch(i, 2_500_000, None)  # by size, no device time
    if kind == 3:
        return batch(i, 400_000, 50_000_000)  # small, span exactly 50 ms
    if kind == 4:
        return batch(i, 12_000_000, 20_000_000)  # large, short span
    return batch(i, 4_000_000, 40_000_000)  # both


def out_batch(i):
    """A batch outside the recommendation; also cycles through the near-boundary misses."""
    kind = i % 4
    if kind == 0:
        return batch(i, 999_999, 50_000_001)
    if kind == 1:
        return batch(i, 8_000_001, None)
    if kind == 2:
        return batch(i, 500_000, 600_000_000)
    return batch(i, 9_000_000, 75_000_000)


def record(out_indices):
    return {
        "schema": "stream-batches/0",
        "origin": "synthetic",
        "duration_ns": str(TEST_STREAM_NS),
        "batches": [out_batch(i) if i in out_indices else in_batch(i) for i in range(100)],
    }


def within(b) -> bool:
    size = int(b["bytes"])
    span = int(b["span_ns"]) if "span_ns" in b else None
    return MIN_BYTES <= size <= MAX_BYTES or (span is not None and span <= MAX_SPAN_NS)


def contiguous(batches) -> bool:
    for prev, cur in zip(batches, batches[1:]):
        if int(cur["first_seq"]) != int(prev["last_seq"]) + 1:
            return False
    return all(int(b["first_seq"]) <= int(b["last_seq"]) for b in batches)


def verdict(rec):
    batches = rec["batches"]
    total = len(batches)
    n_in = sum(1 for b in batches if within(b))
    if total == 0:
        result = {"pass": False, "reason": "no_batches"}
    elif int(rec["duration_ns"]) < TEST_STREAM_NS:
        result = {"pass": False, "reason": "stream_too_short"}
    elif n_in * 100 < PASS_PERCENT * total:
        result = {"pass": False, "reason": "below_threshold"}
    else:
        result = {"pass": True}
    result.update({"total": total, "within": n_in, "contiguous": contiguous(batches)})
    return result


def build_files():
    def fmt(obj) -> str:
        return json.dumps(obj, indent=2, ensure_ascii=False) + "\n"

    pkg = with_template(demo_base())
    ok = record({13, 47, 88})
    bad = record({i for i in range(100) if i % 5 == 4})
    expected = {"sizes_ok.json": verdict(ok), "sizes_bad.json": verdict(bad)}
    assert expected["sizes_ok.json"]["within"] == 97 and expected["sizes_ok.json"]["pass"]
    assert expected["sizes_bad.json"]["within"] == 80 and not expected["sizes_bad.json"]["pass"]
    return {
        "demo_stream_v1.json": fmt(pkg),
        "demo_stream_v1.sha256": canonical_digest(pkg) + "\n",
        "sizes_ok.json": fmt(ok),
        "sizes_bad.json": fmt(bad),
        "expected.json": fmt(expected),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_files()
    if check:
        for name, want in sorted(files.items()):
            path = os.path.join(HERE, name)
            if not os.path.isfile(path):
                print(f"missing fixture: {path}", file=sys.stderr)
                return 1
            with open(path, encoding="utf-8", newline="") as f:
                if f.read() != want:
                    print(f"fixture mismatch: {path}", file=sys.stderr)
                    return 1
        print(f"ok ({files['demo_stream_v1.sha256'].strip()})")
        return 0
    for name, content in sorted(files.items()):
        with open(os.path.join(HERE, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
    print(f"wrote {len(files)} fixtures under {HERE} ({files['demo_stream_v1.sha256'].strip()})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
