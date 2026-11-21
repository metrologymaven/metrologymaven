#!/usr/bin/env python3
"""Generate the identity-read oracle of CORE.PRJ.CONT.LIFE (AC-6, AC-7; REQ-008 to REQ-011).

An independent reference written with Python's standard library only (hashlib, json). It
builds small journals of stream `record` (entry headers with their payloads and the record
blobs they commit to) from its own reading of the spec (spec/core/prj/cont/life, "Project
identity"; spec/core/jrn/entry for the header and commitments; spec/core/rpl/recon/fold
REQ-008), hashes them with its own RFC 8785 writer and hashlib, and labels the expected
outcome of every case BY HAND from the spec's rules. Nothing here runs or copies the Rust
crate `fid-project`; the Rust test feeds these journals to `fid_project::lifecycle` and must
reach the labelled outcome.

File written next to this script (CC-BY-4.0): identity_cases.json, with

- `cases`: each `{name, rule, entries, blobs, expect}`. `entries` are in sequence order from 1,
  each `{header, payload, promoted_from}`: the header and payload as their RFC 8785 text
  (payload null when removed), promoted_from a digest or null. `blobs` maps a digest to the
  stored bytes (ASCII text). `expect` is `{"ok": {...}}` (the identity read: project,
  namespace, source, genesis sequence, the lineage of earlier projects and the copies named by
  source-side save-as entries) or `{"refused": {"code": ..., "seq": ...}}`.

The rules the labels follow (the verifier's contract, module docs of fid_project::lifecycle):
an identity entry is one whose command is project.create_project@1 or
project.save_project_as@1, or whose payload holds ProjectIdentified, ProjectCopiedFrom or
ProjectSavedAs, or whose header output names a stored record blob. In order: 1 the command
owns it (foreign_command); 2 branch main (off_main), not a promoted copy (promoted); 3 payload
stored (payload_missing); 4 no Request* event (request); 5 exactly one output, record and
event, class W2 (create) or W1 (save-as), D0, output entity the record's project (bad_shape);
6 command, record and event belong together (record_kind); 7 the event body is the record
(body_mismatch); 8 a genesis directly follows a session header on main (not_after_session),
is the only genesis of that session_id (second_genesis) and has its principal
(principal_mismatch); 9 a create genesis is at sequence 2 after the session header at 1
(create_position); 10 a copy genesis names the current identity and its session header's prev
as source, keeps the namespace and has a new project ID (source_mismatch); 11 a source-side
save-as names the current identity (wrong_project), has head == its own prev (stale_head) and
a new copy ID (duplicate_copy). Every refusal is fatal and names the entry's sequence.

Run:  python fixtures/core/prj/cont/life/make_identity_cases.py [--check]
"""

import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STREAM = "record"
ZERO = "sha256:" + "0" * 64
REGISTRY = "sha256:" + hashlib.sha256(b"registry manifest v1").hexdigest()
KERNEL = "sha256:" + hashlib.sha256(b"kernel manifest v1").hexdigest()

CREATE = "project.create_project"
SAVE_AS = "project.save_project_as"
IDENTITY_FORMAT = "fid-project-identity/1"
SAVED_AS_FORMAT = "fid-project-saved-as/1"

# Project IDs are UUIDv7 (version nibble 7, variant 10); the namespace is a random UUIDv4.
P1 = "018f3a10-0000-7000-8000-000000000001"
P2 = "018f3a10-0000-7000-8000-000000000002"
P3 = "018f3a10-0000-7000-8000-000000000003"
P9 = "018f3a10-0000-7000-8000-000000000009"
NS = "6f1c2a4e-5b3d-4c2a-9e1f-00000000000a"

HUMAN = {"name": "ada"}
AGENT = {"client": "claude", "id": "agent-1"}


# ------------------------------------------------------------------ RFC 8785 (own writer)

