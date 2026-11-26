#!/usr/bin/env python3
"""Independent reference for AGENT.PROPOSE.IMPACT (`agent.compute_impact@1`) (CC0-1.0).

Standard library only. It shares no code with the Rust crate: it has its own RFC 8785
writer, its own header hashing and chain check, its own branch diff (the CORE.JRN.BR.DIFF
rules, as `fixtures/core/jrn/br/diff/make_fixtures.py` states them) and its own impact
computation. All data is invented for this project.

Writes:
  fixtures/agent/propose/impact-two-moves/project.json  AC-1 project (journal rows, branch
                                                        table, stored objects)
  fixtures/agent/propose/impact-two-moves/impact.json   golden result, canonical JSON bytes
  fixtures/agent/propose/impact-empty/project.json      AC-2 project
  fixtures/agent/propose/impact-empty/impact.json       golden result
  fixtures/agent/propose/impact-cases.json              further positive, refusal and
                                                        tamper cases with expected outcomes

The rules this reference implements (pinned in `fid_agent::impact`):

- Target. The promotion target of branch B is the branch of the journal row whose hash is
  B's `created_from` in the branch table. B unknown: `unknown_branch`. B without
  `created_from` (main), a target that is B itself, an `agent/` branch or not in the table:
  `no_target`. A `created_from` that names no row: `invariant_violated`.
- Diff. `journal.diff_branch` of B against the target over the verified rows (a row that
  fails the chain check refuses the call as `invariant_violated`).
- Rows. Only deltas whose `source_commands` is non-empty count (the agent's own commands):
  old = the target's digest, new = the source's digest (None when absent or deleted). Each
  present digest is read from the object store: absent is `record_missing{digest}`; bytes
  whose SHA-256 is not the digest are `record_invalid`. An object over 16 MiB is not a
  ResultRecord. An object is a ResultRecord when its bytes contain the canonical member
  `"schema":"fid.gdt-result/` or it parses as a JSON object (depth 8, no duplicate keys)
  whose `schema` is a string starting with `fid.gdt-result/`; any other object is not a
  characteristic and is skipped. A ResultRecord
  must be canonical, of schema `/1` or `/2`, and carry `characteristic`, `value.value`
  (`0x` + 16 hex), `value.unit`, `uncertainty.class` (UEC-0..UEC-3) and `decision.outcome`
  (one of the four), else `record_invalid`. Old and new must both be records (or one
  absent) and name the same characteristic, else `record_invalid`.
- Listing. Added (old absent), removed (new absent), or changed in outcome, UEC class or
  value (bits or unit); nothing else is listed. Sorted by entity ID bytes.
- Moves. A changed row whose outcome differs adds 1 at (old, new) of the 4x4 matrix over
  CONFORMS, NONCONFORMS, INDETERMINATE, NOT_DECIDABLE; added and removed rows are counted
  separately.
- Entries. `entry_count` is the number of `command` rows on B with a sequence above the
  `created_from` row's; `classes` is the set of their header `command.class`, in the order
  R0 R1 W1 W2 W3 X1 I1 D1 M1 S1 A0.
- Interpretation (D-045). Each row carries `old_interpretation` / `new_interpretation`:
  `signed_off` only for a canonical `/2` record whose `interpretation` is `signed_off` and
  whose `interpretation_rules` are non-empty and all `signed_off`; otherwise
  `provisional`; null when that side is absent. The result's `interpretation` is
  `provisional` when any row side is, `signed_off` when there are rows and none is, `none`
  when there are no rows.

Run:  python fixtures/agent/propose/make_impact_fixtures.py [--check]
"""

import copy
import hashlib
import json
import struct
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STREAM = "record"
ZERO = "sha256:" + "0" * 64
TOMBSTONE = ZERO
STATE_KINDS = ("command", "head_move")
OUTCOMES = ["CONFORMS", "NONCONFORMS", "INDETERMINATE", "NOT_DECIDABLE"]
UECS = ["UEC-0", "UEC-1", "UEC-2", "UEC-3"]
CLASS_ORDER = ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]
MAX_RECORD_BYTES = 16 * 1024 * 1024
MAX_RECORD_DEPTH = 8
RESULT_SCHEMA = "fid.proposal-impact/1"
MARKER = b'"schema":"fid.gdt-result/'


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


