#!/usr/bin/env python3
"""Independent fixture generator for HAL.ACQ.SESS.REG (device registry of serving sources).

Writes project-authored fixtures (CC-BY-4.0) under fixtures/hal/acq/sess/reg/:
  - three_sources.json         AC-1, AC-4: three registrations (driver, simulator, file), in a
                               deliberately unsorted order; the simulator claims to be a driver;
                               the file source declares neither serial nor firmware
  - expected_show_devices.json AC-4: RFC 8785 canonical result of device.show_devices@1 over
                               the three sources with no session open (no trailing newline)
  - expected_events.json       AC-1: RFC 8785 canonical list of the device.event records the
                               registrations raise (no trailing newline)
  - expected_ids.json          AC-2: the file-source namespace and the UUIDv5 of the file digest

Every declaration is invented: makes, models, serials, firmware strings and digests are made
up and describe no real instrument.

The listing below is written from the leaf's rules (REQ-HAL.ACQ.SESS.REG-001..005) and the
crate's documented wire form, not from the Rust code:
  - the kind comes from the entry point only; a claimed kind that differs is ignored and
    raises one `device.event` with reason `source_kind_mismatch`;
  - file sources get UUIDv5(FILE_NAMESPACE, <digest wire form>), where FILE_NAMESPACE is
    UUIDv5(nil, "metrologymaven/hal.acq.sess.reg/file-source"); others keep their UUIDv7;
  - the listing is sorted by source ID (lowercase hyphenated text, which sorts as the bytes);
    capabilities by name, features by name; identity strings keep the {bytes, truncated} form.
Stdlib only; shares no code with crates/fid-hal-runtime.
Run:  python fixtures/hal/acq/sess/reg/make_fixtures.py [--check]
"""

import hashlib
import json
import os
import sys
import uuid

HERE = os.path.dirname(os.path.abspath(__file__))

FILE_NAMESPACE = uuid.uuid5(uuid.UUID(int=0), "metrologymaven/hal.acq.sess.reg/file-source")

ENTRY_KINDS = {
    "driver": "driver",
    "simulator": "simulator",
    "emulator": "emulator",
    "playback": "playback",
    "file": "file",
}


def jcs(value) -> bytes:
    """RFC 8785 canonical JSON for strings, booleans, arrays and objects."""
    if isinstance(value, bool):
        return b"true" if value else b"false"
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if isinstance(value, list):
        return b"[" + b",".join(jcs(v) for v in value) + b"]"
    if isinstance(value, dict):
        items = sorted(value.items(), key=lambda kv: kv[0].encode("utf-16-be"))
        return b"{" + b",".join(jcs(k) + b":" + jcs(v) for k, v in items) + b"}"
    raise TypeError(f"unsupported type: {type(value)}")


def pretty(value) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def text(s: str) -> dict:
    return {"bytes": s, "truncated": False}


def offer(name, versions, features):
    return {"name": name, "versions": versions, "features": list(features), "limits": {}}


def declaration(driver_id, make, model, serial, capabilities, firmware=None):
    device = {"driver_id": driver_id, "make": text(make), "model": text(model)}
    if serial is not None:
        device["serial"] = text(serial)
    if firmware is not None:
        device["firmware"] = text(firmware)
    return {
        "schema": "capability-declaration/v1",
        "driver_version": "0.2.0",
        "device": device,
        "capabilities": capabilities,
        "time_sources": ["HOST_RECEIVE"],
        "tested_firmware": {"lowest": text("fw 1.0 (invented)"), "highest": text("fw 1.4 (invented)")},
        "conformance_report": "sha256:" + "0" * 63 + "2",
    }


FILE_DIGEST = "sha256:" + hashlib.sha256(b"invented point file for HAL.ACQ.SESS.REG fixtures").hexdigest()