def esc(s):
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\n":
            out.append("\\n")
        elif o < 0x20:
            out.append("\\u%04x" % o)
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def canon(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        assert abs(v) <= 2 ** 53
        return str(v)
    if isinstance(v, str):
        return esc(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v.keys(), key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(esc(k) + ":" + canon(v[k]) for k in keys) + "}"
    raise TypeError(type(v).__name__)


def cbytes(v):
    return canon(v).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


# ------------------------------------------------------------------ records

def identity_record(project, namespace, source=None):
    r = {"format": IDENTITY_FORMAT, "namespace": namespace, "project": project}
    if source is not None:
        r["source"] = {"head": source[1], "project": source[0]}
    return r


def saved_as_record(project, copy, head):
    return {"copy": copy, "format": SAVED_AS_FORMAT, "head": head, "project": project}


# ------------------------------------------------------------------ journal builder

class Journal:
    """Appends entries of stream `record`, hashing each header like CORE.JRN.ENTRY does."""

    def __init__(self, case):
        self.case = case
        self.entries = []
        self.blobs = {}
        self.prev = ZERO
        self.hashes = {}

    def salt(self, seq, field):
        return hashlib.sha256(("%s/%d/%s" % (self.case, seq, field)).encode()).hexdigest()[:32]

    @staticmethod
    def commit(salt_hex, value):
        return sha(bytes.fromhex(salt_hex) + cbytes(value))

    def base(self, seq, kind, branch, who):
        identity = HUMAN if who == "human" else AGENT
        # One identity salt per principal and journal: a genesis commits its principal with
        # its session header's salt, so equal principals have equal commitments (rule 8).
        s_id = self.salt(0, "identity/" + who)
        h = {
            "schema_version": 1,
            "kind": kind,
            "stream": STREAM,
            "seq": str(seq),
            "prev": self.prev,
            "branch": branch,
            "logical_parent": None,
            "time": {"utc": "2026-11-21T10:%02d:00Z" % seq, "mono_ns": str(1_000_000_000 * seq), "sync": "synced"},
            "principal": {"kind": who, "commit": self.commit(s_id, identity)},
            "ai_assistance": "authored" if who == "agent" else "none",
            "manifests": {"registry": REGISTRY, "kernel": KERNEL},
        }
        payload = {"salts": {"identity": s_id}, "identity": identity}
        return h, payload

    def push(self, h, payload, promoted_from=None, drop_payload=False):
        h["hash"] = sha(cbytes(h))
        seq = int(h["seq"])
        self.hashes[seq] = h["hash"]
        self.entries.append({
            "header": canon(h),
            "payload": None if drop_payload else canon(payload),
            "promoted_from": promoted_from,
        })
        self.prev = h["hash"]
        return h["hash"]

    def seq(self):
        return len(self.entries) + 1

    def session(self, session_id, who="human", branch="main"):
        seq = self.seq()
        h, p = self.base(seq, "session_header", branch, who)
        h["session"] = {"session_id": session_id, "app_version": "0.1.0"}
        return self.push(h, p)

    def command(self, cid, events, outputs, cls="W2", who="human", branch="main", args=None,
                promoted_from=None, drop_payload=False):
        """events: list of (type, body); outputs: list of (entity or None, digest)."""
        seq = self.seq()
        h, p = self.base(seq, "command", branch, who)
        args = {} if args is None else args
        evs = [{"body": b, "type": t} for t, b in events]
        s_args, s_events = self.salt(seq, "args"), self.salt(seq, "events")
        outs = []
        for ent, d in outputs:
            o = {"digest": d}
            if ent is not None:
                o["entity"] = ent
            outs.append(o)
        h["command"] = {
            "id": cid,
            "major": 1,
            "class": cls,
            "determinism": "D0",
            "seed": "0",
            "args_commit": self.commit(s_args, args),
            "events_commit": self.commit(s_events, evs),
            "inputs": [],
            "outputs": outs,
            "algorithms": [],
        }
        p["salts"]["args"] = s_args
        p["salts"]["events"] = s_events
        p["args"] = args
        p["events"] = evs
        return self.push(h, p, promoted_from, drop_payload)

    def blob(self, value):
        b = cbytes(value)
        d = sha(b)
        self.blobs[d] = b.decode("ascii")
        return d

    # --- the lifecycle entries, as the commands write them

    def create(self, record, event_body=None, who="human", extra_events=()):
        d = self.blob(record)
        body = record if event_body is None else event_body
        return self.command(CREATE, [("ProjectIdentified", body)] + list(extra_events),
                            [(record["project"], d)], cls="W2", who=who, args={"label": "P1", "target": "root:work/p1"})

    def copy_genesis(self, record, who="human"):
        d = self.blob(record)
        return self.command(SAVE_AS, [("ProjectCopiedFrom", record)], [(record["project"], d)], cls="W1", who=who,
                            args={"target": "root:exports/p_copy"})

    def saved_as(self, record, event="ProjectSavedAs", branch="main", promoted_from=None):
        d = self.blob(record)
        return self.command(SAVE_AS, [(event, record)], [(record["project"], d)], cls="W1", branch=branch,
                            promoted_from=promoted_from, args={"target": "root:exports/p_copy"})

    def work(self, n, entity=None):
        """A document command: one entity output whose digest is a state digest (not a blob)."""
        ent = entity or ("e-%d" % n)
        return self.command("project.create_part", [("ContainerCreated", {"id": ent, "n": n})],
                            [(ent, sha(("state %s %d" % (ent, n)).encode()))])


def created(case):
    """S1 and the create genesis G2 of P1, then one document command: the head at 3."""
    j = Journal(case)
    j.session("s-%s-1" % case)
    j.create(identity_record(P1, NS))
    j.work(3)
    return j


def ok(project, source=None, genesis_seq=2, lineage=(), copies=()):
    return {"ok": {
        "project": project,
        "namespace": NS,
        "source": None if source is None else {"project": source[0], "head": source[1]},
        "genesis_seq": genesis_seq,
        "lineage": list(lineage),
        "saved_as": list(copies),
    }}


def refused(code, seq):
    return {"refused": {"code": code, "seq": seq}}


# ------------------------------------------------------------------ cases

def build_cases():
    cases = []

    def add(name, rule, j, expect):
        cases.append({"name": name, "rule": rule, "entries": j.entries, "blobs": j.blobs, "expect": expect})

    # --- valid journals (AC-6)

    j = Journal("create_valid")
    j.session("s-create-1")
    j.create(identity_record(P1, NS))
    j.work(3)
    # A blob output that is JSON but not a record is not a record.
    j.command("project.create_part", [("ContainerCreated", {"id": "e-4"})], [("e-4", j.blob({"a": 1}))])
    add("create_valid", "S1 then the create genesis at 2: P1, no source", j, ok(P1))

    j = created("source_after_save_as")
    h = j.hashes[3]
    j.saved_as(saved_as_record(P1, P2, h))
    add("source_after_save_as", "the source keeps its identity; ProjectSavedAs names the copy", j,
        ok(P1, copies=[P2]))

    j = created("copy_valid")
    h = j.hashes[3]
    j.session("s-copy-valid-2")
    j.copy_genesis(identity_record(P2, NS, (P1, h)))
    add("copy_valid", "the copy's genesis follows its own session header whose prev is source.head", j,
        ok(P2, source=(P1, h), genesis_seq=5, lineage=[P1]))

    j = created("copy_after_earlier_save_as")
    j.saved_as(saved_as_record(P1, P2, j.hashes[3]))
    j.work(5)
    h = j.hashes[5]
    j.session("s-copy-after-2")
    j.copy_genesis(identity_record(P3, NS, (P1, h)))
    add("copy_after_earlier_save_as", "a second copy holds the first save-as entry in its prefix", j,
        ok(P3, source=(P1, h), genesis_seq=7, lineage=[P1], copies=[P2]))

    # --- forged journals (AC-7), one rule each

    j = created("effect_under_other_command")
    j.command("project.create_part", [("ProjectIdentified", identity_record(P9, NS))], [("e-4", sha(b"state e-4"))])
    add("effect_under_other_command", "(a)(b) an identity effect under another command", j,
        refused("foreign_command", 4))

    j = Journal("body_claims_other_project")
    j.session("s-body-1")
    j.create(identity_record(P1, NS), event_body=identity_record(P9, NS))
    add("body_claims_other_project", "(a) an event body claiming another project ID than the record", j,
        refused("body_mismatch", 2))

    j = created("second_genesis_same_session")
    h = j.hashes[3]
    j.session("s-second_genesis_same_session-1")
    j.copy_genesis(identity_record(P2, NS, (P1, h)))
    add("second_genesis_same_session", "(d) a second genesis in the same session", j,
        refused("second_genesis", 5))

    j = created("genesis_not_after_session")
    j.copy_genesis(identity_record(P2, NS, (P1, j.hashes[3])))
    add("genesis_not_after_session", "(d) a genesis that does not directly follow a session header", j,
        refused("not_after_session", 4))

    j = created("copy_session_prev_not_source_head")
    h = j.hashes[3]
    j.work(4)
    j.session("s-copy-prev-2")
    j.copy_genesis(identity_record(P2, NS, (P1, h)))
    add("copy_session_prev_not_source_head", "(d) a copy genesis whose session header's prev is not source.head",
        j, refused("source_mismatch", 6))

    j = created("source_reidentified")
    j.saved_as(identity_record(P2, NS, (P1, j.hashes[3])))
    add("source_reidentified", "(b) a source-side save-as whose record claims fid-project-identity/1", j,
        refused("record_kind", 4))

    j = created("replayed_saved_as")
    rec = saved_as_record(P1, P2, j.hashes[3])
    j.saved_as(rec)
    j.work(5)
    j.saved_as(rec)
    add("replayed_saved_as", "(d) a replayed ProjectSavedAs", j, refused("stale_head", 6))

    j = Journal("genesis_other_principal")
    j.session("s-principal-1", who="human")
    j.create(identity_record(P1, NS), who="agent")
    add("genesis_other_principal", "(a) a genesis whose principal differs from its session header's", j,
        refused("principal_mismatch", 2))

    j = created("identity_effect_off_main")
    j.saved_as(saved_as_record(P1, P2, j.hashes[3]), branch="side")
    add("identity_effect_off_main", "(e) an identity effect on a branch other than main", j,
        refused("off_main", 4))

    j = Journal("identity_with_request")
    j.session("s-request-1")
    j.create(identity_record(P1, NS), extra_events=[("RequestFiled", {"command": CREATE + "@1"})])
    add("identity_with_request", "(c) an identity entry also carrying a Request* event", j,
        refused("request", 2))

    # --- two more forged journals beyond the ten of AC-7

    j = created("promoted_saved_as")
    j.saved_as(saved_as_record(P1, P2, j.hashes[3]), promoted_from=j.hashes[3])
    add("promoted_saved_as", "(e) an identity entry in a promoted copy", j, refused("promoted", 4))

    j = created("corrupt_record_blob")
    rec = saved_as_record(P1, P2, j.hashes[3])
    d = j.blob(rec)
    j.blobs[d] = canon(saved_as_record(P1, P3, j.hashes[3]))
    j.command(SAVE_AS, [("ProjectSavedAs", rec)], [(P1, d)], cls="W1")
    add("corrupt_record_blob", "a record blob that does not hash to its output digest", j,
        refused("record_corrupt", 4))

    return cases


def build():
    cases = build_cases()
    names = [c["name"] for c in cases]
    assert len(names) == len(set(names))
    forged = [c for c in cases if "refused" in c["expect"]]
    assert len(forged) == 12
    doc = {
        "license": "CC-BY-4.0",
        "generator": "fixtures/core/prj/cont/life/make_identity_cases.py",
        "cases": cases,
    }
    return {"identity_cases": (canon(doc) + "\n").encode("utf-8")}


def main():
    files = build()
    stale = []
    for name, data in files.items():
        path = HERE / (name + ".json")
        if "--check" in sys.argv[1:]:
            if not path.exists() or path.read_bytes() != data:
                stale.append(path.name)
        else:
            path.write_bytes(data)
            print("wrote " + path.name)
    if stale:
        print("stale fixture (run make_identity_cases.py): " + ", ".join(stale))
        sys.exit(1)
    if "--check" in sys.argv[1:]:
        print("fixtures up to date")


if __name__ == "__main__":
    main()