def depth(v):
    if isinstance(v, dict):
        return 1 + max([depth(x) for x in v.values()] or [0])
    if isinstance(v, list):
        return 1 + max([depth(x) for x in v] or [0])
    return 0


def bits(x):
    return "0x%016x" % struct.unpack(">Q", struct.pack(">d", x))[0]


# ------------------------------------------------------------------ invented objects

def uuid(tag):
    """A canonical-looking v7 UUID text ending in `tag` (hex)."""
    return "01900000-0000-7000-8000-" + tag.rjust(12, "0")


C1, C2, C3, C4 = uuid("c1"), uuid("c2"), uuid("c3"), uuid("c4")
F1, F9 = uuid("f1"), uuid("f9")


def record(char, outcome, uec, value, unit="si:m", interp="signed_off", schema=2, run="R1"):
    """An invented `fid.gdt-result` record in the documented wire form."""
    expanded = None if uec == "UEC-0" else {"U": bits(value / 10.0), "k": bits(2.0)}
    r = {
        "schema": "fid.gdt-result/%d" % schema,
        "run": run,
        "characteristic": char,
        "measurand": "flatness of face %s" % char,
        "evaluation_basis": "POINTS",
        "value": {"value": bits(value), "unit": unit},
        "uncertainty": {"class": uec, "uec0_reason": "not_evaluated" if uec == "UEC-0" else None,
                        "method": None if uec == "UEC-0" else "LPU", "statement": None, "unit": unit,
                        "u_std": None, "expanded": expanded, "interval": None, "claimed": False,
                        "declared_input": False, "uncharacterized_input": False},
        "decision": {"source": "unc", "rule": "dr:simple-acceptance@1", "outcome": outcome, "reason": None,
                     "basis": {"class": uec, "method": "LPU", "claimed": False, "declared_input": False,
                               "uncharacterized_input": False, "statement": None}},
        "spec": {"dialect": "ASME", "standards": ["ASME Y14.5-2018"], "defaults_profile": "dp:invented@1",
                 "inherited_from": "explicit"},
        "algorithm": "gdt.form.flatness.minzone@1.0.0",
        "kernel_version": "0.1.0",
        "input_digests": [sha(("points of %s" % char).encode())],
        "journal_entry": sha(("entry of %s %s" % (char, value)).encode()),
        "dmt_row": "dmt:asme/flatness/none@1",
        "operator_chain_digest": sha(b"operator chain"),
        "dmt_digest": sha(b"dmt table"),
    }
    if schema == 2:
        r["interpretation"] = interp
        r["interpretation_register"] = sha(b"register")
        r["interpretation_rules"] = [{"rule": "dmt:asme/flatness/none@1", "kind": "dmt_row", "revision": 1,
                                      "content_digest": sha(b"row flatness"), "status": interp, "reason": None}]
    return cbytes(r)


def feature(name, version):
    return cbytes({"schema": "fid.test-feature/1", "name": name, "version": version})


# ------------------------------------------------------------------ journal builder

REGISTRY = sha(b"registry manifest v1")
KERNEL = sha(b"kernel manifest v1")


class Project:
    """Rows, branch table and object store of one invented project."""

    def __init__(self):
        self.rows = []
        self.hashes = {}
        self.branches = [{"name": "main", "created_from": None}]
        self.objects = {}

    def put(self, data):
        d = sha(data)
        self.objects[d] = data
        return d

    def add(self, seq, branch, pk, kind, cid=None, cls="W2", parent=None, outputs=()):
        assert seq == len(self.rows) + 1
        prev = self.rows[-1]["hash"] if self.rows else ZERO
        h = {
            "schema_version": 1,
            "kind": kind,
            "stream": STREAM,
            "seq": str(seq),
            "prev": prev,
            "branch": branch,
            "logical_parent": None if parent is None else self.hashes[parent],
            "time": {"utc": "2026-11-25T10:%02d:00Z" % seq, "mono_ns": str(1_000_000_000 * seq), "sync": "synced"},
            "principal": {"kind": pk, "commit": sha(b"identity %d" % seq)},
            "ai_assistance": "authored" if pk == "agent" else "none",
            "manifests": {"registry": REGISTRY, "kernel": KERNEL},
        }
        if kind == "session_header":
            h["session"] = {"session_id": "s-%04d" % seq, "app_version": "0.1.0"}
        else:
            outs = []
            for ent, d in outputs:
                o = {"digest": d}
                if ent is not None:
                    o["entity"] = ent
                outs.append(o)
            h["command"] = {
                "id": cid, "major": 1, "class": cls, "determinism": "D0", "seed": str(seq),
                "args_commit": sha(b"args %d" % seq), "events_commit": sha(b"events %d" % seq),
                "inputs": [], "outputs": outs, "algorithms": [],
            }
        h["hash"] = sha(cbytes(h))
        self.hashes[seq] = h["hash"]
        self.rows.append({"seq": seq, "hash": h["hash"], "stream": STREAM, "branch": branch, "kind": kind,
                          "prev_hash": prev, "header": canon(h), "refs": [["payload", sha(b"payload %d" % seq)]]})

    def branch(self, name, from_seq):
        self.branches.append({"name": name, "created_from": self.hashes[from_seq]})

    def to_json(self):
        return {
            "schema": "agent-propose-impact-project/v1",
            "license": "CC0-1.0",
            "stream": STREAM,
            "entries": self.rows,
            "branches": self.branches,
            "objects": {d: b.decode("utf-8") for d, b in sorted(self.objects.items())},
        }