REGISTRATIONS = [
    {
        "entry_point": "simulator",
        "source_id": "0193f0a0-0000-7000-8000-0000000000c3",
        "claimed_kind": "driver",
        "registered_by": "hal.sim.launcher",
        "registered_at": "2000000",
        "declaration": declaration(
            "sim.example.gauge",
            "Example Gauges (invented)",
            "EG-SIM 1 (invented)",
            "INVENTED-SIM-0003",
            [offer("Gauge", ">=1.0 <2", ["zero", "hold"]), offer("Device", ">=1.0 <2", ["health_push"])],
        ),
    },
    {
        "entry_point": "file",
        "file_digest": FILE_DIGEST,
        "claimed_kind": None,
        "registered_by": "hal.acq.filedev",
        "registered_at": "3000000",
        "declaration": declaration(
            "file.example.points",
            "Example Metrology (invented)",
            "EM-CMM 700 (invented)",
            None,
            [offer("Device", ">=1.0 <2", [])],
        ),
    },
    {
        "entry_point": "driver",
        "source_id": "0193f0a0-0000-7000-8000-0000000000a1",
        "claimed_kind": "driver",
        "registered_by": "hal.host.driver",
        "registered_at": "1000000",
        "declaration": declaration(
            "drv.example.linescan",
            "Example Optics (invented)",
            "EX-LS 100 (invented)",
            "INVENTED-SN-0001",
            [
                offer("LineScan", ">=1.2 <2", ["raw_profiles", "intensity", "encoder_trigger"]),
                offer("Device", ">=1.0 <2", ["health_push"]),
            ],
            firmware="ex-fw 4.2 (invented)",
        ),
    },
]


def source_id(r) -> str:
    if r["entry_point"] == "file":
        return str(uuid.uuid5(FILE_NAMESPACE, r["file_digest"]))
    u = uuid.UUID(r["source_id"])
    assert u.version == 7 and u.variant == uuid.RFC_4122, r["source_id"]
    assert str(u) == r["source_id"]
    return r["source_id"]


def view(r) -> dict:
    d = r["declaration"]
    identity = {k: v for k, v in d["device"].items() if k != "driver_id"}
    caps = sorted(
        ({"name": c["name"], "versions": c["versions"], "features": sorted(c["features"])} for c in d["capabilities"]),
        key=lambda c: c["name"],
    )
    return {
        "source_id": source_id(r),
        "source_kind": ENTRY_KINDS[r["entry_point"]],
        "registered_by": r["registered_by"],
        "registered_at": r["registered_at"],
        "session_state": "none",
        "driver_id": d["device"]["driver_id"],
        "driver_version": d["driver_version"],
        "identity": identity,
        "capabilities": caps,
    }


def events() -> list:
    out = []
    for r in REGISTRATIONS:
        kind = ENTRY_KINDS[r["entry_point"]]
        claim = r["claimed_kind"]
        if claim is not None and claim != kind:
            out.append(
                {
                    "event": "device.event",
                    "reason": "source_kind_mismatch",
                    "source_id": source_id(r),
                    "source_kind": kind,
                    "claimed_kind": claim if claim in ENTRY_KINDS else "unrecognised",
                    "at": r["registered_at"],
                }
            )
    return out


def build():
    listing = {"sources": sorted((view(r) for r in REGISTRATIONS), key=lambda v: v["source_id"])}
    ids = {"file_namespace": str(FILE_NAMESPACE), "file_digest": FILE_DIGEST, "file_source_id": source_id(REGISTRATIONS[1])}
    return {
        "three_sources.json": pretty({"registrations": REGISTRATIONS}),
        "expected_show_devices.json": jcs(listing),
        "expected_events.json": jcs(events()),
        "expected_ids.json": pretty(ids),
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    files = build()
    for name, data in sorted(files.items()):
        path = os.path.join(HERE, name)
        if check:
            try:
                with open(path, "rb") as f:
                    have = f.read()
            except FileNotFoundError:
                have = None
            if have != data:
                print(f"drift: {name}")
                bad += 1
        else:
            with open(path, "wb") as f:
                f.write(data)
    if check:
        print("ok" if bad == 0 else f"{bad} file(s) differ")
        return 1 if bad else 0
    print(f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
