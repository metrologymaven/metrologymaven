#!/usr/bin/env python3
"""Generate the fixtures of CORE.JRN.CHAIN.APPEND (AC-1, AC-2 and the time-order rule).

An independent reference written with Python's standard library only (hashlib, json). The
Rust crate `fid-journal` (module `append`) is checked against these files; nothing here is
produced by running the Rust code.

Files written next to this script:

- chain.json: 100 W2 command drafts of one Record Journal stream with fixed salts, the blob
  each entry references, and the expected sequence, previous hash, entry hash and payload
  blob digest of every entry (AC-1), followed by three entries that exercise delegated
  attribution (an agent acting on behalf of a person with a reason, a principal_upgrade and a
  script acting on behalf of a person); these three also carry their full expected header.
  Key `v2_entries` continues the same chain with entries of schema version 2
  (CORE.JRN.ENTRY REQ-013..REQ-015: `agent_coedited` and `command.event_sources`) and one
  version 1 entry after them, each with its full expected header, so one stream holds both
  versions; `entries` is unchanged by version 2.
- time_order.json: hand-picked cases for the three pinned time rules: instant order against
  the chain head, strict calendar validity, and the 300 s limit ahead of the host clock.
- crash/points.json: the 20 numbered injection points inside `append`, in the order a
  single successful append passes them, each hand-labelled with what a reopen must find
  after the process is killed there (AC-2).

Rules implemented here (spec/core/jrn/chain/append/NODE.md, spec/core/jrn/entry/NODE.md):
- sequences start at 1 and grow by one per stream; `prev` of sequence 1 is `sha256:` + 64
  zeros, else the hash of the previous entry of the stream, whatever its branch;
- commitment = "sha256:" + SHA-256(16 salt bytes || RFC 8785 bytes of the value);
- entry hash = SHA-256 of the RFC 8785 header with `hash` omitted; seq, mono_ns and seed are
  decimal strings;
- schema version: 2 exactly when the draft records `agent_coedited: true` or an event source
  other than `handler`; then the header holds `agent_coedited` and, on a command,
  `command.event_sources` (one per event, all `handler` when the draft gave none). Otherwise
  version 1 without either field;
- the payload (every committed value plus `salts`) is stored as one content-addressed blob,
  so its address is SHA-256 of its RFC 8785 bytes;
- time validity: `time.utc` must be a real calendar instant: month 1-12, a day that exists in
  that month (Gregorian leap years), hour 0-23, minute 0-59, second 0-59, and second 60 only
  at 23:59:60 (a leap second); otherwise `schema`;
- time order: an entry whose `time.utc` instant is earlier than the chain head's is refused
  (`time_regressed`); an equal instant is accepted. Instants are compared as instants on the
  Unix time line (calendar.timegm, so 23:59:60 equals the next day's 00:00:00), not as text:
  "...:00Z" equals "...:00.000Z" and is earlier than "...:00.5Z". `mono_ns` is not compared
  (it restarts with the host);
- time ahead: an entry more than 300 s after the host clock (strictly greater) is refused
  (`time_ahead`); exactly 300 s ahead is accepted.
- crash outcome: the SQLite COMMIT is the only point at which an entry becomes visible; a
  kill at any point before it leaves the chain at the old head, a kill at or after it leaves
  the entry fully present.

Values hold no non-integer floats and no object keys outside the BMP, so
json.dumps(sort_keys=True, separators=(",", ":"), ensure_ascii=False) equals RFC 8785.
CC-BY-4.0 project data.
Run:  python fixtures/core/jrn/chain/append/make_fixtures.py [--check]
"""

import calendar
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
    principal = {"kind": p["kind"], "commit": c("identity", p["identity"])}
    if "on_behalf_of" in p:
        principal["on_behalf_of_commit"] = c("on_behalf_of", p["on_behalf_of"])
    coedited = dr.get("agent_coedited", False)
    sources = dr["command"].get("event_sources", []) if dr["kind"] == "command" else []
    if sources and len(sources) != len(dr["command"]["events"]):
        raise ValueError(f"{name}: one event source per event")
    v2 = coedited is True or any(x != "handler" for x in sources)
    h = {
        "schema_version": 2 if v2 else 1,
        "kind": dr["kind"],
        "stream": dr["stream"],
        "seq": str(seq),
        "prev": prev,
        "branch": dr["branch"],
        "logical_parent": dr["logical_parent"],
        "time": dict(dr["time"]),
        "principal": principal,
        "ai_assistance": dr["ai_assistance"],
        "manifests": dict(dr["manifests"]),
    }
    if "reason" in dr:
        h["reason_commit"] = c("reason", dr["reason"])
    if v2:
        h["agent_coedited"] = coedited
    if dr["kind"] == "command":
        cd = dr["command"]
        h["command"] = {
            "id": cd["id"], "major": cd["major"], "class": cd["class"], "determinism": cd["determinism"],
            "seed": cd["seed"], "args_commit": c("args", cd["args"]), "events_commit": c("events", cd["events"]),
            "inputs": list(cd["inputs"]), "outputs": [dict(o) for o in cd["outputs"]],
            "algorithms": list(cd["algorithms"]),
        }
        if v2:
            h["command"]["event_sources"] = list(sources) or ["handler"] * len(cd["events"])
    elif dr["kind"] == "principal_upgrade":
        u = dr["upgrade"]
        h["upgrade"] = {"session_id": u["session_id"], "method": u["method"],
                        "identity_commit": c("upgrade_identity", u["identity"]), "confirmed_at": u["confirmed_at"]}
    else:
        raise ValueError(dr["kind"])
    h["hash"] = sha(canon(h))
    payload["salts"] = salts
    return h, salts, sha(canon(payload))