def base_main(p):
    """main: features F1, F9; records C1, C2, C3 (all CONFORMS, UEC-2, signed off)."""
    p.add(1, "main", "human", "session_header")
    p.add(2, "main", "human", "command", "project.add_features", "W2", None,
          [(F1, p.put(feature("F1", 1))), (F9, p.put(feature("F9", 1)))])
    p.add(3, "main", "human", "command", "gdt.eval_flatness", "W2", 2,
          [(C1, p.put(record("C1", "CONFORMS", "UEC-2", 1.25e-5))),
           (C2, p.put(record("C2", "CONFORMS", "UEC-2", 2.5e-5))),
           (C3, p.put(record("C3", "CONFORMS", "UEC-2", 4.0e-5)))])
    p.add(4, "main", "human", "command", "journal.create_branch", "W2", 3, [])


def two_moves():
    """AC-1: agent/s1 refits F1, which turns C1 CONFORMS -> NONCONFORMS (its new record is
    provisional) and changes C2's value only. A human co-edit on the branch adds class W2;
    main moves F9 meanwhile (not the agent's change, so not listed)."""
    p = Project()
    base_main(p)
    p.branch("agent/s1", 3)
    p.add(5, "agent/s1", "agent", "session_header")
    p.add(6, "agent/s1", "agent", "command", "align.refit_feature", "W1", 3, [(F1, p.put(feature("F1", 2)))])
    p.add(7, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 6,
          [(C1, p.put(record("C1", "NONCONFORMS", "UEC-2", 3.75e-5, interp="provisional"))),
           (C2, p.put(record("C2", "CONFORMS", "UEC-2", 2.625e-5))),
           (None, sha(b"report of entry 7"))])
    p.add(8, "main", "human", "command", "feature.set_nominal", "W2", 4, [(F9, p.put(feature("F9", 2)))])
    p.add(9, "agent/s1", "human", "command", "report.add_note", "W2", 7, [])
    return p


def empty():
    """AC-2: agent/e1 has only its session header since its fork point."""
    p = Project()
    base_main(p)
    p.branch("agent/e1", 4)
    p.add(5, "agent/e1", "agent", "session_header")
    return p


def add_remove():
    """Added C4 (a schema /1 record, so provisional), removed C3, C2 UEC-2 -> UEC-1 only, C1
    changed in its run only (not listed), and a non-JSON object (not a characteristic)."""
    p = Project()
    base_main(p)
    p.branch("agent/s2", 4)
    p.add(5, "agent/s2", "agent", "command", "gdt.eval_flatness", "W1", 4,
          [(C1, p.put(record("C1", "CONFORMS", "UEC-2", 1.25e-5, run="R2"))),
           (uuid("b1"), p.put(b"\x00binary point cloud, not JSON"))])
    p.add(6, "agent/s2", "agent", "command", "gdt.eval_flatness", "W1", 5,
          [(C4, p.put(record("C4", "INDETERMINATE", "UEC-1", 9.0e-6, schema=1))),
           (C3, TOMBSTONE),
           (C2, p.put(record("C2", "CONFORMS", "UEC-1", 2.5e-5)))])
    return p


