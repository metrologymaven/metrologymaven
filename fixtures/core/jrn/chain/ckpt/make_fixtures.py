#!/usr/bin/env python3
"""Generate the fixtures of CORE.JRN.CHAIN.CKPT (branch heads and checkpoints).

An independent reference written with Python's standard library only (hashlib, json,
calendar, datetime, re). The Rust crate `fid-journal` (module `checkpoint`) is checked against
the file written here; nothing in it is produced by running the Rust code. The entry hashes
of the test project come from `../append/chain.json`, itself written by the independent
reference `../append/make_fixtures.py`.

File written next to this script: expected.json, with

- ac1: the checkpoint at project close of the first 50 entries of `../append/chain.json`
  (AC-1): its value, canonical bytes and digest, and the expected head of every branch
  after those 50 entries (AC-2);
- ac4: the same project with a test Access Ledger head, reason `signature` (AC-4);
- encode: more checkpoints with their canonical bytes and digests;
- time: Unix instants and the checkpoint time each one formats to, or `out_of_range`;
- decode: byte strings and whether decoding accepts them (else the error code);
- verify: a synthetic 10-entry chain and checkpoint-versus-journal cases with the expected
  outcome (ok, or the error code and its fields).

Pinned rules (spec/core/jrn/chain/ckpt/NODE.md, Design):
- the checkpoint object has exactly the keys format, stream, seq, head, count, reason, time
  and optionally ledger {seq, head}; format is "linear-v1"; seq, count and ledger.seq are
  decimal strings >= 1 without leading zeros (u64); head and ledger.head are `sha256:` + 64
  lowercase hex; stream is [a-z][a-z0-9_]* of at most 64 bytes; reason is signature, export
  or close; time is YYYY-MM-DDThh:mm:ssZ, a real calendar instant with second 0-59;
- in linear-v1, count equals seq;
- canonical bytes = RFC 8785 form; digest = "sha256:" + SHA-256(canonical bytes);
- time = the host clock truncated to whole seconds, UTC; years after 9999 are out of range;
- decoding: at most 4096 bytes (else input_limit); the bytes must be valid JSON whose
  canonical form is byte-identical to them (else schema); a `format` string other than
  linear-v1 is unknown_format, checked before every other field; any other violation is
  schema;
- verification of a checkpoint against a journal given as links (seq, prev, hash) in order:
  stream differs -> stream_mismatch; walking position p = 1, 2, ...: a link whose seq is not
  p -> reordered {position, seq}; a link whose prev is not the previous link's hash (the
  zero digest at position 1) -> broken_link {seq}; at p = checkpoint seq a hash other than
  the head -> forked_head {seq, checkpoint, journal}; fewer than seq links ->
  truncated {checkpoint_seq, journal_count}; in coverage `exact`, any link after seq ->
  extended {checkpoint_seq, journal_count} (links after seq are counted, not checked); in
  coverage `prefix`, links after seq are not read.

Values are ASCII strings, so json.dumps(sort_keys=True, separators=(",", ":")) equals RFC 8785.
CC-BY-4.0 project data.
Run:  python fixtures/core/jrn/chain/ckpt/make_fixtures.py [--check]
"""

import calendar
import datetime
import hashlib
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
SCHEMA = HERE.parents[4] / "schema" / "checkpoint.schema.json"
ZERO = "sha256:" + "0" * 64
FORMAT = "linear-v1"
MAX_BYTES = 4096
U64_MAX = 2**64 - 1
REASONS = ("signature", "export", "close")
KEYS = {"format", "stream", "seq", "head", "count", "reason", "time"}


