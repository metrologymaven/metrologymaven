#!/usr/bin/env python3
"""Generate the fixtures of CORE.JRN.CHAIN.VERIFY (journal hash-chain verification).

An independent reference written with Python's standard library only (hashlib, json, re).
It builds a physical journal (entry rows, payload blobs, referenced blobs, checkpoint rows)
from its own reading of the spec (spec/core/jrn/entry, spec/core/jrn/chain/{append,ckpt,
verify}), hashes it with its own RFC 8785 serialiser and hashlib, and runs its own verifier
over the good journal and every tampered variant. Nothing here is produced by, or copied
from, the Rust crate `fid-journal`; the Rust tests load these files and must reach the same
outcome.

Files written next to this script (CC-BY-4.0):

- good.json: the `good` journal of AC-1: 500 entries of stream `record` (rows with the stored
  header bytes, the payload blob and the referenced blobs of each), 20 referenced blobs and
  2 checkpoint rows (at sequences 250 and 500).
- cases.json: `cases`, the tamper cases, and `lists`, queries of `journal.list_entries@1`
  over `good` with the sequences each returns, in order. A tamper case is a list of
  mutations applied to `good` (the mutation is input, not an expected value) and the
  expected verification outcome computed here:
  `{"ok": true, "checked": {...}}` or `{"ok": false, "failure": {"seq", "kind", "reason"}}`.

Verification rules (the outcome contract this reference and the Rust verifier share):

Start. from_seq 1 starts at sequence 1 linking to the zero digest (anchor "genesis").
from_seq n > 1 needs a caller's trusted head at exactly n-1 (anchor "trusted"); stored
checkpoints are not authenticated (until TRUST.VERIFY checks their signatures) and never
anchor. Without it, from_seq 0, or above the tail plus one: refused, validation/bad_range.
The anchoring row n-1 itself must pass steps 2 to 13 and hash to the trusted head; if not,
refused as internal/invariant_violated with the located failure (seq n-1). The result
carries from_seq and the anchor. Checkpoints below n are not checked or counted.

Checkpoint rows first, in row-id order. A row id below 1: kind `checkpoint`, reason
`bad_id` (seq null). Body over 4096 bytes, not strict JSON (UTF-8, no
duplicate keys), not an object, missing/invalid fields, or bytes that are not the canonical
form: kind `checkpoint`, reason `malformed` (seq null). A `format` string other than
`linear-v1`, read before every other field: `unknown_format` (seq null). The row's stream
is not the journal's stream: `foreign_stream` (seq null). The body's stream or head differs
from the row's columns: `column_mismatch` (seq null).

Then the entries in sequence order, position p = 1, 2, ... For each row:
 1. column seq != p: kind `sequence`, reason `gap` (column > p) or `reorder` (column < p), at p.
 2. header over 1 MiB: `hash`/`oversize`.
 3. header not strict JSON: `hash`/`malformed`.
 4. header bytes not canonical: `hash`/`non_canonical`.
 5. not an object, or `hash` missing or not a digest: `hash`/`schema`.
 6. SHA-256 of the canonical header without `hash` != header.hash: `hash`/`hash_mismatch`.
 7. schema_version is not the number 1: `hash`/`unknown_version`.
 8. header fails the entry schema: `hash`/`schema`.
 9. hash column != header.hash: `hash`/`column_mismatch`.
10. header.seq != column seq: `sequence`/`header_seq`.
11. header.stream != the journal stream: `sequence`/`foreign_stream`.
12. stream, branch or kind column != header: `hash`/`column_mismatch`.
13. prev_hash column != header.prev: `link`/`column_mismatch`.
14. header.prev != hash of the entry before (zero digest at sequence 1): `link`/`genesis`
    at sequence 1, else `link`/`broken`.
15. entry_blobs roles: a role other than payload/ref: `blob`/`unknown_role`; no payload row:
    `blob`/`payload_ref_missing`; more than one: `blob`/`payload_ref_duplicate`.
16. payload blob: absent from the blob store -> counted as without payload (not a failure);
    stored bytes not hashing to the digest: `blob`/`corrupt`; payload not strict canonical
    JSON, keys other than "salts" plus the header's committed fields, salts not 32 lowercase
    hex, or a commitment that does not match: `blob`/`payload_mismatch`.
17. referenced blobs in digest order, each digest checked once per run: absent: `blob`/
    `missing`; bytes not hashing to the digest: `blob`/`corrupt`.
18. checkpoints at this sequence (row-id order): head != entry hash: `checkpoint`/
    `forked_head`.
A trusted head: with `exact`, any entry after it is `sequence`/`extended` (checked first at
each row); at its sequence a hash other than its head is `checkpoint`/`forked_head` (after
the checkpoints of step 18). After the last row, a checkpoint above it (lowest sequence
first): `checkpoint`/`truncated` at the checkpoint's sequence; then a trusted head above it,
the same at its sequence. Then the head cache, in branch order: a row naming no entry's
stored hash is `link`/`head_past_chain`, one naming an entry of another branch
`link`/`head_branch` (seq null).

The referenced-blob rows of an entry are not committed in its header (CORE.JRN.ENTRY and
CHAIN.APPEND do not hash them): "blobs verified" covers the stored reference set. The first failure stops verification. Success reports the
counts: entries, distinct referenced blobs, checkpoints, entries without payload.

Run:  python fixtures/core/jrn/chain/verify/make_fixtures.py [--check]
"""

import copy
import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
STREAM = "record"
ZERO = "sha256:" + "0" * 64
MAX_HEADER = 1024 * 1024
MAX_CHECKPOINT = 4096
N_ENTRIES = 500
N_BLOBS = 20
DIGEST_RE = re.compile(r"^sha256:[0-9a-f]{64}$")
HEX32_RE = re.compile(r"^[0-9a-f]{32}$")
IDENT_RE = re.compile(r"^[a-z][a-z0-9_]*$")
SEQ_RE = re.compile(r"^[1-9][0-9]{0,19}$")
U64_RE = re.compile(r"^(0|[1-9][0-9]{0,19})$")
BRANCH_RE = re.compile(r"^[A-Za-z0-9][A-Za-z0-9._/-]*$")
UTC_RE = re.compile(r"^[0-9]{4}-[0-9]{2}-[0-9]{2}T[0-9]{2}:[0-9]{2}:[0-9]{2}(\.[0-9]{1,9})?Z$")
CMD_RE = re.compile(r"^[a-z0-9]([a-z0-9_.]*[a-z0-9])?$")
CKPT_TIME_RE = re.compile(r"^([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})Z$")


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