def unit_and_moves():
    """C1 CONFORMS -> NOT_DECIDABLE (UEC-0), C2 unit only (same bits), and C3, which main
    changed to INDETERMINATE after the fork, set to NONCONFORMS by the agent: the old side
    is the target's value. Classes W1 and W3 (out of the fixed order on purpose)."""
    p = Project()
    base_main(p)
    p.branch("agent/s4", 4)
    p.add(5, "main", "human", "command", "gdt.eval_flatness", "W2", 4,
          [(C3, p.put(record("C3", "INDETERMINATE", "UEC-2", 4.5e-5)))])
    p.add(6, "agent/s4", "agent", "command", "gdt.eval_flatness", "W3", 4,
          [(C3, p.put(record("C3", "NONCONFORMS", "UEC-2", 5.0e-5)))])
    p.add(7, "agent/s4", "agent", "command", "gdt.eval_flatness", "W1", 6,
          [(C1, p.put(record("C1", "NOT_DECIDABLE", "UEC-0", 1.25e-5))),
           (C2, p.put(record("C2", "CONFORMS", "UEC-2", 2.5e-5, unit="si:mm")))])
    return p


# ------------------------------------------------------------------ the reference

class Refused(Exception):
    def __init__(self, code, bus, digest=None):
        super().__init__(code)
        self.code, self.bus, self.digest = code, bus, digest


def invariant():
    return Refused("invariant_violated", ["internal", "invariant_violated"])


def verified(m):
    """The header-only chain check (CORE.JRN.CHAIN.VERIFY steps the diff runs)."""
    out = []
    prev = ZERO
    p = 1
    for r in m["entries"]:
        if r["seq"] != p:
            raise invariant()
        b = r["header"].encode("utf-8")
        h = json.loads(b.decode("utf-8"), object_pairs_hook=no_dups)
        if cbytes(h) != b:
            raise invariant()
        bare = dict(h)
        recorded = bare.pop("hash")
        if sha(cbytes(bare)) != recorded or r["hash"] != recorded or h["seq"] != str(p):
            raise invariant()
        if h["stream"] != STREAM:
            raise invariant()
        for col in ("stream", "branch", "kind"):
            if r[col] != h[col]:
                raise invariant()
        if r["prev_hash"] != h["prev"] or h["prev"] != prev:
            raise invariant()
        out.append(h)
        prev = recorded
        p += 1
    return out


def live(d):
    return None if d is None or d == TOMBSTONE else d


def diff(m, headers, source, target):
    table = {b["name"]: b for b in m["branches"]}
    nodes, state, last_by_branch = {}, {}, {}
    for h in headers:
        lp = h["logical_parent"]
        if lp is not None and lp not in nodes:
            raise invariant()
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
            raise invariant()
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
    spath, ancestor = [], None
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

    def fold(hashes):
        st = {}
        for x in sorted(hashes, key=lambda x: int(nodes[x]["seq"])):
            h = nodes[x]
            if h["kind"] != "command":
                continue
            last = {}
            for o in h["command"]["outputs"]:
                if "entity" in o:
                    last[o["entity"]] = o["digest"]
            for ent, d in last.items():
                cur = st.setdefault(ent, {"digest": None, "commands": 0})
                cur["digest"] = d
                cur["commands"] += 1
        return st

    b, s, t = fold(base), fold(spath), fold(tside)
    deltas = []
    for ent in sorted(set(s) | set(t), key=lambda k: k.encode("utf-8")):
        bv = live(b[ent]["digest"]) if ent in b else None
        sv = live(s[ent]["digest"]) if ent in s else bv
        tv = live(t[ent]["digest"]) if ent in t else bv
        if sv == bv and tv == bv:
            continue
        deltas.append({"id": ent, "source": sv, "target": tv, "source_commands": s[ent]["commands"] if ent in s else 0})
    return sh, th, deltas


