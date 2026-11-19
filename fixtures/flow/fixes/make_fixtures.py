"""Writes the FLOW.DEADEND.CATALOG fixtures and is the independent reference for their digest.

The catalogues below are hand-written from the spec's rules (REQ-FLOW.DEADEND.CATALOG-001..005)
and use only reason codes and command IDs that the spec tree defines (UNC.UEC.EXPL,
UNC.DEC.DEFAULT, UNC.DEC.BIND, GDT.CHAR.RECORD, UNC.MODEL.BIND, UNC.INST.SCHEMA,
UNC.INST.LIB.FORMAT, UNC.PROP.SELECT, UNC.UEC.DECL). This script computes the expected digest
of `catalogue-valid.json` itself: entries sorted by reason_code, `source_table` omitted when
absent, then SHA-256 of the RFC 8785 (JCS) form. With ASCII keys and string-only values,
`json.dumps` with sorted keys and no whitespace is the JCS form. The expected loader errors of
the error fixtures are labelled by hand and re-derived here by an independent reading of the
rules, never by running the Rust loader.

It also checks the shipped catalogue `crates/fid-fixes/fix-catalogue.json` against the
hand-labelled table of REQ-FLOW.DEADEND.CATALOG-004 and, when the `jsonschema` package is
installed (it is in CI), validates the valid fixture and the shipped catalogue against the
generated `schema/fix-catalogue-v1.json`.

Run from the repository root:  python fixtures/flow/fixes/make_fixtures.py [--check]
With --check, nothing is written; the script fails if a committed file differs.
Standard library only (jsonschema optional).
"""

import copy
import hashlib
import json
import pathlib
import re
import sys

OUT = pathlib.Path(__file__).resolve().parent
ROOT = OUT.parents[2]
CHECK = "--check" in sys.argv[1:]
PROBLEMS = []

PLACEHOLDERS = {"entity_id", "characteristic_id", "project_id", "dialect", "reason_detail"}
UEC = "uec-fix@1"


def fix(command, label, **args):
    return {"command": command, "args": args, "label_key": label}


# Hand-written, deliberately not in reason_code order, so sorting is exercised.
VALID = {
    "version": "fix-catalogue@1",
    "entries": [
        {
            "reason_code": "unc.u_missing",
            "state_kind": "uec0",
            "owner": "UNC.UEC.EXPL",
            "source_table": UEC,
            "fixes": [
                fix("unc.propagate_auto@1", "fix.unc.u_missing.propagate_auto", result_id="${entity_id}"),
                fix("unc.declare_uncertainty@1", "fix.unc.u_missing.declare_uncertainty",
                    char_id="${characteristic_id}"),
            ],
        },
        {
            "reason_code": "gdt.no_rule_bound",
            "state_kind": "not_decidable",
            "owner": "GDT.CHAR.RECORD",
            "fixes": [
                fix("decide.bind_rule@1", "fix.gdt.no_rule_bound.bind_rule", chars=["${characteristic_id}"]),
                fix("decide.set_default_rule@1", "fix.gdt.no_rule_bound.set_default_rule"),
            ],
        },
        {
            "reason_code": "unc.instrument_missing",
            "state_kind": "uec0",
            "owner": "UNC.UEC.EXPL",
            "source_table": UEC,
            "fixes": [
                fix("unc.bind_instrument_model@1", "fix.unc.instrument_missing.bind_instrument_model",
                    target="project:${project_id}"),
                fix("unc.list_profile@1", "fix.unc.instrument_missing.list_profile"),
            ],
        },
        {
            "reason_code": "decide.rule_missing",
            "state_kind": "not_decidable",
            "owner": "UNC.DEC.DEFAULT",
            "fixes": [
                fix("decide.bind_rule@1", "fix.decide.rule_missing.bind_rule", chars=["${characteristic_id}"]),
                fix("decide.set_default_rule@1", "fix.decide.rule_missing.set_default_rule"),
            ],
        },
        {
            "reason_code": "unc.model_missing",
            "state_kind": "uec0",
            "owner": "UNC.UEC.EXPL",
            "source_table": UEC,
            "fixes": [fix("unc.bind_model@1", "fix.unc.model_missing.bind_model", chars=["${characteristic_id}"])],
        },
        {
            "reason_code": "gdt.unc_no_decision",
            "state_kind": "not_decidable",
            "owner": "GDT.CHAR.RECORD",
            "fixes": [fix("unc.propagate_auto@1", "fix.gdt.unc_no_decision.propagate_auto", result_id="${entity_id}")],
        },
    ],
}

# Two decide.rule_missing entries: expected FIX-DUP decide.rule_missing.
DUP = copy.deepcopy(VALID)
DUP["entries"].append(copy.deepcopy(VALID["entries"][3]))

# A placeholder outside the closed set: expected FIX-PLACEHOLDER.
BADPH = copy.deepcopy(VALID)
BADPH["entries"][3]["fixes"][0]["args"]["chars"] = ["${path}"]

