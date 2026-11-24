#!/usr/bin/env python3
"""Independent fixture generator for HAL.IDL.NEG (capability negotiation).

Writes project-authored fixtures (CC-BY-4.0) under fixtures/hal/idl/neg/:
  - support_v1.json                     runtime-support/v1 table: Device 1.1, LineScan 1.3, HeightMap 1.0
  - support_v2_with_v1_deprecated.json  LineScan 2.0 current, LineScan 1.3 kept and marked deprecated
  - decl_gocator.json        AC-1: Device >=1.0 <2, LineScan >=1.2 <2 (intensity, encoder_trigger, raw_profiles)
  - decl_future.json         AC-2: LineScan >=2.0 <3 only (plus Device)
  - decl_no_device.json      AC-3: no Device major the runtime supports
  - decl_linescan_v1.json    AC-4: LineScan 1.x only
  - decl_duplicate.json      AC-5: LineScan declared twice
  - decl_span.json           a LineScan range spanning majors 1 and 2
  - decl_bad_range.json      REQ-006: a malformed version range
  - decl_unknown_cap.json    a capability the runtime does not know
  - expected_*.json          RFC 8785 canonical negotiation results (no trailing newline)
  - expected_errors.json     expected error code, entry and reason per failing case

Every declaration is invented ("Gocator-style" per the leaf; make, model, serials, firmware
strings, digests and limits are made up and do not describe any real instrument). Limits are
round, declared values, not measurements (D-045).

The negotiation below is written from the leaf's rules (REQ-HAL.IDL.NEG-001..006), not from
the Rust code. Stdlib only; shares no code with crates/fid-hal-idl.
Run:  python fixtures/hal/idl/neg/make_fixtures.py [--check]
"""

import json
import os
import re
import struct
import sys

HERE = os.path.dirname(os.path.abspath(__file__))


def jcs(value) -> bytes:
    """RFC 8785 canonical JSON for strings, booleans, small integers, arrays and objects."""
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


