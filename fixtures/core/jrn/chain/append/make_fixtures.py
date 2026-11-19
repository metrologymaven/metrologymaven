#!/usr/bin/env python3
"""Generate the fixtures of CORE.JRN.CHAIN.APPEND (AC-1, AC-2 and the time-order rule).

An independent reference written with Python's standard library only (hashlib, json). The
Rust crate `fid-journal` (module `append`) is checked against these files; nothing here is
produced by running the Rust code.

Files written next to this script:

- chain.json: 100 W2 command drafts of one Record Journal stream with fixed salts, the blob
  each entry references, and the expected sequence, previous hash, entry hash and payload
  blob digest of every entry (AC-1).
- time_order.json: hand-picked pairs of chain-head time and new entry time, each labelled
  accepted or refused by the pinned rule (see below).
- crash/points.json: the 20 numbered injection points inside `append`, in the order a
  single successful append passes them, each hand-labelled with what a reopen must find
  after the process is killed there (AC-2).

Rules implemented here (spec/core/jrn/chain/append/NODE.md, spec/core/jrn/entry/NODE.md):
- sequences start at 1 and grow by one per stream; `prev` of sequence 1 is `sha256:` + 64
  zeros, else the hash of the previous entry of the stream, whatever its branch;
- commitment = "sha256:" + SHA-256(16 salt bytes || RFC 8785 bytes of the value);
- entry hash = SHA-256 of the RFC 8785 header with `hash` omitted; seq, mono_ns and seed are
  decimal strings;
- the payload (every committed value plus `salts`) is stored as one content-addressed blob,
  so its address is SHA-256 of its RFC 8785 bytes;
- time order: an entry whose `time.utc` instant is earlier than the chain head's is refused
  (`time_regressed`); an equal instant is accepted. Instants are compared as instants, not as
  text: "...:00Z" equals "...:00.000Z" and is earlier than "...:00.5Z". `mono_ns` is not
  compared (it restarts with the host).
- crash outcome: the SQLite COMMIT is the only point at which an entry becomes visible; a
  kill at any point before it leaves the chain at the old head, a kill at or after it leaves
  the entry fully present.

Values hold no non-integer floats and no object keys outside the BMP, so
json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False) equals RFC 8785.
CC-BY-4.0 project data.
Run:  python fixtures/core/jrn/chain/append/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ZERO = "sha256:" + "0" * 64
STREAM = "record"


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def salt_for(name, field):
    return hashlib.sha256(f"append-salt/{name}/{field}".encode("utf-8")).hexdigest()[:32]


def commit(salt_hex, value):
    return sha(bytes.fromhex(salt_hex) + canon(value))


REGISTRY = sha(b"registry-manifest-append")
KERNEL = sha(b"kernel-manifest-append")
BRANCHES = ["main", "feature/bore-check", "agent/claude-code/3"]


def utc_at(i):
    # 2026-11-19T10:00:00Z plus i seconds (i < 3600), every third one with a fraction.
    s = f"2026-11-19T10:{i // 60:02d}:{i % 60:02d}"
    return s + (f".{i:03d}Z" if i % 3 == 1 else "Z")


def draft(i):
    entity = f"F-{i:04d}"
    content = f"nominal table {i}\n".encode("utf-8")
    blob = sha(content)
    return {
        "kind": "command",
        "stream": STREAM,
        "branch": BRANCHES[i % len(BRANCHES)],
        "logical_parent": None if i == 0 else sha(f"state-{i}".encode("utf-8")),
        "time": {"utc": utc_at(i), "mono_ns": str(5_000_000_000 + 1_000_000_000 * i), "sync": "synced"},
        "principal": {"kind": "human", "identity": {"user": "op-17"}},
        "ai_assistance": "none",
        "manifests": {"registry": REGISTRY, "kernel": KERNEL},
        "command": {
            "id": "feature.set_nominal",
            "major": 1,
            "class": "W2",
            "determinism": "D0",
            "seed": str(i),
            "args": {"feature": entity, "nominal_um": 12000 + i, "note": "Ø 12 H7"},
            "events": [{"type": "feature.nominal_set@1", "n": i}],
            "inputs": [],
            "outputs": [{"entity": entity, "digest": blob}],
            "algorithms": [],
        },
    }, content, blob


def build(name, dr, seq, prev):
    salts = {}
    payload = {}

    def c(field, value):
        s = salt_for(name, field)
        salts[field] = s
        payload[field] = value
        return commit(s, value)

    p = dr["principal"]
    cd = dr["command"]
    h = {
        "schema_version": 1,
        "kind": dr["kind"],
        "stream": dr["stream"],
        "seq": str(seq),
        "prev": prev,
        "branch": dr["branch"],
        "logical_parent": dr["logical_parent"],
        "time": dict(dr["time"]),
        "principal": {"kind": p["kind"], "commit": c("identity", p["identity"])},
        "ai_assistance": dr["ai_assistance"],
        "manifests": dict(dr["manifests"]),
        "command": {
            "id": cd["id"], "major": cd["major"], "class": cd["class"], "determinism": cd["determinism"],
            "seed": cd["seed"], "args_commit": c("args", cd["args"]), "events_commit": c("events", cd["events"]),
            "inputs": list(cd["inputs"]), "outputs": [dict(o) for o in cd["outputs"]],
            "algorithms": list(cd["algorithms"]),
        },
    }
    h["hash"] = sha(canon(h))
    payload["salts"] = salts
    return h, salts, sha(canon(payload))


def chain():
    entries = []
    seq, prev = 1, ZERO
    for i in range(100):
        name = f"w2_{i:03d}"
        dr, content, blob = draft(i)
        h, salts, payload_digest = build(name, dr, seq, prev)
        entries.append({
            "name": name, "draft": dr, "salts": salts,
            "blobs": [{"digest": blob, "content": content.decode("utf-8")}],
            "seq": str(seq), "prev": prev, "hash": h["hash"], "payload_digest": payload_digest,
        })
        seq, prev = seq + 1, h["hash"]
    return {"stream": STREAM, "entries": entries}


# ------------------------------------------------------------------ time order

def instant(s):
    """(seconds-of-day key, nanoseconds) of an RFC 3339 UTC instant ending in Z."""
    assert s.endswith("Z") and s[10] == "T", s
    date, clock = s[:10], s[11:-1]
    whole, _, frac = clock.partition(".")
    assert 1 <= len(frac) <= 9 or frac == "", s
    key = int(date.replace("-", "") + whole.replace(":", ""))
    nanos = int((frac + "0" * 9)[:9]) if frac else 0
    return key, nanos


TIME_CASES = [
    ("later_second", "2026-11-19T10:00:00Z", "2026-11-19T10:00:01Z"),
    ("equal", "2026-11-19T10:00:00Z", "2026-11-19T10:00:00Z"),
    ("equal_with_zero_fraction", "2026-11-19T10:00:00Z", "2026-11-19T10:00:00.000Z"),
    ("equal_fraction_widths", "2026-11-19T10:00:00.1Z", "2026-11-19T10:00:00.100000000Z"),
    ("one_second_earlier", "2026-11-19T10:00:01Z", "2026-11-19T10:00:00Z"),
    ("one_nanosecond_earlier", "2026-11-19T10:00:00.000000001Z", "2026-11-19T10:00:00Z"),
    ("one_nanosecond_later", "2026-11-19T10:00:00Z", "2026-11-19T10:00:00.000000001Z"),
    ("fraction_after_whole", "2026-11-19T10:00:00Z", "2026-11-19T10:00:00.5Z"),
    ("whole_after_fraction_textually_greater", "2026-11-19T10:00:00.5Z", "2026-11-19T10:00:00Z"),
    ("fraction_shorter_but_later", "2026-11-19T10:00:00.09Z", "2026-11-19T10:00:00.1Z"),
    ("fraction_longer_but_earlier", "2026-11-19T10:00:00.1Z", "2026-11-19T10:00:00.09999Z"),
    ("previous_day", "2026-11-19T00:00:00Z", "2026-11-18T23:59:59.999999999Z"),
    ("next_year", "2026-12-31T23:59:59.999Z", "2027-01-01T00:00:00Z"),
    ("previous_year", "2027-01-01T00:00:00Z", "2026-12-31T23:59:59.999Z"),
    ("leap_second_after_59", "2016-12-31T23:59:59Z", "2016-12-31T23:59:60Z"),
    ("minute_earlier_second_later", "2026-11-19T10:01:00Z", "2026-11-19T10:00:59Z"),
    ("hour_earlier", "2026-11-19T11:00:00Z", "2026-11-19T10:59:59.999999999Z"),
    ("confirm_stamped_before_head", "2026-11-19T10:00:05Z", "2026-11-19T10:00:04.9Z"),
]


def time_order():
    cases = []
    for name, head, entry in TIME_CASES:
        accept = instant(entry) >= instant(head)
        cases.append({"name": name, "head": head, "entry": entry, "accept": accept})
    return cases


# ------------------------------------------------------------------ crash injection points

# Hand-labelled: the order a single successful append with one referenced blob passes them,
# and what a reopen finds after a kill there. COMMIT (point 18) is the linearisation point.
POINTS = [
    "enqueued", "batch_taken", "lock_verified", "begun", "tail_read", "head_checked", "built",
    "blobs_checked", "payload_encoded", "payload_put", "savepoint_opened", "entry_inserted",
    "payload_ref_inserted", "blob_refs_inserted", "savepoint_released", "batch_built",
    "before_commit", "committed", "results_published", "returning",
]


def crash_points():
    pts = []
    for n, name in enumerate(POINTS, start=1):
        pts.append({"n": n, "name": name, "after_restart": "present" if n >= 18 else "absent"})
    return {"prechain": 3, "target": 3, "points": pts}


def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False) + "\n").encode("utf-8")


def main():
    outputs = {
        HERE / "chain.json": dump(chain()),
        HERE / "time_order.json": dump(time_order()),
        HERE / "crash" / "points.json": dump(crash_points()),
    }
    check = "--check" in sys.argv[1:]
    stale = []
    for path, data in outputs.items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(path.relative_to(HERE).as_posix())
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if stale:
        print("stale fixtures (run make_fixtures.py):", ", ".join(stale))
        sys.exit(1)
    print("fixtures up to date" if check else f"wrote {len(outputs)} files")


if __name__ == "__main__":
    main()
