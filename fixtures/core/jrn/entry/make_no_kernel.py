#!/usr/bin/env python3
"""Generate the "no kernel attested" fixture of CORE.JRN.ENTRY (no_kernel.json).

Independent reference, Python standard library only (hashlib, json). Nothing here runs,
imports or copies the Rust crates; the Rust tests (crates/fid-journal/tests/no_kernel.rs)
load the file and must reach the same values.

Rule under test (spec/core/jrn/entry/NODE.md, Design, "Reserved no-kernel digest"): the
`manifests.kernel` value `sha256:` + hex(SHA-256 of the empty string) is reserved and
means "no kernel manifest attested". Any other digest attests that kernel, including
digests one bit away from the reserved one. Verification (CORE.JRN.CHAIN.VERIFY rules) of a
journal holding such entries succeeds; it counts, among the verified entries, those that
attest no kernel (`without_kernel`). Relabelling an entry's kernel in the stored header
bytes without re-hashing it is `hash/hash_mismatch` at that sequence, and the counts stop
before it. Two kernels are "the same attested kernel" only when both attest a kernel and
the digests are equal; the reserved digest is never the same kernel as anything, itself
included.

All data is invented. Headers are flat ASCII structures of strings, integers, booleans,
null, arrays and objects, for which `json.dumps(sort_keys=True, separators=(",", ":"))` is
exactly RFC 8785 (ASCII keys sort the same by UTF-16 code units and by code point).

Run:         python fixtures/core/jrn/entry/make_no_kernel.py
Check only:  python fixtures/core/jrn/entry/make_no_kernel.py --check
CC-BY-4.0 project data.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "no_kernel.json"
SEED = "CORE.JRN.ENTRY/no_kernel/20261126"
STREAM = "record"
ZERO = "sha256:" + "0" * 64


def canon(v) -> bytes:
    text = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True)
    assert text.isascii()
    return text.encode("ascii")


def sha(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


NO_KERNEL = sha(b"")
assert NO_KERNEL == "sha256:e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"


def flip_last_bit(d: str) -> str:
    last = int(d[-1], 16) ^ 1
    return d[:-1] + "%x" % last


REAL_A = sha(b"invented kernel manifest A")
REAL_B = sha(b"invented kernel manifest B")
NEAR = flip_last_bit(NO_KERNEL)  # one bit from the reserved digest: a real attestation
ZERO_KERNEL = ZERO  # the tombstone pattern is not reserved for kernels: a real attestation
REGISTRY = sha(b"invented registry manifest")


def salt(seq: int, field: str) -> str:
    return hashlib.sha256(("%s/%d/%s" % (SEED, seq, field)).encode()).hexdigest()[:32]


def commitment(salt_hex: str, value) -> str:
    return sha(bytes.fromhex(salt_hex) + canon(value))


# Kernel recorded by each entry, in sequence order (entry 1 is the session header).
KERNELS = [NO_KERNEL, NO_KERNEL, REAL_A, NEAR, NO_KERNEL, REAL_B, ZERO_KERNEL, NO_KERNEL, REAL_A]


def attested(k: str):
    return None if k == NO_KERNEL else k


def build_chain():
    rows = []
    prev = ZERO
    for seq, kernel in enumerate(KERNELS, start=1):
        salts = {}
        values = {}

        def commit(field, value):
            s = salt(seq, field)
            salts[field] = s
            values[field] = value
            return commitment(s, value)

        h = {
            "schema_version": 1,
            "stream": STREAM,
            "seq": str(seq),
            "prev": prev,
            "branch": "main",
            "logical_parent": None if seq == 1 else prev,
            "time": {"utc": "2026-11-26T09:%02d:00Z" % seq, "mono_ns": str(5_000_000_000 * seq), "sync": "synced"},
            "principal": {"kind": "human", "commit": commit("identity", {"user": "op-%02d" % seq})},
            "ai_assistance": "none",
            "manifests": {"registry": REGISTRY, "kernel": kernel},
        }
        if seq == 1:
            h["kind"] = "session_header"
            h["session"] = {"session_id": "s-nokern", "app_version": "0.1.0"}
        else:
            h["kind"] = "command"
            h["command"] = {
                "id": "feature.set_nominal",
                "major": 1,
                "class": "W2",
                "determinism": "D0",
                "seed": str(seq),
                "args_commit": commit("args", {"target": "F-%03d" % seq, "nominal": "12.5"}),
                "events_commit": commit("events", [{"type": "feature.set_nominal@1", "n": seq}]),
                "inputs": [],
                "outputs": [{"entity": "F-%03d" % seq, "digest": sha(b"nominal %d" % seq)}],
                "algorithms": [],
            }
        h["hash"] = sha(canon(h))
        payload = dict(values)
        payload["salts"] = salts
        payload_bytes = canon(payload)
        rows.append({
            "seq": seq,
            "hash": h["hash"],
            "stream": STREAM,
            "branch": "main",
            "kind": h["kind"],
            "prev_hash": prev,
            "header": canon(h).decode("ascii"),
            "payload": payload_bytes.decode("ascii"),
            "payload_digest": sha(payload_bytes),
            "kernel": kernel,
            "attested_kernel": attested(kernel),
        })
        prev = h["hash"]
    return rows


def relabel(rows, seq, new_kernel):
    """The stored header of `seq` with its kernel replaced in place, hash left as it was."""
    r = rows[seq - 1]
    old = '"kernel":"%s"' % r["kernel"]
    assert r["header"].count(old) == 1
    header = r["header"].replace(old, '"kernel":"%s"' % new_kernel)
    # Still canonical (same shape, same key order), so the failure is the hash.
    assert canon(json.loads(header)) == header.encode("ascii")
    return header


def counts_before(rows, seq):
    done = rows[: seq - 1]
    return {"entries": len(done), "without_kernel": sum(1 for r in done if r["kernel"] == NO_KERNEL)}


def build():
    rows = build_chain()
    full = {
        "entries": len(rows),
        "without_kernel": sum(1 for r in rows if r["kernel"] == NO_KERNEL),
        "without_payload": 0,
    }
    assert full == {"entries": 9, "without_kernel": 4, "without_payload": 0}
    tampered = []
    # An attested kernel relabelled as "none attested", and the reverse.
    for name, seq, to in (("attested_to_none", 6, NO_KERNEL), ("none_to_attested", 5, REAL_A)):
        expect = {"ok": False, "failure": {"seq": seq, "kind": "hash", "reason": "hash_mismatch"}}
        expect["checked"] = counts_before(rows, seq)
        tampered.append({"name": name, "seq": seq, "header": relabel(rows, seq, to), "expect": expect})
    # Same-attested-kernel pairs: (recorded, replayed, same).
    pairs = []
    for a in (NO_KERNEL, REAL_A, REAL_B, NEAR):
        for b in (NO_KERNEL, REAL_A, REAL_B, NEAR):
            pairs.append({"recorded": a, "replayed": b, "same": a != NO_KERNEL and b != NO_KERNEL and a == b})
    doc = {
        "no_kernel_manifest": NO_KERNEL,
        "stream": STREAM,
        "rows": rows,
        "expect": {"ok": True, "checked": full},
        "tampered": tampered,
        "same_attested_kernel": pairs,
    }
    return (json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode("ascii")


def main() -> int:
    data = build()
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_bytes() != data:
            print("stale fixture (run make_no_kernel.py): " + OUT.name)
            return 1
        print("fixture up to date")
        return 0
    OUT.write_bytes(data)
    print("wrote " + OUT.name)
    return 0


if __name__ == "__main__":
    sys.exit(main())
