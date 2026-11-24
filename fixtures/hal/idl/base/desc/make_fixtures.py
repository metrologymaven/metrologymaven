#!/usr/bin/env python3
"""Independent fixture generator for HAL.IDL.BASE.DESC (the Device@1 capability package).

Writes project-authored fixtures (CC-BY-4.0) under fixtures/hal/idl/base/desc/:
  - device_v1.json            (AC-1, AC-2: the Device@1 package as capability-package/v1,
                               written from the leaf's rules, not from the Rust code)
  - device_v1.sha256          (AC-2: SHA-256 over this script's own RFC 8785 canonical form
                               with lists sorted as REQ-HAL.IDL.GEN.FMT-003 says)
  - instance.json             (AC-3: a DeviceInstance whose serial is 5000 bytes, with a
                               3-byte UTF-8 character straddling the 4096-byte limit)
  - instance.expected.json    (AC-3: the instance after the DeviceText rule: cut at the last
                               UTF-8 boundary at or below 4096 bytes and marked truncated)

The device in instance.json is invented. Stdlib only; shares no code with crates/fid-hal-idl.
Run:  python fixtures/hal/idl/base/desc/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))

MAX_TEXT_BYTES = 4096  # HAL.IDL.BASE.ERR device text bound


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


# ------------------------------------------------------------------ the package, from the spec

HEALTH_STATES = [
    "OFFLINE", "CONNECTING", "INITIALIZING", "WARMING_UP", "READY",
    "BUSY", "FAULT", "SAFETY_STOPPED", "MAINTENANCE", "INTERRUPTED",
]


def ref(name):
    return {"$ref": "#/$defs/" + name}


def string(max_len):
    return {"type": "string", "maxLength": max_len}


def closed(props, required):
    out = {"type": "object", "additionalProperties": False}
    if props:
        out["properties"] = props
    if required:
        out["required"] = required
    return out


def session_only():
    return closed({"session_id": ref("SessionId")}, ["session_id"])


DEFS = {
    "SessionId": string(36),
    "Digest": string(71),
    "CalDate": string(10),
    "Configuration": {"type": "object"},
    "HealthState": {"type": "string", "enum": HEALTH_STATES},
    "DeviceText": closed(
        {"bytes": string(MAX_TEXT_BYTES), "truncated": {"type": "boolean"}},
        ["bytes", "truncated"],
    ),
    "DeviceInstance": closed(
        {
            "make": ref("DeviceText"),
            "model": ref("DeviceText"),
            "serial": ref("DeviceText"),
            "firmware": ref("DeviceText"),
            "driver_id": string(128),
        },
        ["driver_id", "make", "model"],
    ),
    "CalibrationState": closed(
        {
            "dcc_digests": {"type": "array", "items": ref("Digest")},
            "qualification_ref": ref("DeviceText"),
            "compensation_id": ref("DeviceText"),
            "last_interim_check": ref("CalDate"),
            "valid_until": ref("CalDate"),
        },
        ["dcc_digests"],
    ),
}

# (name, class, allowed errors, request schema, response schema): REQ-HAL.IDL.BASE.DESC-001
# fixes the seven names and the classes; the error lists are this leaf's choice.
READ_ERRORS = ["TRANSIENT", "COMM_LOST", "INTERNAL"]
OPERATIONS = [
    ("describe", "R0", READ_ERRORS,
     closed({}, []),
     closed({"instance": ref("DeviceInstance")}, ["instance"])),
    ("open_session", "D1",
     ["TRANSIENT", "COMM_LOST", "NOT_QUALIFIED", "CALIBRATION_EXPIRED", "SAFETY_STOP_OBSERVED",
      "VENDOR_LICENSE", "UNSUPPORTED", "INTERNAL"],
     session_only(),
     closed({"session_id": ref("SessionId"), "health": ref("HealthState")}, ["health", "session_id"])),
    ("close_session", "D1", READ_ERRORS,
     session_only(),
     closed({"health": ref("HealthState")}, ["health"])),
    ("get_health", "R0", READ_ERRORS,
     session_only(),
     closed({"health": ref("HealthState")}, ["health"])),
    ("get_configuration", "R0", READ_ERRORS,
     session_only(),
     closed({"configuration": ref("Configuration"), "config_digest": ref("Digest")},
            ["config_digest", "configuration"])),
    ("set_configuration", "D1",
     ["TRANSIENT", "COMM_LOST", "CONFIG_INVALID", "OUT_OF_RANGE", "LIMIT", "UNSUPPORTED", "INTERNAL"],
     closed({"session_id": ref("SessionId"), "configuration": ref("Configuration")},
            ["configuration", "session_id"]),
     closed({"config_digest": ref("Digest")}, ["config_digest"])),
    ("get_calibration_state", "R0", ["TRANSIENT", "COMM_LOST", "UNSUPPORTED", "INTERNAL"],
     session_only(),
     closed({"calibration": ref("CalibrationState")}, ["calibration"])),
]

SEMANTICS = (
    "# Device@1 semantics\n"
    "\n"
    "The base capability package that every device, driver, simulator, playback source and file "
    "pseudo-device implements.\n"
    "\n"
    "- `describe` returns the DeviceInstance: make, model, serial and firmware as untrusted device "
    "text of at most 4096 bytes each, and the ID of the serving driver.\n"
    "- `open_session` and `close_session` bracket all other session work. The runtime assigns the "
    "session ID.\n"
    "- `get_health` reports one state of the HAL.IDL.BASE.HEALTH machine.\n"
    "- `get_configuration` and `set_configuration` carry a configuration/0 document checked against "
    "the device's config-schema/0, and return its configuration digest.\n"
    "- `get_calibration_state` returns calibration references and validity dates only. No "
    "operation writes calibration, qualification or compensation data.\n"
    "- Device-supplied text is data and is never interpreted as instructions.\n"
)

# REQ-HAL.IDL.BASE.DESC-004: the six Device-core@1 items, with stable IDs.
PROFILE_ITEMS = [
    ("DC-01", "Describe consistency", [
        "describe returns a DeviceInstance with make, model and driver_id",
        "repeated describe calls return the same instance",
    ]),
    ("DC-02", "Session open and close", [
        "open_session returns the requested session ID and a health state",
        "close_session ends the session and later session calls fail with a declared code",
    ]),
    ("DC-03", "Health reporting within the transition table", [
        "every reported health change is a transition of the HAL.IDL.BASE.HEALTH table for its actor",
    ]),
    ("DC-04", "Error-code closure", [
        "every error is a Device@1 error code",
        "every error an operation returns is in that operation's declared list",
    ]),
    ("DC-05", "Configuration round-trip and digest stability", [
        "get_configuration after set_configuration returns the values set",
        "the configuration digest is the same on every read and every platform",
    ]),
    ("DC-06", "Calibration state is read-only", [
        "get_calibration_state returns references and validity dates only",
        "no operation changes the calibration state",
    ]),
]


def package() -> dict:
    return {
        "schema": "capability-package/v1",
        "manifest": {"name": "Device", "version": "1.0.0", "deprecated": False, "tier3_usable": True},
        "$defs": DEFS,
        "operations": [
            {
                "name": name,
                "class": cls,
                "request": req,
                "response": resp,
                "errors": errors,
                "live_settings": [],
            }
            for name, cls, errors, req, resp in OPERATIONS
        ],
        "batch_schemas": [],
        "semantics": {"text": SEMANTICS, "digest": sha256(SEMANTICS.encode("utf-8"))},
        "profile": {
            "id": "Device-core@1",
            "items": [
                {"id": i, "summary": f"{title}: {'; '.join(checks)}."} for i, title, checks in PROFILE_ITEMS
            ],
        },
    }


def canonical_digest(pkg: dict) -> str:
    out = copy.deepcopy(pkg)
    out["operations"] = sorted(out["operations"], key=lambda o: o["name"])
    for op in out["operations"]:
        op["errors"] = sorted(op["errors"])
        op["live_settings"] = sorted(op["live_settings"])
    out["batch_schemas"] = sorted(out["batch_schemas"], key=lambda b: b["id"])
    out["profile"]["items"] = sorted(out["profile"]["items"], key=lambda i: i["id"])
    return sha256(jcs(out))


# ------------------------------------------------------------------ AC-3 instance

def cut(text: str):
    """The DeviceText rule: keep the longest prefix of at most 4096 UTF-8 bytes that ends on a
    character boundary; mark it truncated when anything was dropped."""
    raw = text.encode("utf-8")
    if len(raw) <= MAX_TEXT_BYTES:
        return text, False
    end = MAX_TEXT_BYTES
    while end > 0 and (raw[end] & 0xC0) == 0x80:
        end -= 1
    return raw[:end].decode("utf-8"), True


def instance_files():
    head = ("SN-EXAMPLE-" + "0123456789" * 410)[:4094]
    serial = head + "€" + ("X" * 903)  # the euro sign takes bytes 4094..4096
    assert len(serial.encode("utf-8")) == 5000
    raw = {
        "driver_id": "drv.example.linescan",
        "make": {"bytes": "Example Optics", "truncated": False},
        "model": {"bytes": "LS-100", "truncated": False},
        "serial": {"bytes": serial, "truncated": False},
        "firmware": {"bytes": "4.2.1", "truncated": False},
    }
    kept, truncated = cut(serial)
    assert len(kept.encode("utf-8")) == 4094 and truncated
    expected = copy.deepcopy(raw)
    expected["serial"] = {"bytes": kept, "truncated": truncated}
    return raw, expected


def build_files():
    def fmt(obj) -> str:
        return json.dumps(obj, indent=2, ensure_ascii=False) + "\n"

    pkg = package()
    raw, expected = instance_files()
    return {
        "device_v1.json": fmt(pkg),
        "device_v1.sha256": canonical_digest(pkg) + "\n",
        "instance.json": fmt(raw),
        "instance.expected.json": fmt(expected),
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
        print(f"ok ({files['device_v1.sha256'].strip()})")
        return 0
    for name, content in sorted(files.items()):
        with open(os.path.join(HERE, name), "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
    print(f"wrote {len(files)} fixtures under {HERE} ({files['device_v1.sha256'].strip()})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
