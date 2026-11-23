#!/usr/bin/env python3
"""Deterministic fixture generator for ECO.DATA.BENCH.RUN (AC-1..AC-5).

Generates synthetic benchmark suite, profiles, catalogue, fake `fid` runner, and sample
records under `fixtures/bench/` (MIT OR Apache-2.0):
  - `blob-a.dat`, `blob-b.dat`: synthetic datasets
  - `datasets.toml`: catalogue entries for `bench-ds-a` and `bench-ds-b`
  - `suite.toml`: two workloads (`bench.load`, `bench.fit`) on catalogue datasets
  - `suite-missing-ds.toml`: workload referencing uncatalogued `bench-ds-missing`
  - `profiles.toml`: `minimum`, `recommended`, and `min-2026`
  - `fake-fid`: executable Python stand-in emitting fixed PLAT.HW results (100, 102, 98)
  - `record-a.json`, `record-b.json`: two records differing in `machine_profile_id` (AC-5)

Usage: python3 fixtures/bench/make_fixtures.py [--check]
Licence: MIT OR Apache-2.0.
"""

from __future__ import annotations

import hashlib
import json
import os
import stat
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

BLOB_A = b"metrology-maven bench synthetic dataset A v1\n"
BLOB_B = b"metrology-maven bench synthetic dataset B v1\n"

SHA_A = hashlib.sha256(BLOB_A).hexdigest()
SHA_B = hashlib.sha256(BLOB_B).hexdigest()
SHA_MISSING = hashlib.sha256(b"missing-dataset").hexdigest()

DATASETS_TOML = f"""[policy]
redistributable = ["CC0-1.0", "CC-BY-4.0", "MIT", "Apache-2.0"]

[[dataset]]
id = "bench-ds-a"
title = "Synthetic benchmark dataset A (ECO.DATA.BENCH.RUN)"
source = "https://example.invalid/bench-ds-a.dat"
licence = "CC-BY-4.0"
size_bytes = {len(BLOB_A)}
sha256 = "{SHA_A}"
owner = "ECO"
redistribution = "mirror"
urls = ["https://example.invalid/bench-ds-a.dat"]
notes = "Synthetic benchmark fixture A."

[[dataset]]
id = "bench-ds-b"
title = "Synthetic benchmark dataset B (ECO.DATA.BENCH.RUN)"
source = "https://example.invalid/bench-ds-b.dat"
licence = "CC-BY-4.0"
size_bytes = {len(BLOB_B)}
sha256 = "{SHA_B}"
owner = "ECO"
redistribution = "mirror"
urls = ["https://example.invalid/bench-ds-b.dat"]
notes = "Synthetic benchmark fixture B."
""".encode("utf-8")

SUITE_TOML = f"""schema_version = 1
suite_version = "0.0.0-bench-fixture"

[[workloads]]
id = "bench.load"
dataset = {{ id = "bench-ds-a", sha256 = "sha256:{SHA_A}" }}
repetitions = 3
timeout_s = 60

[[workloads.metrics]]
name = "elapsed_ms"
unit = "ms"
better = "lower"

[[workloads]]
id = "bench.fit"
dataset = {{ id = "bench-ds-b", sha256 = "sha256:{SHA_B}" }}
repetitions = 3
timeout_s = 60

[[workloads.metrics]]
name = "elapsed_ms"
unit = "ms"
better = "lower"
""".encode("utf-8")

SUITE_MISSING_TOML = f"""schema_version = 1
suite_version = "0.0.0-bench-fixture"

[[workloads]]
id = "bench.missing"
dataset = {{ id = "bench-ds-missing", sha256 = "sha256:{SHA_MISSING}" }}
repetitions = 3
timeout_s = 60

[[workloads.metrics]]
name = "elapsed_ms"
unit = "ms"
better = "lower"
""".encode("utf-8")

PROFILES_TOML = """schema_version = 1

[profiles.minimum]
cores = 4
ram_gib = 16
gpu_class = "integrated"
vram_gib = 2
storage_free_gib = 20
display = [1920, 1080]
cpu_level = { x86_64 = "x86-64-v2", aarch64 = "armv8.0" }

[profiles.min-2026]
cores = 4
ram_gib = 16
gpu_class = "integrated"
vram_gib = 2
storage_free_gib = 20
display = [1920, 1080]
cpu_level = { x86_64 = "x86-64-v2", aarch64 = "armv8.0" }

[profiles.recommended]
cores = 8
ram_gib = 32
gpu_class = "discrete_mid"
vram_gib = 8
storage_free_gib = 100
display = [2560, 1440]
cpu_level = { x86_64 = "x86-64-v3", aarch64 = "armv8.2" }
""".encode("utf-8")