def utf16_key(k):
    return k.encode("utf-16-be")


class Unsupported(Exception):
    pass


def canon(v):
    if v is None:
        return "null"
    if v is True:
        return "true"
    if v is False:
        return "false"
    if isinstance(v, int):
        if abs(v) > 2 ** 53:
            raise Unsupported("integer beyond 2^53")
        return str(v)
    if isinstance(v, float):
        if v != v or v in (float("inf"), float("-inf")):
            raise Unsupported("non-finite")
        if v == int(v) and abs(v) < 1e21:
            return str(int(v))
        raise Unsupported("fractional number")
    if isinstance(v, str):
        return esc(v)
    if isinstance(v, list):
        return "[" + ",".join(canon(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v.keys(), key=utf16_key)
        return "{" + ",".join(esc(k) + ":" + canon(v[k]) for k in keys) + "}"
    raise Unsupported(type(v).__name__)


def cbytes(v):
    return canon(v).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


class Malformed(Exception):
    pass


def no_dups(pairs):
    d = {}
    for k, v in pairs:
        if k in d:
            raise Malformed("duplicate key")
        d[k] = v
    return d


def bad_constant(name):
    raise Malformed("constant " + name)


def strict_parse(b, max_bytes):
    if len(b) > max_bytes:
        raise Malformed("too long")
    try:
        text = b.decode("utf-8", errors="strict")
    except UnicodeDecodeError:
        raise Malformed("utf-8")
    try:
        return json.loads(text, object_pairs_hook=no_dups, parse_constant=bad_constant)
    except (ValueError, RecursionError):
        raise Malformed("json")


def is_canonical(b, v):
    try:
        return cbytes(v) == b
    except Unsupported:
        return False


# ------------------------------------------------------------------ deterministic draws

class Draw:
    """SHA-256 counter stream: stable across Python versions."""

    def __init__(self, seed):
        self.seed = seed
        self.n = 0

    def bytes(self, k):
        out = b""
        while len(out) < k:
            out += hashlib.sha256(("%s/%d" % (self.seed, self.n)).encode()).digest()
            self.n += 1
        return out[:k]

    def below(self, m):
        return int.from_bytes(self.bytes(8), "big") % m


# ------------------------------------------------------------------ building the journal

def utc_of(seq):
    # 2026-11-01T08:00:00Z plus seq * 7 seconds; every third entry carries a fraction.
    total = 8 * 3600 + seq * 7
    day = 1 + total // 86400
    rem = total % 86400
    s = "2026-11-%02dT%02d:%02d:%02d" % (day, rem // 3600, rem % 3600 // 60, rem % 60)
    if seq % 3 == 0:
        s += ".%03d" % (seq % 1000)
    return s + "Z"


def commitment(salt_hex, value):
    return sha(bytes.fromhex(salt_hex) + cbytes(value))


COMMANDS = ["project.set_tags", "feature.set_nominal", "program.add_step"]
PRINCIPALS = ["human", "agent", "script", "service"]


def blob_content(i):
    return "measurement table %02d\nrows: %d\n" % (i, 3 + i)


def build_good():
    d = Draw("CORE.JRN.CHAIN.VERIFY/good")
    registry = sha(b"registry manifest v1")
    kernel = sha(b"kernel manifest v1")
    blobs = [blob_content(i) for i in range(N_BLOBS)]
    blob_digests = [sha(c.encode()) for c in blobs]
    entries = []
    prev = ZERO
    for seq in range(1, N_ENTRIES + 1):
        salts = {}
        values = {}

        def commit(field, value):
            salt = d.bytes(16).hex()
            salts[field] = salt
            values[field] = value
            return commitment(salt, value)

        branch = "alt/1" if 300 <= seq < 340 else "main"
        pk = "human" if seq == 1 else PRINCIPALS[seq % 4]
        principal = {"kind": pk, "commit": commit("identity", {"user": "op-%02d" % (seq % 17), "name": "Søren"})}
        if pk == "agent":
            principal["on_behalf_of_commit"] = commit("on_behalf_of", {"user": "op-03"})
        h = {
            "schema_version": 1,
            "stream": STREAM,
            "seq": str(seq),
            "prev": prev,
            "branch": branch,
            "logical_parent": None if seq == 1 else prev,
            "time": {"utc": utc_of(seq), "mono_ns": str(1_000_000_000 + seq * 7_000_000_000), "sync": "synced"},
            "principal": principal,
            "ai_assistance": ["none", "suggested", "authored", "executed"][seq % 4] if pk == "agent" else "none",
            "manifests": {"registry": registry, "kernel": kernel},
        }
        if seq % 11 == 0:
            h["reason_commit"] = commit("reason", "operator note %d" % seq)
        if seq == 1:
            h["kind"] = "session_header"
            h["session"] = {"session_id": "s-0001", "app_version": "0.1.0"}
        elif seq % 50 == 0:
            h["kind"] = "head_move"
            h["head_move"] = {"from": sha(b"state %d" % (seq - 1)), "to": sha(b"state %d" % seq)}
        else:
            cid = COMMANDS[seq % 3]
            args = {"target": "F-%04d" % seq, "tags": ["t%d" % (seq % 5)]}
            events = [{"type": cid + "@1", "n": seq}]
            outputs = [{"entity": "F-%04d" % seq, "digest": sha(b"out %d" % seq)}]
            inputs = [{"tag": "param", "digest": sha(b"in %d" % seq)}] if seq % 5 == 0 else []
            h["kind"] = "command"
            h["command"] = {
                "id": cid,
                "major": 1,
                "class": "W2",
                "determinism": "D0",
                "seed": str(seq),
                "args_commit": commit("args", args),
                "events_commit": commit("events", events),
                "inputs": inputs,
                "outputs": outputs,
                "algorithms": [],
            }
        h["hash"] = sha(cbytes(h))
        header = cbytes(h)
        payload = dict(values)
        payload["salts"] = salts
        payload_bytes = cbytes(payload)
        refs = []
        if seq % 7 == 0:
            refs.append(blob_digests[(seq // 7) % N_BLOBS])
            if seq % 14 == 0:
                refs.append(blob_digests[(seq // 7 + 5) % N_BLOBS])
        refs = sorted(set(refs))
        entries.append({
            "seq": seq,
            "hash": h["hash"],
            "stream": STREAM,
            "branch": branch,
            "kind": h["kind"],
            "prev_hash": prev,
            "header": header.decode("utf-8"),
            "payload": payload_bytes.decode("utf-8"),
            "payload_digest": sha(payload_bytes),
            "refs": refs,
        })
        prev = h["hash"]
    used = sorted({r for e in entries for r in e["refs"]})
    assert used == sorted(blob_digests), "every blob is referenced"
    checkpoints = []
    for cid, (at, reason, t) in enumerate([(250, "signature", "2026-11-01T08:29:10Z"),
                                           (500, "close", "2026-11-01T09:00:00Z")], start=1):
        body = {"format": "linear-v1", "stream": STREAM, "seq": str(at), "head": entries[at - 1]["hash"],
                "count": str(at), "reason": reason, "time": t}
        checkpoints.append({"id": cid, "stream": STREAM, "entry_hash": body["head"], "body": canon(body)})
    return {
        "stream": STREAM,
        "entries": entries,
        "blobs": [{"digest": dg, "content": c} for dg, c in zip(blob_digests, blobs)],
        "checkpoints": checkpoints,
    }


# ------------------------------------------------------------------ the model the verifier reads

def to_model(good):
    """Rows as bytes; the blob store as a digest -> bytes map."""
    rows = []
    store = {}
    for e in good["entries"]:
        rows.append({
            "seq": e["seq"], "hash": e["hash"], "stream": e["stream"], "branch": e["branch"],
            "kind": e["kind"], "prev_hash": e["prev_hash"], "header": e["header"].encode("utf-8"),
            "refs": [["payload", e["payload_digest"]]] + [["ref", r] for r in e["refs"]],
        })
        store[e["payload_digest"]] = e["payload"].encode("utf-8")
    for b in good["blobs"]:
        store[b["digest"]] = b["content"].encode("utf-8")
    cps = [{"id": c["id"], "stream": c["stream"], "entry_hash": c["entry_hash"], "body": c["body"].encode("utf-8")}
           for c in good["checkpoints"]]
    return {"rows": rows, "store": store, "checkpoints": cps}


def flip(b, bit):
    arr = bytearray(b)
    arr[bit // 8] ^= 1 << (bit % 8)
    return bytes(arr)


def row_at(m, seq):
    for r in m["rows"]:
        if r["seq"] == seq:
            return r
    raise KeyError(seq)


def apply(m, muts):
    m = copy.deepcopy(m)
    for mu in muts:
        op = mu["op"]
        if op == "flip_header":
            r = row_at(m, mu["seq"])
            r["header"] = flip(r["header"], mu["bit"])
        elif op == "set_header":
            row_at(m, mu["seq"])["header"] = mu["header"].encode("utf-8")
        elif op == "set_column":
            row_at(m, mu["seq"])[mu["column"]] = mu["value"]
        elif op == "flip_store":
            m["store"][mu["digest"]] = flip(m["store"][mu["digest"]], mu["bit"])
        elif op == "remove_store":
            del m["store"][mu["digest"]]
        elif op == "remove_payloads":
            for r in m["rows"]:
                for role, dg in r["refs"]:
                    if role == "payload":
                        m["store"].pop(dg, None)
        elif op == "put_store":
            m["store"][mu["digest"]] = mu["content"].encode("utf-8")
        elif op == "set_refs":
            row_at(m, mu["seq"])["refs"] = [list(x) for x in mu["refs"]]
        elif op == "drop":
            m["rows"] = [r for r in m["rows"] if r["seq"] != mu["seq"]]
        elif op == "drop_renumber":
            m["rows"] = [r for r in m["rows"] if r["seq"] != mu["seq"]]
            for r in m["rows"]:
                if r["seq"] > mu["seq"]:
                    r["seq"] -= 1
        elif op == "swap":
            a, b = row_at(m, mu["a"]), row_at(m, mu["b"])
            for k in ("hash", "stream", "branch", "kind", "prev_hash", "header", "refs"):
                a[k], b[k] = b[k], a[k]
        elif op == "truncate":
            m["rows"] = [r for r in m["rows"] if r["seq"] <= mu["keep"]]
        elif op == "add_checkpoint":
            m["checkpoints"].append({"id": mu["id"], "stream": mu["stream"], "entry_hash": mu["entry_hash"],
                                     "body": mu["body"].encode("utf-8")})
        elif op == "flip_checkpoint":
            for c in m["checkpoints"]:
                if c["id"] == mu["id"]:
                    c["body"] = flip(c["body"], mu["bit"])
        elif op == "add_head":
            m.setdefault("heads", []).append([mu["branch"], mu["entry_hash"]])
        elif op == "relink_from":
            # The reviewer's attack: rewrite the stored hash of entry n-1 and re-chain n.. on it.
            n = mu["seq"]
            prev = mu["fake_below"]
            row_at(m, n - 1)["hash"] = prev
            for r in sorted(m["rows"], key=lambda r: r["seq"]):
                if r["seq"] < n:
                    continue
                h = json.loads(r["header"].decode("utf-8"))
                h["prev"] = prev
                if h.get("logical_parent") is not None:
                    h["logical_parent"] = prev
                h.pop("hash")
                h["hash"] = sha(cbytes(h))
                r["header"] = cbytes(h)
                r["hash"] = h["hash"]
                r["prev_hash"] = prev
                prev = h["hash"]
        elif op == "drop_checkpoints":
            m["checkpoints"] = []
        else:
            raise ValueError(op)
    return m


# ------------------------------------------------------------------ the reference verifier

class Fail(Exception):
    def __init__(self, seq, kind, reason):
        super().__init__(kind + "/" + reason)
        self.seq, self.kind, self.reason = seq, kind, reason


def days_in_month(y, mo):
    if mo == 2:
        return 29 if (y % 4 == 0 and y % 100 != 0) or y % 400 == 0 else 28
    return 30 if mo in (4, 6, 9, 11) else 31


def ckpt_time_ok(s):
    m = CKPT_TIME_RE.match(s)
    if not m or len(s) != 20:
        return False
    y, mo, d, h, mi, se = (int(x) for x in m.groups())
    return 1 <= mo <= 12 and 1 <= d <= days_in_month(y, mo) and h <= 23 and mi <= 59 and se <= 59


def parse_seq(s):
    if not isinstance(s, str) or not SEQ_RE.match(s):
        return None
    n = int(s)
    return n if 1 <= n < 2 ** 64 else None


def decode_checkpoint(body):
    """(seq, head, stream) or raises Fail with seq None."""
    try:
        v = strict_parse(body, MAX_CHECKPOINT)
    except Malformed:
        raise Fail(None, "checkpoint", "malformed")
    if not isinstance(v, dict):
        raise Fail(None, "checkpoint", "malformed")
    fmt = v.get("format")
    if isinstance(fmt, str) and fmt != "linear-v1":
        raise Fail(None, "checkpoint", "unknown_format")
    if fmt != "linear-v1":
        raise Fail(None, "checkpoint", "malformed")
    allowed = {"format", "stream", "seq", "head", "count", "ledger", "reason", "time"}
    if any(k not in allowed for k in v):
        raise Fail(None, "checkpoint", "malformed")
    for k in ("stream", "seq", "head", "count", "reason", "time"):
        if not isinstance(v.get(k), str):
            raise Fail(None, "checkpoint", "malformed")
    seq, count = parse_seq(v["seq"]), parse_seq(v["count"])
    ok = (seq is not None and count is not None and DIGEST_RE.match(v["head"])
          and v["reason"] in ("signature", "export", "close") and IDENT_RE.match(v["stream"])
          and len(v["stream"]) <= 64 and seq == count and ckpt_time_ok(v["time"]))
    if "ledger" in v:
        led = v["ledger"]
        ok = ok and isinstance(led, dict) and set(led) == {"seq", "head"} and parse_seq(led.get("seq")) is not None \
            and isinstance(led.get("head"), str) and DIGEST_RE.match(led["head"])
    if not ok or not is_canonical(body, v):
        raise Fail(None, "checkpoint", "malformed")
    return seq, v["head"], v["stream"]


def schema_ok(h):
    """The entry-schema checks a tampered-but-rehashed header can reach in these cases."""
    required = {"schema_version", "kind", "stream", "seq", "prev", "branch", "logical_parent", "time",
                "principal", "ai_assistance", "manifests", "hash"}
    body_of = {"command": "command", "session_header": "session", "principal_upgrade": "upgrade",
               "head_move": "head_move"}
    kind = h.get("kind")
    if kind not in body_of:
        return False
    allowed = required | {"reason_commit", "human_assertion", body_of[kind]}
    if not required.issubset(h) or body_of[kind] not in h or any(k not in allowed for k in h):
        return False
    t = h["time"]
    return (isinstance(h["stream"], str) and IDENT_RE.match(h["stream"]) and parse_seq(h["seq"]) is not None
            and isinstance(h["prev"], str) and DIGEST_RE.match(h["prev"])
            and isinstance(h["branch"], str) and BRANCH_RE.match(h["branch"])
            and isinstance(t, dict) and set(t) == {"utc", "mono_ns", "sync"} and isinstance(t["utc"], str)
            and UTC_RE.match(t["utc"]) and isinstance(t["mono_ns"], str) and U64_RE.match(t["mono_ns"]))


COMMITTED = [("identity", ("principal", "commit")), ("on_behalf_of", ("principal", "on_behalf_of_commit")),
             ("reason", ("reason_commit",)), ("args", ("command", "args_commit")),
             ("events", ("command", "events_commit")), ("upgrade_identity", ("upgrade", "identity_commit"))]


def commitments_of(h):
    out = {}
    for field, path in COMMITTED:
        v = h
        for p in path:
            v = v.get(p) if isinstance(v, dict) else None
        if v is not None:
            out[field] = v
    return out


def payload_ok(h, b):
    try:
        p = strict_parse(b, 8 * 1024 * 1024)
    except Malformed:
        return False
    if not isinstance(p, dict) or not is_canonical(b, p):
        return False
    want = commitments_of(h)
    salts = p.get("salts")
    if not isinstance(salts, dict) or set(p) != set(want) | {"salts"} or set(salts) != set(want):
        return False
    for field, c in want.items():
        s = salts[field]
        if not isinstance(s, str) or not HEX32_RE.match(s):
            return False
        try:
            if commitment(s, p[field]) != c:
                return False
        except Unsupported:
            return False
    return True


class Refused(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def checkpoint_row_ok(c, stream):
    """(seq, head) of a checkpoint row that passes on its own, else raises Fail (seq None)."""
    if c["id"] < 1:
        raise Fail(None, "checkpoint", "bad_id")
    seq, head, body_stream = decode_checkpoint(c["body"])
    if c["stream"] != stream:
        raise Fail(None, "checkpoint", "foreign_stream")
    if body_stream != c["stream"] or head != c["entry_hash"]:
        raise Fail(None, "checkpoint", "column_mismatch")
    return seq, head


def resolve_start(m, stream, from_seq, trusted):
    """(first seq, prev, anchor), or raises Refused("bad_range") or Corrupt (a located failure
    at the anchoring entry). Stored checkpoints are not authenticated (TRUST.VERIFY is not
    built), so only a caller's trusted head anchors a partial verification."""
    tail = max((r["seq"] for r in m["rows"]), default=0)
    if from_seq < 1 or from_seq > tail + 1:
        raise Refused("bad_range")
    if from_seq == 1:
        return 1, ZERO, {"kind": "genesis"}
    below_seq = from_seq - 1
    if trusted is None or trusted["seq"] != below_seq:
        raise Refused("bad_range")
    below = [r for r in m["rows"] if r["seq"] == below_seq]
    if not below:
        raise Refused("bad_range")
    try:
        h = check_row(below[0], below_seq, stream)
    except Fail as f:
        raise Corrupt(f)
    if h["hash"] != trusted["head"]:
        raise Corrupt(Fail(below_seq, "checkpoint", "forked_head"))
    return from_seq, trusted["head"], {"kind": "trusted", "seq": str(below_seq)}


class Corrupt(Exception):
    def __init__(self, fail):
        super().__init__(fail.kind)
        self.fail = fail


def verify(m, stream=STREAM, from_seq=1, trusted=None):
    """The report dict, or raises Fail (with the start attached) or Refused."""
    first, prev, anchor = resolve_start(m, stream, from_seq, trusted)
    start = {"from_seq": str(first), "anchor": anchor}
    try:
        return dict(walk(m, stream, first, prev, trusted), **start)
    except Fail as f:
        f.start = start
        raise


def check_row(r, p, stream):
    """Steps 2 to 13 on row r at sequence p; returns the header or raises Fail."""
    hb = r["header"]
    if len(hb) > MAX_HEADER:
        raise Fail(p, "hash", "oversize")
    try:
        h = strict_parse(hb, MAX_HEADER)
    except Malformed:
        raise Fail(p, "hash", "malformed")
    if not is_canonical(hb, h):
        raise Fail(p, "hash", "non_canonical")
    if not isinstance(h, dict) or not isinstance(h.get("hash"), str) or not DIGEST_RE.match(h["hash"]):
        raise Fail(p, "hash", "schema")
    bare = {k: v for k, v in h.items() if k != "hash"}
    if sha(cbytes(bare)) != h["hash"]:
        raise Fail(p, "hash", "hash_mismatch")
    sv = h.get("schema_version")
    if not (type(sv) in (int, float) and sv == 1):
        raise Fail(p, "hash", "unknown_version")
    if not schema_ok(h):
        raise Fail(p, "hash", "schema")
    if r["hash"] != h["hash"]:
        raise Fail(p, "hash", "column_mismatch")
    if h["seq"] != str(r["seq"]):
        raise Fail(p, "sequence", "header_seq")
    if h["stream"] != stream:
        raise Fail(p, "sequence", "foreign_stream")
    if r["stream"] != h["stream"] or r["branch"] != h["branch"] or r["kind"] != h["kind"]:
        raise Fail(p, "hash", "column_mismatch")
    if r["prev_hash"] != h["prev"]:
        raise Fail(p, "link", "column_mismatch")
    return h


def walk(m, stream, first, prev, trusted):
    cps = []
    for c in sorted(m["checkpoints"], key=lambda c: c["id"]):
        seq, head = checkpoint_row_ok(c, stream)
        if seq >= first:
            cps.append((seq, c["id"], head))
    rows = sorted((r for r in m["rows"] if r["seq"] >= first), key=lambda r: r["seq"])
    p = first - 1
    checked_blobs = set()
    without_payload = 0
    for r in rows:
        p += 1
        if trusted is not None and trusted["exact"] and p > trusted["seq"]:
            raise Fail(p, "sequence", "extended")
        if r["seq"] != p:
            raise Fail(p, "sequence", "gap" if r["seq"] > p else "reorder")
        h = check_row(r, p, stream)
        if h["prev"] != prev:
            raise Fail(p, "link", "genesis" if p == 1 else "broken")
        roles = [role for role, _ in r["refs"]]
        if any(x not in ("payload", "ref") for x in roles):
            raise Fail(p, "blob", "unknown_role")
        payloads = [dg for role, dg in r["refs"] if role == "payload"]
        if not payloads:
            raise Fail(p, "blob", "payload_ref_missing")
        if len(payloads) > 1:
            raise Fail(p, "blob", "payload_ref_duplicate")
        pb = m["store"].get(payloads[0])
        if pb is None:
            without_payload += 1
        else:
            if sha(pb) != payloads[0]:
                raise Fail(p, "blob", "corrupt")
            if not payload_ok(h, pb):
                raise Fail(p, "blob", "payload_mismatch")
        for dg in sorted(dg for role, dg in r["refs"] if role == "ref"):
            if dg in checked_blobs:
                continue
            b = m["store"].get(dg)
            if b is None:
                raise Fail(p, "blob", "missing")
            if sha(b) != dg:
                raise Fail(p, "blob", "corrupt")
            checked_blobs.add(dg)
        for seq, _, head in sorted(cps, key=lambda x: x[1]):
            if seq == p and head != h["hash"]:
                raise Fail(p, "checkpoint", "forked_head")
        if trusted is not None and trusted["seq"] == p and trusted["head"] != h["hash"]:
            raise Fail(p, "checkpoint", "forked_head")
        prev = h["hash"]
    beyond = sorted((seq, cid) for seq, cid, _ in cps if seq > p)
    if beyond:
        raise Fail(beyond[0][0], "checkpoint", "truncated")
    if trusted is not None and trusted["seq"] > p:
        raise Fail(trusted["seq"], "checkpoint", "truncated")
    # The head cache: every row must name an entry of the journal, on its own branch.
    by_hash = {r["hash"]: r for r in m["rows"]}
    for branch, entry_hash in sorted(m.get("heads", [])):
        r = by_hash.get(entry_hash)
        if r is None:
            raise Fail(None, "link", "head_past_chain")
        if r["branch"] != branch:
            raise Fail(None, "link", "head_branch")
    return {"entries": p - first + 1, "blobs": len(checked_blobs), "checkpoints": len(cps),
            "without_payload": without_payload}


def outcome(m, from_seq=1, trusted=None):
    try:
        r = verify(m, STREAM, from_seq, trusted)
        start = {"from_seq": r.pop("from_seq"), "anchor": r.pop("anchor")}
        return dict({"ok": True, "checked": r}, **start)
    except Fail as f:
        return dict({"ok": False, "failure": {"seq": f.seq, "kind": f.kind, "reason": f.reason}}, **f.start)
    except Refused as e:
        return {"refused": e.code}
    except Corrupt as c:
        f = c.fail
        return {"refused": "invariant_violated", "failure": {"seq": f.seq, "kind": f.kind, "reason": f.reason}}


# ------------------------------------------------------------------ cases

def rehash(h):
    h = dict(h)
    h.pop("hash", None)
    h["hash"] = sha(cbytes(h))
    return h


def header_of(good, seq):
    return json.loads(good["entries"][seq - 1]["header"])


def cases(good):
    model = to_model(good)
    d = Draw("CORE.JRN.CHAIN.VERIFY/cases")
    out = []

    def add(name, muts, from_seq=None, trusted=None):
        case = {"name": name, "mutations": muts}
        if from_seq is not None:
            case["from_seq"] = str(from_seq)
        if trusted is not None:
            case["trusted"] = {"seq": str(trusted["seq"]), "head": trusted["head"], "exact": trusted["exact"]}
        case["expect"] = outcome(apply(model, muts), 1 if from_seq is None else from_seq, trusted)
        out.append(case)

    add("good", [])
    add("no_payloads", [{"op": "remove_payloads"}])
    add("no_checkpoints", [{"op": "drop_checkpoints"}])

    # AC-2: one byte changed at a known sequence.
    e137 = good["entries"][136]
    add("bad_hash", [{"op": "set_header", "seq": 137,
                      "header": e137["header"].replace('"seed":"137"', '"seed":"138"')}])
    e211 = good["entries"][210]
    bad_prev = e211["prev_hash"][:-1] + ("0" if e211["prev_hash"][-1] != "0" else "1")
    h = header_of(good, 211)
    h["prev"] = bad_prev
    h = rehash(h)
    add("bad_link", [{"op": "set_header", "seq": 211, "header": canon(h)},
                     {"op": "set_column", "seq": 211, "column": "hash", "value": h["hash"]},
                     {"op": "set_column", "seq": 211, "column": "prev_hash", "value": bad_prev}])
    b7 = good["blobs"][7]["digest"]
    first_ref = min(e["seq"] for e in good["entries"] if b7 in e["refs"])
    add("bad_blob", [{"op": "flip_store", "digest": b7, "bit": 8 * 3 + 1}])
    cp1 = good["checkpoints"][0]
    add("bad_checkpoint", [{"op": "flip_checkpoint", "id": cp1["id"],
                            "bit": 8 * cp1["body"].index('"head":"sha256:') + 8 * 20}])

    # Single flipped bits at many positions of headers.
    for i in range(48):
        seq = 1 + d.below(N_ENTRIES)
        nbits = 8 * len(good["entries"][seq - 1]["header"].encode("utf-8"))
        add("flip_header_%02d" % i, [{"op": "flip_header", "seq": seq, "bit": d.below(nbits)}])
    # ... of payload blobs, referenced blobs and checkpoint bodies.
    for i in range(8):
        e = good["entries"][d.below(N_ENTRIES)]
        add("flip_payload_%d" % i, [{"op": "flip_store", "digest": e["payload_digest"],
                                     "bit": d.below(8 * len(e["payload"].encode("utf-8")))}])
    for i in range(6):
        b = good["blobs"][d.below(N_BLOBS)]
        add("flip_blob_%d" % i, [{"op": "flip_store", "digest": b["digest"],
                                  "bit": d.below(8 * len(b["content"]))}])
    for i in range(24):
        c = good["checkpoints"][i % 2]
        add("flip_checkpoint_%02d" % i, [{"op": "flip_checkpoint", "id": c["id"],
                                          "bit": d.below(8 * len(c["body"]))}])
    # Flips of the stored columns.
    for i, col in enumerate(["hash", "prev_hash"]):
        seq = 2 + d.below(N_ENTRIES - 1)
        v = good["entries"][seq - 1][col]
        pos = 7 + d.below(64)
        nv = v[:pos] + ("a" if v[pos] != "a" else "b") + v[pos + 1:]
        add("flip_column_%s" % col, [{"op": "set_column", "seq": seq, "column": col, "value": nv}])
    add("column_branch", [{"op": "set_column", "seq": 320, "column": "branch", "value": "main"}])
    add("column_kind", [{"op": "set_column", "seq": 100, "column": "kind", "value": "command"}])
    add("column_stream", [{"op": "set_column", "seq": 12, "column": "stream", "value": "ledger"}])

    # Dropped, reordered, swapped and truncated entries.
    add("drop_middle", [{"op": "drop", "seq": 123}])
    add("drop_first", [{"op": "drop", "seq": 1}])
    add("drop_renumber", [{"op": "drop_renumber", "seq": 123}])
    add("drop_renumber_first", [{"op": "drop_renumber", "seq": 1}])
    add("swap_adjacent", [{"op": "swap", "a": 40, "b": 41}])
    add("swap_far", [{"op": "swap", "a": 10, "b": 390}])
    add("truncate_tail", [{"op": "truncate", "keep": 490}])
    add("truncate_below_first_checkpoint", [{"op": "truncate", "keep": 200}])
    add("truncate_tail_no_checkpoint", [{"op": "truncate", "keep": 490}, {"op": "drop_checkpoints"}])

    # A fork: entry 300 replaced by another validly hashed entry linking to 299.
    h = header_of(good, 300)
    h["time"]["sync"] = "unsynced"
    h = rehash(h)
    fork = [{"op": "set_header", "seq": 300, "header": canon(h)},
            {"op": "set_column", "seq": 300, "column": "hash", "value": h["hash"]}]
    add("fork_middle", fork)
    h = header_of(good, 500)
    h["time"]["sync"] = "unsynced"
    h = rehash(h)
    add("fork_tail", [{"op": "set_header", "seq": 500, "header": canon(h)},
                      {"op": "set_column", "seq": 500, "column": "hash", "value": h["hash"]}])

    # Forged checkpoints.
    def ckpt(cid, seq, head, fmt="linear-v1", count=None, stream=STREAM, entry_hash=None):
        body = {"format": fmt, "stream": stream, "seq": str(seq), "head": head,
                "count": str(seq if count is None else count), "reason": "export", "time": "2026-11-02T00:00:00Z"}
        return {"op": "add_checkpoint", "id": cid, "stream": stream,
                "entry_hash": head if entry_hash is None else entry_hash, "body": canon(body)}

    real300 = good["entries"][299]["hash"]
    add("forged_checkpoint_head", [ckpt(3, 300, sha(b"forged head"))])
    add("forged_checkpoint_beyond", [ckpt(3, 600, sha(b"future head"))])
    add("forged_checkpoint_count", [ckpt(3, 300, real300, count=301)])
    add("forged_checkpoint_format", [ckpt(3, 300, real300, fmt="merkle-v1")])
    add("forged_checkpoint_row", [ckpt(3, 300, real300, entry_hash=good["entries"][298]["hash"])])
    add("forged_checkpoint_stream", [ckpt(3, 300, real300, stream="ledger")])
    add("genuine_extra_checkpoint", [ckpt(3, 300, real300)])
    body = canon({"format": "linear-v1", "stream": STREAM, "seq": "300", "head": real300, "count": "300",
                  "reason": "export", "time": "2026-11-02T00:00:00Z"})
    add("checkpoint_non_canonical", [{"op": "add_checkpoint", "id": 3, "stream": STREAM, "entry_hash": real300,
                                      "body": body.replace(",", ", ", 1)}])

    # Header variants with a valid hash.
    for name, edit in [
        ("unknown_version", lambda h: h.__setitem__("schema_version", 2)),
        ("schema_missing_branch", lambda h: h.pop("branch")),
        ("schema_unknown_field", lambda h: h.__setitem__("extra", "x")),
    ]:
        h = header_of(good, 77)
        edit(h)
        h = rehash(h)
        add(name, [{"op": "set_header", "seq": 77, "header": canon(h)},
                   {"op": "set_column", "seq": 77, "column": "hash", "value": h["hash"]}])
    hb = good["entries"][87]["header"]  # seq 88: human principal, branch main
    assert hb.startswith('{"ai_assistance":"none","branch":"main",')
    add("non_canonical_whitespace", [{"op": "set_header", "seq": 88, "header": hb.replace(",", ", ", 1)}])
    add("non_canonical_order", [{"op": "set_header", "seq": 88, "header": hb.replace(
        '{"ai_assistance":"none","branch":"main",', '{"branch":"main","ai_assistance":"none",', 1)}])
    add("malformed_header", [{"op": "set_header", "seq": 88, "header": hb[:-1]}])
    add("duplicate_key_header", [{"op": "set_header", "seq": 88,
                                  "header": hb.replace('{', '{"ai_assistance":"none",', 1)}])
    add("header_seq_mismatch", [{"op": "set_header", "seq": 92, "header": good["entries"][92]["header"]},
                                {"op": "set_column", "seq": 92, "column": "hash",
                                 "value": good["entries"][92]["hash"]}])
    h = header_of(good, 1)
    h["prev"] = sha(b"not genesis")
    h = rehash(h)
    add("genesis_prev", [{"op": "set_header", "seq": 1, "header": canon(h)},
                         {"op": "set_column", "seq": 1, "column": "hash", "value": h["hash"]},
                         {"op": "set_column", "seq": 1, "column": "prev_hash", "value": h["prev"]}])
    h = header_of(good, 33)
    h["stream"] = "ledger"
    h = rehash(h)
    add("foreign_stream_header", [{"op": "set_header", "seq": 33, "header": canon(h)},
                                  {"op": "set_column", "seq": 33, "column": "hash", "value": h["hash"]}])

    # Blob references.
    e = good["entries"][48]  # seq 49 = 7 * 7
    add("blob_missing", [{"op": "remove_store", "digest": e["refs"][0]}])
    add("payload_missing_ref", [{"op": "set_refs", "seq": 49, "refs": [["ref", r] for r in e["refs"]]}])
    other = good["entries"][49]["payload_digest"]
    add("payload_duplicate_ref", [{"op": "set_refs", "seq": 49, "refs": [["payload", e["payload_digest"]],
                                                                       ["payload", other]]}])
    add("payload_swapped", [{"op": "set_refs", "seq": 49, "refs": [["payload", other]] +
                             [["ref", r] for r in e["refs"]]}])
    add("unknown_role", [{"op": "set_refs", "seq": 49, "refs": [["payload", e["payload_digest"]],
                                                              ["thumbnail", e["refs"][0]]]}])
    forged = json.loads(e["payload"])
    forged["args"] = {"target": "F-9999", "tags": ["t0"]}
    fb = canon(forged)
    add("payload_forged", [{"op": "put_store", "digest": sha(fb.encode()), "content": fb},
                           {"op": "set_refs", "seq": 49, "refs": [["payload", sha(fb.encode())]] +
                            [["ref", r] for r in e["refs"]]}])
    add("one_payload_removed", [{"op": "remove_store", "digest": good["entries"][9]["payload_digest"]}])

    # Checkpoint row ids that append never writes.
    for cid in (0, -5):
        add("checkpoint_id_%s" % str(cid).replace("-", "minus"), [ckpt(cid, 300, real300)])

    # The head cache.
    add("head_ok", [{"op": "add_head", "branch": "main", "entry_hash": good["entries"][499]["hash"]},
                    {"op": "add_head", "branch": "alt/1", "entry_hash": good["entries"][338]["hash"]}])
    add("head_past_chain", [{"op": "add_head", "branch": "main", "entry_hash": sha(b"a head beyond the chain")}])
    add("head_other_branch", [{"op": "add_head", "branch": "alt/1", "entry_hash": good["entries"][499]["hash"]}])

    # Partial verification and its anchor (from_seq > 1).
    fake = sha(b"forged hash of entry 300")
    attack = [{"op": "relink_from", "seq": 301, "fake_below": fake}, {"op": "drop_checkpoints"}]
    real300_head = {"seq": 300, "head": real300, "exact": False}
    add("from_unanchored", [], from_seq=301)
    add("from_attack_unanchored", attack, from_seq=301)
    add("from_attack_full", attack)
    add("from_checkpoint_anchor", [ckpt(3, 300, real300)], from_seq=301)
    # The re-review's attack: a forged checkpoint row matching the forged hash at n-1.
    add("from_forged_checkpoint_attack", attack + [ckpt(3, 300, fake)], from_seq=301)
    add("from_trusted_anchor_row_tampered", [{"op": "flip_header", "seq": 300, "bit": 8 * 40 + 1}], from_seq=301,
        trusted=real300_head)
    add("from_trusted_head_mismatch", [], from_seq=301,
        trusted={"seq": 300, "head": good["entries"][298]["hash"], "exact": False})
    add("from_checkpoint_anchor_wrong_head", [ckpt(3, 300, good["entries"][298]["hash"],
                                                    entry_hash=real300)], from_seq=301)
    add("from_existing_checkpoint", [], from_seq=251)
    add("from_trusted_anchor", [], from_seq=301, trusted=real300_head)
    add("from_trusted_anchor_attack", attack, from_seq=301, trusted=real300_head)
    add("from_trusted_below_anchor", [], from_seq=301, trusted={"seq": 200, "head": good["entries"][199]["hash"],
                                                              "exact": False})
    add("from_trusted_above_unanchored", [], from_seq=301,
        trusted={"seq": 400, "head": good["entries"][399]["hash"], "exact": False})
    add("from_tail_plus_one", [], from_seq=501, trusted={"seq": 500, "head": good["entries"][499]["hash"],
                                                       "exact": True})
    add("from_beyond_tail", [], from_seq=502)
    add("from_zero", [], from_seq=0)
    add("trusted_forked", [], trusted={"seq": 250, "head": sha(b"x"), "exact": False})
    add("trusted_exact_extended", [], trusted={"seq": 250, "head": good["entries"][249]["hash"], "exact": True})
    add("trusted_truncated", [{"op": "truncate", "keep": 490}, {"op": "drop_checkpoints"}],
        trusted={"seq": 500, "head": good["entries"][499]["hash"], "exact": True})
    return out


def list_expect(good, q):
    """Sequences of the entries `journal.list_entries@1` returns for query q (REQ-005)."""
    out = []
    for e in good["entries"]:
        h = json.loads(e["header"])
        seq = e["seq"]
        if seq < int(q.get("from_seq", "1")) or ("to_seq" in q and seq > int(q["to_seq"])):
            continue
        if "branch" in q and h["branch"] != q["branch"]:
            continue
        if "principal_kind" in q and h["principal"]["kind"] != q["principal_kind"]:
            continue
        if "command" in q:
            name, major = q["command"].split("@")
            c = h.get("command")
            if c is None or c["id"] != name or c["major"] != int(major):
                continue
        out.append(seq)
    return out


LIST_QUERIES = [
    {"branch": "main", "command": "project.set_tags@1"},
    {"principal_kind": "agent", "from_seq": "100", "to_seq": "200"},
    {"branch": "alt/1"},
    {"command": "feature.set_nominal@2"},
    {},
]


def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False, sort_keys=True) + "\n").encode("utf-8")


def build():
    good = build_good()
    model = to_model(good)
    res = outcome(model)
    assert res == {"ok": True, "checked": {"entries": 500, "blobs": 20, "checkpoints": 2, "without_payload": 0},
                   "from_seq": "1", "anchor": {"kind": "genesis"}}, res
    cs = cases(good)
    names = [c["name"] for c in cs]
    assert len(names) == len(set(names))
    expect = {c["name"]: c["expect"] for c in cs}
    # AC-2 and AC-3 as the spec states them.
    assert expect["bad_hash"]["failure"] == {"seq": 137, "kind": "hash", "reason": "hash_mismatch"}
    assert expect["bad_link"]["failure"] == {"seq": 211, "kind": "link", "reason": "broken"}
    assert expect["bad_blob"]["failure"]["kind"] == "blob"
    assert expect["bad_checkpoint"]["failure"]["kind"] == "checkpoint"
    assert expect["no_payloads"]["checked"] == {"entries": 500, "blobs": 20, "checkpoints": 2,
                                                 "without_payload": 500}
    assert expect["from_unanchored"] == {"refused": "bad_range"}
    assert expect["from_attack_unanchored"] == {"refused": "bad_range"}
    assert expect["from_attack_full"]["failure"]["seq"] == 300
    assert expect["from_trusted_anchor_attack"] == {
        "refused": "invariant_violated", "failure": {"seq": 300, "kind": "hash", "reason": "column_mismatch"}}
    for name in ("from_checkpoint_anchor", "from_existing_checkpoint", "from_forged_checkpoint_attack"):
        assert expect[name] == {"refused": "bad_range"}, name
    assert expect["from_trusted_anchor"]["ok"]
    assert expect["from_trusted_anchor_row_tampered"]["refused"] == "invariant_violated"
    assert expect["from_trusted_head_mismatch"]["failure"]["reason"] == "forked_head"
    # No tamper case except the intended ones passes.
    for c in cs:
        if c["name"].startswith(("flip_header", "flip_payload", "flip_blob", "flip_column", "drop", "swap",
                                 "fork", "forged", "head_past", "head_other", "checkpoint_id")):
            assert not c["expect"]["ok"], c["name"]
    lists = [{"query": q, "seqs": list_expect(good, q)} for q in LIST_QUERIES]
    assert len(lists[0]["seqs"]) > 100 and lists[3]["seqs"] == [] and len(lists[4]["seqs"]) == N_ENTRIES
    return {"good": dump(good), "cases": dump({"stream": STREAM, "cases": cs, "lists": lists})}


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
