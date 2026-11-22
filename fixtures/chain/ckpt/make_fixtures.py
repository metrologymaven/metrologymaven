#!/usr/bin/env python3
"""Independent fixture generator for TRUST.CHAIN.CKPT (AC-1..AC-4). CC-BY-4.0 project data.

Stdlib only. Reuses the repository's independent stdlib reference generators:
  - fixtures/dsse/make_fixtures.py (RFC 6979 P-256, RFC 8032 Ed25519, DSSE PAE & envelope)
  - fixtures/pkg/make_fixtures.py  (deterministic Result Package ZIP writer)

Writes deterministic Result Packages under fixtures/chain/ckpt/:
  signed-ok.zip       3-entry journal + DSSE-signed linear-v1 checkpoint at seq 3 (P-256)    (AC-1)
  signed-ed25519.zip  3-entry journal + embedded-signature linear-v1 checkpoint (Ed25519)    (AC-1b)
  wrong-head.zip      checkpoint at seq 3 with altered head digest -> MISMATCH               (AC-2)
  far-index.zip       checkpoint at seq 99 beyond 3-entry segment -> OUT_OF_RANGE            (AC-2)
  flipped-sig.zip     same as signed-ok.zip with 1 byte flipped in signature -> MATCH/INVALID(AC-3)
  bad-version.zip     2 checkpoints: seq 2 with format_version=99 (ckpt.malformed) + seq 3 ok(AC-4)
  expected.json       independently computed entry hashes, fingerprints and verdicts

TEST KEYS ONLY (same published RFC 6979 / RFC 8032 keys as fixtures/troot/roots/both.json).

Run:
  python fixtures/chain/ckpt/make_fixtures.py
  python fixtures/chain/ckpt/make_fixtures.py --check
"""

import argparse
import base64
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


dsse = load("ckpt_dsse_ref", "dsse/make_fixtures.py")
pkg = load("ckpt_pkg_ref", "pkg/make_fixtures.py")

CHECKPOINT_PAYLOAD_TYPE = "application/vnd.metrologymaven.checkpoint+json"
CHECKPOINT_TIME = "2026-06-15T12:00:00Z"

# Published RFC test keys matching fixtures/troot/roots/both.json.
P_X = dsse.RFC6979_X
P_SPKI = dsse.spki_p256(dsse.p_public(P_X))
P_FP = dsse.fingerprint(P_SPKI)

ED_SEED = bytes.fromhex(dsse.RFC8032[0]["secret"])
ED_SPKI = dsse.spki_ed25519(dsse.ed_public(ED_SEED))
ED_FP = dsse.fingerprint(ED_SPKI)


