#!/usr/bin/env python3
"""Independent fixture generator for TRUST.CHAIN.LINKS (AC-2 and anchor/checkpoint cases).
CC-BY-4.0 project data. Stdlib only.

Reuses the repository's independent stdlib reference generators:
  - fixtures/pkg/make_fixtures.py   (deterministic Result Package ZIP writer)
  - fixtures/pkg/make_golden.py     (golden points.zip journal + ResultRecord model)
  - fixtures/chain/ckpt/make_fixtures.py (P-256 DSSE-signed linear-v1 checkpoint helper)

Writes deterministic Result Packages under fixtures/chain/tamper/:
  base.zip           24-entry points-tier package (indices 0..23) + signed checkpoint at seq 12 -> VALID
  modified.zip       one edit at entry 17 (header field altered without re-hashing) -> BROKEN, 17, HASH_MISMATCH
  removed.zip        one edit at entry 17 (entry 17 deleted) -> BROKEN, 17, LINK_MISMATCH + SEQUENCE_GAP
  inserted.zip       one edit at entry 17 (validly chained entry inserted with repeated seq 17) -> BROKEN, 17, SEQUENCE_REPEAT
  reordered.zip      one edit at entry 17 (entries 17 and 18 swapped) -> BROKEN, 17, LINK_MISMATCH + SEQUENCE_GAP
  truncated.zip      one edit at entry 17 (segment truncated mid-JSON inside entry 17) -> BROKEN, 17, TRUNCATED
  unanchored.zip     mid-journal slice starting at seq 6 with no anchoring checkpoint -> BROKEN, 0, UNANCHORED
  anchored-ckpt.zip  mid-journal slice starting at seq 6 anchored by signed journal/checkpoints/6.json -> VALID
  expect.json        expected verdicts, indices, finding kinds, and linkage counts

Run:
  python fixtures/chain/tamper/make_fixtures.py
  python fixtures/chain/tamper/make_fixtures.py --check
"""

import argparse
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


pkg_ref = load("tamper_pkg_ref", "pkg/make_fixtures.py")
golden_ref = load("tamper_golden_ref", "pkg/make_golden.py")
ckpt_ref = load("tamper_ckpt_ref", "chain/ckpt/make_fixtures.py")


def build_base_case():
    """Build a 24-entry points-tier case (indices 0..23) extending points.zip's model.

    Layout:
      0      : session_header (seq 1)
      1..12  : 12 io.points.import entries (hole-01 .. hole-12) (seq 2..13)
      13..23 : 11 gdt.fit_circle entries (hole-01 .. hole-11)   (seq 14..24)
               Note: entry 17 is gdt.fit_circle for hole-05 (seq 18),
                     entry 18 is gdt.fit_circle for hole-06 (seq 19).
    """
    c = golden_ref.Case("points")
    point_digests = []
    for k in range(1, 13):
        point_digests.append(c.import_points(f"hole-{k:02d}"))
    records = []
    for k in range(1, 12):
        h, _out = c.fit(point_digests[k - 1], f"hole-{k:02d}")
        records.append(
            golden_ref.record(
                f"hole-{k:02d}",
                "POINTS",
                golden_ref.FIT,
                h,
                [point_digests[k - 1]],
                value_mm=10.0 + k * 0.001,
            )
        )
    c.records = records
    assert len(c.journal.lines) == 24, len(c.journal.lines)
    return c


def signed_ckpt_bytes(seq: int, head: str) -> bytes:
    body = ckpt_ref.checkpoint_body(seq, head)
    body_bytes = golden_ref.cbytes(body)
    env = ckpt_ref.sign_p256_envelope(body_bytes, flip_last_sig_byte=False)
    return golden_ref.cbytes(env)


