#!/usr/bin/env python3
"""Independent stdlib-Python fixture generator for IO.RES.TRANS.VIEW (CC-BY-4.0).

Writes `fixtures/io/res/trans/view/mark.json` in RFC 8785 canonical JSON form for AC-1
(transcribed result from VendorX 4.2). Run with `--check` in CI to verify the committed
fixture has not drifted.
"""

from __future__ import annotations

import argparse
import json
from pathlib import Path

HERE = Path(__file__).resolve().parent


def canonical_json(obj: dict[str, object]) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def expected_files() -> dict[str, bytes]:
    mark = {
        "record_status": "transcribed",
        "source_software": "VendorX",
        "source_version": "4.2",
    }
    return {
        "mark.json": canonical_json(mark) + b"\n",
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or verify IO.RES.TRANS.VIEW fixtures.")
    ap.add_argument("--check", action="store_true", help="Verify committed fixtures match generator output.")
    args = ap.parse_args()

    files = expected_files()
    if args.check:
        for name, expected in sorted(files.items()):
            path = HERE / name
            if not path.is_file():
                raise SystemExit(f"missing fixture: {path}")
            actual = path.read_bytes()
            if actual != expected:
                raise SystemExit(f"fixture drift in {path}")
        print(f"ok ({len(files)} file checked)")
        return 0

    for name, content in sorted(files.items()):
        (HERE / name).write_bytes(content)
    print(f"wrote {len(files)} file(s)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