def canon_bytes(v) -> bytes:
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256_digest(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def build_segment_entries() -> tuple[str, dict[int, str]]:
    """Build a 3-entry journal segment with canonical entry hashes (matching TRUST.CHAIN.CANON)."""
    prev = "sha256:" + "0" * 64
    lines = []
    hashes: dict[int, str] = {}
    for seq in (1, 2, 3):
        header = {
            "command": f"project.step_{seq}@1",
            "prev": prev,
            "seq": str(seq),
            "stream": "main",
            "time": f"2026-06-15T11:59:0{seq}Z",
        }
        h = sha256_digest(canon_bytes(header))
        entry = dict(header)
        entry["hash"] = h
        lines.append(canon_bytes(entry).decode("utf-8"))
        hashes[seq] = h
        prev = h
    return "\n".join(lines) + "\n", hashes


def checkpoint_body(seq: int, head: str, reason: str = "signature", time: str = CHECKPOINT_TIME) -> dict:
    return {
        "count": str(seq),
        "format": "linear-v1",
        "head": head,
        "reason": reason,
        "seq": str(seq),
        "stream": "main",
        "time": time,
    }


def sign_p256_envelope(payload_bytes: bytes, flip_last_sig_byte: bool = False) -> dict:
    msg = dsse.pae(CHECKPOINT_PAYLOAD_TYPE.encode("utf-8"), payload_bytes)
    r, s = dsse.p_sign(P_X, msg)
    sig_der = bytearray(dsse.ecdsa_der(r, dsse.low_s(s)))
    if flip_last_sig_byte:
        sig_der[-1] ^= 0x01
    return {
        "payload": base64.b64encode(payload_bytes).decode("ascii"),
        "payloadType": CHECKPOINT_PAYLOAD_TYPE,
        "signatures": [{"keyid": P_FP, "sig": base64.b64encode(bytes(sig_der)).decode("ascii")}],
    }


def sign_ed25519_envelope(payload_bytes: bytes) -> dict:
    msg = dsse.pae(CHECKPOINT_PAYLOAD_TYPE.encode("utf-8"), payload_bytes)
    sig = dsse.ed_sign(ED_SEED, msg)
    return {
        "payload": base64.b64encode(payload_bytes).decode("ascii"),
        "payloadType": CHECKPOINT_PAYLOAD_TYPE,
        "signatures": [{"keyid": ED_FP, "sig": base64.b64encode(sig).decode("ascii")}],
    }


def make_pkg(segment_text: str, checkpoints: dict[str, bytes]) -> bytes:
    members = [
        {"path": "journal/segment.jsonl", "media_type": "application/x-ndjson", "body": segment_text},
        {"path": "profile/profile.json", "media_type": "application/json", "body": pkg.PROFILE_BODY},
        {"path": "kernel/manifest.json", "media_type": "application/json", "body": pkg.KERNEL_BODY},
    ]
    for rel_path, data in sorted(checkpoints.items()):
        members.append({
            "path": rel_path,
            "media_type": "application/json",
            "body": data.decode("utf-8"),
        })
    contents = {
        "created_by": "fid-chain ckpt fixture generator 1",
        "evidence_tier": "points",
        "profile_digest": pkg.digest_of(pkg.PROFILE_BODY),
        "kernel_manifest_digest": pkg.digest_of(pkg.KERNEL_BODY),
        "result_count": 1,
        "members": members,
    }
    _mbytes, _sbytes, zip_bytes = pkg.build_package(contents)
    return zip_bytes


def expected_outputs() -> dict[str, bytes]:
    segment_text, hashes = build_segment_entries()
    head3 = hashes[3]
    wrong_head = "sha256:" + ("ff" * 32)

    body3 = checkpoint_body(3, head3)
    body3_bytes = canon_bytes(body3)

    # AC-1: DSSE-wrapped signed checkpoint (P-256 from both.json).
    env_p256 = sign_p256_envelope(body3_bytes, flip_last_sig_byte=False)
    signed_ok_zip = make_pkg(segment_text, {"journal/checkpoints/3.json": canon_bytes(env_p256)})

    # AC-1b: Embedded `signature` member on linear-v1 JSON (Ed25519 from both.json).
    body3_with_ed = dict(body3)
    body3_with_ed["signature"] = sign_ed25519_envelope(body3_bytes)
    signed_ed_zip = make_pkg(segment_text, {"journal/checkpoints/3.json": canon_bytes(body3_with_ed)})

    # AC-2a: Wrong head digest -> MISMATCH.
    wrong_body = checkpoint_body(3, wrong_head, reason="export")
    wrong_head_zip = make_pkg(segment_text, {"journal/checkpoints/3.json": canon_bytes(wrong_body)})

    # AC-2b: Out-of-range index (seq 99 on 3-entry segment) -> OUT_OF_RANGE.
    far_body = checkpoint_body(99, head3, reason="export")
    far_index_zip = make_pkg(segment_text, {"journal/checkpoints/99.json": canon_bytes(far_body)})

    # AC-3: Valid head digest, flipped signature byte -> MATCH + INVALID.
    env_flipped = sign_p256_envelope(body3_bytes, flip_last_sig_byte=True)
    flipped_sig_zip = make_pkg(segment_text, {"journal/checkpoints/3.json": canon_bytes(env_flipped)})

    # AC-4: Unknown format version 99 at seq 2 + valid signed checkpoint at seq 3.
    bad_v99 = canon_bytes({
        "format_version": 99,
        "head_sha256": hashes[2],
        "index": "2",
    })
    bad_version_zip = make_pkg(
        segment_text,
        {
            "journal/checkpoints/2.json": bad_v99,
            "journal/checkpoints/3.json": canon_bytes(env_p256),
        },
    )

    expected = {
        "ed25519_fingerprint": ED_FP,
        "p256_fingerprint": P_FP,
        "segment_hashes": {str(k): v for k, v in sorted(hashes.items())},
        "cases": {
            "signed-ok.zip": [{"path": "journal/checkpoints/3.json", "index": 3, "status": "MATCH", "sig_status": "VALID_TRUSTED", "fingerprint": P_FP}],
            "signed-ed25519.zip": [{"path": "journal/checkpoints/3.json", "index": 3, "status": "MATCH", "sig_status": "VALID_TRUSTED", "fingerprint": ED_FP}],
            "wrong-head.zip": [{"path": "journal/checkpoints/3.json", "index": 3, "status": "MISMATCH", "sig_status": None, "fingerprint": None}],
            "far-index.zip": [{"path": "journal/checkpoints/99.json", "index": 99, "status": "OUT_OF_RANGE", "sig_status": None, "fingerprint": None}],
            "flipped-sig.zip": [{"path": "journal/checkpoints/3.json", "index": 3, "status": "MATCH", "sig_status": "INVALID", "fingerprint": P_FP}],
            "bad-version.zip": [
                {"path": "journal/checkpoints/2.json", "index": None, "status": "MALFORMED", "error_code": "ckpt.malformed", "sig_status": None, "fingerprint": None},
                {"path": "journal/checkpoints/3.json", "index": 3, "status": "MATCH", "sig_status": "VALID_TRUSTED", "fingerprint": P_FP},
            ],
        },
    }

    return {
        "signed-ok.zip": signed_ok_zip,
        "signed-ed25519.zip": signed_ed_zip,
        "wrong-head.zip": wrong_head_zip,
        "far-index.zip": far_index_zip,
        "flipped-sig.zip": flipped_sig_zip,
        "bad-version.zip": bad_version_zip,
        "expected.json": canon_bytes(expected),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="Verify fixtures match regenerated bytes")
    args = ap.parse_args()

    outputs = expected_outputs()
    if args.check:
        ok = True
        for name, data in sorted(outputs.items()):
            p = HERE / name
            if not p.exists():
                print(f"MISSING {p}", file=sys.stderr)
                ok = False
            elif p.read_bytes() != data:
                print(f"DRIFT {p}", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    HERE.mkdir(parents=True, exist_ok=True)
    for name, data in sorted(outputs.items()):
        (HERE / name).write_bytes(data)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
