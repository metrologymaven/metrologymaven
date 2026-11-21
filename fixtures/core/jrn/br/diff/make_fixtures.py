#!/usr/bin/env python3
"""Generate the fixtures of CORE.JRN.BR.DIFF (entity and result diff between branches).

An independent reference written with Python's standard library only (hashlib, json). It
builds a small physical journal (entry rows with their header bytes) and a branch table from
its own reading of the spec (spec/core/jrn/entry, spec/core/jrn/chain/{append,verify},
spec/core/jrn/br/{name,diff}), hashes it with its own RFC 8785 writer and hashlib, and runs
its own header-only chain check and its own diff over the journal and every tampered or
forged variant. Nothing here is produced by, or copied from, the Rust crate `fid-journal`;
the Rust tests load these files and must reach the same outcome.

Files written next to this script (CC-BY-4.0):

- fork.json: the `fork` journal of AC-1 (rows of stream `record` in sequence order, each with
  its stored header bytes and its entry_blobs rows) and the branch table (name and
  created_from of every branch, in creation order).
- cases.json: `diffs`, branch pairs over the untouched journal with the expected result value
  of `journal.diff_branch@1` or the expected refusal; `tampers`, lists of mutations of the
  journal or the table (the mutation is input, not an expected value), a branch pair, an
  optional trusted head, and the expected outcome computed here.

Rules (the contract this reference and the Rust diff share):

Verification, headers only. Rows in sequence order from sequence 1, the first linking to the
zero digest. Per row: column seq equal to the position (else sequence/gap or
sequence/reorder); the header strict JSON, canonical, with `hash` equal to SHA-256 of the
canonical header without `hash` (hash/hash_mismatch); the hash column, header seq, header
stream, and the stream, branch and kind columns equal to the header (hash/column_mismatch,
sequence/header_seq, sequence/foreign_stream); the prev_hash column equal to header.prev
(link/column_mismatch); header.prev equal to the hash of the entry before (link/broken, or
link/genesis at sequence 1). Payload and referenced blobs are never read. A trusted head
(seq, head, exact): with exact, any row after it is sequence/extended; at its sequence a hash
other than its head is checkpoint/forked_head; after the last row, a trusted head above it is
checkpoint/truncated at its sequence. A failure refuses the call as
internal/invariant_violated, carrying the failure's seq, kind and reason.

Logical DAG. Every verified entry is a node; its parent is header.logical_parent, which must
name an entry verified before it (else the call is refused, internal/invariant_violated). A
head_move's parent is its head_move.to: `to` must name an entry verified before it and equal
its logical_parent (else internal/invariant_violated), so undo and redo walk through `to`. A
branch must be in the branch table (else not_found/unknown_branch). Its head is the verified
entry with the highest sequence whose header.branch is the branch and whose kind is `command`
or `head_move` (session headers and principal upgrades are not logical states); a branch with
no such entry is at its created_from, which must name a verified entry (else
internal/invariant_violated); a created_from naming a session header or principal upgrade
resolves to the last state entry of that entry's branch at or before it (none: the empty
state); `main` with neither has no head (the empty state). The ancestor
is the first node of the source head's parent path that is also on the target head's parent
path (null when the paths do not meet).

Fold. The base is the target path from the ancestor to its root; each side is its path above
the ancestor. Each is folded in sequence order over the entity-tagged outputs of `command`
entries (an output without an entity is ignored; within one entry the last output for an
entity wins); a digest of 64 zeros (the tombstone) means deleted. For every entity touched on
either side: base, source and target values (null when absent or deleted); a side changed it
when its value differs from the base. Neither changed: not listed. Only the source changed:
`added` if the base is null, `removed` if the source value is null, else `changed`. Otherwise
`changed`, with `conflict` true when both changed it to different values. Each delta carries
both values and, per side, the commands that output the entity since the ancestor, in
sequence order, each as {entry, seq, command "id@major", principal}: the principal kind is the
verified header's, never anything from an event body or payload. Lists are sorted by entity ID
(byte order). Results also name each branch with its head and its owner (agent for the agent/
namespace, else human).

Run:  python fixtures/core/jrn/br/diff/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STREAM = "record"
ZERO = "sha256:" + "0" * 64
TOMBSTONE = ZERO
STATE_KINDS = ("command", "head_move")


# ------------------------------------------------------------------ RFC 8785 (own writer)

def esc(s):
    out = ['"']
    for ch in s:
        o = ord(ch)
        if ch == '"':
            out.append('\\"')
        elif ch == "\\":
            out.append("\\\\")
        elif ch == "\b":
            out.append("\\b")
        elif ch == "\f":
            out.append("\\f")
        elif ch == "\n":
            out.append("\\n")
        elif ch == "\r":
            out.append("\\r")
        elif ch == "\t":
            out.append("\\t")
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


def no_dups(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise ValueError("duplicate key")
        d[k] = v
    return d


# ------------------------------------------------------------------ the fork journal

def val(entity, version):
    """The output digest of an entity at a version."""
    return sha(("%s %s" % (entity, version)).encode())


NON_ASCII = ["zeta", "\u00c4ra", "\u00c9mile", "\u00e9mile", "e10", "\uff5e", "\U0001f600"]

REGISTRY = sha(b"registry manifest v1")
KERNEL = sha(b"kernel manifest v1")

# seq: (branch, principal, kind, command, logical parent seq or None, outputs)
# An output is (entity or None, digest).
PLAN = [
    (1, "main", "human", "session_header", None, None, []),
    (2, "main", "human", "command", "project.add_features", None, [("e1", val("e1", 1)), ("e2", val("e2", 1))]),
    (3, "main", "human", "command", "project.add_features", 2,
     [("e3", val("e3", 1)), ("e4", val("e4", 1)), ("e5", val("e5", 1))]),
    (4, "main", "human", "command", "journal.create_branch", 3, []),
    (5, "s", "agent", "session_header", None, None, []),
    (6, "s", "agent", "command", "feature.add", 3, [("e6", val("e6", 1))]),
    (7, "main", "human", "command", "feature.set_nominal", 4, [("e4", val("e4", "t"))]),
    (8, "s", "agent", "command", "feature.edit", 6, [("e2", TOMBSTONE), ("e3", val("e3", "s"))]),
    (9, "main", "human", "command", "feature.set_nominal", 7, [("e3", val("e3", "t"))]),
    (10, "s", "agent", "command", "feature.set_nominal", 8, [("e5", val("e5", "x")), (None, sha(b"report 10"))]),
    (11, "s", "human", "command", "feature.set_nominal", 10, [("e5", val("e5", 1))]),
    (12, "main", "human", "session_header", None, None, []),
    (13, "orphan", "human", "command", "project.add_features", None, [("e9", val("e9", 1))]),
    (14, "agent/claude/1", "agent", "command", "feature.set_nominal", 9,
     [("e1", val("e1", "a")), ("e1", val("e1", "a2"))]),
    # Entity IDs whose UTF-8 byte order differs from UTF-16 order (U+FF5E before U+1F600).
    (15, "x", "human", "command", "project.add_features", 9,
     [(e, val(e, 1)) for e in NON_ASCII]),
    # Undo on w: back from the state at 9 to the state at 7. A head_move's logical parent is its `to`.
    (16, "w", "human", "head_move", (9, 7), 7, []),
    # Undo then redo on w2.
    (17, "w2", "human", "head_move", (9, 7), 7, []),
    (18, "w2", "human", "head_move", (7, 9), 9, []),
]


def build_fork():
    entries = []
    hashes = {}
    prev = ZERO
    for seq, branch, pk, kind, cid, parent, outputs in PLAN:
        h = {
            "schema_version": 1,
            "kind": kind,
            "stream": STREAM,
            "seq": str(seq),
            "prev": prev,
            "branch": branch,
            "logical_parent": None if parent is None else hashes[parent],
            "time": {"utc": "2026-11-20T09:%02d:00Z" % seq, "mono_ns": str(1_000_000_000 * seq), "sync": "synced"},
            "principal": {"kind": pk, "commit": sha(b"identity %d" % seq)},
            "ai_assistance": "authored" if pk == "agent" else "none",
            "manifests": {"registry": REGISTRY, "kernel": KERNEL},
        }
        if kind == "session_header":
            h["session"] = {"session_id": "s-%04d" % seq, "app_version": "0.1.0"}
        elif kind == "head_move":
            h["head_move"] = {"from": hashes[cid[0]], "to": hashes[cid[1]]}
        else:
            outs = []
            for ent, d in outputs:
                o = {"digest": d}
                if ent is not None:
                    o["entity"] = ent
                outs.append(o)
            h["command"] = {
                "id": cid,
                "major": 1,
                "class": "W2",
                "determinism": "D0",
                "seed": str(seq),
                "args_commit": sha(b"args %d" % seq),
                "events_commit": sha(b"events %d" % seq),
                "inputs": [],
                "outputs": outs,
                "algorithms": [],
            }
        h["hash"] = sha(cbytes(h))
        hashes[seq] = h["hash"]
        entries.append({
            "seq": seq,
            "hash": h["hash"],
            "stream": STREAM,
            "branch": branch,
            "kind": kind,
            "prev_hash": prev,
            "header": canon(h),
            "refs": [["payload", sha(b"payload %d" % seq)]],
        })
        prev = h["hash"]
    # The branch table in creation order. `main` is the genesis branch (created_from null);
    # agent/claude/1 comes from the bus routing module, the others from journal.create_branch.
    branches = [
        {"name": "main", "created_from": None},
        {"name": "s", "created_from": hashes[3]},
        {"name": "orphan", "created_from": hashes[3]},
        {"name": "t2", "created_from": hashes[9]},
        {"name": "agent/claude/1", "created_from": hashes[9]},
        {"name": "u", "created_from": hashes[12]},
        {"name": "x", "created_from": hashes[9]},
        {"name": "w", "created_from": hashes[9]},
        {"name": "w2", "created_from": hashes[9]},
    ]
    return {"stream": STREAM, "entries": entries, "branches": branches}, hashes


# ------------------------------------------------------------------ the model and mutations

def to_model(fork):
    rows = [{"seq": e["seq"], "hash": e["hash"], "stream": e["stream"], "branch": e["branch"], "kind": e["kind"],
             "prev_hash": e["prev_hash"], "header": e["header"].encode("utf-8"), "refs": [list(r) for r in e["refs"]]}
            for e in fork["entries"]]
    return {"rows": rows, "branches": copy.deepcopy(fork["branches"])}


def row_at(m, seq):
    for r in m["rows"]:
        if r["seq"] == seq:
            return r
    raise KeyError(seq)


def rewrite(m, seq, edit, relink):
    """Edits the header of `seq` and re-hashes it; with relink, re-chains every later row on
    it (prev, and any logical_parent naming a rewritten hash), as an editor of the file can."""
    renamed = {}
    for r in sorted(m["rows"], key=lambda r: r["seq"]):
        if r["seq"] < seq:
            continue
        if r["seq"] > seq and not relink:
            break
        h = json.loads(r["header"].decode("utf-8"))
        if r["seq"] == seq:
            edit(h)
        else:
            h["prev"] = renamed.get(h["prev"], h["prev"])
        if h.get("logical_parent") in renamed:
            h["logical_parent"] = renamed[h["logical_parent"]]
        if "head_move" in h:
            for k in ("from", "to"):
                h["head_move"][k] = renamed.get(h["head_move"][k], h["head_move"][k])
        old = h.pop("hash")
        h["hash"] = sha(cbytes(h))
        renamed[old] = h["hash"]
        r["header"] = cbytes(h)
        r["hash"] = h["hash"]
        r["prev_hash"] = h["prev"]


def apply(m, muts):
    m = copy.deepcopy(m)
    for mu in muts:
        op = mu["op"]
        if op == "set_header_field":
            # Edits header.<path> and keeps the stored hash (no re-hash).
            r = row_at(m, mu["seq"])
            h = json.loads(r["header"].decode("utf-8"))
            o = h
            for k in mu["path"][:-1]:
                o = o[k]
            o[mu["path"][-1]] = mu["value"]
            r["header"] = cbytes(h)
        elif op == "rewrite_field":
            # Edits header.<path>, re-hashes, and re-chains every later row on it.
            def edit(h, mu=mu):
                o = h
                for k in mu["path"][:-1]:
                    o = o[k]
                o[mu["path"][-1]] = mu["value"]
            rewrite(m, mu["seq"], edit, True)
        elif op == "set_column":
            row_at(m, mu["seq"])[mu["column"]] = mu["value"]
        elif op == "drop":
            m["rows"] = [r for r in m["rows"] if r["seq"] != mu["seq"]]
        elif op == "set_refs":
            for r in m["rows"]:
                r["refs"] = [list(x) for x in mu["refs"]]
        elif op == "set_created_from":
            for b in m["branches"]:
                if b["name"] == mu["branch"]:
                    b["created_from"] = mu["value"]
        else:
            raise ValueError(op)
    return m


# ------------------------------------------------------------------ the reference

class Refused(Exception):
    def __init__(self, code, reason, failure=None):
        super().__init__(code + "/" + reason)
        self.code, self.reason, self.failure = code, reason, failure


def fail(seq, kind, reason):
    return Refused("internal", "invariant_violated", {"seq": str(seq), "kind": kind, "reason": reason})


def verified(m, trusted):
    """The header-only chain check: the verified headers in sequence order."""
    out = []
    prev = ZERO
    p = 1
    for r in m["rows"]:
        if trusted and trusted["exact"] and p > int(trusted["seq"]):
            raise fail(p, "sequence", "extended")
        if r["seq"] != p:
            raise fail(p, "sequence", "gap" if r["seq"] > p else "reorder")
        b = r["header"]
        h = json.loads(b.decode("utf-8"), object_pairs_hook=no_dups)
        if cbytes(h) != b:
            raise fail(p, "hash", "non_canonical")
        bare = dict(h)
        recorded = bare.pop("hash")
        if sha(cbytes(bare)) != recorded:
            raise fail(p, "hash", "hash_mismatch")
        if r["hash"] != recorded:
            raise fail(p, "hash", "column_mismatch")
        if h["seq"] != str(p):
            raise fail(p, "sequence", "header_seq")
        if h["stream"] != STREAM:
            raise fail(p, "sequence", "foreign_stream")
        for col in ("stream", "branch", "kind"):
            if r[col] != h[col]:
                raise fail(p, "hash", "column_mismatch")
        if r["prev_hash"] != h["prev"]:
            raise fail(p, "link", "column_mismatch")
        if h["prev"] != prev:
            raise fail(p, "link", "genesis" if p == 1 else "broken")
        if trusted and int(trusted["seq"]) == p and trusted["head"] != recorded:
            raise fail(p, "checkpoint", "forked_head")
        out.append(h)
        prev = recorded
        p += 1
    if trusted and int(trusted["seq"]) >= p:
        raise fail(int(trusted["seq"]), "checkpoint", "truncated")
    return out


def tampered():
    return Refused("internal", "invariant_violated")


def live(d):
    return None if d is None or d == TOMBSTONE else d


def diff(m, source, target, trusted=None):
    table = {b["name"]: b for b in m["branches"]}
    for name in (source, target):
        if name not in table:
            raise Refused("not_found", "unknown_branch")
    headers = verified(m, trusted)
    nodes = {}
    state = {}
    last_by_branch = {}
    for h in headers:
        lp = h["logical_parent"]
        if lp is not None and lp not in nodes:
            raise tampered()
        if h["kind"] == "head_move" and (h["head_move"]["to"] not in nodes or lp != h["head_move"]["to"]):
            raise tampered()
        nodes[h["hash"]] = h
        if h["kind"] in STATE_KINDS:
            last_by_branch[h["branch"]] = h["hash"]
            state[h["hash"]] = h["hash"]
        else:
            state[h["hash"]] = last_by_branch.get(h["branch"])

    def head_of(name):
        best = None
        for h in headers:
            if h["branch"] == name and h["kind"] in STATE_KINDS:
                best = h["hash"]
        if best is not None:
            return best
        cf = table[name]["created_from"]
        if cf is None:
            return None
        if cf not in nodes:
            raise tampered()
        return state[cf]

    def path(hd):
        out = []
        while hd is not None:
            out.append(hd)
            hd = nodes[hd]["logical_parent"]
        return out

    sh, th = head_of(source), head_of(target)
    tpath = path(th)
    tset = set(tpath)
    spath = []
    ancestor = None
    for x in path(sh):
        if x in tset:
            ancestor = x
            break
        spath.append(x)
    if ancestor is None:
        tside, base = tpath, []
    else:
        i = tpath.index(ancestor)
        tside, base = tpath[:i], tpath[i:]

    def fold(hashes, with_commands):
        state = {}
        for x in sorted(hashes, key=lambda x: int(nodes[x]["seq"])):
            h = nodes[x]
            if h["kind"] != "command":
                continue
            c = h["command"]
            last = {}
            for o in c["outputs"]:
                if "entity" in o:
                    last[o["entity"]] = o["digest"]
            ref = {"entry": x, "seq": h["seq"], "command": "%s@%d" % (c["id"], c["major"]),
                   "principal": h["principal"]["kind"]}
            for ent, d in last.items():
                cur = state.setdefault(ent, {"digest": None, "commands": []})
                cur["digest"] = d
                if with_commands:
                    cur["commands"].append(ref)
        return state

    b = fold(base, False)
    s = fold(spath, True)
    t = fold(tside, True)
    added, removed, changed = [], [], []
    for ent in sorted(set(s) | set(t), key=lambda k: k.encode("utf-8")):
        bv = live(b[ent]["digest"]) if ent in b else None
        sv = live(s[ent]["digest"]) if ent in s else bv
        tv = live(t[ent]["digest"]) if ent in t else bv
        sc, tc = sv != bv, tv != bv
        if not sc and not tc:
            continue
        delta = {
            "id": ent,
            "target_digest": tv,
            "source_digest": sv,
            "target_commands": t[ent]["commands"] if ent in t else [],
            "source_commands": s[ent]["commands"] if ent in s else [],
            "conflict": sc and tc and sv != tv,
        }
        if sc and not tc and bv is None:
            added.append(delta)
        elif sc and not tc and sv is None:
            removed.append(delta)
        else:
            changed.append(delta)

    def side(name, hd):
        return {"branch": name, "head": hd, "owner": "agent" if name.startswith("agent/") else "human"}

    return {"ancestor": ancestor, "source": side(source, sh), "target": side(target, th),
            "added": added, "removed": removed, "changed": changed}


def outcome(m, source, target, trusted=None):
    try:
        return {"result": diff(m, source, target, trusted)}
    except Refused as e:
        err = {"code": e.code, "reason": e.reason}
        if e.failure is not None:
            err["failure"] = e.failure
        return {"error": err}


# ------------------------------------------------------------------ cases

DIFF_PAIRS = [
    ("ac1_fork", "s", "main"),
    ("ac2_no_changes", "t2", "main"),
    ("swapped", "main", "s"),
    ("agent_branch", "agent/claude/1", "main"),
    ("main_vs_agent_branch", "main", "agent/claude/1"),
    ("disjoint", "orphan", "main"),
    ("same_branch", "s", "s"),
    ("created_from_session_entry", "u", "main"),
    ("non_ascii_ids", "x", "main"),
    ("undo", "w", "main"),
    ("source_vs_undo", "s", "w"),
    ("redo", "w2", "main"),
    ("ac4_unknown_source", "nope", "main"),
    ("ac4_unknown_target", "main", "nope"),
    ("ac4_malformed_name", "Bad Name", "main"),
]


def tamper_cases(model, hashes):
    last = len(PLAN)
    tail = {"seq": str(last), "head": hashes[last], "exact": True}
    cs = [
        ("ac3_payloads_removed", [{"op": "set_refs", "refs": []}], None),
        ("payload_rows_forged", [{"op": "set_refs", "refs": [["payload", sha(b"forged events: principal human")]]}],
         None),
        ("principal_edited_no_rehash",
         [{"op": "set_header_field", "seq": 8, "path": ["principal", "kind"], "value": "human"}], None),
        ("output_edited_no_rehash",
         [{"op": "set_header_field", "seq": 6, "path": ["command", "outputs", 0, "digest"], "value": val("e6", 9)}],
         None),
        ("branch_column_forged", [{"op": "set_column", "seq": 8, "column": "branch", "value": "main"}], None),
        ("hash_column_forged", [{"op": "set_column", "seq": 9, "column": "hash", "value": hashes[7]}], None),
        ("row_dropped", [{"op": "drop", "seq": 7}], None),
        ("principal_rewritten_unanchored",
         [{"op": "rewrite_field", "seq": 8, "path": ["principal", "kind"], "value": "human"}], None),
        ("principal_rewritten_trusted_tail",
         [{"op": "rewrite_field", "seq": 8, "path": ["principal", "kind"], "value": "human"}], tail),
        ("tail_truncated_trusted", [{"op": "drop", "seq": last}], tail),
        ("tail_extended_trusted", [], {"seq": str(last - 1), "head": hashes[last - 1], "exact": True}),
        ("dangling_logical_parent",
         [{"op": "rewrite_field", "seq": 9, "path": ["logical_parent"], "value": sha(b"nowhere")}], None),
        ("table_created_from_dangling", [{"op": "set_created_from", "branch": "t2", "value": sha(b"nowhere")}],
         None),
        ("head_move_parent_mismatch",
         [{"op": "rewrite_field", "seq": 16, "path": ["logical_parent"], "value": hashes[4]}], None),
        ("head_move_to_dangling",
         [{"op": "rewrite_field", "seq": 16, "path": ["head_move", "to"], "value": sha(b"nowhere")}], None),
    ]
    out = []
    for name, muts, trusted in cs:
        pair = {"table_created_from_dangling": ("t2", "main"), "head_move_parent_mismatch": ("w", "main"),
                "head_move_to_dangling": ("w", "main")}.get(name, ("s", "main"))
        m = apply(model, muts)
        c = {"name": name, "mutations": muts, "source": pair[0], "target": pair[1],
             "expect": outcome(m, pair[0], pair[1], trusted)}
        if trusted is not None:
            c["trusted"] = trusted
        out.append(c)
    return out


def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8")


def build():
    fork, hashes = build_fork()
    model = to_model(fork)
    diffs = [{"name": n, "source": s, "target": t, "expect": outcome(model, s, t)} for n, s, t in DIFF_PAIRS]
    tampers = tamper_cases(model, hashes)
    by = {d["name"]: d["expect"] for d in diffs}
    tb = {c["name"]: c["expect"] for c in tampers}
    # AC-1 as the spec states it.
    ac1 = by["ac1_fork"]["result"]
    assert [d["id"] for d in ac1["added"]] == ["e6"]
    assert [d["id"] for d in ac1["removed"]] == ["e2"]
    assert [(d["id"], d["conflict"]) for d in ac1["changed"]] == [("e3", True), ("e4", False)]
    assert ac1["ancestor"] == hashes[3]
    # AC-2: no changes since the ancestor.
    ac2 = by["ac2_no_changes"]["result"]
    assert ac2["added"] == ac2["removed"] == ac2["changed"] == [] and ac2["ancestor"] == hashes[9]
    # AC-3: headers only.
    assert tb["ac3_payloads_removed"] == by["ac1_fork"] and tb["payload_rows_forged"] == by["ac1_fork"]
    # AC-4.
    for n in ("ac4_unknown_source", "ac4_unknown_target", "ac4_malformed_name"):
        assert by[n] == {"error": {"code": "not_found", "reason": "unknown_branch"}}, n
    assert by["disjoint"]["result"]["ancestor"] is None
    # A created_from naming a session entry resolves to main's last state at or before it.
    assert by["created_from_session_entry"]["result"]["ancestor"] == hashes[9]
    assert by["created_from_session_entry"]["result"]["changed"] == []
    # Undo walks through head_move.to: e3's change at 9 is undone on w; redo restores it.
    undo = by["undo"]["result"]
    assert undo["ancestor"] == hashes[7] and [d["id"] for d in undo["changed"]] == ["e3"]
    assert by["redo"]["result"]["ancestor"] == hashes[9] and by["redo"]["result"]["changed"] == []
    # Byte order, not UTF-16 order.
    ids = [d["id"] for d in by["non_ascii_ids"]["result"]["added"]]
    assert ids == sorted(NON_ASCII, key=lambda k: k.encode("utf-8")), ids
    assert ids != sorted(NON_ASCII, key=lambda k: k.encode("utf-16-be"))
    for n in ("head_move_parent_mismatch", "head_move_to_dangling"):
        assert tb[n] == {"error": {"code": "internal", "reason": "invariant_violated"}}, n
    # The documented limit of CHAIN.VERIFY: a full rewrite is caught only with a trusted head.
    rew = tb["principal_rewritten_unanchored"]["result"]
    assert any(c["principal"] == "human" and c["seq"] == "8" for d in rew["changed"] for c in d["source_commands"])
    assert tb["principal_rewritten_trusted_tail"]["error"]["failure"]["reason"] == "forked_head"
    for name, e in tb.items():
        if name not in ("ac3_payloads_removed", "payload_rows_forged", "principal_rewritten_unanchored"):
            assert "error" in e, name
    return {"fork": dump(fork), "cases": dump({"stream": STREAM, "diffs": diffs, "tampers": tampers})}


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
        print("stale fixture (run make_fixtures.py): " + ", ".join(stale))
        sys.exit(1)
    if "--check" in sys.argv[1:]:
        print("fixtures up to date")


if __name__ == "__main__":
    main()
