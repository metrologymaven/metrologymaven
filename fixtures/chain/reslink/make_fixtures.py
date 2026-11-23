#!/usr/bin/env python3
"""Independent fixture generator for TRUST.CHAIN.RESLINK (AC-1..AC-4). CC-BY-4.0 project data.

Stdlib only. Reuses the repository's independent stdlib reference generators:
  - fixtures/pkg/make_fixtures.py (deterministic Result Package ZIP writer)
  - fixtures/pkg/make_golden.py   (golden points.zip model: journal, points, ResultRecords)

Writes deterministic Result Packages under fixtures/chain/reslink/:
  foreign-hash.zip   copy of golden/points.zip with hole-2's journal_entry replaced by a
                     foreign digest not in the segment -> hole-2 UNLINKED (AC-2)
  broken-at-5.zip    11-entry journal where entry 5's stored hash is tampered (chain breaks at
                     index 5); record hole-early links to entry 4 (< 5 -> LINKED) and record
                     hole-late links to entry 9 (>= 5 -> UNLINKED) (AC-3)
  wrong-entry.zip    copy of golden/points.zip with hole-2 linked to entry 1 (valid
                     io.points.import entry, not result-producing) and hole-3 linked to entry 4
                     (valid gdt.fit_circle entry for hole-1, wrong inputs) -> MISLINKED (AC-4)
  expected.json      expected per-record LinkResult summaries for AC-1..AC-4

Run:
  python fixtures/chain/reslink/make_fixtures.py
  python fixtures/chain/reslink/make_fixtures.py --check
"""

import argparse
import hashlib
import importlib.util
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIXTURES = HERE.parents[1]


def load(name: str, rel: str):
    spec = importlib.util.spec_from_file_location(name, FIXTURES / rel)
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


pkg_ref = load("reslink_pkg_ref", "pkg/make_fixtures.py")
golden_ref = load("reslink_golden_ref", "pkg/make_golden.py")