def canon(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def fmt_time(unix_secs):
    if unix_secs < 0 or unix_secs > calendar.timegm((9999, 12, 31, 23, 59, 59, 0, 0, 0)):
        return None
    t = datetime.datetime(1970, 1, 1) + datetime.timedelta(seconds=unix_secs)
    return t.strftime("%Y-%m-%dT%H:%M:%SZ")


def checkpoint(stream, seq, head, reason, unix_secs, ledger=None):
    cp = {"format": FORMAT, "stream": stream, "seq": str(seq), "head": head, "count": str(seq),
          "reason": reason, "time": fmt_time(unix_secs)}
    if ledger is not None:
        cp["ledger"] = {"seq": str(ledger[0]), "head": ledger[1]}
    return cp


def encoded(cp):
    b = canon(cp)
    return {"value": cp, "canonical": b.decode("utf-8"), "digest": sha(b)}


# ------------------------------------------------------------------ validation (decode)

SEQ_RE = re.compile(r"[1-9][0-9]*")
DIGEST_RE = re.compile(r"sha256:[0-9a-f]{64}")
STREAM_RE = re.compile(r"[a-z][a-z0-9_]{0,63}")
TIME_RE = re.compile(r"([0-9]{4})-([0-9]{2})-([0-9]{2})T([0-9]{2}):([0-9]{2}):([0-9]{2})Z")


def is_seq(s):
    return isinstance(s, str) and SEQ_RE.fullmatch(s) is not None and int(s) <= U64_MAX


def is_digest(s):
    return isinstance(s, str) and DIGEST_RE.fullmatch(s) is not None


def is_time(s):
    if not isinstance(s, str):
        return False
    m = TIME_RE.fullmatch(s)
    if m is None:
        return False
    y, mo, d, h, mi, se = (int(x) for x in m.groups())
    if not 1 <= mo <= 12 or not 1 <= d <= calendar.monthrange(y, mo)[1]:
        return False
    return h <= 23 and mi <= 59 and se <= 59


def check_value(v):
    """None if `v` is a valid linear-v1 checkpoint, else the error code."""
    if not isinstance(v, dict):
        return "schema"
    f = v.get("format")
    if not isinstance(f, str):
        return "schema"
    if f != FORMAT:
        return "unknown_format"
    keys = set(v)
    if not KEYS <= keys or not keys <= KEYS | {"ledger"}:
        return "schema"
    if not isinstance(v["stream"], str) or STREAM_RE.fullmatch(v["stream"]) is None:
        return "schema"
    if not is_seq(v["seq"]) or not is_seq(v["count"]) or v["seq"] != v["count"]:
        return "schema"
    if not is_digest(v["head"]) or v["reason"] not in REASONS or not is_time(v["time"]):
        return "schema"
    if "ledger" in v:
        led = v["ledger"]
        if not isinstance(led, dict) or set(led) != {"seq", "head"}:
            return "schema"
        if not is_seq(led["seq"]) or not is_digest(led["head"]):
            return "schema"
    return None


def no_dupes(pairs):
    seen = {}
    for k, val in pairs:
        if k in seen:
            raise ValueError("duplicate key")
        seen[k] = val
    return seen


def decode(text):
    b = text.encode("utf-8")
    if len(b) > MAX_BYTES:
        return "input_limit"
    try:
        v = json.loads(b.decode("utf-8"), object_pairs_hook=no_dupes)
    except ValueError:
        return "schema"
    # Every accepted value is strings and objects only, so canon() is RFC 8785 for it; a value
    # holding numbers or other types is refused by check_value (or below) anyway.
    code = check_value(v)
    if code is not None:
        return code
    if canon(v) != b:
        return "schema"
    return None


# ------------------------------------------------------------------ schema subset check

def schema_ok(schema, v, root=None):
    """Validates `v` against the subset of JSON Schema the checkpoint schema uses."""
    root = root or schema
    if "$ref" in schema:
        name = schema["$ref"].rsplit("/", 1)[-1]
        return schema_ok(root["$defs"][name], v, root)
    if "const" in schema and v != schema["const"]:
        return False
    if "enum" in schema and v not in schema["enum"]:
        return False
    t = schema.get("type")
    if t == "string":
        if not isinstance(v, str) or re.search(schema["pattern"], v) is None or "\n" in v:
            return False
    if t == "object":
        if not isinstance(v, dict) or not set(schema["required"]) <= set(v):
            return False
        if schema.get("additionalProperties") is False and not set(v) <= set(schema["properties"]):
            return False
        return all(schema_ok(schema["properties"][k], x, root) for k, x in v.items())
    return True


# ------------------------------------------------------------------ test project (AC-1, AC-2, AC-4)

def project():
    chain = json.loads((HERE.parent / "append" / "chain.json").read_text(encoding="utf-8"))
    entries = chain["entries"][:50]
    for i, e in enumerate(entries):
        assert e["seq"] == str(i + 1)
    heads = {}
    for e in entries:
        heads[e["draft"]["branch"]] = {"seq": e["seq"], "hash": e["hash"]}
    clock = calendar.timegm((2026, 11, 19, 12, 0, 0, 0, 0, 0))
    head = entries[-1]["hash"]
    ac1 = encoded(checkpoint(chain["stream"], 50, head, "close", clock))
    ac1.update({"entries": 50, "clock_unix": str(clock), "heads": dict(sorted(heads.items()))})
    ledger = (7, sha(b"access-ledger-head-7"))
    ac4 = encoded(checkpoint(chain["stream"], 50, head, "signature", clock, ledger))
    ac4.update({"entries": 50, "clock_unix": str(clock)})
    return ac1, ac4


def encode_vectors():
    out = []
    for name, cp in [
        ("export_seq_1", checkpoint("record", 1, sha(b"entry-1"), "export", 0)),
        ("signature_max_seq", checkpoint("record", U64_MAX, sha(b"entry-max"), "signature",
                                         calendar.timegm((9999, 12, 31, 23, 59, 59, 0, 0, 0)))),
        ("close_with_ledger", checkpoint("activity", 123456789, sha(b"entry-123456789"), "close",
                                         calendar.timegm((2000, 2, 29, 23, 59, 59, 0, 0, 0)),
                                         (1, sha(b"ledger-1")))),
    ]:
        e = encoded(cp)
        e["name"] = name
        out.append(e)
    return out


def time_vectors():
    cases = [
        ("epoch", 0, 0),
        ("epoch_plus_nanos_truncated", 0, 999_999_999),
        ("leap_day_2000", calendar.timegm((2000, 2, 29, 0, 0, 0, 0, 0, 0)), 0),
        ("end_of_2099", calendar.timegm((2099, 12, 31, 23, 59, 59, 0, 0, 0)), 500_000_000),
        ("project_close", calendar.timegm((2026, 11, 19, 12, 0, 0, 0, 0, 0)), 1),
        ("last_second_of_9999", calendar.timegm((9999, 12, 31, 23, 59, 59, 0, 0, 0)), 999_999_999),
        ("year_10000", calendar.timegm((9999, 12, 31, 23, 59, 59, 0, 0, 0)) + 1, 0),
    ]
    out = []
    for name, secs, nanos in cases:
        t = fmt_time(secs)
        out.append({"name": name, "unix_secs": str(secs), "nanos": nanos, "time": t if t else "out_of_range"})
    return out


def decode_cases(ac1):
    good = ac1["canonical"]
    v = ac1["value"]

    def with_(**kw):
        x = dict(v)
        for k, val in kw.items():
            if val is None:
                x.pop(k, None)
            else:
                x[k] = val
        return canon(x).decode("utf-8")

    cases = [
        ("canonical", good),
        ("with_ledger", with_(ledger={"seq": "3", "head": sha(b"l")})),
        ("whitespace", good.replace(",", ", ", 1)),
        ("keys_out_of_order", '{"format":"linear-v1",' + good[1:].replace(',"format":"linear-v1"', "")),
        ("duplicate_key", good[:-1] + ',"time":' + json.dumps(v["time"]) + "}"),
        ("not_json", good[:-1]),
        ("array", "[]"),
        ("unknown_format", with_(format="merkle-v1")),
        ("unknown_format_with_other_fields", canon({"format": "merkle-v1", "tree_size": "50"}).decode()),
        ("format_missing", with_(format=None)),
        ("format_number", with_(format=1)),
        ("extra_key", with_(extra="x")),
        ("time_missing", with_(time=None)),
        ("seq_zero", with_(seq="0", count="0")),
        ("seq_leading_zero", with_(seq="050", count="050")),
        ("seq_over_u64", with_(seq="18446744073709551616", count="18446744073709551616")),
        ("seq_number", canon(dict(v, seq=50)).decode()),
        ("count_differs", with_(count="49")),
        ("head_uppercase", with_(head=v["head"].upper().replace("SHA256", "sha256"))),
        ("stream_uppercase", with_(stream="Record")),
        ("stream_65_bytes", with_(stream="r" * 65)),
        ("stream_64_bytes", with_(stream="r" * 64)),
        ("reason_unknown", with_(reason="manual")),
        ("time_fraction", with_(time="2026-11-19T12:00:00.5Z")),
        ("time_feb_30", with_(time="2026-02-30T12:00:00Z")),
        ("time_leap_second", with_(time="2016-12-31T23:59:60Z")),
        ("ledger_extra_key", with_(ledger={"seq": "3", "head": sha(b"l"), "x": "y"})),
        ("ledger_seq_zero", with_(ledger={"seq": "0", "head": sha(b"l")})),
        ("ledger_null", canon(dict(v, ledger=None)).decode()),
        ("too_long", with_(stream="r") + " " * (MAX_BYTES + 1)),
    ]
    out = []
    for name, text in cases:
        code = decode(text)
        out.append({"name": name, "bytes": text, "ok": code is None, **({"code": code} if code else {})})
    return out


# ------------------------------------------------------------------ verification

def chain_links(n, tag="ckpt-link", fork_from=None):
    links, prev = [], ZERO
    for i in range(1, n + 1):
        h = sha(f"{'alt-link' if fork_from and i >= fork_from else tag}-{i}".encode())
        links.append([str(i), prev, h])
        prev = h
    return links


def verify(cp, stream, links, coverage):
    code = check_value(cp)
    if code is not None:
        return {"code": code}
    if cp["stream"] != stream:
        return {"code": "stream_mismatch"}
    target = int(cp["seq"])
    p, prev = 0, ZERO
    for seq, lprev, h in links:
        if p >= target:
            if coverage == "prefix":
                break
            p += 1
            continue
        p += 1
        if seq != str(p):
            return {"code": "reordered", "position": str(p), "seq": seq}
        if lprev != prev:
            return {"code": "broken_link", "seq": seq}
        if p == target and h != cp["head"]:
            return {"code": "forked_head", "seq": seq, "checkpoint": cp["head"], "journal": h}
        prev = h
    if p < target:
        return {"code": "truncated", "checkpoint_seq": cp["seq"], "journal_count": str(p)}
    if p > target:
        return {"code": "extended", "checkpoint_seq": cp["seq"], "journal_count": str(p)}
    return {"code": "ok", "checked": cp["seq"]}


def verify_cases():
    t = calendar.timegm((2026, 11, 19, 12, 0, 0, 0, 0, 0))
    full = chain_links(10)
    cp6 = checkpoint("record", 6, full[5][2], "export", t)
    cp10 = checkpoint("record", 10, full[9][2], "close", t)

    def swapped(links, a, b):
        x = [list(link) for link in links]
        x[a], x[b] = x[b], x[a]
        return x

    def edited(links, i, col, val):
        x = [list(link) for link in links]
        x[i][col] = val
        return x

    alt6 = edited(edited(full, 5, 2, sha(b"fork-6")), 6, 1, sha(b"fork-6"))
    cases = [
        ("exact_full", cp10, "record", full, "exact"),
        ("prefix_full", cp10, "record", full, "prefix"),
        ("prefix_longer_journal", cp6, "record", full, "prefix"),
        ("exact_longer_journal_is_extended", cp6, "record", full, "exact"),
        ("truncated", cp10, "record", full[:7], "exact"),
        ("truncated_prefix", cp10, "record", full[:7], "prefix"),
        ("truncated_to_empty", cp6, "record", [], "exact"),
        ("reordered", cp10, "record", swapped(full, 3, 4), "exact"),
        ("reordered_after_checkpoint_prefix", cp6, "record", swapped(full, 7, 8), "prefix"),
        ("reordered_after_checkpoint_exact", cp6, "record", swapped(full, 7, 8), "exact"),
        ("sequence_gap", cp10, "record", edited(full, 4, 0, "6"), "exact"),
        ("duplicate_sequence", cp10, "record", edited(full, 3, 0, "3"), "exact"),
        ("forked_head_at_checkpoint", cp6, "record", alt6, "prefix"),
        ("forked_below_checkpoint", cp6, "record", chain_links(10, fork_from=4), "prefix"),
        ("checkpoint_head_from_another_chain", checkpoint("record", 6, sha(b"elsewhere"), "close", t),
         "record", full, "prefix"),
        ("broken_link", cp10, "record", edited(full, 4, 1, sha(b"garbage")), "exact"),
        ("broken_genesis", cp10, "record", edited(full, 0, 1, sha(b"not-zero")), "exact"),
        ("stream_mismatch", cp6, "ledger", full, "prefix"),
        ("unknown_format", dict(cp6, format="merkle-v1"), "record", full, "prefix"),
        ("count_differs_is_schema", dict(cp6, count="5"), "record", full, "prefix"),
    ]
    out = []
    for name, cp, stream, links, coverage in cases:
        out.append({"name": name, "checkpoint": canon(cp).decode("utf-8"), "stream": stream, "links": links,
                    "coverage": coverage, "expect": verify(cp, stream, links, coverage)})
    return out


# Rules JSON Schema does not express: canonical form, duplicate keys, the byte bound, count
# equal to seq, and calendar validity of the time.
OUTSIDE_SCHEMA = ("whitespace", "keys_out_of_order", "too_long", "duplicate_key", "count_differs", "time_feb_30",
                  "time_leap_second")


def build():
    schema = json.loads(SCHEMA.read_text(encoding="utf-8"))
    ac1, ac4 = project()
    enc = encode_vectors()
    dec = decode_cases(ac1)
    for e in [ac1, ac4] + enc:
        assert check_value(e["value"]) is None
        assert schema_ok(schema, e["value"]), e
    for c in dec:
        v = None
        try:
            v = json.loads(c["bytes"])
        except ValueError:
            pass
        # The schema and the decoder agree on every value-level case (canonical form,
        # duplicate keys and the byte bound are outside JSON Schema).
        if v is not None and c["name"] not in OUTSIDE_SCHEMA:
            assert schema_ok(schema, v) == c["ok"], c["name"]
    return {
        "format": FORMAT,
        "max_bytes": MAX_BYTES,
        "ac1": ac1,
        "ac4": ac4,
        "encode": enc,
        "time": time_vectors(),
        "decode": dec,
        "verify": verify_cases(),
    }


def dump(v):
    return (json.dumps(v, indent=1, ensure_ascii=False) + "\n").encode("utf-8")


def main():
    path = HERE / "expected.json"
    data = dump(build())
    if "--check" in sys.argv[1:]:
        if not path.exists() or path.read_bytes() != data:
            print("stale fixture (run make_fixtures.py): expected.json")
            sys.exit(1)
        print("fixtures up to date")
        return
    path.write_bytes(data)
    print("wrote expected.json")


if __name__ == "__main__":
    main()
