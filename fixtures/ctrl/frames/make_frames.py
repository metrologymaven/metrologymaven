#!/usr/bin/env python3
"""Seed corpus and expected verdicts for the control-channel framing (AUTO.CTRL.WIRE AC-4).

Writes fixtures/ctrl/frames/*.jsonl (one byte stream each, also the `ctrl_frame` fuzz
seed corpus) and expected.txt (`<file> <verdict> <verdict> ...`). The verdicts are computed
by this script's own reading of the wire rules, independently of the Rust code:

- a stream is split at b"\\n"; trailing bytes without a newline are `partial`;
- a line over 1 MiB is `too_large` and nothing after it is read;
- a line must be UTF-8 JSON (no duplicate keys, no NaN/Infinity, nesting <= 64) holding
  an object with exactly v, id, op, params: v an integer 0..65535, id an integer
  0..2^53-1, op dot-separated [a-z][a-z0-9_]* segments of at most 64 bytes in all,
  params an object; `hello` params are exactly {"versions": [<= 64 integers 0..65535]};
- a valid line is `ok:<op>`; an invalid one is `malformed:<id>` when id is a valid
  integer, else `malformed:-`.

Project-authored data, CC-BY-4.0. Run: python fixtures/ctrl/frames/make_frames.py [--check]
"""

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
LIMIT = 1024 * 1024
MAX_ID = 2**53 - 1
OP = re.compile(r"^[a-z][a-z0-9_]*(\.[a-z][a-z0-9_]*)*$")

CASES = {
    "ok_ping": b'{"v":1,"id":1,"op":"ping","params":{}}\n',
    "ok_hello": b'{"v":1,"id":2,"op":"hello","params":{"versions":[1]}}\n',
    "ok_hello_empty_versions": b'{"op":"hello","params":{"versions":[]},"v":1,"id":3}\n',
    "ok_hello_unknown_versions": b'{"v":1,"id":4,"op":"hello","params":{"versions":[99]}}\n',
    "ok_dotted_op": b'{"v":1,"id":5,"op":"ui.screenshot","params":{"scale":2}}\n',
    "ok_whitespace": b' { "v" : 1 , "id" : 6 , "op" : "jobs.list" , "params" : { } } \n',
    "ok_max_id": b'{"v":1,"id":9007199254740991,"op":"ping","params":{}}\n',
    "ok_unicode_params": '{"v":1,"id":7,"op":"ping","params":{"s":"µm ⌀"}}\n'.encode(),
    "ok_v_future": b'{"v":2,"id":8,"op":"ping","params":{}}\n',
    "multi_three_lines": b'{"v":1,"id":1,"op":"ping","params":{}}\nnope\n{"v":1,"id":2,"op":"a.b","params":{}}\n',
    "mal_not_json": b"hello world\n",
    "mal_empty_line": b"\n",
    "mal_array": b"[1,2,3]\n",
    "mal_string": b'"ping"\n',
    "mal_truncated_json": b'{"v":1,"id":10,"op":"ping"\n',
    "mal_missing_params": b'{"v":1,"id":11,"op":"ping"}\n',
    "mal_missing_v": b'{"id":12,"op":"ping","params":{}}\n',
    "mal_missing_id": b'{"v":1,"op":"ping","params":{}}\n',
    "mal_extra_field": b'{"v":1,"id":13,"op":"ping","params":{},"auth":"x"}\n',
    "mal_params_array": b'{"v":1,"id":14,"op":"ping","params":[]}\n',
    "mal_op_upper": b'{"v":1,"id":15,"op":"Ping","params":{}}\n',
    "mal_op_empty": b'{"v":1,"id":16,"op":"","params":{}}\n',
    "mal_op_trailing_dot": b'{"v":1,"id":17,"op":"ui.","params":{}}\n',
    "mal_op_too_long": b'{"v":1,"id":18,"op":"' + b"a" * 65 + b'","params":{}}\n',
    "mal_op_number": b'{"v":1,"id":19,"op":7,"params":{}}\n',
    "mal_v_negative": b'{"v":-1,"id":20,"op":"ping","params":{}}\n',
    "mal_v_too_big": b'{"v":65536,"id":21,"op":"ping","params":{}}\n',
    "mal_v_fraction": b'{"v":1.5,"id":22,"op":"ping","params":{}}\n',
    "mal_v_string": b'{"v":"1","id":23,"op":"ping","params":{}}\n',
    "mal_id_negative": b'{"v":1,"id":-5,"op":"ping","params":{}}\n',
    "mal_id_too_big": b'{"v":1,"id":9007199254740992,"op":"ping","params":{}}\n',
    "mal_id_string": b'{"v":1,"id":"24","op":"ping","params":{}}\n',
    "mal_id_fraction": b'{"v":1,"id":2.5,"op":"ping","params":{}}\n',
    "mal_duplicate_key": b'{"v":1,"id":25,"op":"ping","params":{},"op":"ping"}\n',
    "mal_nan": b'{"v":1,"id":26,"op":"ping","params":{"x":NaN}}\n',
    "mal_invalid_utf8": b'{"v":1,"id":27,"op":"ping","params":{"s":"\xff"}}\n',
    "mal_deep_nesting": b'{"v":1,"id":28,"op":"ping","params":{"a":' + b"[" * 200 + b"]" * 200 + b"}}\n",
    "mal_hello_no_versions": b'{"v":1,"id":29,"op":"hello","params":{}}\n',
    "mal_hello_versions_string": b'{"v":1,"id":30,"op":"hello","params":{"versions":"1"}}\n',
    "mal_hello_extra_field": b'{"v":1,"id":31,"op":"hello","params":{"versions":[1],"token":"x"}}\n',
    "mal_hello_bad_version": b'{"v":1,"id":32,"op":"hello","params":{"versions":[1,70000]}}\n',
    "mal_hello_too_many": b'{"v":1,"id":33,"op":"hello","params":{"versions":[' + b",".join([b"1"] * 65) + b"]}}\n",
    "partial_no_newline": b'{"v":1,"id":34,"op":"ping","params":{}}',
    "partial_after_line": b'{"v":1,"id":35,"op":"ping","params":{}}\n{"v":1,',
    "empty_stream": b"",
}