def build_zip(case, created_by: str, seg_text: str, checkpoints: dict[str, bytes] | None = None) -> bytes:
    records = sorted(case.records, key=lambda r: r["id"].encode("utf-8"))
    members = [
        {"path": "results/records.json", "media_type": "application/json", "body": golden_ref.canon(records)},
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
    if checkpoints:
        for path, raw in sorted(checkpoints.items()):
            members.append({
                "path": path,
                "media_type": "application/json",
                "body": raw.decode("utf-8"),
            })
    contents = {
        "created_by": created_by,
        "evidence_tier": case.tier,
        "profile_digest": pkg_ref.digest_of(pkg_ref.PROFILE_BODY),
        "kernel_manifest_digest": golden_ref.KERNEL_DIGEST,
        "result_count": len(records),
        "members": members,
    }
    _m, _s, zbytes = pkg_ref.build_package(contents)
    return zbytes


def generate_all() -> dict[str, bytes]:
    base = build_base_case()
    lines = list(base.journal.lines)
    # Entry 11 has seq "12"; place a signed checkpoint at seq 12 on the base & tamper fixtures.
    entry11_obj = json.loads(lines[11])
    assert entry11_obj["header"]["seq"] == "12"
    ckpt12 = {"journal/checkpoints/12.json": signed_ckpt_bytes(12, entry11_obj["header"]["hash"])}

    # Base untampered package (24 entries, indices 0..23).
    base_seg = "".join(line + "\n" for line in lines)
    base_zip = build_zip(base, "fid-chain tamper fixture generator 1 (base)", base_seg, ckpt12)

    # 1. modified.zip: mutate header.time.mono_ns at entry 17 without updating header.hash.
    mod_lines = list(lines)
    e17 = json.loads(mod_lines[17])
    e17["header"]["time"]["mono_ns"] = "18001"
    mod_lines[17] = golden_ref.canon(e17)
    modified_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (modified)",
        "".join(line + "\n" for line in mod_lines),
        ckpt12,
    )

    # 2. removed.zip: remove entry 17 (old entry 18 shifts into index 17).
    rem_lines = lines[:17] + lines[18:]
    removed_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (removed)",
        "".join(line + "\n" for line in rem_lines),
        ckpt12,
    )

    # 3. inserted.zip: insert a newly hashed entry at index 17 chained to entry 16 with repeated seq "17".
    e16_hash = json.loads(lines[16])["header"]["hash"]
    ins_header = {
        "schema_version": 1,
        "kind": "command",
        "stream": "main",
        "seq": "17",
        "prev": e16_hash,
        "branch": "main",
        "logical_parent": None,
        "time": {"utc": "2026-01-01T00:00:00Z", "mono_ns": "17500", "sync": "test"},
        "principal": {"kind": "script", "commit": golden_ref.label_digest("principal")},
        "ai_assistance": "none",
        "manifests": {"registry": golden_ref.label_digest("registry"), "kernel": golden_ref.KERNEL_DIGEST},
        "command": {
            "id": "project.note",
            "major": 1,
            "class": "W2",
            "determinism": "D0",
            "seed": "0",
            "algorithms": [],
            "inputs": [],
            "outputs": [],
            "args_commit": golden_ref.label_digest("inserted-args"),
            "events_commit": golden_ref.label_digest("inserted-events"),
        },
    }
    ins_header["hash"] = golden_ref.sha(golden_ref.cbytes(ins_header))
    ins_line = golden_ref.canon({"header": ins_header})
    ins_lines = lines[:17] + [ins_line] + lines[17:]
    inserted_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (inserted)",
        "".join(line + "\n" for line in ins_lines),
        ckpt12,
    )

    # 4. reordered.zip: swap entry 17 and entry 18.
    reo_lines = list(lines)
    reo_lines[17], reo_lines[18] = reo_lines[18], reo_lines[17]
    reordered_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (reordered)",
        "".join(line + "\n" for line in reo_lines),
        ckpt12,
    )

    # 5. truncated.zip: lines 0..16 intact, line 17 cut mid-JSON at EOF (no trailing newline).
    cut_line17 = lines[17][: len(lines[17]) // 2]
    trunc_seg = "".join(line + "\n" for line in lines[:17]) + cut_line17
    truncated_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (truncated)",
        trunc_seg,
        ckpt12,
    )

    # 6 & 7. Mid-journal slice starting at index 5 (seq 6, kind "command"):
    #    - unanchored.zip: no checkpoint at seq 6 -> UNANCHORED at index 0
    #    - anchored-ckpt.zip: signed checkpoint at seq 6 -> VALID
    slice_lines = lines[5:]
    slice_seg = "".join(line + "\n" for line in slice_lines)
    slice_first = json.loads(slice_lines[0])
    assert slice_first["header"]["seq"] == "6"
    assert slice_first["header"]["kind"] == "command"
    ckpt6 = {"journal/checkpoints/6.json": signed_ckpt_bytes(6, slice_first["header"]["hash"])}

    unanchored_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (unanchored)",
        slice_seg,
        None,
    )
    anchored_ckpt_zip = build_zip(
        base,
        "fid-chain tamper fixture generator 1 (anchored-ckpt)",
        slice_seg,
        ckpt6,
    )

    expect = {
        "schema": "trust-chain-links-tamper-expect/v1",
        "cases": {
            "base.zip": {
                "verdict": "VALID",
                "entries_verified": 24,
                "first_failing_index": None,
                "expected_kinds": [],
                "unlinked_count": 0,
            },
            "modified.zip": {
                "verdict": "BROKEN",
                "entries_verified": 17,
                "first_failing_index": 17,
                "expected_kinds": ["HASH_MISMATCH"],
                "unlinked_count": 7,
            },
            "removed.zip": {
                "verdict": "BROKEN",
                "entries_verified": 17,
                "first_failing_index": 17,
                "expected_kinds": ["LINK_MISMATCH", "SEQUENCE_GAP"],
                "unlinked_count": 7,
            },
            "inserted.zip": {
                "verdict": "BROKEN",
                "entries_verified": 17,
                "first_failing_index": 17,
                "expected_kinds": ["SEQUENCE_REPEAT"],
                "unlinked_count": 7,
            },
            "reordered.zip": {
                "verdict": "BROKEN",
                "entries_verified": 17,
                "first_failing_index": 17,
                "expected_kinds": ["LINK_MISMATCH", "SEQUENCE_GAP"],
                "unlinked_count": 7,
            },
            "truncated.zip": {
                "verdict": "BROKEN",
                "entries_verified": 17,
                "first_failing_index": 17,
                "expected_kinds": ["TRUNCATED"],
                "unlinked_count": 7,
            },
            "unanchored.zip": {
                "verdict": "BROKEN",
                "entries_verified": 0,
                "first_failing_index": 0,
                "expected_kinds": ["UNANCHORED"],
                "unlinked_count": 11,
            },
            "anchored-ckpt.zip": {
                "verdict": "VALID",
                "entries_verified": 19,
                "first_failing_index": None,
                "expected_kinds": [],
                "unlinked_count": 0,
            },
        },
    }

    return {
        "base.zip": base_zip,
        "modified.zip": modified_zip,
        "removed.zip": removed_zip,
        "inserted.zip": inserted_zip,
        "reordered.zip": reordered_zip,
        "truncated.zip": truncated_zip,
        "unanchored.zip": unanchored_zip,
        "anchored-ckpt.zip": anchored_ckpt_zip,
        "expect.json": (json.dumps(expect, indent=2, sort_keys=True) + "\n").encode("utf-8"),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description="Generate or check TRUST.CHAIN.LINKS tamper fixtures.")
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
        print(f"tamper fixtures up to date ({len(files)} files)")
        return 0

    HERE.mkdir(parents=True, exist_ok=True)
    for name, data in sorted(files.items()):
        (HERE / name).write_bytes(data)
    print(f"tamper fixtures written ({len(files)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
