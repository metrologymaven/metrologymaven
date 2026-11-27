#!/usr/bin/env python3
"""Fixtures and expected results for spec node QUAL.VAP.PROTO (fid-xtask qual::vap::proto).

Writes (or, with --check, regenerates in memory and compares byte for byte):

  manifest.json           a reduced, synthetic Registry Manifest (format 1): only the fields
                          check-protocols reads (id, major) plus a title per command. It is not
                          a VAL-valid manifest and carries no definition digests.
  qual-record-1.json      a JSON Schema of the qualification record qual-record/1, written
                          from spec node QUAL.IQOQ.RECORD (Scope and Build prompt)
  <case>/*.json           protocol template sets derived from vap/templates/{iq,oq}-protocol.json
                          by one edit each (AC-2, AC-3, AC-4 and further error paths)
  expected.json           per case: template directory, exit code and stdout lines

The expected results come from an independent stdlib-only implementation of the check rules
written in the spec node and in the module doc of crates/fid-xtask/src/qual/vap/proto.rs; no
Rust code is run. The vap-template/1 format check itself is QUAL.VAP.TPL's and is not
re-implemented here: every case keeps the template valid under it, so its result is decided
only by the protocol rules. When `jsonschema` is importable (CI installs it), every protocol is
also validated against schemas/qual/vap-template-1.schema.json.

  python fixtures/qual/vap/proto/make_fixtures.py           # write
  python fixtures/qual/vap/proto/make_fixtures.py --check   # fail if stale
"""

from __future__ import annotations

import copy
import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
TEMPLATES = ROOT / "vap" / "templates"
TPL_SCHEMA = ROOT / "schemas" / "qual" / "vap-template-1.schema.json"
REL = "fixtures/qual/vap/proto"

PROTOCOLS = ["iq-protocol", "oq-protocol"]
REQUIRED = {
    "iq-protocol": ["validate.run_iq@1", "validate.export_bundle@1"],
    "oq-protocol": ["validate.run_oq@1"],
}
REGISTER_GOLDEN = "validate.register_golden@1"
STEP_COLUMNS = [
    ("step", "static"),
    ("purpose", "static"),
    ("command", "static"),
    ("record-field", "static"),
    ("acceptance", "static"),
    ("result", "customer"),
    ("signature", "customer"),
]
COMMAND_ID = re.compile(r"^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$")
SEGMENT = re.compile(r"^[a-z0-9_]+$")

# ------------------------------------------------------------------ inputs

MANIFEST = {
    "format": 1,
    "commands": [
        {"id": "validate.export_bundle", "major": 1, "title": "Export qualification bundle"},
        {"id": "validate.register_golden", "major": 1, "title": "Register golden journal"},
        {"id": "validate.run_iq", "major": 1, "title": "Run installation qualification"},
        {"id": "validate.run_oq", "major": 1, "title": "Run operational qualification"},
        {"id": "validate.show_record", "major": 1, "title": "Show qualification record or status"},
    ],
}

