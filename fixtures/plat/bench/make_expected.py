#!/usr/bin/env python3
"""Independent reference for PLAT.HW.SCHEMA (stdlib only; does not run the Rust code).

1. `result-sample.canonical.json`: the RFC 8785 canonical form of `result-sample.json`
   (keys sorted, no whitespace). The Rust test checks that `BenchResult::to_json` of the
   parsed sample gives exactly these bytes (AC-5 determinism, fixed key order).
2. `platform/bench-result.schema.json`: the result JSON Schema written out by hand from the
   leaf spec (REQ-PLAT.HW.SCHEMA-002, -003, -005). The Rust test checks that
   `cargo xtask plat-hw result-schema` produces exactly these bytes, so the two independent
   statements of the schema agree.

Python's sorted keys equal RFC 8785 order here because every key is ASCII. Every number in
the sample is either a JSON integer or a float whose shortest repr is also the ECMAScript
form, so `json.dumps` matches RFC 8785 number formatting for this file; the script checks
that no float is integral (where Python would print `1.0` and ECMAScript `1`).

Usage: python fixtures/plat/bench/make_expected.py [--check]
"""

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent
SAMPLE = HERE / "result-sample.json"
CANONICAL = HERE / "result-sample.canonical.json"
SCHEMA = ROOT / "platform" / "bench-result.schema.json"

NAME_PATTERN = "^[a-z0-9][a-z0-9_.-]*$"
DIGEST_PATTERN = "^sha256:[0-9a-f]{64}$"
UNITS = ["s", "ms", "fps", "MiB", "points_per_s", "triangles_per_s"]
STATUSES = ["ok", "timeout", "failed", "skipped"]
OS_FAMILIES = ["windows", "ubuntu", "debian", "fedora", "rhel", "macos"]  # PLAT.MATRIX.FILE
ARCHES = ["x86_64", "aarch64"]
CONTEXT_KEYS = [
    "suite_version", "app_version", "os", "os_version", "arch", "cpu_model", "ram_mib",
    "gpu_backend", "adapter_name", "gpu_driver_version", "machine",
]
METRIC_KEYS = ["name", "unit", "dataset_sha256", "repetitions", "median", "min", "max", "status"]


def canonical(obj) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def no_integral_floats(v) -> None:
    if isinstance(v, float) and v.is_integer():
        raise SystemExit(f"integral float {v!r} in sample: write it as a JSON integer")
    if isinstance(v, dict):
        for x in v.values():
            no_integral_floats(x)
    if isinstance(v, list):
        for x in v:
            no_integral_floats(x)


def closed(keys, props):
    return {"type": "object", "additionalProperties": False, "required": list(keys), "properties": props}


def schema():
    nonempty = {"type": "string", "minLength": 1}
    name = {"type": "string", "pattern": NAME_PATTERN}
    ctx_props = {}
    for k in CONTEXT_KEYS:
        if k == "os":
            ctx_props[k] = {"enum": OS_FAMILIES}
        elif k == "arch":
            ctx_props[k] = {"enum": ARCHES}
        elif k == "ram_mib":
            ctx_props[k] = {"type": "integer", "minimum": 1, "maximum": 4294967295}
        else:
            ctx_props[k] = nonempty
    stat = {"type": ["number", "null"], "minimum": 0}

    def typed(t):
        return {"properties": {"median": {"type": t}, "min": {"type": t}, "max": {"type": t}}}

    metric = closed(METRIC_KEYS, {
        "name": name,
        "unit": {"enum": UNITS},
        "dataset_sha256": {"type": "string", "pattern": DIGEST_PATTERN},
        "repetitions": {"type": "integer", "minimum": 1, "maximum": 100},
        "median": stat,
        "min": stat,
        "max": stat,
        "status": {"enum": STATUSES},
    })
    metric["if"] = {"properties": {"status": {"const": "ok"}}, "required": ["status"]}
    metric["then"] = typed("number")
    metric["else"] = typed("null")
    workload = closed(["id", "metrics"], {
        "id": name,
        "metrics": {"type": "array", "minItems": 1, "items": metric},
    })
    top = {
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "title": "Metrology Maven benchmark result, schema_version 1 (PLAT.HW.SCHEMA)",
    }
    top.update(closed(["schema_version", "context", "workloads"], {
        "schema_version": {"const": 1},
        "context": closed(CONTEXT_KEYS, ctx_props),
        "workloads": {"type": "array", "minItems": 1, "items": workload},
    }))
    return top


def main() -> int:
    check = "--check" in sys.argv[1:]
    sample = json.loads(SAMPLE.read_text(encoding="utf-8"))
    no_integral_floats(sample)
    outputs = {CANONICAL: canonical(sample), SCHEMA: canonical(schema()) + "\n"}
    bad = 0
    for path, text in outputs.items():
        data = text.encode("utf-8")
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad += 1
                print(f"STALE {path.relative_to(ROOT)}: run make_expected.py")
        else:
            path.write_bytes(data)
            print(f"wrote {path.relative_to(ROOT)}")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
