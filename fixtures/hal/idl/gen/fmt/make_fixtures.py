#!/usr/bin/env python3
"""Independent fixture generator for HAL.IDL.GEN.FMT (capability-package/v1).

Writes project-authored demo descriptions (CC-BY-4.0) under fixtures/hal/idl/gen/fmt/:
  - demo_v1.json          (AC-1, AC-6: valid two-operation demo package with internal $defs,
                           batch schema with declared EncodingDescriptors, semantics note and
                           conformance profile)
  - demo_v1.sha256        (AC-1: independent Python RFC 8785 JCS + SHA-256 digest over the
                           sorted-lists canonical form)
  - dup_op.json           (AC-2: duplicate operation name at /operations/1/name)
  - bad_class.json        (AC-3: unknown side-effect class "Z9" at /operations/0/class)
  - no_encoding.json      (AC-4: batch column missing encoding at /batch_schemas/0/columns/0/encoding)
  - foreign_keyword.json  (AC-5: schema fragment using "patternProperties")

Stdlib only (copy, hashlib, json, os, struct, sys). Shares no code with crates/fid-hal-idl.
Run:  python fixtures/hal/idl/gen/fmt/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import os
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def f64_hex(x: float) -> str:
    return f"0x{struct.unpack('<Q', struct.pack('<d', x))[0]:016x}"


def sha256_prefixed(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def jcs_dumps(value) -> bytes:
    """RFC 8785 canonical JSON for the subset of values used in demo_v1 (ASCII strings,
    booleans, small integers, arrays, objects sorted by UTF-16 code units = ASCII order)."""
    if value is None:
        return b"null"
    if isinstance(value, bool):
        return b"true" if value else b"false"
    if isinstance(value, int):
        return str(value).encode("ascii")
    if isinstance(value, float):
        if value.is_integer():
            return str(int(value)).encode("ascii")
        return repr(value).encode("ascii")
    if isinstance(value, str):
        return json.dumps(value, ensure_ascii=False, separators=(",", ":")).encode("utf-8")
    if isinstance(value, list):
        return b"[" + b",".join(jcs_dumps(v) for v in value) + b"]"
    if isinstance(value, dict):
        items = sorted(value.items(), key=lambda kv: kv[0].encode("utf-16-be"))
        parts = [jcs_dumps(k) + b":" + jcs_dumps(v) for k, v in items]
        return b"{" + b",".join(parts) + b"}"
    raise TypeError(f"unsupported type: {type(value)}")


def canonicalise_package(pkg: dict) -> dict:
    """Sort operations by name, batch_schemas by id, profile.items by id, errors & live_settings."""
    out = copy.deepcopy(pkg)
    out["operations"] = sorted(out["operations"], key=lambda o: o["name"])
    for op in out["operations"]:
        op["errors"] = sorted(op["errors"])
        op["live_settings"] = sorted(op.get("live_settings", []))
    out["batch_schemas"] = sorted(out["batch_schemas"], key=lambda b: b["id"])
    out["profile"]["items"] = sorted(out["profile"]["items"], key=lambda i: i["id"])
    return out


def make_demo_v1() -> dict:
    sem_text = (
        "# Demo@1 semantics\n\n"
        "Two-operation demonstration capability package for `HAL.IDL.GEN.FMT`.\n"
        "- `read_sample` reads one calibrated point without energising motion.\n"
        "- `set_gain` updates the live gain setting.\n"
    )
    return {
        "schema": "capability-package/v1",
        "manifest": {
            "name": "Demo",
            "version": "1.0.0",
            "deprecated": False,
            "tier3_usable": True,
        },
        "$defs": {
            "GainValue": {
                "type": "number",
                "minimum": 0,
                "maximum": 100,
            }
        },
        "operations": [
            {
                "name": "set_gain",
                "class": "D1",
                "request": {
                    "type": "object",
                    "additionalProperties": False,
                    "properties": {
                        "gain": {"$ref": "#/$defs/GainValue"},
                        "channel": {"type": "string", "maxLength": 32},
                    },
                    "required": ["gain"],
                },
                "response": {
                    "type": "object",
                    "additionalProperties": False,
                    "properties": {
                        "applied": {"$ref": "#/$defs/GainValue"},
                    },
                    "required": ["applied"],
                },
                "errors": ["OUT_OF_RANGE", "CONFIG_INVALID"],
                "live_settings": ["sensor.gain"],
            },
            {
                "name": "read_sample",
                "class": "R0",
                "request": {
                    "type": "object",
                    "additionalProperties": False,
                    "properties": {
                        "mode": {"type": "string", "enum": ["single", "average"]},
                    },
                    "required": ["mode"],
                },
                "response": {
                    "type": "object",
                    "additionalProperties": False,
                    "properties": {
                        "samples": {
                            "type": "array",
                            "items": {"type": "number"},
                        }
                    },
                    "required": ["samples"],
                },
                "errors": ["TRANSIENT", "COMM_LOST"],
                "live_settings": [],
            },
        ],
        "batch_schemas": [
            {
                "id": "demo.points",
                "columns": [
                    {
                        "name": "x_pos",
                        "unit": "m",
                        "encoding": {
                            "numeric": "i32",
                            "scale": f64_hex(1e-6),
                            "offset": f64_hex(0.0),
                            "unit": "m",
                        },
                    },
                    {
                        "name": "temp_k",
                        "unit": "K",
                        "encoding": {
                            "numeric": "f32",
                            "scale": f64_hex(1.0),
                            "offset": f64_hex(273.15),
                            "unit": "K",
                        },
                    },
                ],
                "validity_mask": True,
                "seq_range": True,
            }
        ],
        "semantics": {
            "text": sem_text,
            "digest": sha256_prefixed(sem_text.encode("utf-8")),
        },
        "profile": {
            "id": "hal.demo.v1",
            "items": [
                {
                    "id": "DEMO-002",
                    "summary": "Live gain update stays within [0, 100].",
                },
                {
                    "id": "DEMO-001",
                    "summary": "read_sample returns calibrated positions in metres.",
                    "fixture_digest": sha256_prefixed(b"demo-fixture-v1"),
                },
            ],
        },
    }


def build_files() -> dict:
    demo = make_demo_v1()
    canon_bytes = jcs_dumps(canonicalise_package(demo))
    demo_digest = sha256_prefixed(canon_bytes) + "\n"

    # AC-2: duplicate operation name at /operations/1/name
    dup_op = copy.deepcopy(demo)
    dup_op["operations"][1]["name"] = dup_op["operations"][0]["name"]

    # AC-3: bad side-effect class "Z9" at /operations/0/class
    bad_class = copy.deepcopy(demo)
    bad_class["operations"][0]["class"] = "Z9"

    # AC-4: data-plane column without encoding at /batch_schemas/0/columns/0/encoding
    no_encoding = copy.deepcopy(demo)
    del no_encoding["batch_schemas"][0]["columns"][0]["encoding"]

    # AC-5: foreign JSON Schema keyword "patternProperties"
    foreign_kw = copy.deepcopy(demo)
    foreign_kw["operations"][0]["request"]["patternProperties"] = {
        "^x_": {"type": "string"}
    }

    def fmt(obj) -> str:
        return json.dumps(obj, indent=2, ensure_ascii=False) + "\n"

    return {
        "demo_v1.json": fmt(demo),
        "demo_v1.sha256": demo_digest,
        "dup_op.json": fmt(dup_op),
        "bad_class.json": fmt(bad_class),
        "no_encoding.json": fmt(no_encoding),
        "foreign_keyword.json": fmt(foreign_kw),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_files()
    if check:
        for name, expected in sorted(files.items()):
            path = os.path.join(HERE, name)
            if not os.path.isfile(path):
                print(f"missing fixture: {path}", file=sys.stderr)
                return 1
            with open(path, encoding="utf-8") as f:
                actual = f.read()
            if actual != expected:
                print(f"fixture mismatch: {path}", file=sys.stderr)
                return 1
        print(f"ok ({files['demo_v1.sha256'].strip()})")
        return 0

    os.makedirs(HERE, exist_ok=True)
    for name, content in sorted(files.items()):
        path = os.path.join(HERE, name)
        with open(path, "w", encoding="utf-8", newline="\n") as f:
            f.write(content)
    print(f"wrote {len(files)} fixtures under {HERE} ({files['demo_v1.sha256'].strip()})")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
