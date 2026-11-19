#!/usr/bin/env python3
"""Independent reference for GDT.CHAR.DPROF fixtures (stdlib only).

For every placeholder profile in this folder, computes the RFC 8785 (JCS) canonical
form with its own serializer (not the Rust code under test), its SHA-256, and the
expected profile reference `dp:<dialect>-<edition>@<version>#sha256:<hex>`, and writes
them to `expected.json`. With `--check` it recomputes and fails if `expected.json`
differs.

The canonicalizer covers the value kinds a `fid.defaults-profile/1` document may hold:
objects, arrays, strings, booleans and integers (non-integer numbers are refused, as the
profile format refuses them). Object members are sorted by UTF-16 code units; strings
escape only `"`, `\\` and control characters, using the short forms where RFC 8785 has
them and lowercase `\\u00xx` otherwise.

All fixtures are synthetic placeholders with no standards content; CC0.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PROFILES = [
    "asme_placeholder_1.0.0.json",
    "asme_placeholder_1.0.0_modified.json",
    "asme_placeholder_1.1.0.json",
    "iso_placeholder_1.0.0.json",
]
# Profiles the format refuses because a number literal is not an integer literal, even
# where its value is integral (malformed_profile{bad_number}).
REFUSED = [
    "bad_number_exponent.json",
    "bad_number_fraction.json",
    "bad_number_long_fraction.json",
]
SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}


def no_dupes(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise ValueError(f"duplicate key {k!r}")
        out[k] = v
    return out


def refuse_float(text):
    raise ValueError(f"non-integer number {text!r}")


def jcs_string(s):
    out = ['"']
    for ch in s:
        if ch in SHORT:
            out.append(SHORT[ch])
        elif ord(ch) < 0x20:
            out.append("\\u%04x" % ord(ch))
        else:
            out.append(ch)
    out.append('"')
    return "".join(out)


def jcs(v):
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return "null"
    if isinstance(v, int):
        if abs(v) > 2**53 - 1:
            raise ValueError("integer outside the safe range")
        return str(v)
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v, key=lambda k: k.encode("utf-16-be"))
        return "{" + ",".join(jcs_string(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise ValueError(f"unsupported value {v!r}")


def case(name):
    raw = (HERE / name).read_text(encoding="utf-8")
    doc = json.loads(raw, object_pairs_hook=no_dupes, parse_float=refuse_float)
    canonical = jcs(doc).encode("utf-8")
    digest = hashlib.sha256(canonical).hexdigest()
    ref = f"dp:{doc['dialect']}-{doc['edition']}@{doc['version']}#sha256:{digest}"
    return {"file": name, "canonical": canonical.decode("utf-8"), "sha256": "sha256:" + digest, "ref": ref}


def refused(name):
    raw = (HERE / name).read_text(encoding="utf-8")
    try:
        json.loads(raw, object_pairs_hook=no_dupes, parse_float=refuse_float)
    except ValueError as e:
        if "non-integer number" in str(e):
            return {"file": name, "refused": "bad_number"}
        raise
    raise SystemExit(f"{name}: expected a refusal")


def build():
    cases = [case(n) for n in PROFILES]
    bad = [refused(n) for n in REFUSED]
    doc = {"generator": "make_cases.py", "cases": cases, "refused": bad}
    return json.dumps(doc, indent=2, ensure_ascii=False) + "\n"


def main():
    text = build()
    target = HERE / "expected.json"
    if "--check" in sys.argv[1:]:
        if target.read_text(encoding="utf-8") != text:
            print("expected.json is stale: run python fixtures/gdt/dprof/make_cases.py")
            return 1
        print("expected.json is up to date")
        return 0
    target.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote {target}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
