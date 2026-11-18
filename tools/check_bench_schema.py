#!/usr/bin/env python3
"""Validate benchmark result documents against platform/bench-result.schema.json
(PLAT.HW.SCHEMA AC-4, REQ-PLAT.HW.SCHEMA-005) with an independent JSON Schema validator.

The sample fixture must validate; each hand-labelled mutation of it must not, starting
with the one AC-4 names (removing `context.gpu_driver_version`). Extra paths given on the
command line (e.g. a result written by a benchmark tool) must validate too.

Needs: pip install jsonschema
Usage: python tools/check_bench_schema.py [result.json ...]
"""

import copy
import json
import sys
from pathlib import Path

from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parent.parent
SCHEMA = ROOT / "platform" / "bench-result.schema.json"
SAMPLE = ROOT / "fixtures" / "plat" / "bench" / "result-sample.json"


def drop(path):
    def f(doc):
        *parents, last = path
        node = doc
        for p in parents:
            node = node[p]
        del node[last]
    return f


def put(path, value):
    def f(doc):
        *parents, last = path
        node = doc
        for p in parents:
            node = node[p]
        node[last] = value
    return f


M0 = ["workloads", 0, "metrics", 0]
M2 = ["workloads", 1, "metrics", 0]

# (label, mutation) pairs; every mutated document must be invalid.
INVALID = [
    ("AC-4: context.gpu_driver_version removed", drop(["context", "gpu_driver_version"])),
    ("context.machine removed", drop(["context", "machine"])),
    ("unknown context field", put(["context", "colour"], "blue")),
    ("schema_version 2", put(["schema_version"], 2)),
    ("os not in the matrix", put(["context", "os"], "plan9")),
    ("ram_mib 0", put(["context", "ram_mib"], 0)),
    ("unit furlongs", put(M0 + ["unit"], "furlongs")),
    ("dataset_sha256 removed", drop(M0 + ["dataset_sha256"])),
    ("dataset_sha256 upper-case", put(M0 + ["dataset_sha256"], "sha256:" + "A" * 64)),
    ("repetitions 101", put(M0 + ["repetitions"], 101)),
    ("repetitions 0", put(M0 + ["repetitions"], 0)),
    ("status ok with null median", put(M0 + ["median"], None)),
    ("negative min", put(M0 + ["min"], -1)),
    ("status skipped with a median", put(M2 + ["median"], 1.5)),
    ("status unknown", put(M0 + ["status"], "crashed")),
    ("no workloads", put(["workloads"], [])),
    ("workload without metrics", put(["workloads", 0, "metrics"], [])),
    ("bad workload id", put(["workloads", 0, "id"], "Mini Load")),
]


def main() -> int:
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    sample = json.loads(SAMPLE.read_text(encoding="utf-8"))
    bad = 0
    for f in [SAMPLE] + [Path(p) for p in sys.argv[1:]]:
        errs = list(v.iter_errors(json.loads(f.read_text(encoding="utf-8"))))
        if errs:
            bad += 1
            print(f"FAIL {f}: should validate: {errs[0].message}")
    for label, mutate in INVALID:
        doc = copy.deepcopy(sample)
        mutate(doc)
        if v.is_valid(doc):
            bad += 1
            print(f"FAIL mutation '{label}': should NOT validate")
    print("bench result schema check:", "failed" if bad else "ok")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
