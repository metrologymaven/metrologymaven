#!/usr/bin/env python3
"""Deterministic fixture generator for PLAT.MATRIX.POLICY.ROWS (AC-1..AC-5).

Licensed with the code (MIT OR Apache-2.0, D-001).

Writes synthetic test matrices and RC gate verdicts under fixtures/plat/matrix/:
  rows-three-ubuntu.toml       3 Tier 1 Ubuntu LTS x86_64 rows (AC-2 -> OutsideWindow on 22.04)
  rows-win11-arm64-t1.toml     Windows 11 aarch64 at Tier 1 (AC-3 -> OutsideWindow)
  rows-demoted-macos.toml      macOS 15 marked `demoted: 2026-11-22 eol` (AC-5 -> not expected in window)
  promote-old.toml             baseline matrix with Ubuntu 26.04 aarch64 at Tier 2 (AC-4)
  promote-new.toml             candidate matrix raising Ubuntu 26.04 aarch64 to Tier 1 (AC-4)
  promote-pass/rc1-pass.json   RC1 gate verdict with ubuntu-26.04-aarch64 build & full_test passed
  promote-pass/rc2-pass.json   RC2 gate verdict with ubuntu-26.04-aarch64 build & full_test passed
  promote-fail/rc1-pass.json   RC1 gate verdict passed
  promote-fail/rc2-fail.json   RC2 gate verdict with ubuntu-26.04-aarch64-full_test failed

Usage:
    python3 fixtures/plat/matrix/make_policy_rows_fixtures.py
    python3 fixtures/plat/matrix/make_policy_rows_fixtures.py --check
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]


def build_files() -> dict[str, bytes]:
    shipped = (REPO / "platform" / "support-matrix.toml").read_text(encoding="utf-8")

    # AC-2: shipped matrix + a third older Tier 1 Ubuntu LTS (22.04).
    three_ubuntu = (
        shipped
        + '\n[[row]]\nos = "ubuntu"\nversion_min = "22.04"\nversion_max = "22.05"\narch = "x86_64"\n'
        + 'tier = "tier1"\nsince_phase = "P1a"\nnotes = "fixture: third older Ubuntu LTS [unverified]"\n'
    )

    # AC-3: Windows 11 aarch64 raised to Tier 1.
    win11_arm64_t1 = shipped.replace(
        'os = "windows"\nversion_min = "10.0.22000"\nversion_max = "10.0.27000"\narch = "aarch64"\ntier = "tier3"',
        'os = "windows"\nversion_min = "10.0.22000"\nversion_max = "10.0.27000"\narch = "aarch64"\ntier = "tier1"',
        1,
    )
    assert win11_arm64_t1 != shipped

    # AC-5: macOS 15 row demoted via `demoted:` note (excluded from expected Tier 1 window).
    demoted_macos = shipped.replace(
        'notes = "macOS 15 aarch64 Tier 1 (second newest macOS major per D-010 window of 2); exact version at P1a release [unverified]."',
        'notes = "macOS 15 aarch64; demoted: 2026-11-22 eol [unverified]."',
        1,
    )
    assert demoted_macos != shipped

    # AC-4: promote-old.toml vs promote-new.toml raising ubuntu 26.04 aarch64 from tier2 -> tier1.
    promote_old = shipped
    promote_new = shipped.replace(
        'os = "ubuntu"\nversion_min = "26.04"\nversion_max = "26.05"\narch = "aarch64"\ntier = "tier2"',
        'os = "ubuntu"\nversion_min = "26.04"\nversion_max = "26.05"\narch = "aarch64"\ntier = "tier1"',
        1,
    )
    assert promote_new != promote_old

    def jbytes(obj: dict) -> bytes:
        return (json.dumps(obj, sort_keys=True, indent=2) + "\n").encode("utf-8")

    rc1_pass = jbytes({
        "verdict": "PASS",
        "blocking": [],
        "tier2_notes": [],
        "jobs": [
            {"id": "ubuntu-26.04-aarch64-build", "status": "passed"},
            {"id": "ubuntu-26.04-aarch64-full_test", "status": "passed"},
        ],
    })
    rc2_pass = jbytes({
        "verdict": "PASS",
        "blocking": [],
        "tier2_notes": [],
        "jobs": [
            {"id": "ubuntu-26.04-aarch64-build", "status": "passed"},
            {"id": "ubuntu-26.04-aarch64-full_test", "status": "passed"},
        ],
    })
    rc2_fail = jbytes({
        "verdict": "BLOCK",
        "blocking": [["ubuntu-26.04-aarch64-full_test", "failed"]],
        "tier2_notes": [],
        "jobs": [
            {"id": "ubuntu-26.04-aarch64-build", "status": "passed"},
            {"id": "ubuntu-26.04-aarch64-full_test", "status": "failed"},
        ],
    })

    return {
        "rows-three-ubuntu.toml": three_ubuntu.encode("utf-8"),
        "rows-win11-arm64-t1.toml": win11_arm64_t1.encode("utf-8"),
        "rows-demoted-macos.toml": demoted_macos.encode("utf-8"),
        "promote-old.toml": promote_old.encode("utf-8"),
        "promote-new.toml": promote_new.encode("utf-8"),
        "promote-pass/rc1-pass.json": rc1_pass,
        "promote-pass/rc2-pass.json": rc2_pass,
        "promote-fail/rc1-pass.json": rc1_pass,
        "promote-fail/rc2-fail.json": rc2_fail,
    }


def main() -> int:
    files = build_files()
    if "--check" in sys.argv[1:]:
        mismatches = []
        for rel, want in sorted(files.items()):
            p = HERE / rel
            if not p.is_file() or p.read_bytes() != want:
                mismatches.append(rel)
        if mismatches:
            print("FAIL: policy-rows fixture drift: " + ", ".join(mismatches), file=sys.stderr)
            return 1
        print(f"policy-rows fixtures: ok ({len(files)} files)")
        return 0

    for rel, data in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"policy-rows fixtures: wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