def read_record(objects, d):
    """None: not a ResultRecord. Else the fields the impact reads."""
    if d not in objects:
        raise Refused("record_missing", ["internal", "invariant_violated"], d)
    data = objects[d]
    if sha(data) != d:
        raise Refused("record_invalid", ["internal", "invariant_violated"], d)
    if len(data) > MAX_RECORD_BYTES:
        return None
    bad = Refused("record_invalid", ["internal", "invariant_violated"], d)
    # Bytes carrying the canonical schema member are meant as a record: they must parse.
    marked = MARKER in data
    try:
        v = json.loads(data.decode("utf-8"), object_pairs_hook=no_dups)
    except (ValueError, UnicodeDecodeError):
        if marked:
            raise bad
        return None
    if not isinstance(v, dict) or depth(v) > MAX_RECORD_DEPTH:
        if marked:
            raise bad
        return None
    schema = v.get("schema")
    if not isinstance(schema, str) or not schema.startswith("fid.gdt-result/"):
        if marked:
            raise bad
        return None
    if schema not in ("fid.gdt-result/1", "fid.gdt-result/2") or cbytes(v) != data:
        raise bad
    try:
        char = v["characteristic"]
        val = v["value"]["value"]
        unit = v["value"]["unit"]
        uec = v["uncertainty"]["class"]
        outcome = v["decision"]["outcome"]
    except (KeyError, TypeError):
        raise bad
    hexdigits = "0123456789abcdef"
    if not (isinstance(char, str) and char and isinstance(unit, str) and isinstance(val, str)
            and len(val) == 18 and val.startswith("0x") and all(c in hexdigits for c in val[2:])
            and uec in UECS and outcome in OUTCOMES):
        raise bad
    rules = v.get("interpretation_rules")
    signed = (schema == "fid.gdt-result/2" and v.get("interpretation") == "signed_off" and isinstance(rules, list)
              and len(rules) > 0 and all(isinstance(r, dict) and r.get("status") == "signed_off" for r in rules))
    return {"characteristic": char, "value": {"unit": unit, "value": val}, "uec": uec, "outcome": outcome,
            "interpretation": "signed_off" if signed else "provisional"}


def impact(m, branch):
    table = {b["name"]: b for b in m["branches"]}
    if branch not in table:
        raise Refused("unknown_branch", ["not_found", "unknown_branch"])
    cf = table[branch]["created_from"]
    if cf is None:
        raise Refused("no_target", ["precondition", "false"])
    fork = [r for r in m["entries"] if r["hash"] == cf]
    if not fork:
        raise invariant()
    target, fork_seq = fork[0]["branch"], fork[0]["seq"]
    if target == branch or target.startswith("agent/") or target not in table:
        raise Refused("no_target", ["precondition", "false"])
    headers = verified(m)
    sh, th, deltas = diff(m, headers, branch, target)
    objects = {d: t.encode("utf-8") for d, t in m["objects"].items()}
    moves = [[0] * 4 for _ in range(4)]
    added = removed = 0
    rows = []
    for dl in deltas:
        if dl["source_commands"] == 0:
            continue
        old_d, new_d = dl["target"], dl["source"]
        old = read_record(objects, old_d) if old_d is not None else None
        new = read_record(objects, new_d) if new_d is not None else None
        if (old_d is not None and old is None and new is not None) or \
           (new_d is not None and new is None and old is not None):
            raise Refused("record_invalid", ["internal", "invariant_violated"], new_d if new is None else old_d)
        if old is None and new is None:
            continue
        if old is not None and new is not None and old["characteristic"] != new["characteristic"]:
            raise Refused("record_invalid", ["internal", "invariant_violated"], new_d)
        if old is None:
            change = "added"
        elif new is None:
            change = "removed"
        else:
            change = "changed"
        value_changed = old is None or new is None or old["value"] != new["value"]
        if change == "changed" and not value_changed and old["outcome"] == new["outcome"] and old["uec"] == new["uec"]:
            continue
        if change == "added":
            added += 1
        elif change == "removed":
            removed += 1
        elif old["outcome"] != new["outcome"]:
            moves[OUTCOMES.index(old["outcome"])][OUTCOMES.index(new["outcome"])] += 1
        g = lambda r, k: None if r is None else r[k]  # noqa: E731
        rows.append({
            "id": dl["id"],
            "characteristic": (new or old)["characteristic"],
            "change": change,
            "old_record": old_d if old is not None else None,
            "new_record": new_d if new is not None else None,
            "old_outcome": g(old, "outcome"), "new_outcome": g(new, "outcome"),
            "old_uec": g(old, "uec"), "new_uec": g(new, "uec"),
            "old_value": g(old, "value"), "new_value": g(new, "value"),
            "value_changed": value_changed,
            "old_interpretation": g(old, "interpretation"), "new_interpretation": g(new, "interpretation"),
        })
    count = 0
    classes = set()
    for h in headers:
        if h["branch"] == branch and h["kind"] == "command" and int(h["seq"]) > fork_seq:
            count += 1
            classes.add(h["command"]["class"])
    labels = [x for r in rows for x in (r["old_interpretation"], r["new_interpretation"]) if x is not None]
    interp = "none" if not rows else ("provisional" if "provisional" in labels else "signed_off")
    return {
        "schema": RESULT_SCHEMA,
        "branch": branch,
        "target": target,
        "source_head": sh,
        "target_head": th,
        "characteristics": rows,
        "outcomes": OUTCOMES,
        "moves": moves,
        "added": added,
        "removed": removed,
        "entry_count": count,
        "classes": [c for c in CLASS_ORDER if c in classes],
        "interpretation": interp,
    }