DIGEST = {"type": "string", "pattern": "^sha256:[0-9a-f]{64}$"}
VERDICT = {"type": "string", "enum": ["PASS", "FAIL"]}
RECORD_SCHEMA = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "qual-record/1",
    "title": "Qualification record (qual-record/1, spec node QUAL.IQOQ.RECORD)",
    "type": "object",
    "additionalProperties": False,
    "required": [
        "schema",
        "kind",
        "product_version",
        "kernel_manifest_digest",
        "evidence_statement_digest",
        "env_digest",
        "checks",
        "verdict",
    ],
    "properties": {
        "schema": {"const": "qual-record/1"},
        "kind": {"type": "string", "enum": ["iq", "oq"]},
        "product_version": {"type": "string", "minLength": 1},
        "kernel_manifest_digest": DIGEST,
        "evidence_statement_digest": DIGEST,
        "env_digest": DIGEST,
        "checks": {"type": "array", "items": {"$ref": "#/$defs/check"}},
        "verdict": VERDICT,
        "rdl_subset_digest": DIGEST,
        "goldens": {"type": "array", "items": {"$ref": "#/$defs/golden"}},
    },
    "$defs": {
        "check": {
            "type": "object",
            "additionalProperties": False,
            "required": ["id", "verdict", "detail"],
            "properties": {
                "id": {"type": "string", "minLength": 1},
                "verdict": VERDICT,
                "detail": {"type": "string"},
            },
        },
        "golden": {
            "type": "object",
            "additionalProperties": False,
            "required": ["golden_id", "outcome", "exceedances"],
            "properties": {
                "golden_id": {"type": "string", "minLength": 1},
                "outcome": {"type": "string", "enum": ["IDENTICAL", "EQUIVALENT", "DIVERGENT", "NOT_REPLAYABLE"]},
                "reasons": {"type": "array", "items": {"type": "string"}},
                "exceedances": {"type": "array", "items": {"$ref": "#/$defs/exceedance"}},
            },
        },
        "exceedance": {
            "type": "object",
            "additionalProperties": False,
            "required": ["characteristic", "delta", "decision_changed"],
            "properties": {
                "characteristic": {"type": "string", "minLength": 1},
                "delta": {"type": "string"},
                "decision_changed": {"type": "boolean"},
            },
        },
    },
}


def no_dup(pairs):
    out = {}
    for k, v in pairs:
        if k in out:
            raise SystemExit(f"duplicate key {k!r}")
        out[k] = v
    return out


def load_json(p: Path):
    return json.loads(p.read_text(encoding="utf-8"), object_pairs_hook=no_dup)


