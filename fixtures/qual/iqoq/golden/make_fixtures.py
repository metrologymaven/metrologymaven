#!/usr/bin/env python3
"""Independent fixture generator for QUAL.IQOQ.GOLDEN (stdlib only; CC-BY-4.0).

Builds `fixtures/qual/iqoq/golden/plate.fid` (a synthetic datum-plate inspection run on
branch `main`, entries 1..=50, with 6 committed ResultRecords in range 10..=42 and
non-result entries outside/between) plus `expected.json` and `README.md`.

Usage:
  python3 fixtures/qual/iqoq/golden/make_fixtures.py          # write files
  python3 fixtures/qual/iqoq/golden/make_fixtures.py --check  # verify files match byte-for-byte
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).parent
LICENSE = "CC-BY-4.0"
PRODUCT_VERSION = "1.4.0"


def sha256_str(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canonical_json(obj) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def build_fixtures():
    kernel_digest = sha256_str(b"fid-qual:golden:plate:kernel-manifest:1.4.0")

    # 6 synthetic fits on a datum plate committed in range 10..=42:
    result_seqs = {
        12: ("fit.plane@1", "plate:datum_a:lsq_plane"),
        18: ("fit.circle@1", "plate:hole_1:lsq_circle"),
        23: ("fit.circle@1", "plate:hole_2:lsq_circle"),
        29: ("fit.circle@1", "plate:hole_3:lsq_circle"),
        35: ("fit.circle@1", "plate:hole_4:lsq_circle"),
        41: ("gdt.evaluate@1", "plate:pattern_position:iso1101"),
    }

    entries = []
    range_digests = []
    for seq in range(1, 51):
        if seq in result_seqs:
            cmd_id, tag = result_seqs[seq]
            rd = sha256_str(f"plate.fid:result:{seq}:{tag}".encode("utf-8"))
            if 10 <= seq <= 42:
                range_digests.append(rd)
        else:
            cmd_id = "project.note@1" if seq < 10 else ("view.bookmark@1" if seq > 42 else "align.step@1")
            rd = None

        entry = {
            "branch": "main",
            "command_id": cmd_id,
            "kernel_manifest_digest": kernel_digest,
            "product_version": PRODUCT_VERSION,
            "seq": seq,
        }
        if rd is not None:
            entry["result_record_digest"] = rd
        entries.append(entry)

    assert len(range_digests) == 6
    expected_digest = sha256_str(canonical_json(range_digests))

    plate_doc = {
        "entries": entries,
        "format": "fid-qual-golden-project/1",
        "goldens": [],
        "license": LICENSE,
        "title": "QUAL.IQOQ.GOLDEN synthetic datum-plate project (6 ResultRecords in main:10..=42)",
    }

    expected_doc = {
        "branch": "main",
        "empty_range": {"from_seq": 1, "to_seq": 9},
        "expected_digest": expected_digest,
        "from_seq": 10,
        "kernel_manifest_digest": kernel_digest,
        "license": LICENSE,
        "product_version": PRODUCT_VERSION,
        "result_count": len(range_digests),
        "result_digests": range_digests,
        "to_seq": 42,
    }

    readme = (
        "# QUAL.IQOQ.GOLDEN test fixtures\n"
        "\n"
        "- **Spec node:** `QUAL.IQOQ.GOLDEN` (`validate.register_golden@1`)\n"
        "- **License:** CC-BY-4.0 (synthetic datum-plate inspection project)\n"
        "- **Generator:** `python3 fixtures/qual/iqoq/golden/make_fixtures.py` (verify with `--check`)\n"
        "\n"
        "## Files\n"
        "\n"
        "- `plate.fid`: synthetic project journal on branch `main` (`seq` 1..=50) containing 6 committed\n"
        "  `ResultRecord` entries inside range `10..=42` (`seq` 12, 18, 23, 29, 35, 41) and non-result\n"
        "  entries in `1..=9` and `43..=50`.\n"
        "- `expected.json`: independently computed SHA-256 of the RFC 8785 canonical JSON array of the\n"
        "  6 `ResultRecord` digests in `main:10..=42`.\n"
    )

    return {
        HERE / "plate.fid": json.dumps(plate_doc, indent=2, sort_keys=True) + "\n",
        HERE / "expected.json": json.dumps(expected_doc, indent=2, sort_keys=True) + "\n",
        HERE / "README.md": readme,
    }


def main() -> int:
    check = "--check" in sys.argv[1:]
    files = build_fixtures()
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