def outcome(m, branch):
    try:
        return {"result": impact(m, branch)}
    except Refused as e:
        err = {"code": e.code, "bus": e.bus}
        if e.digest is not None:
            err["digest"] = e.digest
        return {"error": err}


# ------------------------------------------------------------------ mutations for the cases

def row(m, seq):
    return [r for r in m["entries"] if r["seq"] == seq][0]


def set_class_no_relink(m, seq, cls):
    """Rewrites one header's class and re-hashes it, but leaves later rows linked to the old
    hash (a forged class the chain check must catch)."""
    r = row(m, seq)
    h = json.loads(r["header"])
    h["command"]["class"] = cls
    h.pop("hash")
    h["hash"] = sha(cbytes(h))
    r["header"], r["hash"] = canon(h), h["hash"]


def cases():
    out = []

    def case(name, project, branch):
        m = project if isinstance(project, dict) else project.to_json()
        out.append({"name": name, "branch": branch, "project": m, "expect": outcome(m, branch)})
        return out[-1]["expect"]

    tm = two_moves().to_json()
    case("two_moves", tm, "agent/s1")
    e = case("unknown_branch", tm, "agent/nope")
    assert e == {"error": {"code": "unknown_branch", "bus": ["not_found", "unknown_branch"]}}
    case("malformed_branch_name", tm, "Agent/S1!")
    assert case("main_has_no_target", tm, "main")["error"]["code"] == "no_target"

    ar = add_remove()
    r = case("add_remove_uec", ar, "agent/s2")["result"]
    assert [x["change"] for x in r["characteristics"]] == ["changed", "removed", "added"]
    assert r["added"] == 1 and r["removed"] == 1 and r["moves"] == [[0] * 4 for _ in range(4)]
    assert r["characteristics"][2]["new_interpretation"] == "provisional"

    r = case("unit_and_moves", unit_and_moves(), "agent/s4")["result"]
    assert r["moves"] == [[0, 0, 0, 1], [0] * 4, [0, 1, 0, 0], [0] * 4], r["moves"]
    assert r["characteristics"][1]["value_changed"] and r["characteristics"][1]["old_value"]["value"] == \
        r["characteristics"][1]["new_value"]["value"]
    assert r["classes"] == ["W1", "W3"] and r["interpretation"] == "signed_off"

    nested = two_moves()
    nested.branch("agent/s3", 7)
    nested.add(10, "agent/s3", "agent", "command", "gdt.eval_flatness", "W1", 7, [])
    assert case("forked_from_agent_branch", nested, "agent/s3")["error"]["code"] == "no_target"

    m = copy.deepcopy(tm)
    c2_new = json.loads(row(m, 7)["header"])["command"]["outputs"][1]["digest"]
    del m["objects"][c2_new]
    e = case("record_missing", m, "agent/s1")["error"]
    assert e["code"] == "record_missing" and e["digest"] == c2_new

    m = copy.deepcopy(tm)
    m["objects"][c2_new] = m["objects"][c2_new].replace("C2", "C9")
    assert case("record_bytes_not_digest", m, "agent/s1")["error"]["code"] == "record_invalid"

    # A record that is not canonical (stored under its own digest).
    p = Project()
    bad = record("C2", "CONFORMS", "UEC-2", 2.625e-5).replace(b'{"algorithm"', b'{ "algorithm"')
    assert bad.startswith(b'{ "algorithm"')
    base_main(p)
    p.branch("agent/s1", 3)
    p.add(5, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 3, [(C2, p.put(bad))])
    assert case("record_not_canonical", p, "agent/s1")["error"]["code"] == "record_invalid"

    p = Project()
    base_main(p)
    p.branch("agent/s1", 3)
    p.add(5, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 3,
          [(C2, p.put(record("C9", "CONFORMS", "UEC-2", 2.5e-5)))])
    assert case("record_names_other_characteristic", p, "agent/s1")["error"]["code"] == "record_invalid"

    p = Project()
    base_main(p)
    p.branch("agent/s1", 3)
    p.add(5, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 3, [(C2, p.put(feature("C2", 9)))])
    assert case("record_replaced_by_non_record", p, "agent/s1")["error"]["code"] == "record_invalid"

    p = Project()
    base_main(p)
    p.branch("agent/s1", 3)
    p.add(5, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 3,
          [(C2, p.put(record("C2", "PASS", "UEC-2", 2.5e-5)))])
    assert case("record_bad_outcome", p, "agent/s1")["error"]["code"] == "record_invalid"

    p = Project()
    base_main(p)
    p.branch("agent/s1", 3)
    dup = record("C2", "CONFORMS", "UEC-2", 2.5e-5).replace(b'{"algorithm"', b'{"run":"R9","algorithm"')
    assert dup.startswith(b'{"run":"R9","algorithm"')
    p.add(5, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 3, [(C2, p.put(dup))])
    assert case("record_duplicate_key", p, "agent/s1")["error"]["code"] == "record_invalid"

    p = Project()
    base_main(p)
    p.branch("agent/s1", 3)
    p.add(5, "agent/s1", "agent", "command", "gdt.eval_flatness", "W1", 3,
          [(C2, p.put(record("C2", "CONFORMS", "UEC-2", 2.5e-5, schema=9)))])
    assert case("record_unknown_schema_version", p, "agent/s1")["error"]["code"] == "record_invalid"

    m = copy.deepcopy(tm)
    set_class_no_relink(m, 7, "W3")
    e = case("forged_class_header", m, "agent/s1")["error"]
    assert e["code"] == "invariant_violated"

    m = copy.deepcopy(tm)
    row(m, 7)["branch"] = "main"
    assert case("branch_column_forged", m, "agent/s1")["error"]["code"] == "invariant_violated"

    m = copy.deepcopy(tm)
    for b in m["branches"]:
        if b["name"] == "agent/s1":
            b["created_from"] = sha(b"nowhere")
    assert case("created_from_dangling", m, "agent/s1")["error"]["code"] == "invariant_violated"
    return out