# Hand-labelled from REQ-FLOW.DEADEND.CATALOG-004 and the leaf's build prompt.
REQ004 = {
    "decide.rule_missing": ["decide.bind_rule@1", "decide.set_default_rule@1"],
    "gdt.no_rule_bound": ["decide.bind_rule@1", "decide.set_default_rule@1"],
    "gdt.unc_no_decision": ["unc.propagate_auto@1"],
    "unc.instrument_missing": ["unc.bind_instrument_model@1", "unc.list_profile@1"],
    "unc.model_missing": ["unc.bind_model@1"],
    "unc.u_missing": ["unc.propagate_auto@1", "unc.declare_uncertainty@1"],
}


# Hand-labelled parameter types from the commands' Design sections: "list" for Vec<...>,
# "scalar" otherwise. Every argument the shipped catalogue pre-fills must be listed here with
# the matching JSON shape (array for list, string for scalar).
PARAM_TYPES = {
    ("decide.bind_rule@1", "chars"): "list",  # UNC.DEC.BIND: chars: Vec<EntityId> | plan_default
    ("unc.bind_model@1", "chars"): "list",  # UNC.MODEL.BIND: chars: Vec<EntityId>
    ("unc.bind_instrument_model@1", "target"): "scalar",  # UNC.INST.SCHEMA: target: DeviceId | Project
    ("unc.declare_uncertainty@1", "char_id"): "scalar",  # UNC.UEC.DECL: char_id: EntityId
    ("unc.propagate_auto@1", "result_id"): "scalar",  # UNC.PROP.SELECT: result_id
}


def type_problems(doc):
    out = []
    for e in doc["entries"]:
        for f in e["fixes"]:
            for name, v in f["args"].items():
                want = PARAM_TYPES.get((f["command"], name))
                got = "list" if isinstance(v, list) else "scalar"
                if want != got:
                    out.append("%s %s %s: want %s, got %s" % (e["reason_code"], f["command"], name, want, got))
    return out


def reference_error(doc):
    """Independent reading of the placeholder and duplicate rules; returns the expected error."""
    for e in doc["entries"]:
        for f in e["fixes"]:
            for v in f["args"].values():
                for t in v if isinstance(v, list) else [v]:
                    for name in re.findall(r"\$\{([^}]*)\}", t):
                        if name not in PLACEHOLDERS:
                            return "FIX-PLACEHOLDER"
    codes = sorted(e["reason_code"] for e in doc["entries"])
    for a, b in zip(codes, codes[1:]):
        if a == b:
            return "FIX-DUP " + a
    return None


def normalised(doc):
    return {"version": doc["version"], "entries": sorted(doc["entries"], key=lambda e: e["reason_code"].encode())}


def jcs(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def pretty(doc):
    return (json.dumps(doc, indent=2, ensure_ascii=False) + "\n").encode("utf-8")


def emit(rel, data):
    path = OUT / rel
    if CHECK:
        if not path.exists() or path.read_bytes() != data:
            PROBLEMS.append(rel)
        return
    path.write_bytes(data)


def main():
    assert reference_error(VALID) is None
    assert reference_error(DUP) == "FIX-DUP decide.rule_missing"
    assert reference_error(BADPH) == "FIX-PLACEHOLDER"
    assert len(VALID["entries"]) == 6

    emit("catalogue-valid.json", pretty(VALID))
    emit("catalogue-dup.json", pretty(DUP))
    emit("catalogue-badph.json", pretty(BADPH))
    digest = "sha256:" + hashlib.sha256(jcs(normalised(VALID))).hexdigest()
    emit("catalogue-valid.digest", (digest + "\n").encode("ascii"))

    shipped = json.loads((ROOT / "crates/fid-fixes/fix-catalogue.json").read_text(encoding="utf-8"))
    got = {e["reason_code"]: [f["command"] for f in e["fixes"]] for e in shipped["entries"]}
    if got != REQ004:
        PROBLEMS.append("crates/fid-fixes/fix-catalogue.json does not match REQ-004: %r" % got)
    PROBLEMS.extend(type_problems(shipped))
    PROBLEMS.extend(type_problems(VALID))
    if reference_error(shipped) is not None:
        PROBLEMS.append("shipped catalogue fails the reference rules: " + reference_error(shipped))

    try:
        import jsonschema
    except ImportError:
        print("jsonschema not installed; schema validation skipped")
    else:
        schema = json.loads((ROOT / "schema/fix-catalogue-v1.json").read_text(encoding="utf-8"))
        for name, doc in (("catalogue-valid.json", VALID), ("fix-catalogue.json", shipped)):
            errors = list(jsonschema.Draft202012Validator(schema).iter_errors(doc))
            if errors:
                PROBLEMS.append("%s fails the schema: %s" % (name, errors[0].message))

    if PROBLEMS:
        for p in PROBLEMS:
            print("problem:", p)
        sys.exit(1)
    print("ok" if CHECK else "written")


if __name__ == "__main__":
    main()