def dump_json(v) -> bytes:
    return (json.dumps(v, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


# ------------------------------------------------------------------ cases


def steps_rows(t: dict) -> list:
    for s in t["sections"]:
        for f in s["fields"]:
            if f["id"] == "steps":
                return f["rows"]
    raise SystemExit("no steps table")


def steps_field(t: dict) -> dict:
    for s in t["sections"]:
        for f in s["fields"]:
            if f["id"] == "steps":
                return f
    raise SystemExit("no steps table")


def row(t: dict, rid: str) -> dict:
    for r in steps_rows(t):
        if r["id"] == rid:
            return r
    raise SystemExit(f"no step {rid}")


def cases(iq: dict, oq: dict) -> dict:
    """Case name -> {stem: template}; each derived from the shipped protocols by one edit."""
    out = {}

    # AC-2: one IQ step names a major the manifest does not list.
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    row(a, "iq-signatures")["cells"]["command"] = "validate.run_iq@2"
    out["bad-command"] = {"iq-protocol": a, "oq-protocol": b}

    # AC-3: one IQ step names a field the record schema does not have.
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    row(a, "iq-artefacts")["cells"]["record-field"] = "checks.verdictt"
    out["bad-field"] = {"iq-protocol": a, "oq-protocol": b}

    # AC-4: the OQ protocol without its validate.run_oq@1 steps (the golden registration
    # step stays, so only run_oq is missing).
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    f = steps_field(b)
    f["rows"] = [r for r in f["rows"] if r["cells"]["command"] != "validate.run_oq@1"]
    out["no-run-oq"] = {"iq-protocol": a, "oq-protocol": b}

    # REQ-003: OQ names goldens fields but has no registration step.
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    f = steps_field(b)
    f["rows"] = [r for r in f["rows"] if r["cells"]["command"] != REGISTER_GOLDEN]
    out["goldens-no-register"] = {"iq-protocol": a, "oq-protocol": b}

    # REQ-003: IQ without the bundle export step.
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    f = steps_field(a)
    f["rows"] = [r for r in f["rows"] if r["cells"]["command"] != "validate.export_bundle@1"]
    out["no-export"] = {"iq-protocol": a, "oq-protocol": b}

    # Step table shape: the signature column removed (customer column, so no cells change).
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    f = steps_field(b)
    f["columns"] = [c for c in f["columns"] if c["id"] != "signature"]
    out["bad-columns"] = {"iq-protocol": a, "oq-protocol": b}

    # A step cell that differs from the row ID.
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    row(a, "iq-version")["cells"]["step"] = "iq-versions"
    out["step-id-mismatch"] = {"iq-protocol": a, "oq-protocol": b}

    # A command that is not a command ID and a field that is not a path, in one step.
    a, b = copy.deepcopy(iq), copy.deepcopy(oq)
    c = row(b, "oq-version")["cells"]
    c["command"] = "validate.run_oq"
    c["record-field"] = "Product version"
    out["malformed-step"] = {"iq-protocol": a, "oq-protocol": b}

    # The OQ protocol absent.
    out["missing-oq"] = {"iq-protocol": copy.deepcopy(iq)}
    return out


# ------------------------------------------------------------------ the check (independent)


def manifest_ids(m: dict) -> set:
    return {f"{c['id']}@{c['major']}" for c in m["commands"]}


def deref(schema: dict, node: dict):
    for _ in range(16):
        ref = node.get("$ref")
        if ref is None:
            return node
        if not isinstance(ref, str) or not ref.startswith("#/$defs/"):
            return None
        node = schema.get("$defs", {}).get(ref[len("#/$defs/"):])
        if not isinstance(node, dict):
            return None
    return None


def is_path(s: str) -> bool:
    segs = s.split(".")
    return len(segs) <= 16 and all(SEGMENT.match(g) for g in segs)


def resolves(schema: dict, field: str) -> bool:
    if not is_path(field):
        return False
    node = schema
    for seg in field.split("."):
        cur = deref(schema, node)
        while cur is not None and cur.get("type") == "array":
            items = cur.get("items")
            cur = deref(schema, items) if isinstance(items, dict) else None
        if cur is None:
            return False
        props = cur.get("properties")
        if not isinstance(props, dict) or not isinstance(props.get(seg), dict):
            return False
        node = props[seg]
    return True


def debug_str(s: str) -> str:
    # Rust's {:?} of a plain printable string: double quotes, with \ and " escaped.
    return '"' + s.replace("\\", "\\\\").replace('"', '\\"') + '"'


def check(templates: dict, manifest: set, schema: dict) -> tuple[int, list]:
    lines = []
    count_protocols = 0
    count_steps = 0
    for pid in PROTOCOLS:
        t = templates.get(pid)
        if t is None:
            lines.append(f"PROTO-TEMPLATE {pid}: {pid}.json is missing")
            continue
        tables = [f for s in t["sections"] for f in s["fields"] if f["id"] == "steps"]
        if len(tables) != 1:
            lines.append(f"PROTO-TEMPLATE {pid}: has no field \"steps\"" if not tables else
                         f"PROTO-TEMPLATE {pid}: has more than one field \"steps\"")
            continue
        f = tables[0]
        if f["kind"] != "table":
            lines.append(f"PROTO-TEMPLATE {pid}: field \"steps\" is not a table")
            continue
        if [(c["id"], c["fill"]) for c in f["columns"]] != STEP_COLUMNS:
            want = ", ".join(f"{i} ({fill})" for i, fill in STEP_COLUMNS)
            lines.append(f"PROTO-TEMPLATE {pid}: step table columns must be, in order: {want}")
            continue
        if not f["rows"]:
            lines.append(f"PROTO-TEMPLATE {pid}: step table has no steps")
            continue
        bad = None
        for r in f["rows"]:
            if r["cells"]["step"] != r["id"]:
                bad = f"PROTO-TEMPLATE {pid}: step {r['id']}: the step cell {debug_str(r['cells']['step'])} differs from the row ID"
                break
        if bad:
            lines.append(bad)
            continue
        named = set()
        goldens = False
        for r in f["rows"]:
            c = r["cells"]
            subj = f"{pid}/{r['id']}"
            cmd, fld = c["command"], c["record-field"]
            if not COMMAND_ID.match(cmd):
                lines.append(f"PROTO-COMMAND {subj}: {debug_str(cmd)} is not a command ID (name@major)")
            elif cmd not in manifest:
                lines.append(f"PROTO-COMMAND {subj}: {cmd} is not in the registry manifest")
            named.add(cmd)
            if not is_path(fld):
                lines.append(f"PROTO-FIELD {subj}: {debug_str(fld)} is not a record field path")
            elif not resolves(schema, fld):
                lines.append(f"PROTO-FIELD {subj}: {fld} is not a field of qual-record/1")
            if fld.split(".")[0] == "goldens":
                goldens = True
        req = list(REQUIRED[pid])
        if pid == "oq-protocol" and goldens:
            req.append(REGISTER_GOLDEN)
        for cmd in req:
            if cmd not in named:
                if cmd == REGISTER_GOLDEN:
                    lines.append(f"PROTO-REQUIRED {cmd}: {pid} uses goldens but has no step that runs it")
                else:
                    lines.append(f"PROTO-REQUIRED {cmd}: {pid} has no step that runs it")
        count_protocols += 1
        count_steps += len(f["rows"])
    if lines:
        return 1, lines + [f"check-protocols: {len(lines)} findings"]
    return 0, [f"check-protocols: ok, {count_protocols} protocols, {count_steps} steps"]


# ------------------------------------------------------------------ main


def build() -> dict[str, bytes]:
    iq = load_json(TEMPLATES / "iq-protocol.json")
    oq = load_json(TEMPLATES / "oq-protocol.json")
    files: dict[str, bytes] = {
        "manifest.json": dump_json(MANIFEST),
        "qual-record-1.json": dump_json(RECORD_SCHEMA),
    }
    ids = manifest_ids(MANIFEST)
    expected = {}
    code, out = check({"iq-protocol": iq, "oq-protocol": oq}, ids, RECORD_SCHEMA)
    if code != 0:
        raise SystemExit("the shipped protocols fail the check:\n" + "\n".join(out))
    expected["shipped"] = {"dir": "vap/templates", "exit": code, "stdout": out}
    for name, templates in sorted(cases(iq, oq).items()):
        for stem, t in sorted(templates.items()):
            files[f"{name}/{stem}.json"] = dump_json(t)
        code, out = check(templates, ids, RECORD_SCHEMA)
        expected[name] = {"dir": f"{REL}/{name}", "exit": code, "stdout": out}
    files["expected.json"] = dump_json(expected)
    return files


def schema_cross_check(files: dict[str, bytes]) -> None:
    try:
        import jsonschema  # type: ignore
    except ImportError:
        return
    schema = load_json(TPL_SCHEMA)
    for rel, data in sorted(files.items()):
        if rel.endswith("-protocol.json"):
            jsonschema.validate(json.loads(data), schema)
    for stem in PROTOCOLS:
        jsonschema.validate(load_json(TEMPLATES / f"{stem}.json"), schema)
    jsonschema.Draft202012Validator.check_schema(RECORD_SCHEMA)


def main(argv: list[str]) -> int:
    check_mode = "--check" in argv[1:]
    if [a for a in argv[1:] if a != "--check"]:
        sys.stderr.write(f"usage: {argv[0]} [--check]\n")
        return 2
    files = build()
    schema_cross_check(files)
    stale = []
    for rel, data in sorted(files.items()):
        p = HERE / rel
        if check_mode:
            if not p.exists() or p.read_bytes() != data:
                stale.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    if check_mode:
        for p in sorted(HERE.rglob("*.json")):
            rel = p.relative_to(HERE).as_posix()
            if rel not in files:
                stale.append(f"{rel} (unexpected)")
        if stale:
            sys.stderr.write("stale fixtures:\n" + "\n".join(stale) + "\n")
            return 1
        print(f"make_fixtures: {len(files)} fixtures up to date")
    else:
        print(f"make_fixtures: wrote {len(files)} fixtures")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