def no_dupes(pairs):
    keys = [k for k, _ in pairs]
    if len(keys) != len(set(keys)):
        raise ValueError("duplicate key")
    return dict(pairs)


def reject_constant(name):
    raise ValueError("non-finite number " + name)


def depth(x):
    if isinstance(x, dict):
        return 1 + max((depth(v) for v in x.values()), default=0)
    if isinstance(x, list):
        return 1 + max((depth(v) for v in x), default=0)
    return 0


def is_uint(x, top):
    if isinstance(x, bool):
        return False
    if isinstance(x, float):
        if not x.is_integer():
            return False
        x = int(x)
    return isinstance(x, int) and 0 <= x <= top


def classify(line):
    try:
        text = line.decode("utf-8")
        obj = json.loads(text, object_pairs_hook=no_dupes, parse_constant=reject_constant)
    except (UnicodeDecodeError, ValueError):
        return "malformed:-"
    if not isinstance(obj, dict) or depth(obj) > 64:
        return "malformed:-"
    if "id" not in obj or not is_uint(obj["id"], MAX_ID):
        return "malformed:-"
    bad = "malformed:%d" % int(obj["id"])
    if set(obj) != {"v", "id", "op", "params"}:
        return bad
    if not is_uint(obj["v"], 65535):
        return bad
    op = obj["op"]
    if not isinstance(op, str) or not (1 <= len(op.encode()) <= 64) or not OP.match(op):
        return bad
    if not isinstance(obj["params"], dict):
        return bad
    if op == "hello":
        p = obj["params"]
        if set(p) != {"versions"} or not isinstance(p["versions"], list) or len(p["versions"]) > 64:
            return bad
        if not all(is_uint(v, 65535) for v in p["versions"]):
            return bad
    return "ok:" + op


def verdicts(stream):
    out = []
    rest = stream
    while rest:
        i = rest.find(b"\n")
        if i < 0:
            out.append("too_large" if len(rest) > LIMIT else "partial")
            break
        if i > LIMIT:
            out.append("too_large")
            break
        out.append(classify(rest[:i]))
        rest = rest[i + 1:]
    return out


def main():
    check = "--check" in sys.argv
    lines = []
    stale = []
    for name in sorted(CASES):
        data = CASES[name]
        path = HERE / (name + ".jsonl")
        if check:
            if not path.exists() or path.read_bytes() != data:
                stale.append(path.name)
        else:
            path.write_bytes(data)
        lines.append(" ".join([path.name] + verdicts(data)))
    expected = ("\n".join(lines) + "\n").encode()
    exp_path = HERE / "expected.txt"
    if check:
        if not exp_path.exists() or exp_path.read_bytes() != expected:
            stale.append(exp_path.name)
        if stale:
            print("stale: " + ", ".join(stale))
            return 1
        print("ctrl frames: %d cases up to date" % len(CASES))
        return 0
    exp_path.write_bytes(expected)
    print("wrote %d cases" % len(CASES))
    return 0


if __name__ == "__main__":
    sys.exit(main())