FAKE_FID = """#!/usr/bin/env python3
\"\"\"Fake `fid` benchmark runner for ECO.DATA.BENCH.RUN tests (AC-1..AC-4).

Accepts: `doctor run-benchmark --workload <id> --dataset <path> --json`
Emits fixed PLAT.HW.SCHEMA v1 result documents cycling through [100, 102, 98] per workload.
\"\"\"

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
from pathlib import Path

VALUES = [100, 102, 98]


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "doctor" or argv[2] != "run-benchmark":
        sys.stderr.write("expected: doctor run-benchmark ...\\n")
        return 2
    parser = argparse.ArgumentParser(prog="fake-fid")
    parser.add_argument("--workload", required=True)
    parser.add_argument("--dataset", required=True)
    parser.add_argument("--json", action="store_true")
    args = parser.parse_args(argv[3:])

    log_path = os.environ.get("FAKE_FID_CALL_LOG")
    if log_path:
        with open(log_path, "a", encoding="utf-8") as fh:
            fh.write(f"{args.workload}\\n")

    state_dir = Path(os.environ.get("FAKE_FID_STATE_DIR", "/tmp"))
    state_dir.mkdir(parents=True, exist_ok=True)
    counter_file = state_dir / f"count-{args.workload}.txt"
    idx = 0
    if counter_file.exists():
        try:
            idx = int(counter_file.read_text(encoding="utf-8").strip())
        except ValueError:
            idx = 0
    counter_file.write_text(str(idx + 1), encoding="utf-8")

    val = VALUES[idx % len(VALUES)]
    ds_bytes = Path(args.dataset).read_bytes()
    ds_sha = "sha256:" + hashlib.sha256(ds_bytes).hexdigest()

    doc = {
        "context": {
            "adapter_name": "synthetic fixture adapter",
            "app_version": "0.0.1",
            "arch": "x86_64",
            "cpu_model": "synthetic fixture CPU",
            "gpu_backend": "vulkan",
            "gpu_driver_version": "0.0.0-fixture",
            "machine": "local",
            "os": "ubuntu",
            "os_version": "24.04",
            "ram_mib": 16384,
            "suite_version": "0.0.0-bench-fixture",
        },
        "schema_version": 1,
        "workloads": [
            {
                "id": args.workload,
                "metrics": [
                    {
                        "dataset_sha256": ds_sha,
                        "max": val,
                        "median": val,
                        "min": val,
                        "name": "elapsed_ms",
                        "repetitions": 1,
                        "status": "ok",
                        "unit": "ms",
                    }
                ],
            }
        ],
    }
    print(json.dumps(doc, sort_keys=True, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
""".encode("utf-8")


def make_record(machine_profile: str, median: str, mn: str, mx: str, spread: str) -> bytes:
    obj = {
        "benchmark_id": "bench.load",
        "build_profile": "release",
        "dataset": {"id": "bench-ds-a", "sha256": f"sha256:{SHA_A}"},
        "harness_version": "0.0.1",
        "machine_profile_id": machine_profile,
        "metrics": [
            {
                "max": mx,
                "median": median,
                "min": mn,
                "name": "elapsed_ms",
                "rel_spread": spread,
                "unit": "ms",
            }
        ],
        "repetitions": 3,
        "results": [],
        "revision": "1111111111111111111111111111111111111111",
        "schema": 1,
        "status": "ok",
    }
    return (json.dumps(obj, sort_keys=True, separators=(",", ":")) + "\n").encode("utf-8")


def build_all() -> dict[str, tuple[bytes, bool]]:
    return {
        "blob-a.dat": (BLOB_A, False),
        "blob-b.dat": (BLOB_B, False),
        "datasets.toml": (DATASETS_TOML, False),
        "suite.toml": (SUITE_TOML, False),
        "suite-missing-ds.toml": (SUITE_MISSING_TOML, False),
        "profiles.toml": (PROFILES_TOML, False),
        "fake-fid": (FAKE_FID, True),
        "record-a.json": (make_record("min-2026", "100", "98", "102", "0.04"), False),
        "record-b.json": (make_record("recommended", "80", "79", "82", "0.0375"), False),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    files = build_all()
    if check:
        drift = []
        for name, (expected, is_exec) in files.items():
            p = HERE / name
            if not p.exists() or p.read_bytes() != expected:
                drift.append(name)
                continue
            if is_exec and not os.access(p, os.X_OK):
                drift.append(f"{name}(not executable)")
        if drift:
            print(f"bench fixtures out of date: {', '.join(drift)}", file=sys.stderr)
            return 1
        print(f"bench fixtures: ok ({len(files)} files)")
        return 0

    for name, (content, is_exec) in files.items():
        p = HERE / name
        p.write_bytes(content)
        if is_exec:
            mode = p.stat().st_mode
            p.chmod(mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)
    print(f"wrote {len(files)} bench fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