def delegated(i):
    """Entries 100..102: delegated attribution and a principal upgrade."""
    dr, content, blob = draft(i)
    if i == 100:
        # An agent acting for a person, with a reason (on_behalf_of_commit, reason_commit).
        dr["principal"] = {"kind": "agent", "identity": "claude-code#7",
                           "on_behalf_of": {"id": "op-17", "kind": "human"}}
        dr["ai_assistance"] = "executed"
        dr["reason"] = "re-run after recalibration"
        return dr, [(content, blob)]
    if i == 101:
        # The bus's upgrade entry (CORE.BUS.PRIN): script principal, confirming OS user.
        up = {k: dr[k] for k in ("stream", "branch", "logical_parent", "time", "ai_assistance", "manifests")}
        up["kind"] = "principal_upgrade"
        up["principal"] = {"kind": "script", "identity": "op-17"}
        up["upgrade"] = {"session_id": "cli#3", "method": "os_presence", "identity": "op-17",
                         "confirmed_at": dr["time"]["utc"]}
        return up, []
    # A script acting on behalf of a person.
    dr["principal"] = {"kind": "script", "identity": "fid-cli#3", "on_behalf_of": {"id": "op-17", "kind": "human"}}
    return dr, [(content, blob)]


def v2_entry(i):
    """Entries 103..106: schema version 2, then a version 1 entry on the same chain."""
    dr, content, blob = draft(i)
    c = dr["command"]
    if i == 103:
        # An agent commit routed to a new agent branch: the route stage adds BranchCreated.
        dr["branch"] = "agent/claude-code/4"
        dr["principal"] = {"kind": "agent", "identity": "claude-code#7"}
        dr["ai_assistance"] = "executed"
        c["events"] = [c["events"][0],
                       {"type": "BranchCreated", "body": {"name": "agent/claude-code/4", "from": ZERO}}]
        c["event_sources"] = ["handler", "bus_route"]
    elif i == 104:
        # An agent commit co-edited onto `main` under a human opt-in; every event the handler's.
        dr["branch"] = "main"
        dr["principal"] = {"kind": "agent", "identity": "claude-code#7"}
        dr["ai_assistance"] = "executed"
        dr["agent_coedited"] = True
    elif i == 105:
        # The first W3 release record on `main`: the commit stage adds release protection.
        dr["branch"] = "main"
        c.update({"id": "report.release", "class": "W3"})
        c["events"] = [{"type": "report.released@1", "n": i},
                       {"type": "BranchProtected", "body": {"name": "main", "cause": "release"}}]
        c["event_sources"] = ["handler", "bus_release"]
    # 106: an ordinary entry, written as version 1 again.
    return dr, [(content, blob)]


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
    for i in range(100, 103):
        name = f"delegated_{i:03d}"
        dr, blobs = delegated(i)
        h, salts, payload_digest = build(name, dr, seq, prev)
        entries.append({
            "name": name, "draft": dr, "salts": salts,
            "blobs": [{"digest": b, "content": c.decode("utf-8")} for c, b in blobs],
            "seq": str(seq), "prev": prev, "hash": h["hash"], "payload_digest": payload_digest, "header": h,
        })
        seq, prev = seq + 1, h["hash"]
    v2_entries = []
    for i in range(103, 107):
        name = f"v2_{i:03d}"
        dr, blobs = v2_entry(i)
        h, salts, payload_digest = build(name, dr, seq, prev)
        v2_entries.append({
            "name": name, "draft": dr, "salts": salts,
            "blobs": [{"digest": b, "content": c.decode("utf-8")} for c, b in blobs],
            "seq": str(seq), "prev": prev, "hash": h["hash"], "payload_digest": payload_digest, "header": h,
        })
        seq, prev = seq + 1, h["hash"]
    return {"stream": STREAM, "entries": entries, "v2_entries": v2_entries}


