#!/usr/bin/env python3
"""Generate and verify synthetic EOL fixtures under fixtures/plat/matrix/ for PLAT.MATRIX.POLICY.EOL.

Licensed with the code (MIT OR Apache-2.0, D-001). Invented fixture dates.

Usage:
    python3 fixtures/plat/matrix/make_eol_fixtures.py
    python3 fixtures/plat/matrix/make_eol_fixtures.py --check
"""

from __future__ import annotations

import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent


def build_files() -> dict[str, str]:
    # AC-1 & AC-4: matches fixtures/plat/matrix/valid.toml.
    # Windows 10 (10.0.19041) has eol already passed (2025-10-14) but extended_until = 2028-10-10.
    eol_valid = """# Synthetic EOL fixture for PLAT.MATRIX.POLICY.EOL AC-1 and AC-4 (invented dates).
# MIT OR Apache-2.0 (D-001).
schema_version = 1

[[entry]]
os = "windows"
version = "10.0.19041"
eol = "2025-10-14"
extended_until = "2028-10-10"
source = "https://example.invalid/windows-10-esu"

[[entry]]
os = "windows"
version = "10.0.22000"
eol = "2029-10-09"
source = "https://example.invalid/windows-11"

[[entry]]
os = "ubuntu"
version = "24.04"
eol = "2029-05-31"
source = "https://example.invalid/ubuntu-2404"

[[entry]]
os = "macos"
version = "15"
eol = "2027-09-30"
source = "https://example.invalid/macos-15"
"""

    # AC-2 & AC-3: macOS 15 ends 2026-03-01 (warn on 2026-01-15, fail PastEndOfSupport on 2026-03-02).
    eol_ending = """# Synthetic EOL fixture for PLAT.MATRIX.POLICY.EOL AC-2 and AC-3 (macos 15 ends 2026-03-01).
# MIT OR Apache-2.0 (D-001).
schema_version = 1

[[entry]]
os = "windows"
version = "10.0.19041"
eol = "2025-10-14"
extended_until = "2028-10-10"
source = "https://example.invalid/windows-10-esu"

[[entry]]
os = "windows"
version = "10.0.22000"
eol = "2029-10-09"
source = "https://example.invalid/windows-11"

[[entry]]
os = "ubuntu"
version = "24.04"
eol = "2029-05-31"
source = "https://example.invalid/ubuntu-2404"

[[entry]]
os = "macos"
version = "15"
eol = "2026-03-01"
source = "https://example.invalid/macos-15"
"""

    # AC-3 second half: same as valid.toml, with `demoted: 2026-03-02 eol` on macos 15 aarch64.
    valid_text = (ROOT / "valid.toml").read_text(encoding="utf-8")
    demoted_matrix = valid_text.replace(
        'os = "macos"\nversion_min = "15"\nversion_max = "16"\narch = "aarch64"\ntier = "tier1"\nsince_phase = "P1a"\nnotes = "fixture"',
        'os = "macos"\nversion_min = "15"\nversion_max = "16"\narch = "aarch64"\ntier = "tier1"\nsince_phase = "P1a"\nnotes = "fixture; demoted: 2026-03-02 eol"',
        1,
    )
    assert "demoted: 2026-03-02 eol" in demoted_matrix

    # AC-5: valid.toml plus a Tier 2 Ubuntu 22.04 row missing from eol-valid.toml.
    missing_t2_matrix = (
        valid_text
        + '\n[[row]]\nos = "ubuntu"\nversion_min = "22.04"\nversion_max = "22.10"\narch = "x86_64"\ntier = "tier2"\nsince_phase = "P1b"\nnotes = "fixture: tier2 without EOL entry"\n'
    )

    return {
        "eol-valid.toml": eol_valid,
        "eol-ending-2026-03-01.toml": eol_ending,
        "eol-demoted-matrix.toml": demoted_matrix,
        "eol-missing-t2-matrix.toml": missing_t2_matrix,
    }


def main() -> int:
    files = build_files()
    if "--check" in sys.argv[1:]:
        errors: list[str] = []
        for rel, want in sorted(files.items()):
            p = ROOT / rel
            if not p.is_file():
                errors.append(f"missing fixture file: {rel}")
                continue
            if p.read_text(encoding="utf-8") != want:
                errors.append(f"fixture drift: {rel}")
        if errors:
            for e in errors:
                print(e, file=sys.stderr)
            return 1
        return 0

    for rel, content in sorted(files.items()):
        (ROOT / rel).write_text(content, encoding="utf-8")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
