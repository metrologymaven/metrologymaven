#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for UNC.RDL.EXT.LIC (AC-1..AC-3).

Generates `fixtures/rdl/ext/lic/table.csv` (12 rows: licence, evidence_present, expected_visibility)
under the `rdl-redist@1` redistribution allowlist.

Usage: python3 fixtures/rdl/ext/lic/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[4]
HERE = Path(__file__).resolve().parent

REDIST_V1 = {
    "CC0-1.0",
    "CC-BY-4.0",
    "CC-BY-SA-4.0",
    "MIT",
    "Apache-2.0",
    "BSD-3-Clause",
    "ODbL-1.0",
    "PDDL-1.0",
}

CASES = [
    ("CC-BY-4.0", True),
    ("CC-BY-4.0", False),
    ("CC0-1.0", True),
    ("CC0-1.0", False),
    ("MIT", True),
    ("Apache-2.0", True),
    ("BSD-3-Clause", False),
    ("CC-BY-NC-4.0", True),
    ("CC-BY-NC-4.0", False),
    ("GPL-3.0-only", True),
    ("LicenseRef-Proprietary", True),
    ("LicenseRef-Unconfirmed-PD", False),
]


def build_table() -> bytes:
    lines = ["licence,evidence_present,expected_visibility"]
    for lic, has_ev in CASES:
        vis = "public" if (lic in REDIST_V1 and has_ev) else "reference_only"
        lines.append(f"{lic},{'true' if has_ev else 'false'},{vis}")
    return ("\n".join(lines) + "\n").encode("utf-8")


def main() -> int:
    target = HERE / "table.csv"
    want = build_table()
    if "--check" in sys.argv[1:]:
        if not target.is_file() or target.read_bytes() != want:
            print(f"FAIL: stale fixture {target.relative_to(ROOT)}")
            return 1
        print("fixtures/rdl/ext/lic: ok")
        return 0
    target.parent.mkdir(parents=True, exist_ok=True)
    target.write_bytes(want)
    print(f"wrote {target}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
