#!/usr/bin/env python3
"""Generate the CORE.RPL.EXEC.SUBST fixtures (AC-1 .. AC-5, REQ-003 .. REQ-006).

Independent oracle: stdlib Python only, nothing here runs or imports the Rust code.
Digests are SHA-256 from hashlib; commitments are SHA-256 over the 16-byte salt followed
by the RFC 8785 bytes of the arguments (CORE.JRN.ENTRY REQ-003). The arguments are flat
objects of ASCII strings, for which `json.dumps(sort_keys=True, separators=(",", ":"))` is
exactly RFC 8785. Expected pre-flight results are hand-labelled from the leaf's rules.
Salts are derived from a fixed seed string, so the output is byte-stable.
CC-BY-4.0 project data.

Test handlers (implemented independently in crates/fid-replay/tests/subst.rs):
  probe.stamp@1  reads one `clock` then one `salt` observation;
                 output = sha256(clock_bytes || salt_bytes).
  probe.read@1   reads one `device` observation;
                 output = sha256(b"probe.read:" || device_bytes).
  Both then take the commitment salt for `args` and recompute `args_commit`.

Run:         python fixtures/core/rpl/exec/subst/make_fixtures.py
Check only:  python fixtures/core/rpl/exec/subst/make_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SEED = "CORE.RPL.EXEC.SUBST/20261118"


def sha(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def salt(case: str, seq: int, field: str) -> bytes:
    return hashlib.sha256(f"{SEED}:{case}:{seq}:{field}".encode()).digest()[:16]


def canon(obj) -> bytes:
    # Flat objects of ASCII strings only: sorted keys and no whitespace is RFC 8785.
    assert all(isinstance(k, str) and isinstance(v, str) and v.isascii() for k, v in obj.items())
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode()


def commit(s: bytes, args) -> str:
    return sha(s + canon(args))


KERNEL_1 = sha(b"fixture kernel manifest 1")
KERNEL_2 = sha(b"fixture kernel manifest 2 (not available)")

ACTIVE = {"state": "active", "since": "2026-01-15"}


def decl(command, **life):
    return {"command": command, "lifecycle": life or dict(ACTIVE)}


def entry_hash(case: str, seq: int) -> str:
    # Stand-in entry hash: the span model only carries it as the ID of a payload.
    return sha(f"{SEED}:{case}:entry:{seq}".encode())


files = {}


def emit(rel: str, data: bytes) -> None:
    files[rel] = data


def emit_json(rel: str, doc) -> None:
    emit(rel, (json.dumps(doc, indent=1, sort_keys=True, ensure_ascii=True) + "\n").encode())


# ---------------------------------------------------------------------------
# AC-1 (and AC-5): clock_and_salt. Three probe.stamp@1 steps; each read the clock once and
# drew one salt, both recorded inline in the payload.
# ---------------------------------------------------------------------------
case = "clock_and_salt"
steps, handler = [], {}
for seq in (1, 2, 3):
    clock = canon({"mono_ns": str(1_000_000_000 * seq + 17), "sync": "ntp", "utc": f"2026-11-18T09:00:0{seq}Z"})
    drawn = hashlib.sha256(f"{SEED}:{case}:{seq}:drawn".encode()).digest()[:16]
    args = {"feature": f"F{seq}", "probe": "T1"}
    s_args = salt(case, seq, "args")
    steps.append({
        "seq": seq, "entry": entry_hash(case, seq), "command": "probe.stamp@1", "kernel": KERNEL_1,
        "observations": [
            {"kind": "clock", "digest": sha(clock), "storage": "inline"},
            {"kind": "salt", "digest": sha(drawn), "storage": "inline"},
        ],
        "payload": {"salts": {"args": s_args.hex()}, "inline": {sha(clock): clock.hex(), sha(drawn): drawn.hex()}},
    })
    handler[str(seq)] = {"args": args, "args_commit": commit(s_args, args), "output": sha(clock + drawn)}
emit_json(f"{case}/case.json", {
    "span": {"steps": steps}, "decls": [decl("probe.stamp@1")], "migrators": [], "kernels": [KERNEL_1],
    "handler": handler,
    # Hand-labelled: every input is present, so nothing is missing and every step is IDENTICAL.
    "expect": {"preflight": [], "outcomes": {"1": "IDENTICAL", "2": "IDENTICAL", "3": "IDENTICAL"}},
})

# ---------------------------------------------------------------------------
# AC-2 / AC-3: device_read. Two probe.read@1 steps, each an I1 simulator read recorded as
# an observation blob. AC-3 deletes the blob of step 1 from the store.
# ---------------------------------------------------------------------------
case = "device_read"
steps, handler, blobs = [], {}, []
for seq, (x, y, z) in ((1, ("0x3f847ae147ae147b", "0x0000000000000000", "0xbf50624dd2f1a9fc")),
                       (2, ("0x3f947ae147ae147b", "0x3f50624dd2f1a9fc", "0xbf50624dd2f1a9fc"))):
    blob = canon({"device": "sim-i1", "op": "probe_point", "x_m": x, "y_m": y, "z_m": z})
    blobs.append(sha(blob))
    emit(f"{case}/blobs/{hashlib.sha256(blob).hexdigest()}", blob)
    args = {"feature": f"P{seq}", "probe": "T1"}
    s_args = salt(case, seq, "args")
    steps.append({
        "seq": seq, "entry": entry_hash(case, seq), "command": "probe.read@1", "kernel": KERNEL_1,
        "observations": [{"kind": "device", "digest": sha(blob), "storage": "blob"}],
        "payload": {"salts": {"args": s_args.hex()}, "inline": {}},
    })
    handler[str(seq)] = {"args": args, "args_commit": commit(s_args, args), "output": sha(b"probe.read:" + blob)}
emit_json(f"{case}/case.json", {
    "span": {"steps": steps}, "decls": [decl("probe.read@1")], "migrators": [], "kernels": [KERNEL_1],
    "handler": handler,
    "expect": {
        "preflight": [], "outcomes": {"1": "IDENTICAL", "2": "IDENTICAL"},
        # AC-3, hand-labelled: with step 1's blob deleted, the scan names that blob for step 1
        # and step 1 is NOT_REPLAYABLE; step 2 is unaffected.
        "deleted_blob": blobs[0],
        "preflight_without_blob": [{"kind": "blob", "id": blobs[0], "steps": [1]}],
        "outcomes_without_blob": {"1": "NOT_REPLAYABLE", "2": "IDENTICAL"},
    },
})

# ---------------------------------------------------------------------------
# AC-4: removed_command. probe.legacy@1 is removed and no migrator leads from it;
# probe.legacy@2 is active and probe.hidden@1 is hidden (retained, so still replayable).
# ---------------------------------------------------------------------------
case = "removed_command"
steps = []
for seq, cmd in ((1, "probe.legacy@1"), (2, "probe.legacy@2"), (3, "probe.legacy@1"), (4, "probe.hidden@1")):
    steps.append({"seq": seq, "entry": entry_hash(case, seq), "command": cmd, "kernel": KERNEL_1,
                  "observations": [], "payload": {"salts": {}, "inline": {}}})
emit_json(f"{case}/case.json", {
    "span": {"steps": steps},
    "decls": [
        decl("probe.legacy@1", state="removed", since="2026-09-01", replaced_by="probe.legacy@2"),
        decl("probe.legacy@2"),
        decl("probe.hidden@1", state="hidden", since="2026-06-01", replaced_by="probe.hidden@2",
             remove_after="2027-06-01"),
        decl("probe.hidden@2"),
    ],
    "migrators": [],
    "kernels": [KERNEL_1],
    "expect": {"preflight": [{"kind": "command", "id": "probe.legacy@1", "steps": [1, 3]}]},
})

# ---------------------------------------------------------------------------
# REQ-005 / REQ-006: preflight_all. One span with every kind of missing item:
#   blob B shared by steps 2 and 5 (absent from the store), blob C present;
#   step 3 runs a removed command; step 4's payload is detached; step 6's inline value
#   does not hash to its recorded digest (damaged payload); steps 5 and 6 use kernel 2,
#   which the kernel source cannot supply.
# ---------------------------------------------------------------------------
case = "preflight_all"
blob_b = b"external file contents, never stored"
blob_c = b"external file contents, stored"
emit(f"{case}/blobs/{hashlib.sha256(blob_c).hexdigest()}", blob_c)
good_text = b"operator note: fixture plate seated"
bad_text_recorded = b"operator note: fixture plate seated."
pay = lambda: {"salts": {}, "inline": {}}
steps = [
    {"seq": 1, "entry": entry_hash(case, 1), "command": "probe.read@1", "kernel": KERNEL_1,
     "observations": [{"kind": "external_file", "digest": sha(blob_c), "storage": "blob"}], "payload": pay()},
    {"seq": 2, "entry": entry_hash(case, 2), "command": "probe.read@1", "kernel": KERNEL_1,
     "observations": [{"kind": "external_file", "digest": sha(blob_b), "storage": "blob"}], "payload": pay()},
    {"seq": 3, "entry": entry_hash(case, 3), "command": "probe.legacy@1", "kernel": KERNEL_1,
     "observations": [], "payload": pay()},
    {"seq": 4, "entry": entry_hash(case, 4), "command": "probe.read@1", "kernel": KERNEL_1,
     "observations": [{"kind": "human_text", "digest": sha(good_text), "storage": "inline"}], "payload": None},
    {"seq": 5, "entry": entry_hash(case, 5), "command": "probe.read@1", "kernel": KERNEL_2,
     "observations": [{"kind": "external_file", "digest": sha(blob_b), "storage": "blob"}], "payload": pay()},
    {"seq": 6, "entry": entry_hash(case, 6), "command": None, "kernel": KERNEL_2,
     "observations": [{"kind": "human_text", "digest": sha(bad_text_recorded), "storage": "inline"}],
     "payload": {"salts": {}, "inline": {sha(bad_text_recorded): good_text.hex()}}},
]
expected = [
    {"kind": "blob", "id": sha(blob_b), "steps": [2, 5]},
    {"kind": "command", "id": "probe.legacy@1", "steps": [3]},
]
# Payload items are named by entry hash and ordered by that string.
expected += sorted(({"kind": "payload", "id": entry_hash(case, s), "steps": [s]} for s in (4, 6)),
                   key=lambda m: m["id"])
expected.append({"kind": "kernel", "id": KERNEL_2, "steps": [5, 6]})
emit_json(f"{case}/case.json", {
    "span": {"steps": steps},
    "decls": [decl("probe.read@1"),
              decl("probe.legacy@1", state="removed", since="2026-09-01", replaced_by="probe.legacy@2"),
              decl("probe.legacy@2")],
    "migrators": [],
    "kernels": [KERNEL_1],
    "expect": {"preflight": expected},
})


def main() -> int:
    check = "--check" in sys.argv[1:]
    bad = 0
    for rel, data in sorted(files.items()):
        path = HERE / rel
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {rel}")
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if check:
        print("fixtures are fresh" if bad == 0 else f"{bad} stale fixture file(s)")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