# ------------------------------------------------------------------ time order

def instant(s):
    """(Unix seconds, nanoseconds) of a valid RFC 3339 UTC instant ending in Z, else None."""
    if len(s) < 20 or not s.endswith("Z") or s[4] != "-" or s[7] != "-" or s[10] != "T" \
            or s[13] != ":" or s[16] != ":":
        return None
    digits = s[0:4] + s[5:7] + s[8:10] + s[11:13] + s[14:16] + s[17:19]
    if not (digits.isascii() and digits.isdigit()):
        return None
    y, mo, d, h, mi, se = int(s[0:4]), int(s[5:7]), int(s[8:10]), int(s[11:13]), int(s[14:16]), int(s[17:19])
    frac = s[19:-1]
    if frac:
        f = frac[1:]
        if frac[0] != "." or not (1 <= len(f) <= 9) or not (f.isascii() and f.isdigit()):
            return None
        nanos = int((f + "0" * 9)[:9])
    else:
        nanos = 0
    if not 1 <= mo <= 12:
        return None
    if not 1 <= d <= calendar.monthrange(y, mo)[1]:
        return None
    if h > 23 or mi > 59 or se > 60 or (se == 60 and (h, mi) != (23, 59)):
        return None
    return calendar.timegm((y, mo, d, h, mi, se, 0, 0, 0)), nanos


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


MALFORMED = [
    ("month_13", "2026-13-01T00:00:00Z"),
    ("month_00", "2026-00-10T00:00:00Z"),
    ("day_00", "2026-11-00T00:00:00Z"),
    ("november_31", "2026-11-31T00:00:00Z"),
    ("february_29_common_year", "2026-02-29T00:00:00Z"),
    ("february_29_1900", "1900-02-29T00:00:00Z"),
    ("hour_24", "2026-11-19T24:00:00Z"),
    ("minute_60", "2026-11-19T10:60:00Z"),
    ("second_61", "2026-11-19T23:59:61Z"),
    ("second_60_not_at_end_of_day", "2026-11-19T10:00:60Z"),
    ("no_z", "2026-11-19T10:00:00"),
    ("offset", "2026-11-19T10:00:00+00:00"),
    ("empty_fraction", "2026-11-19T10:00:00.Z"),
    ("ten_fraction_digits", "2026-11-19T10:00:00.1234567890Z"),
    ("space_separator", "2026-11-19 10:00:00Z"),
    ("lowercase_t", "2026-11-19t10:00:00Z"),
]
VALID = [
    ("february_29_leap_year", "2028-02-29T00:00:00Z"),
    ("february_29_2000", "2000-02-29T12:00:00Z"),
    ("leap_second", "2016-12-31T23:59:60Z"),
    ("nine_fraction_digits", "2026-11-19T10:00:00.999999999Z"),
]
# (name, clock instant, entry instant): the host clock is given as Unix seconds below.
AHEAD = [
    ("behind_clock", "2026-11-19T10:00:00Z", "2026-11-19T09:00:00Z"),
    ("at_clock", "2026-11-19T10:00:00Z", "2026-11-19T10:00:00Z"),
    ("exactly_300_s_ahead", "2026-11-19T10:00:00Z", "2026-11-19T10:05:00Z"),
    ("300_s_and_1_ns_ahead", "2026-11-19T10:00:00Z", "2026-11-19T10:05:00.000000001Z"),
    ("one_day_ahead", "2026-11-19T10:00:00Z", "2026-11-20T10:00:00Z"),
    ("far_future", "2026-11-19T10:00:00Z", "9999-12-31T23:59:59Z"),
]


def time_order():
    order = []
    for name, head, entry in TIME_CASES:
        accept = instant(entry) >= instant(head)
        order.append({"name": name, "head": head, "entry": entry, "accept": accept})
    validity = [{"name": n, "utc": u, "valid": instant(u) is not None} for n, u in MALFORMED + VALID]
    ahead = []
    for name, clock, entry in AHEAD:
        now = instant(clock)[0]
        e = instant(entry)
        accept = e[0] * 10**9 + e[1] <= (now + 300) * 10**9
        ahead.append({"name": name, "clock_unix": str(now), "entry": entry, "accept": accept})
    return {"max_ahead_s": 300, "order": order, "validity": validity, "ahead": ahead}


# ------------------------------------------------------------------ crash injection points

# Hand-labelled: the order a single successful append with one referenced blob passes them,
# and what a reopen finds after a kill there. COMMIT (point 18) is the linearisation point.
POINTS = [
    "prechecked", "batch_taken", "lock_verified", "begun", "tail_read", "head_checked", "built",
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
