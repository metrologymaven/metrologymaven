#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for TRUST.PROFILE.GATE.QUERY (CC-BY-4.0).

Derives `fixtures/profile/gates/{lean,regulated,assistive_only}.json` directly from
`fixtures/profile/builtin/{lean,regulated,assistive_only}.json` using the fixed gate table
of REQ-TRUST.PROFILE.GATE.QUERY-004. Run with `--check` in CI to verify no drift.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent
BUILTIN_DIR = HERE.parent / "builtin"


def canonical_json(obj: dict[str, object]) -> bytes:
    return (json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False) + "\n").encode("utf-8")


def gates_from_builtin(doc: dict[str, object]) -> dict[str, object]:
    params = doc["params"]
    assert isinstance(params, dict)
    return {
        "agent_direct_write": "enabled" if params["agent_writes"] == "branch" else "disabled",
        "require_signature_before_release": bool(params["release_requires_signatures"]),
        "required_meanings": list(params["required_meanings"]),
        "software_key_signing": "enabled" if params["allow_software_keys"] else "disabled",
    }


def expected_files() -> dict[str, bytes]:
    out: dict[str, bytes] = {}
    for name in ("lean", "regulated", "assistive_only"):
        doc = json.loads((BUILTIN_DIR / f"{name}.json").read_text(encoding="utf-8"))
        out[f"{name}.json"] = canonical_json(gates_from_builtin(doc))
    return out


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or check TRUST.PROFILE.GATE.QUERY gate fixtures.")
    ap.add_argument("--check", action="store_true", help="Verify committed fixtures match generator output.")
    args = ap.parse_args()

    files = expected_files()
    if args.check:
        for fname, expected in sorted(files.items()):
            path = HERE / fname
            if not path.is_file():
                raise SystemExit(f"missing fixture: {path}")
            if path.read_bytes() != expected:
                raise SystemExit(f"drift in {path}")
        print(f"ok ({len(files)} files checked)")
        return 0

    for fname, data in sorted(files.items()):
        (HERE / fname).write_bytes(data)
    print(f"wrote {len(files)} files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
