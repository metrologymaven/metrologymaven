#!/usr/bin/env python3
"""Independent reference for the TRUST.PROFILE.DEFS fixtures (crate fid-profile, module defs).

Stdlib only (jsonschema is used for the optional schema check when it is installed, as it
is in CI). Nothing here runs or imports the Rust code under test. The built-in parameter
values are typed in from the spec, with the node each value comes from:

- spec/trust/profile/defs/NODE.md REQ-TRUST.PROFILE.DEFS-003: Regulated signatures required,
  [AUTHORED, APPROVED], software keys not allowed, program release by other "always";
  Assistive-only agent_writes "proposals_only"; Lean signatures not required.
- spec/trust/profile/defs/NODE.md Open questions: Lean allow_software_keys true.
- spec/trust/profile/gate/query/NODE.md REQ-...GATE.QUERY-004: the only restrictive gates are
  Regulated (signature before release, software keys disabled) and Assistive-only (agent
  direct write disabled); Lean is permissive. So agent_writes is "branch" for Lean and
  Regulated, and Assistive-only signs like Lean (no signatures required, software keys
  allowed, no required meanings).
- spec/cmm/exec/ticket/NODE.md REQ-CMM.EXEC.TICKET-002: release by another human is needed
  for agent-authored programs, or always under Regulated. So Lean and Assistive-only are
  "agent_authored".

The profile version (1) is the first version of each built-in. Fixture content is synthetic
(CC-BY-4.0).

Outputs (all under fixtures/profile/):
- builtin/{lean,regulated,assistive_only}.json  canonical documents (RFC 8785, no newline)
- regulated-with-xqa.json                        tenant file for AC-2
- expected/regulated-with-xqa.json               its instantiated canonical document
- digests.json                                   pinned SHA-256 digests
- tenant/*.json + tenant/cases.json              tenant files with hand-labelled outcomes

Run:  python fixtures/profile/make_fixtures.py          (write)
      python fixtures/profile/make_fixtures.py --check  (fail if any file would change)
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
VERSION = 1

BUILTIN = {
    "lean": {
        "agent_writes": "branch",
        "allow_software_keys": True,
        "program_release_by_other": "agent_authored",
        "release_requires_signatures": False,
        "required_meanings": [],
        "tenant_meanings": [],
    },
    "regulated": {
        "agent_writes": "branch",
        "allow_software_keys": False,
        "program_release_by_other": "always",
        "release_requires_signatures": True,
        "required_meanings": ["AUTHORED", "APPROVED"],
        "tenant_meanings": [],
    },
    "assistive_only": {
        "agent_writes": "proposals_only",
        "allow_software_keys": True,
        "program_release_by_other": "agent_authored",
        "release_requires_signatures": False,
        "required_meanings": [],
        "tenant_meanings": [],
    },
}

# ---------------------------------------------------------------- RFC 8785 subset
# Only strings, booleans, small non-negative integers, arrays and objects occur here.


def jcs_string(s: str) -> str:
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


def utf16_key(s: str):
    return list(s.encode("utf-16-be"))


def jcs(v) -> str:
    if v is True:
        return "true"
    if v is False:
        return "false"
    if v is None:
        return "null"
    if isinstance(v, int):
        assert 0 <= v < 2**53
        return str(v)
    if isinstance(v, str):
        return jcs_string(v)
    if isinstance(v, list):
        return "[" + ",".join(jcs(x) for x in v) + "]"
    if isinstance(v, dict):
        keys = sorted(v.keys(), key=utf16_key)
        return "{" + ",".join(jcs_string(k) + ":" + jcs(v[k]) for k in keys) + "}"
    raise TypeError(type(v))


def canonical(doc) -> bytes:
    return jcs(doc).encode("utf-8")


def digest(b: bytes) -> str:
    return "sha256:" + hashlib.sha256(b).hexdigest()


def document(name, params):
    return {"name": name, "version": VERSION, "params": params}


def pretty(v) -> bytes:
    return (json.dumps(v, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


# ---------------------------------------------------------------- tenant files

XQA_TENANT = {
    "tenant_meanings": [
        {
            "code": "X_QA_RELEASE",
            "label": "QA release",
            "description": "The site quality team has released the record for use.",
        }
    ],
    "required_meanings": ["AUTHORED", "X_QA_RELEASE", "APPROVED"],
}


def xqa_document():
    params = dict(BUILTIN["regulated"])
    params["tenant_meanings"] = sorted(XQA_TENANT["tenant_meanings"], key=lambda m: utf16_key(m["code"]))
    params["required_meanings"] = list(XQA_TENANT["required_meanings"])
    return document("regulated", params)


def meaning(code, label="Site meaning", description="A meaning defined by the site."):
    return {"code": code, "label": label, "description": description}


# Raw text of each tenant file (some are deliberately not valid JSON or repeat a key).
TENANT_FILES = {
    "empty.json": b"{}\n",
    "two-meanings.json": pretty(
        {"tenant_meanings": [meaning("X_SUPPLIER_OK", "Supplier accepted"), meaning("X_CUSTOMER_OK", "Customer accepted")]}
    ),
    "two-meanings-reordered.json": pretty(
        {"tenant_meanings": [meaning("X_CUSTOMER_OK", "Customer accepted"), meaning("X_SUPPLIER_OK", "Supplier accepted")]}
    ),
    "bad-software-keys.json": pretty({"allow_software_keys": True}),
    "bad-release-signatures-off.json": pretty({"release_requires_signatures": False}),
    "bad-agent-writes.json": pretty({"agent_writes": "branch"}),
    "bad-program-release.json": pretty({"program_release_by_other": "agent_authored"}),
    "bad-name.json": pretty({"name": "lean"}),
    "bad-unknown-key.json": pretty({"colour": "red"}),
    "bad-no-approved.json": pretty({"required_meanings": ["AUTHORED", "REVIEWED"]}),
    "bad-empty-sequence.json": pretty({"required_meanings": []}),
    "bad-unknown-code.json": pretty({"required_meanings": ["AUTHORED", "X_UNDEFINED", "APPROVED"]}),
    "bad-lowercase-code.json": pretty({"required_meanings": ["authored", "APPROVED"]}),
    "bad-repeated-code.json": pretty({"required_meanings": ["AUTHORED", "APPROVED", "APPROVED"]}),
    "bad-sequence-too-long.json": pretty(
        {
            "tenant_meanings": [meaning("X_S%d" % i) for i in range(1, 8)],
            "required_meanings": ["X_S%d" % i for i in range(1, 8)] + ["AUTHORED", "APPROVED"],
        }
    ),
    "bad-tenant-code-syntax.json": pretty({"tenant_meanings": [meaning("x_lower")]}),
    "bad-tenant-code-builtin.json": pretty({"tenant_meanings": [meaning("APPROVED")]}),
    "bad-tenant-code-no-prefix.json": pretty({"tenant_meanings": [meaning("QA_RELEASE")]}),
    "bad-tenant-code-too-long.json": pretty({"tenant_meanings": [meaning("X_" + "A" * 31)]}),
    "bad-tenant-code-repeated.json": pretty({"tenant_meanings": [meaning("X_QA"), meaning("X_QA")]}),
    "bad-tenant-extra-member.json": pretty(
        {"tenant_meanings": [{"code": "X_QA", "label": "QA", "description": "QA.", "colour": "red"}]}
    ),
    "bad-tenant-missing-label.json": pretty({"tenant_meanings": [{"code": "X_QA", "description": "QA."}]}),
    "bad-tenant-empty-label.json": pretty({"tenant_meanings": [meaning("X_QA", "")]}),
    "bad-tenant-label-too-long.json": pretty({"tenant_meanings": [meaning("X_QA", "L" * 65)]}),
    "bad-tenant-label-control.json": pretty({"tenant_meanings": [meaning("X_QA", "QA\nrelease")]}),
    "bad-tenant-description-too-long.json": pretty({"tenant_meanings": [meaning("X_QA", "QA", "D" * 401)]}),
    "bad-tenant-too-many.json": pretty({"tenant_meanings": [meaning("X_M%02d" % i) for i in range(33)]}),
    "bad-tenant-not-array.json": pretty({"tenant_meanings": "X_QA"}),
    "bad-sequence-not-array.json": pretty({"required_meanings": "APPROVED"}),
    "bad-duplicate-key.json": b'{"tenant_meanings": [], "tenant_meanings": []}\n',
    "bad-duplicate-key-nested.json": (
        b'{"tenant_meanings": [{"code": "X_QA", "code": "X_QB", "label": "QA", "description": "QA."}]}\n'
    ),
    "bad-not-object.json": b"[]\n",
    "bad-not-json.json": b"{\"tenant_meanings\": [\n",
}

# Expected outcome per (file, profile): "ok" or the error code, hand-labelled from
# REQ-TRUST.PROFILE.DEFS-004/005 and the tenant schema. Order: forbidden keys are reported
# before any value is checked; unparseable input (including a repeated key) is malformed.
CASES = [
    ("../regulated-with-xqa.json", "regulated", "ok"),
    ("../regulated-with-xqa.json", "lean", "profile.param_forbidden"),
    ("../regulated-with-xqa.json", "assistive_only", "profile.param_forbidden"),
    ("empty.json", "lean", "ok"),
    ("empty.json", "regulated", "ok"),
    ("empty.json", "assistive_only", "ok"),
    ("two-meanings.json", "lean", "ok"),
    ("two-meanings.json", "assistive_only", "ok"),
    ("two-meanings-reordered.json", "lean", "ok"),
    ("bad-software-keys.json", "regulated", "profile.param_forbidden"),
    ("bad-software-keys.json", "lean", "profile.param_forbidden"),
    ("bad-release-signatures-off.json", "regulated", "profile.param_forbidden"),
    ("bad-agent-writes.json", "assistive_only", "profile.param_forbidden"),
    ("bad-program-release.json", "regulated", "profile.param_forbidden"),
    ("bad-name.json", "regulated", "profile.param_forbidden"),
    ("bad-unknown-key.json", "lean", "profile.param_forbidden"),
    ("bad-no-approved.json", "regulated", "profile.param_invalid"),
    ("bad-no-approved.json", "lean", "profile.param_forbidden"),
    ("bad-empty-sequence.json", "regulated", "profile.param_invalid"),
    ("bad-unknown-code.json", "regulated", "profile.param_invalid"),
    ("bad-lowercase-code.json", "regulated", "profile.param_invalid"),
    ("bad-repeated-code.json", "regulated", "profile.param_invalid"),
    ("bad-sequence-too-long.json", "regulated", "profile.param_invalid"),
    ("bad-tenant-code-syntax.json", "lean", "profile.param_invalid"),
    ("bad-tenant-code-builtin.json", "regulated", "profile.param_invalid"),
    ("bad-tenant-code-no-prefix.json", "lean", "profile.param_invalid"),
    ("bad-tenant-code-too-long.json", "lean", "profile.param_invalid"),
    ("bad-tenant-code-repeated.json", "lean", "profile.param_invalid"),
    ("bad-tenant-extra-member.json", "lean", "profile.param_invalid"),
    ("bad-tenant-missing-label.json", "lean", "profile.param_invalid"),
    ("bad-tenant-empty-label.json", "lean", "profile.param_invalid"),
    ("bad-tenant-label-too-long.json", "lean", "profile.param_invalid"),
    ("bad-tenant-label-control.json", "lean", "profile.param_invalid"),
    ("bad-tenant-description-too-long.json", "lean", "profile.param_invalid"),
    ("bad-tenant-too-many.json", "lean", "profile.param_invalid"),
    ("bad-tenant-not-array.json", "lean", "profile.param_invalid"),
    ("bad-sequence-not-array.json", "regulated", "profile.param_invalid"),
    ("bad-duplicate-key.json", "lean", "profile.malformed"),
    ("bad-duplicate-key-nested.json", "lean", "profile.malformed"),
    ("bad-not-object.json", "lean", "profile.malformed"),
    ("bad-not-json.json", "regulated", "profile.malformed"),
]


def outputs():
    files = {}
    digests = {}
    for name, params in BUILTIN.items():
        b = canonical(document(name, params))
        files["builtin/%s.json" % name] = b
        digests[name] = digest(b)
    files["regulated-with-xqa.json"] = pretty(XQA_TENANT)
    xqa = canonical(xqa_document())
    files["expected/regulated-with-xqa.json"] = xqa
    digests["regulated-with-xqa"] = digest(xqa)
    # Instantiating Lean or Assistive-only with two tenant meanings: the document lists
    # them sorted by code, so file order does not change the digest.
    for profile in ("lean", "assistive_only"):
        params = dict(BUILTIN[profile])
        tm = json.loads(TENANT_FILES["two-meanings.json"])["tenant_meanings"]
        params["tenant_meanings"] = sorted(tm, key=lambda m: utf16_key(m["code"]))
        b = canonical(document(profile, params))
        files["expected/%s-two-meanings.json" % profile] = b
        digests["%s-two-meanings" % profile] = digest(b)
    files["digests.json"] = pretty(digests)
    for fname, raw in TENANT_FILES.items():
        files["tenant/" + fname] = raw
    files["tenant/cases.json"] = pretty([{"file": f, "profile": p, "expect": e} for f, p, e in CASES])
    return files, digests


def sanity(files, digests):
    assert len(set(digests.values())) == len(digests), "digests must differ"
    for f, _, _ in CASES:
        assert ("tenant/" + f) in files or f == "../regulated-with-xqa.json", f
    used = {f for f, _, _ in CASES}
    for fname in TENANT_FILES:
        assert fname in used, "tenant file without a case: " + fname


def schema_check(files):
    try:
        from jsonschema import Draft202012Validator
        from referencing import Registry, Resource
    except ImportError:
        print("jsonschema not installed: schema validation skipped")
        return
    doc_schema = json.loads((ROOT / "schema/profile-v1.schema.json").read_text(encoding="utf-8"))
    tenant_schema = json.loads((ROOT / "schema/profile-tenant-v1.schema.json").read_text(encoding="utf-8"))
    Draft202012Validator.check_schema(doc_schema)
    Draft202012Validator.check_schema(tenant_schema)
    registry = Registry().with_resource(doc_schema["$id"], Resource.from_contents(doc_schema))
    doc_v = Draft202012Validator(doc_schema, registry=registry)
    tenant_v = Draft202012Validator(tenant_schema, registry=registry)
    for path, b in files.items():
        if path.startswith("builtin/") or path.startswith("expected/"):
            errs = list(doc_v.iter_errors(json.loads(b)))
            assert not errs, "%s: %s" % (path, errs[0].message)
    good = json.loads(files["regulated-with-xqa.json"])
    assert not list(tenant_v.iter_errors(good)), "regulated-with-xqa.json fails the tenant schema"
    for f, _, expect in CASES:
        raw = files["tenant/" + f] if not f.startswith("../") else files[f[3:]]
        try:
            v = json.loads(raw)
        except ValueError:
            continue
        if expect == "ok":
            errs = list(tenant_v.iter_errors(v))
            assert not errs, "%s: %s" % (f, errs[0].message)
    # Schema-detectable bad files are rejected by the schema too.
    for f in ("bad-software-keys.json", "bad-unknown-key.json", "bad-tenant-code-syntax.json",
              "bad-tenant-code-no-prefix.json", "bad-tenant-code-too-long.json", "bad-tenant-extra-member.json",
              "bad-tenant-missing-label.json", "bad-tenant-empty-label.json", "bad-tenant-label-too-long.json",
              "bad-tenant-label-control.json", "bad-tenant-description-too-long.json", "bad-tenant-too-many.json",
              "bad-repeated-code.json", "bad-sequence-too-long.json", "bad-empty-sequence.json",
              "bad-lowercase-code.json", "bad-not-object.json"):
        assert list(tenant_v.iter_errors(json.loads(files["tenant/" + f]))), "schema accepts " + f
    print("schema validation passed")


def main():
    check = "--check" in sys.argv[1:]
    files, digests = outputs()
    sanity(files, digests)
    schema_check(files)
    bad = []
    for rel, b in sorted(files.items()):
        p = HERE / rel
        if check:
            if not p.exists() or p.read_bytes() != b:
                bad.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(b)
    if check:
        expected = {(HERE / r).resolve() for r in files}
        # Only the folders this script owns; other leaves add their own under fixtures/profile/.
        owned = [p for d in ("builtin", "expected", "tenant") for p in (HERE / d).rglob("*.json")]
        for p in owned:
            if p.resolve() not in expected:
                bad.append("unexpected file " + str(p.relative_to(HERE)))
        if bad:
            print("out of date: " + ", ".join(bad))
            sys.exit(1)
        print("fixtures/profile: %d files up to date" % len(files))
    else:
        print("wrote %d files" % len(files))


if __name__ == "__main__":
    main()
