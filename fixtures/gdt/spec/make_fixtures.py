#!/usr/bin/env python3
"""Independent reference for GDT.CHAR.SPEC fixtures (stdlib only).

Writes, from the leaf's Design rules and the synthetic GDT.CHAR.DPROF placeholder profiles:

- plan_asme.fidproj: profile store holding the ASME 1.0.0 and ISO 1.0.0 placeholders on the
  working branch, plan `plan-1` defaulting to ASME Y14.5-2018 with the ASME profile, no
  tenant default and no characteristics (AC-1, AC-2, AC-6, AC-7).
- no_spec.fidproj: the ASME placeholder and characteristic `c-flat-1` with no governing spec
  (AC-4).
- digest_mismatch.fidproj: the ASME placeholder and characteristic `c-flat-1` whose spec names
  ASME 1.0.0 with the digest of the *modified* placeholder (AC-5).
- expected.json: the profile references, the digest of the explicit test key vocabulary
  (pinned with each stored profile), the RFC 8785 canonical bytes of the AC-1 and AC-2
  journal events, and the canonical form of plan_asme.fidproj.

The RFC 8785 (JCS) serializer here is this script's own (not the Rust code under test). It
covers objects, arrays, strings, booleans, null and integers: object members sorted by UTF-16
code units; strings escape `"`, `\\` and control characters (short forms where JCS has them,
lowercase `\\u00xx` otherwise). With `--check` it rebuilds every file in memory and fails if
any committed file differs byte for byte.

All content is synthetic placeholder data with no standards text; CC0.
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
DPROF = HERE.parent / "dprof"
SHORT = {'"': '\\"', "\\": "\\\\", "\b": "\\b", "\f": "\\f", "\n": "\\n", "\r": "\\r", "\t": "\\t"}
COMMAND = "gdt.set_governing_spec@1"


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


def load_profile(name):
    text = (DPROF / name).read_text(encoding="utf-8")
    return json.loads(text, object_pairs_hook=no_dupes, parse_float=refuse_float)


def ref_of(doc):
    digest = hashlib.sha256(jcs(doc).encode("utf-8")).hexdigest()
    return f"dp:{doc['dialect']}-{doc['edition']}@{doc['version']}#sha256:{digest}"


def vocabulary_digest():
    """Digest of the explicit test vocabulary (`../dprof/vocabulary.json`, every key accepting
    any value, no dialect), per the GDT.CHAR.DPROF vocabulary canonical form:
    `{"schema": "fid.dprof-vocabulary/1", "dialect": null, "keys": {"<key>": "any"}}`."""
    text = (DPROF / "vocabulary.json").read_text(encoding="utf-8")
    keys = json.loads(text, object_pairs_hook=no_dupes)["keys"]
    assert len(set(keys)) == len(keys)
    doc = {"schema": "fid.dprof-vocabulary/1", "dialect": None, "keys": {k: "any" for k in keys}}
    return "sha256:" + hashlib.sha256(jcs(doc).encode("utf-8")).hexdigest()


def store(*docs):
    vocab = vocabulary_digest()
    entries = [
        {"ref": ref_of(d), "branch": "working", "author": "human", "adopted": False, "vocabulary": vocab,
         "profile": d}
        for d in docs
    ]
    return {"schema": "fid.dprof-store/2", "profiles": entries, "bindings": {"working": {}, "agent": {}}}


def spec(dialect, standards, profile, origin):
    return {"dialect": dialect, "standards": standards, "defaults_profile": profile, "inherited_from": origin}


def project(store_v, tenant, plans, working, agent=None):
    return {
        "schema": "fid.gdt-spec-project/2",
        "profile_store": store_v,
        "tenant_default": tenant,
        "plan_defaults": plans,
        "characteristics": {"working": working, "agent": agent or {}},
    }


def pretty(v):
    # Readable, non-canonical layout: the tests compare the Rust writer against `jcs`.
    return json.dumps(v, indent=1, ensure_ascii=False) + "\n"


def build():
    asme = load_profile("asme_placeholder_1.0.0.json")
    asme_mod = load_profile("asme_placeholder_1.0.0_modified.json")
    iso = load_profile("iso_placeholder_1.0.0.json")
    r_asme, r_asme_mod, r_iso = ref_of(asme), ref_of(asme_mod), ref_of(iso)
    asme_std = ["std:asme:y14.5:2018"]
    iso_std = ["std:iso:1101:2017"]

    plan_asme = project(store(asme, iso), None, {"plan-1": spec("ASME", asme_std, r_asme, "explicit")}, {})
    no_spec = project(store(asme), None, {}, {"c-flat-1": None})
    mismatch = project(store(asme), None, {}, {"c-flat-1": spec("ASME", asme_std, r_asme_mod, "explicit")})

    inherited = spec("ASME", asme_std, r_asme, "plan")
    ac1 = {
        "command": COMMAND, "principal": "human", "branch": "working", "char": "flat-1",
        "plan": "plan-1", "inherited_from": "plan", "old": None, "new": inherited,
    }
    ac2 = {
        "command": COMMAND, "principal": "human", "branch": "working", "char": "flat-1",
        "plan": None, "inherited_from": "explicit", "old": inherited,
        "new": spec("ISO", iso_std, r_iso, "explicit"),
    }
    expected = {
        "note": "Written by make_fixtures.py from the GDT.CHAR.SPEC Design rules; synthetic, CC0.",
        "refs": {"asme_1.0.0": r_asme, "asme_1.0.0_modified": r_asme_mod, "iso_1.0.0": r_iso},
        "vocabulary": vocabulary_digest(),
        "ac1_event": jcs(ac1),
        "ac2_event": jcs(ac2),
        "plan_asme_canonical": jcs(plan_asme),
    }
    return {
        "plan_asme.fidproj": pretty(plan_asme),
        "no_spec.fidproj": pretty(no_spec),
        "digest_mismatch.fidproj": pretty(mismatch),
        "expected.json": pretty(expected),
    }


def main():
    check = "--check" in sys.argv[1:]
    failed = False
    for name, text in build().items():
        data = text.encode("utf-8")
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"{name}: differs from make_fixtures.py output", file=sys.stderr)
                failed = True
        else:
            path.write_bytes(data)
    if failed:
        sys.exit(1)


if __name__ == "__main__":
    main()
