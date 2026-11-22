#!/usr/bin/env python3
"""Independent stdlib reference generator for TRUST.ESIG.MEANING fixtures (CC-BY-4.0).

Writes and checks:
- `fixtures/profile/bad-meaning-collision.json`: tenant file colliding with `APPROVED` (AC-3).
- `fixtures/profile/bad-meaning-syntax.json`: tenant file using invalid lowercase code `x_lower` (AC-3).
- `fixtures/trust/esig/meaning/expected_list.json`: independent expected outputs for AC-1 (Lean)
  and AC-2 (`fixtures/profile/regulated-with-xqa.json`).

Usage:
    python3 fixtures/trust/esig/meaning/make_fixtures.py
    python3 fixtures/trust/esig/meaning/make_fixtures.py --check
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent.parent.parent
PROFILE_DIR = ROOT / "fixtures" / "profile"

BUILTINS = [
    {
        "code": "AUTHORED",
        "description": "The signer created or prepared the record for review.",
        "label": "Authored",
        "origin": "builtin",
    },
    {
        "code": "REVIEWED",
        "description": "The signer checked the record for technical accuracy and completeness.",
        "label": "Reviewed",
        "origin": "builtin",
    },
    {
        "code": "APPROVED",
        "description": "The signer formally approved the record as fit for its intended purpose.",
        "label": "Approved",
        "origin": "builtin",
    },
    {
        "code": "RELEASED",
        "description": "The signer authorized release of the record for operational use.",
        "label": "Released",
        "origin": "builtin",
    },
]


def pretty(obj: object) -> str:
    return json.dumps(obj, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def outputs() -> dict[Path, str]:
    bad_collision = {
        "tenant_meanings": [
            {
                "code": "APPROVED",
                "description": "Attempts to redefine a built-in meaning code.",
                "label": "Duplicate approved",
            }
        ]
    }
    bad_syntax = {
        "tenant_meanings": [
            {
                "code": "x_lower",
                "description": "Uses lowercase characters forbidden by the meaning code grammar.",
                "label": "Lowercase code",
            }
        ]
    }

    xqa = json.loads((PROFILE_DIR / "regulated-with-xqa.json").read_text(encoding="utf-8"))
    xqa_entries = [
        {
            "code": m["code"],
            "description": m["description"],
            "label": m["label"],
            "origin": "profile",
        }
        for m in sorted(xqa["tenant_meanings"], key=lambda item: item["code"])
    ]

    expected = {
        "ac1_lean": {"meanings": BUILTINS},
        "ac2_regulated_with_xqa": {"meanings": BUILTINS + xqa_entries},
    }

    return {
        PROFILE_DIR / "bad-meaning-collision.json": pretty(bad_collision),
        PROFILE_DIR / "bad-meaning-syntax.json": pretty(bad_syntax),
        HERE / "expected_list.json": pretty(expected),
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad: list[str] = []
    for path, want in sorted(outputs().items()):
        if check:
            if not path.is_file() or path.read_text(encoding="utf-8") != want:
                bad.append(str(path.relative_to(ROOT)))
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_text(want, encoding="utf-8")
    if bad:
        print("stale TRUST.ESIG.MEANING fixtures: " + ", ".join(bad), file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