def sha256_prefixed(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def canon_str(obj) -> str:
    return golden_ref.canon(obj)


def canon_bytes(obj) -> bytes:
    return canon_str(obj).encode("utf-8")


def build_zip(case, created_by: str, override_segment_text=None) -> bytes:
    records = sorted(case.records, key=lambda r: r["id"].encode("utf-8"))
    seg_text = override_segment_text if override_segment_text is not None else case.journal.text()
    members = [
        {"path": "results/records.json", "media_type": "application/json", "body": canon_str(records)},
        {"path": "results/results.qif", "media_type": "application/xml", "body": golden_ref.QIF_PLACEHOLDER},
        {"path": "journal/segment.jsonl", "media_type": "application/x-ndjson", "body": seg_text},
        {"path": "kernel/manifest.json", "media_type": "application/json", "body": golden_ref.KERNEL},
        {"path": "profile/profile.json", "media_type": "application/json", "body": pkg_ref.PROFILE_BODY},
    ]
    for d, body in sorted(case.points.items()):
        members.append({
            "path": "points/" + d[len("sha256:"):] + ".pts",
            "media_type": "text/plain",
            "body": body,
        })
    contents = {
        "created_by": created_by,
        "evidence_tier": case.tier,
        "profile_digest": pkg_ref.digest_of(pkg_ref.PROFILE_BODY),
        "kernel_manifest_digest": golden_ref.KERNEL_DIGEST,
        "result_count": len(records),
        "members": members,
    }
    _mbytes, _sbytes, zbytes = pkg_ref.build_package(contents)
    return zbytes


def case_foreign_hash():
    """AC-2: points.zip with hole-2's journal_entry replaced by a foreign digest."""
    c = golden_ref.case_points()
    foreign = sha256_prefixed(b"TRUST.CHAIN.RESLINK foreign journal entry hash")
    for r in c.records:
        if r["id"] == "hole-2":
            r["journal_entry"] = foreign
    return build_zip(c, "fid-chain reslink fixture generator 1 (foreign-hash)"), foreign


def case_broken_at_5():
    """AC-3: 11-entry segment tampered at index 5; hole-early links to index 4, hole-late to index 9."""
    c = golden_ref.Case("points")
    # Indices:
    #   0: session_header
    #   1: io.points.import (early-1)
    #   2: io.points.import (early-2)
    #   3: io.points.import (mid)
    #   4: gdt.fit_circle   (early-1 -> hole-early, BEFORE break at 5)
    #   5: io.points.import (late-1)  <-- TAMPERED IN SEGMENT (chain breaks at index 5)
    #   6: io.points.import (late-2)
    #   7: gdt.fit_circle   (early-2)
    #   8: gdt.fit_circle   (mid)
    #   9: gdt.fit_circle   (late-1 -> hole-late, AFTER break at 5)
    #  10: gdt.fit_circle   (late-2)
    p_early1 = c.import_points("early-1")
    p_early2 = c.import_points("early-2")
    p_mid = c.import_points("mid")
    h_early, _ = c.fit(p_early1, "early-1")       # index 4
    p_late1 = c.import_points("late-1")           # index 5 (will be tampered)
    p_late2 = c.import_points("late-2")           # index 6
    c.fit(p_early2, "early-2")                    # index 7
    c.fit(p_mid, "mid")                           # index 8
    h_late, _ = c.fit(p_late1, "late-1")          # index 9
    c.fit(p_late2, "late-2")                      # index 10

    assert len(c.journal.lines) == 11, len(c.journal.lines)
    c.records = [
        golden_ref.record("hole-early", "POINTS", golden_ref.FIT, h_early, [p_early1], value_mm=10.001),
        golden_ref.record("hole-late", "POINTS", golden_ref.FIT, h_late, [p_late1], value_mm=10.009),
    ]

    # Tamper entry at index 5 by replacing its stored header hash so the chain breaks at index 5.
    tampered_lines = list(c.journal.lines)
    entry5 = json.loads(tampered_lines[5])
    entry5["header"]["hash"] = sha256_prefixed(b"TRUST.CHAIN.RESLINK tampered entry 5")
    tampered_lines[5] = canon_str(entry5)
    seg_text = "".join(line + "\n" for line in tampered_lines)

    return build_zip(c, "fid-chain reslink fixture generator 1 (broken-at-5)", override_segment_text=seg_text)


def case_wrong_entry():
    """AC-4: points.zip with hole-2 linked to entry 1 (io.points.import) and hole-3 linked to entry 4 (hole-1's fit)."""
    c = golden_ref.case_points()
    entry1_hash = json.loads(c.journal.lines[1])["header"]["hash"]
    entry4_hash = json.loads(c.journal.lines[4])["header"]["hash"]
    for r in c.records:
        if r["id"] == "hole-2":
            r["journal_entry"] = entry1_hash
        elif r["id"] == "hole-3":
            r["journal_entry"] = entry4_hash
    return build_zip(c, "fid-chain reslink fixture generator 1 (wrong-entry)")


def generate_all() -> dict[str, bytes]:
    foreign_zip, foreign_digest = case_foreign_hash()
    broken_zip = case_broken_at_5()
    wrong_zip = case_wrong_entry()

    expected = {
        "schema": "trust-chain-reslink-expected/v1",
        "foreign_digest": foreign_digest,
        "cases": {
            "points.zip": [
                {"record_id": "hole-1", "status": "LINKED", "entry_index": 4},
                {"record_id": "hole-2", "status": "LINKED", "entry_index": 5},
                {"record_id": "hole-3", "status": "LINKED", "entry_index": 6},
            ],
            "foreign-hash.zip": [
                {"record_id": "hole-1", "status": "LINKED", "entry_index": 4},
                {"record_id": "hole-2", "status": "UNLINKED", "entry_index": None},
                {"record_id": "hole-3", "status": "LINKED", "entry_index": 6},
            ],
            "broken-at-5.zip": {
                "first_failing_index": 5,
                "results": [
                    {"record_id": "hole-early", "status": "LINKED", "entry_index": 4},
                    {"record_id": "hole-late", "status": "UNLINKED", "entry_index": None},
                ],
            },
            "wrong-entry.zip": [
                {"record_id": "hole-1", "status": "LINKED", "entry_index": 4},
                {
                    "record_id": "hole-2",
                    "status": "MISLINKED",
                    "entry_index": 1,
                    "reason": "not_result_producing_command",
                },
                {
                    "record_id": "hole-3",
                    "status": "MISLINKED",
                    "entry_index": 4,
                    "reason": "input_digest_mismatch",
                },
            ],
        },
    }

    return {
        "foreign-hash.zip": foreign_zip,
        "broken-at-5.zip": broken_zip,
        "wrong-entry.zip": wrong_zip,
        "expected.json": (json.dumps(expected, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or check TRUST.CHAIN.RESLINK fixtures.")
    ap.add_argument("--check", action="store_true", help="Fail if committed fixtures differ from regenerated output.")
    args = ap.parse_args()

    files = generate_all()
    if args.check:
        mismatches = []
        for name, want in sorted(files.items()):
            path = HERE / name
            if not path.is_file() or path.read_bytes() != want:
                mismatches.append(name)
        if mismatches:
            sys.stderr.write("fixture mismatch: " + ", ".join(mismatches) + "\n")
            return 1
        print(f"reslink fixtures up to date ({len(files)} files)")
        return 0

    HERE.mkdir(parents=True, exist_ok=True)
    for name, data in sorted(files.items()):
        (HERE / name).write_bytes(data)
    print(f"reslink fixtures written ({len(files)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
