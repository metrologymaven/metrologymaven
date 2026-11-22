#!/usr/bin/env python3
"""Independent fixture generator for UNC.MODEL.SUBST.INPUT (stdlib only; CC-BY-4.0).

Produces synthetic calibrated ring-gauge and bore substitution datasets in SI units (`si:m`):
  fixtures/unc/model/subst/ring_gauge_ok.json                 AC-1: 50.0012 mm, U=0.6 um, k=2, 20x3 runs
  fixtures/unc/model/subst/bore_ok.json                       Valid bore dataset (certificate source)
  fixtures/unc/model/subst/ring_gauge_insufficient_runs.json  AC-2: only 5 runs (profile requires 20)
  fixtures/unc/model/subst/ring_gauge_strategy_mismatch.json  AC-3: different production strategy_digest
  fixtures/unc/model/subst/ring_gauge_missing_k.json          AC-4: missing coverage factor k
  fixtures/unc/model/subst/README.md                          Provenance and CC-BY-4.0 license

Usage:
  python3 fixtures/unc/model/subst/make_fixtures.py          # write files
  python3 fixtures/unc/model/subst/make_fixtures.py --check  # verify byte-for-byte match
"""

import hashlib
import json
import random
import sys
from pathlib import Path

HERE = Path(__file__).parent
LICENSE = "CC-BY-4.0"


def sha256_str(tag: str) -> str:
    return "sha256:" + hashlib.sha256(tag.encode("utf-8")).hexdigest()


def make_runs(seed: int, mean_m: float, sigma_m: float, n_runs: int, n_reps: int):
    rng = random.Random(seed)
    runs = []
    for i in range(1, n_runs + 1):
        vals = [round(mean_m + rng.gauss(0.0, sigma_m), 9) for _ in range(n_reps)]
        runs.append({"run_id": f"run-{i:02d}", "values": vals})
    return runs


def base_conditions(tag: str, feature_ref: str, machine_id: str):
    return {
        "characteristic_kind": "diameter",
        "feature_ref": feature_ref,
        "machine_id": machine_id,
        "probe_config_digest": sha256_str(f"subst:{tag}:probe-config"),
        "program_digest": sha256_str(f"subst:{tag}:program"),
        "strategy_digest": sha256_str(f"subst:{tag}:strategy"),
    }


def build_ring_gauge_ok():
    cond = base_conditions("ring50", "ring-50mm-master", "cmm-lab-01")
    return {
        "calibrated_runs": {
            "conditions": dict(cond),
            "runs": make_runs(0x5011, 0.0500014, 0.00000035, 20, 3),
            "unit": "si:m",
        },
        "license": LICENSE,
        "production_runs": {
            "conditions": dict(cond),
            "runs": make_runs(0x5012, 0.0500021, 0.00000050, 5, 3),
            "unit": "si:m",
        },
        "reference": {
            "k": 2.0,
            "source_digest": sha256_str("subst:ring50:dcc-2026-001"),
            "source_kind": "dcc",
            "u": 6.0e-7,
            "unit": "si:m",
            "value": 0.0500012,
        },
        "schema": "subst-input/1",
        "title": "UNC.MODEL.SUBST.INPUT AC-1: calibrated 50.0012 mm ring gauge, U=0.6 um, k=2, 20 runs x 3 reps",
    }


def build_bore_ok():
    cond = base_conditions("bore25", "housing-bore-a", "cmm-shop-02")
    return {
        "calibrated_runs": {
            "conditions": dict(cond),
            "runs": make_runs(0x2511, 0.0250007, 0.00000040, 20, 3),
            "unit": "si:m",
        },
        "license": LICENSE,
        "production_runs": {
            "conditions": dict(cond),
            "runs": make_runs(0x2512, 0.0250015, 0.00000060, 4, 3),
            "unit": "si:m",
        },
        "reference": {
            "k": 2.0,
            "source_digest": sha256_str("subst:bore25:cert-2026-042"),
            "source_kind": "certificate",
            "u": 8.0e-7,
            "unit": "si:m",
            "value": 0.0250005,
        },
        "schema": "subst-input/1",
        "title": "UNC.MODEL.SUBST.INPUT: calibrated 25.0005 mm bore workpiece, U=0.8 um, k=2, certificate source",
    }


def build_insufficient_runs():
    doc = build_ring_gauge_ok()
    doc["title"] = "UNC.MODEL.SUBST.INPUT AC-2: 5 calibrated runs where default profile requires 20"
    doc["calibrated_runs"]["runs"] = doc["calibrated_runs"]["runs"][:5]
    return doc


def build_strategy_mismatch():
    doc = build_ring_gauge_ok()
    doc["title"] = "UNC.MODEL.SUBST.INPUT AC-3: production measurements taken with a different strategy_digest"
    doc["production_runs"]["conditions"]["strategy_digest"] = sha256_str("subst:ring50:strategy-different")
    return doc


def build_missing_k():
    doc = build_ring_gauge_ok()
    doc["title"] = "UNC.MODEL.SUBST.INPUT AC-4: calibrated reference missing coverage factor k"
    del doc["reference"]["k"]
    return doc


def build_readme() -> str:
    return (
        "# UNC.MODEL.SUBST.INPUT test fixtures\n"
        "\n"
        "- **Spec node:** `UNC.MODEL.SUBST.INPUT` (ISO 15530-3 calibrated-workpiece inputs & checks)\n"
        "- **License:** CC-BY-4.0 (synthetic ring-gauge and bore datasets in SI units)\n"
        "- **Generator:** `python3 fixtures/unc/model/subst/make_fixtures.py` (verify with `--check`)\n"
        "\n"
        "## Files\n"
        "\n"
        "- `ring_gauge_ok.json` (AC-1): calibrated ring `50.0012 mm` (`0.0500012` m), `U = 0.6 um` (`6.0e-7` m),\n"
        "  `k = 2`, DCC source, 20 runs of 3 repetitions on the calibrated workpiece and 5 production runs,\n"
        "  all under identical `Conditions`.\n"
        "- `bore_ok.json`: calibrated bore `25.0005 mm` (`0.0250005` m), `U = 0.8 um` (`8.0e-7` m), `k = 2`,\n"
        "  certificate source, 20 runs of 3 repetitions.\n"
        "- `ring_gauge_insufficient_runs.json` (AC-2): 5 calibrated runs where default profile requires 20.\n"
        "- `ring_gauge_strategy_mismatch.json` (AC-3): production measurements with a different `strategy_digest`.\n"
        "- `ring_gauge_missing_k.json` (AC-4): missing coverage factor `k` in calibrated reference.\n"
    )


def render(obj) -> str:
    return json.dumps(obj, indent=2, sort_keys=True) + "\n"


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = {
        HERE / "ring_gauge_ok.json": render(build_ring_gauge_ok()),
        HERE / "bore_ok.json": render(build_bore_ok()),
        HERE / "ring_gauge_insufficient_runs.json": render(build_insufficient_runs()),
        HERE / "ring_gauge_strategy_mismatch.json": render(build_strategy_mismatch()),
        HERE / "ring_gauge_missing_k.json": render(build_missing_k()),
        HERE / "README.md": build_readme(),
    }
    if check:
        ok = True
        for path, expected in files.items():
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_text(encoding="utf-8") != expected:
                print(f"MISMATCH: {path}", file=sys.stderr)
                ok = False
        return 0 if ok else 1
    HERE.mkdir(parents=True, exist_ok=True)
    for path, content in files.items():
        path.write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
