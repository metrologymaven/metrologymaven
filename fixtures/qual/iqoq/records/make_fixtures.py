#!/usr/bin/env python3
"""Fixture generator and independent oracle for spec node QUAL.IQOQ.RECORD (Metrology Maven).

CC-BY-4.0 synthetic test data. Every digest, check and record is synthetic and carries no
standards text or real measurement data.

Writes (or, with --check, regenerates and compares byte for byte):
  v1.4-qualified.fid   AC-1/AC-2/AC-3/AC-5 synthetic Record Journal containing one IQ PASS
                       event (`validate.run_iq@1`) and one OQ PASS event (`validate.run_oq@1`)
                       for product version 1.4.0, with independently computed RFC 8785
                       canonical JSON result digests and chain-linked record IDs.
  expected.json        Independent oracle outputs for AC-1 (1.4.0 -> QUALIFIED), AC-2
                       (1.5.0 -> UNQUALIFIED_NEW_VERSION), and AC-3 (record IDs and SHA-256
                       digests of canonical record bodies).

Run (stdlib only, Python 3.9+):
  python fixtures/qual/iqoq/records/make_fixtures.py           # rewrite
  python fixtures/qual/iqoq/records/make_fixtures.py --check   # fail if stale
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GENESIS_PREV = "sha256:" + ("0" * 64)


def canonical_bytes(obj) -> bytes:
    """RFC 8785 canonical JSON for ASCII strings, booleans, ints, arrays and dicts."""
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256_prefixed(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def synthetic_digest(label: str) -> str:
    return sha256_prefixed(f"metrologymaven:qual-iqoq-record-fixture:{label}".encode("utf-8"))


def make_event(seq: int, prev_id: str, command_id: str, body: dict) -> dict:
    result_digest = sha256_prefixed(canonical_bytes(body))
    id_preimage = {
        "command_id": command_id,
        "prev": prev_id,
        "result_digest": result_digest,
        "seq": seq,
    }
    event_id = sha256_prefixed(canonical_bytes(id_preimage))
    return {
        "body": body,
        "command_id": command_id,
        "id": event_id,
        "result_digest": result_digest,
    }


def build_fixtures() -> dict[str, bytes]:
    kernel_manifest_digest = synthetic_digest("v1.4.0/kernel-manifest")
    evidence_statement_digest = synthetic_digest("v1.4.0/evidence-statement")
    env_digest = synthetic_digest("v1.4.0/doctor-env-redacted")
    rdl_subset_digest = synthetic_digest("v1.4.0/rdl-subset-small")

    iq_record = {
        "checks": [
            {"detail": "MATCH", "id": "artefact:fid-cli", "verdict": "PASS"},
            {"detail": "MATCH", "id": "artefact:fid-kernel", "verdict": "PASS"},
            {"detail": "verified against installed trust root", "id": "sig:sums", "verdict": "PASS"},
            {"detail": "verified against installed trust root", "id": "sig:evidence", "verdict": "PASS"},
            {"detail": "kernel manifest digest matches signed evidence statement", "id": "digest:kernel_manifest", "verdict": "PASS"},
        ],
        "env_digest": env_digest,
        "evidence_statement_digest": evidence_statement_digest,
        "kernel_manifest_digest": kernel_manifest_digest,
        "kind": "iq",
        "product_version": "1.4.0",
        "schema": "qual-record/1",
        "verdict": "PASS",
    }

    oq_record = {
        "checks": [
            {"detail": "residual within REP-1", "id": "rdl:fit-circle-lsq-001", "verdict": "PASS"},
            {"detail": "residual within REP-1", "id": "rdl:fit-plane-lsq-001", "verdict": "PASS"},
            {"detail": "IDENTICAL under rep-1@1.0.0", "id": "golden:synthetic-001", "verdict": "PASS"},
        ],
        "env_digest": env_digest,
        "evidence_statement_digest": evidence_statement_digest,
        "goldens": [
            {
                "exceedances": [],
                "golden_id": "synthetic-001",
                "outcome": "IDENTICAL",
            }
        ],
        "kernel_manifest_digest": kernel_manifest_digest,
        "kind": "oq",
        "product_version": "1.4.0",
        "rdl_subset_digest": rdl_subset_digest,
        "schema": "qual-record/1",
        "verdict": "PASS",
    }

    iq_event = make_event(1, GENESIS_PREV, "validate.run_iq@1", iq_record)
    oq_event = make_event(2, iq_event["id"], "validate.run_oq@1", oq_record)

    journal = {
        "events": [iq_event, oq_event],
        "format": "fid-qual-journal/1",
    }

    expected = {
        "ac1_status_1_4_0": {
            "iq_record": iq_event["id"],
            "oq_record": oq_event["id"],
            "product_version": "1.4.0",
            "status": "QUALIFIED",
        },
        "ac2_status_1_5_0": {
            "product_version": "1.5.0",
            "status": "UNQUALIFIED_NEW_VERSION",
        },
        "iq_record_id": iq_event["id"],
        "iq_result_digest": iq_event["result_digest"],
        "oq_record_id": oq_event["id"],
        "oq_result_digest": oq_event["result_digest"],
    }

    return {
        "v1.4-qualified.fid": canonical_bytes(journal) + b"\n",
        "expected.json": canonical_bytes(expected) + b"\n",
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    extra = [a for a in argv[1:] if a != "--check"]
    if extra:
        sys.stderr.write(f"usage: {argv[0]} [--check]\n")
        return 2

    files = build_fixtures()
    if check:
        stale = []
        for name, want in sorted(files.items()):
            path = HERE / name
            if not path.is_file() or path.read_bytes() != want:
                stale.append(name)
        if stale:
            sys.stderr.write(f"stale fixtures in {HERE}: {', '.join(stale)}\n")
            return 1
        return 0

    for name, content in sorted(files.items()):
        (HERE / name).write_bytes(content)
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