# ------------------------------------------------------------------ output

def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8")


def build():
    tm, em = two_moves().to_json(), empty().to_json()
    r1 = impact(tm, "agent/s1")
    # AC-1 as the spec states it.
    assert [x["characteristic"] for x in r1["characteristics"]] == ["C1", "C2"]
    assert r1["moves"] == [[0, 1, 0, 0], [0] * 4, [0] * 4, [0] * 4]
    assert r1["characteristics"][1]["old_outcome"] == r1["characteristics"][1]["new_outcome"] == "CONFORMS"
    assert r1["characteristics"][1]["value_changed"]
    assert r1["classes"] == ["W1", "W2"] and r1["entry_count"] == 3
    assert r1["interpretation"] == "provisional"
    r2 = impact(em, "agent/e1")
    # AC-2.
    assert r2["characteristics"] == [] and r2["entry_count"] == 0 and r2["classes"] == []
    assert r2["moves"] == [[0] * 4 for _ in range(4)] and r2["added"] == r2["removed"] == 0
    return {
        "impact-two-moves/project.json": dump(tm),
        "impact-two-moves/impact.json": cbytes(r1),
        "impact-empty/project.json": dump(em),
        "impact-empty/impact.json": cbytes(r2),
        "impact-cases.json": (json.dumps({"cases": cases()}, ensure_ascii=False, sort_keys=True,
                                         separators=(",", ":")) + "\n").encode("utf-8"),
    }


def main():
    files = build()
    stale = []
    for name, data in files.items():
        path = HERE / name
        if "--check" in sys.argv[1:]:
            if not path.exists() or path.read_bytes() != data:
                stale.append(name)
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
            print("wrote " + name)
    if stale:
        print("stale fixtures (run make_impact_fixtures.py): " + ", ".join(stale))
        sys.exit(1)
    if "--check" in sys.argv[1:]:
        print("fixtures up to date")


if __name__ == "__main__":
    main()