def pretty(value) -> bytes:
    return (json.dumps(value, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def f64_bits(x: float) -> str:
    return "0x" + struct.pack(">d", x).hex()


def text(s: str) -> dict:
    return {"bytes": s, "truncated": False}


# ------------------------------------------------------------------ runtime support tables

def major(m, highest_minor, features, deprecated=False):
    return {"major": m, "highest_minor": highest_minor, "features": features, "deprecated": deprecated}


SUPPORT_V1 = {
    "schema": "runtime-support/v1",
    "capabilities": {
        "Device": [major(1, 1, ["health_push"])],
        "HeightMap": [major(1, 0, [])],
        "LineScan": [major(1, 3, ["encoder_trigger", "intensity", "multi_exposure"])],
    },
}

SUPPORT_V2 = {
    "schema": "runtime-support/v1",
    "capabilities": {
        "Device": [major(1, 1, ["health_push"])],
        "HeightMap": [major(1, 0, [])],
        # Listed newest first on purpose: order in the file carries no meaning.
        "LineScan": [
            major(2, 0, ["encoder_trigger", "intensity", "multi_exposure", "surface_track"]),
            major(1, 3, ["encoder_trigger", "intensity", "multi_exposure"], deprecated=True),
        ],
    },
}

# ------------------------------------------------------------------ declarations (invented)


def offer(name, versions, features=(), limits=None):
    return {"name": name, "versions": versions, "features": list(features), "limits": limits or {}}


def declaration(capabilities, serial="INVENTED-SN-0001"):
    return {
        "schema": "capability-declaration/v1",
        "driver_version": "0.3.0",
        "device": {
            "driver_id": "drv.example.linescan",
            "make": text("Example Optics (invented)"),
            "model": text("EX-LS 100 (invented)"),
            "serial": text(serial),
            "firmware": text("ex-fw 4.2 (invented)"),
        },
        "capabilities": capabilities,
        "time_sources": ["PTP", "HOST_RECEIVE"],
        "tested_firmware": {"lowest": text("ex-fw 4.0 (invented)"), "highest": text("ex-fw 4.2 (invented)")},
        "conformance_report": "sha256:" + "0" * 63 + "1",
    }


LS_LIMITS = {
    # Round declared values (D-045): a spec-sheet style claim, not a measurement.
    "min_profile_period": {"unit": "s", "value": f64_bits(0.001)},
    "measuring_range_z": {"unit": "m", "value": f64_bits(0.1)},
}

DEVICE = offer("Device", ">=1.0 <2", ["health_push"])

DECLS = {
    # Capabilities deliberately out of name order, features out of order: AC-6 and REQ-002
    # require sorted output regardless of input order.
    "decl_gocator.json": declaration(
        [offer("LineScan", ">=1.2 <2", ["intensity", "encoder_trigger", "raw_profiles"], LS_LIMITS), DEVICE]
    ),
    "decl_future.json": declaration([DEVICE, offer("LineScan", ">=2.0 <3", ["intensity"])], "INVENTED-SN-0002"),
    "decl_no_device.json": declaration(
        [offer("Device", ">=2.0 <3"), offer("LineScan", ">=1.0 <2", ["intensity"])], "INVENTED-SN-0003"
    ),
    "decl_linescan_v1.json": declaration(
        [DEVICE, offer("LineScan", ">=1.0 <2", ["encoder_trigger", "surface_track"])], "INVENTED-SN-0004"
    ),
    "decl_duplicate.json": declaration(
        [DEVICE, offer("LineScan", ">=1.0 <2"), offer("LineScan", ">=1.2 <2", ["intensity"])], "INVENTED-SN-0005"
    ),
    "decl_span.json": declaration(
        [DEVICE, offer("LineScan", ">=1.2 <3", ["surface_track", "intensity"])], "INVENTED-SN-0006"
    ),
    "decl_bad_range.json": declaration([DEVICE, offer("LineScan", ">=1.2 < 2")], "INVENTED-SN-0007"),
    "decl_unknown_cap.json": declaration(
        [DEVICE, offer("Gauge", ">=1.0 <2", ["tolerance_band"]), offer("HeightMap", ">=1.0 <2")], "INVENTED-SN-0008"
    ),
}

# Which declaration runs against which table, and where the result goes.
CASES = [
    ("expected_gocator.json", "support_v1.json", "decl_gocator.json"),
    ("expected_future.json", "support_v1.json", "decl_future.json"),
    ("expected_deprecated.json", "support_v2_with_v1_deprecated.json", "decl_linescan_v1.json"),
    ("expected_gocator_v2.json", "support_v2_with_v1_deprecated.json", "decl_gocator.json"),
    ("expected_span_v1.json", "support_v1.json", "decl_span.json"),
    ("expected_span_v2.json", "support_v2_with_v1_deprecated.json", "decl_span.json"),
    ("expected_unknown_cap.json", "support_v1.json", "decl_unknown_cap.json"),
]
ERROR_CASES = [
    ("support_v1.json", "decl_no_device.json"),
    ("support_v1.json", "decl_duplicate.json"),
    ("support_v1.json", "decl_bad_range.json"),
]

# ------------------------------------------------------------------ the rules, from the spec

RANGE = re.compile(r"^>=(0|[1-9][0-9]{0,8})\.(0|[1-9][0-9]{0,8}) <(0|[1-9][0-9]{0,8})$")


class NegFail(Exception):
    def __init__(self, code, entry, reason):
        super().__init__(code)
        self.code, self.entry, self.reason = code, entry, reason


def parse_range(s):
    """`>=a.b <c`: majors a..c-1, and minors >= b within major a. Returns None if malformed
    or empty (c <= a)."""
    m = RANGE.match(s)
    if not m:
        return None
    a, b, c = (int(g) for g in m.groups())
    if c <= a:
        return None
    return a, b, c


def negotiate(support, decl):
    seen = set()
    for o in decl["capabilities"]:
        rng = parse_range(o["versions"])
        if rng is None:
            raise NegFail("declaration_invalid", o["name"], "version_range_invalid")
    for o in decl["capabilities"]:
        if o["name"] in seen:
            raise NegFail("declaration_invalid", o["name"], "duplicate_capability")
        seen.add(o["name"])

    per_cap, ignored = {}, []
    for o in decl["capabilities"]:
        name = o["name"]
        lo_major, lo_minor, hi_major = parse_range(o["versions"])
        majors = support["capabilities"].get(name)
        if majors is None:
            per_cap[name] = {"status": "UNSUPPORTED", "reason": "unknown_capability"}
            continue
        chosen = None
        for row in sorted(majors, key=lambda r: -r["major"]):  # highest major first
            mj = row["major"]
            if not (lo_major <= mj < hi_major):
                continue
            driver_lowest_minor = lo_minor if mj == lo_major else 0
            if row["highest_minor"] >= driver_lowest_minor:
                chosen = row
                break
        if chosen is None:
            per_cap[name] = {"status": "UNSUPPORTED", "reason": "no_common_major"}
            continue
        known = set(chosen["features"])
        per_cap[name] = {
            "status": "negotiated",
            "version": f"{chosen['major']}.{chosen['highest_minor']}",
            "features": sorted(f for f in o["features"] if f in known),
            "deprecated": chosen["deprecated"],
        }
        ignored += [{"capability": name, "feature": f} for f in o["features"] if f not in known]

    if per_cap.get("Device", {}).get("status") != "negotiated":
        raise NegFail("UNSUPPORTED", "Device", "device_unsupported")
    ignored.sort(key=lambda x: (x["capability"], x["feature"]))
    return {"per_capability": per_cap, "ignored_features": ignored}


# ------------------------------------------------------------------ build and check


def build():
    out = {
        "support_v1.json": pretty(SUPPORT_V1),
        "support_v2_with_v1_deprecated.json": pretty(SUPPORT_V2),
    }
    tables = {"support_v1.json": SUPPORT_V1, "support_v2_with_v1_deprecated.json": SUPPORT_V2}
    for name, d in DECLS.items():
        out[name] = pretty(d)
    for expected, table, decl in CASES:
        out[expected] = jcs(negotiate(tables[table], DECLS[decl]))
    errors = {}
    for table, decl in ERROR_CASES:
        try:
            negotiate(tables[table], DECLS[decl])
        except NegFail as e:
            errors[decl] = {"code": e.code, "entry": e.entry, "reason": e.reason, "support": table}
        else:
            raise SystemExit(f"{decl} was expected to fail")
    out["expected_errors.json"] = pretty(errors)
    return out


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, data in sorted(build().items()):
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
    print(f"wrote {len(build())} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
