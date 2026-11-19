#!/usr/bin/env python3
"""Generate the fixtures of spec node CORE.REG.VER.COMPAT (AC-1 .. AC-5).

Hand-labelled: every case below states, by hand and from the leaf's rule table
(REQ-001 / REQ-002 / REQ-005), which JSON pointers change and with which reason and
kind. This script never runs the Rust classifier. It only adds what cannot be written by
hand: the SHA-256 digest of the RFC 8785 form of the old and new value at each pointer,
computed here with Python's own json + hashlib (every fixture number is an integer and
every key and string is ASCII, so sorted-key compact JSON is exactly RFC 8785).

Outputs (CC-BY-4.0 project data):
  pairs/NNN-<name>.json           one old/new definition pair each, with its `expect` list
  ../../manifest/v1.json, v2.json the five-change manifest pair (CORE.REG.QRY AC-3)
  ../../manifest/v1_v2.diff.json  the expected diff of v1 -> v2

Run:    python fixtures/core/reg/ver/compat/make_fixtures.py
Check:  python fixtures/core/reg/ver/compat/make_fixtures.py --check
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
PAIRS = HERE / "pairs"
MANIFEST = HERE.parent.parent / "manifest"

# The leaf's rule table: reason -> kind. Additive only per REQ-001; all else breaking.
KIND = {
    "doc_changed": "additive",
    "ui_hint_changed": "additive",
    "web_changed": "additive",
    "cli_bare_changed": "additive",
    "optional_param_added": "additive",
    "result_field_added": "additive",
}
BREAKING = [
    "identity_changed", "param_removed", "result_field_removed", "required_param_added",
    "optional_param_without_default", "param_added_to_open_object", "param_made_required",
    "required_changed", "type_changed", "quantity_changed", "unit_changed", "ref_changed",
    "enum_narrowed", "enum_widened", "enum_changed", "range_narrowed", "range_widened",
    "default_changed", "constraint_changed", "precondition_changed", "side_effect_changed",
    "mcp_projection_changed", "determinism_changed", "idempotency_changed", "fs_effect_changed",
    "undo_changed", "network_changed", "human_only_changed", "effects_changed",
    "algorithms_changed", "exposure_changed", "unclassified_construct",
]
for r in BREAKING:
    KIND[r] = "breaking"

# Field order of a definition's JSON form (all fields written out; web_note only if set).
FIELDS = ["id", "major", "title", "description", "params_schema", "result_schema", "side_effect",
          "mcp_projection", "determinism", "idempotency", "fs_effect", "undo", "preconditions",
          "effects", "algorithms", "exposure", "lifecycle", "human_only", "network", "web",
          "web_note", "cli_bare"]
SURFACES = ["cli", "mcp", "python", "gui", "control"]


def jcs(v):
    def walk(x):
        assert not isinstance(x, float), "fixtures use integers only"
        if isinstance(x, dict):
            for k, y in x.items():
                assert k.isascii()
                walk(y)
        elif isinstance(x, list):
            for y in x:
                walk(y)
        elif isinstance(x, str):
            assert x.isascii() and x.isprintable()
    walk(v)
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=True).encode("ascii")


def sha(v):
    return "sha256:" + hashlib.sha256(jcs(v)).hexdigest()


def at(doc, pointer):
    """RFC 6901 lookup; returns None when absent."""
    if pointer == "":
        return doc
    cur = doc
    for raw in pointer.split("/")[1:]:
        tok = raw.replace("~1", "/").replace("~0", "~")
        if isinstance(cur, dict) and tok in cur:
            cur = cur[tok]
        else:
            return None
    return cur


def digest_at(doc, pointer):
    v = at(doc, pointer)
    return None if v is None else sha(v)


def check_def(d):
    keys = [k for k in FIELDS if k in d]
    assert set(d) == set(keys), sorted(set(d) - set(FIELDS))
    missing = [k for k in FIELDS if k not in d and k != "web_note"]
    assert not missing, missing
    assert d["exposure"] == [s for s in SURFACES if s in d["exposure"]], "exposure order"


def base():
    return {
        "id": "demo.measure",
        "major": 1,
        "title": "Measure a feature",
        "description": "Measures one feature against a tolerance.",
        "params_schema": {
            "type": "object",
            "additionalProperties": False,
            "required": ["feature", "tol"],
            "properties": {
                "feature": {"type": "string", "x-ref": "feature", "description": "Feature to measure."},
                "tol": {"type": "number", "x-quantity": "length", "x-unit": "mm", "minimum": 0, "maximum": 10},
                "mode": {"type": "string", "enum": ["fast", "precise"], "default": "fast"},
                "samples": {"type": "integer", "minimum": 1, "maximum": 1000, "default": 25,
                            "x-ui": {"widget": "slider"}},
                "points": {
                    "type": "array",
                    "maxItems": 100,
                    "items": {
                        "type": "object",
                        "additionalProperties": False,
                        "required": ["x"],
                        "properties": {"x": {"type": "number", "x-quantity": "length", "x-unit": "mm"}},
                    },
                },
                "label": {"type": "string", "maxLength": 64, "pattern": "^[a-z]+$"},
            },
        },
        "result_schema": {
            "type": "object",
            "additionalProperties": False,
            "required": ["deviation", "status"],
            "properties": {
                "deviation": {"type": "number", "x-quantity": "length", "x-unit": "mm"},
                "status": {"type": "string", "enum": ["ok", "fail"]},
                "note": {"type": "string"},
            },
        },
        "side_effect": "R0",
        "mcp_projection": "exposed",
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "head_move",
        "preconditions": [{"expr": "params.tol >= 0.0", "bound": 100, "message_key": "demo.tol_negative"}],
        "effects": [],
        "algorithms": ["geo.fit.lsq@1.0.0"],
        "exposure": ["cli", "mcp"],
        "lifecycle": {"state": "active", "since": "2026-01-01"},
        "human_only": False,
        "network": "none",
        "web": "partial",
        "web_note": "No probe access in the browser.",
        "cli_bare": False,
    }


P = "/params_schema/properties"
R = "/result_schema/properties"
CASES = []


def case(name, rule, mutate, expect, old_mutate=None):
    """`mutate` edits a copy of the (possibly pre-edited) old definition into the new one.
    `expect` is the hand-labelled list of (pointer, reason)."""
    CASES.append((name, rule, mutate, expect, old_mutate))


def setp(path, value):
    def f(d):
        cur = d
        for k in path[:-1]:
            cur = cur[k]
        cur[path[-1]] = value
    return f


def delp(path):
    def f(d):
        cur = d
        for k in path[:-1]:
            cur = cur[k]
        del cur[path[-1]]
    return f


def both(*fs):
    def f(d):
        for g in fs:
            g(d)
    return f


PS = ["params_schema", "properties"]
RS = ["result_schema", "properties"]

# ---- top-level fields (REQ-001 additive list; REQ-002 policy fields)
case("title", "title is documentation", setp(["title"], "Measure one feature"), [("/title", "doc_changed")])
case("description", "description is documentation", setp(["description"], "New text."),
     [("/description", "doc_changed")])
case("web", "web is additive", setp(["web"], "unavailable"), [("/web", "web_changed")])
case("web_note", "web_note is additive", setp(["web_note"], "Needs a device."), [("/web_note", "web_changed")])
case("web_note_removed", "web and web_note together",
     both(setp(["web"], "available"), delp(["web_note"])),
     [("/web", "web_changed"), ("/web_note", "web_changed")])
case("cli_bare", "cli_bare is additive", setp(["cli_bare"], True), [("/cli_bare", "cli_bare_changed")])
case("side_effect", "side_effect is breaking", setp(["side_effect"], "R1"), [("/side_effect", "side_effect_changed")])
case("mcp_projection", "mcp_projection is breaking", setp(["mcp_projection"], "request_only"),
     [("/mcp_projection", "mcp_projection_changed")])
case("determinism", "determinism is breaking", setp(["determinism"], "D1"), [("/determinism", "determinism_changed")])
case("idempotency", "idempotency is breaking", setp(["idempotency"], "keyed"), [("/idempotency", "idempotency_changed")])
case("fs_effect", "fs_effect is breaking", setp(["fs_effect"], "read_root"), [("/fs_effect", "fs_effect_changed")])
case("undo", "undo is breaking", setp(["undo"], "not_undoable"), [("/undo", "undo_changed")])
case("network", "network is breaking", setp(["network"], "egress"), [("/network", "network_changed")])
case("human_only_set", "human_only change is breaking", setp(["human_only"], True),
     [("/human_only", "human_only_changed")])
case("human_only_cleared", "human_only change is breaking (either way)", setp(["human_only"], False),
     [("/human_only", "human_only_changed")], old_mutate=setp(["human_only"], True))
case("precondition_added", "precondition is breaking",
     setp(["preconditions"], base()["preconditions"] + [
         {"expr": "params.tol <= 10.0", "bound": 100, "message_key": "demo.tol_large"}]),
     [("/preconditions", "precondition_changed")])
case("precondition_expr", "precondition is breaking",
     setp(["preconditions"], [{"expr": "params.tol > 0.0", "bound": 100, "message_key": "demo.tol_negative"}]),
     [("/preconditions", "precondition_changed")])
case("error_code", "a precondition's message key is its error code: breaking",
     setp(["preconditions"], [{"expr": "params.tol >= 0.0", "bound": 100, "message_key": "demo.bad_tol"}]),
     [("/preconditions", "precondition_changed")])
case("effects", "no additive rule: breaking", setp(["effects"], ["demo.measure@1"]), [("/effects", "effects_changed")])
case("algorithms", "no additive rule: breaking", setp(["algorithms"], ["geo.fit.lsq@1.1.0"]),
     [("/algorithms", "algorithms_changed")])
case("exposure", "no additive rule: breaking", setp(["exposure"], ["cli", "mcp", "python"]),
     [("/exposure", "exposure_changed")])
case("major", "a different (ID, major) is breaking", setp(["major"], 2), [("/major", "identity_changed")])
case("lifecycle_only", "lifecycle is reported by the diff engine, not the classifier",
     setp(["lifecycle"], {"state": "deprecated", "since": "2026-06-01"}), [])
case("identical", "no change", lambda d: None, [])

# ---- parameters
case("param_optional_default", "new optional parameter with default: additive",
     setp(PS + ["retries"], {"type": "integer", "default": 0}), [(P + "/retries", "optional_param_added")])
case("param_optional_no_default", "optional parameter without default: breaking",
     setp(PS + ["retries"], {"type": "integer"}), [(P + "/retries", "optional_param_without_default")])
case("param_required_added", "new required parameter: breaking",
     both(setp(PS + ["frame"], {"type": "string", "default": "world"}),
          setp(["params_schema", "required"], ["feature", "frame", "tol"])),
     [(P + "/frame", "required_param_added")])
case("param_open_object", "parameter added to an open object: breaking",
     setp(PS + ["retries"], {"type": "integer", "default": 0}),
     [(P + "/retries", "param_added_to_open_object")],
     old_mutate=delp(["params_schema", "additionalProperties"]))
case("param_removed", "removed parameter: breaking", delp(PS + ["label"]), [(P + "/label", "param_removed")])
case("param_required_removed", "removed required parameter: breaking",
     both(delp(PS + ["tol"]), setp(["params_schema", "required"], ["feature"])),
     [(P + "/tol", "param_removed")])
case("param_renamed", "renamed parameter: removed plus added",
     both(delp(PS + ["label"]), setp(PS + ["name"], {"type": "string", "maxLength": 64, "pattern": "^[a-z]+$"})),
     [(P + "/label", "param_removed"), (P + "/name", "optional_param_without_default")])
case("param_type", "changed JSON type: breaking", setp(PS + ["tol", "type"], "string"),
     [(P + "/tol/type", "type_changed")])
case("param_quantity", "changed x-quantity: breaking", setp(PS + ["tol", "x-quantity"], "angle"),
     [(P + "/tol/x-quantity", "quantity_changed")])
case("param_unit", "changed x-unit: breaking", setp(PS + ["tol", "x-unit"], "um"),
     [(P + "/tol/x-unit", "unit_changed")])
case("param_ref", "changed x-ref: breaking", setp(PS + ["feature", "x-ref"], "datum"),
     [(P + "/feature/x-ref", "ref_changed")])
case("enum_narrowed", "narrowed enum: breaking", setp(PS + ["mode", "enum"], ["fast"]),
     [(P + "/mode/enum", "enum_narrowed")])
case("enum_widened_param", "widened enum: not in the additive list, breaking",
     setp(PS + ["mode", "enum"], ["fast", "precise", "auto"]), [(P + "/mode/enum", "enum_widened")])
case("enum_replaced", "enum value replaced: breaking", setp(PS + ["mode", "enum"], ["fast", "exact"]),
     [(P + "/mode/enum", "enum_changed")])
case("enum_reordered", "enums compare as sets: no change", setp(PS + ["mode", "enum"], ["precise", "fast"]), [])
case("enum_added", "an enum where there was none narrows", setp(PS + ["label", "enum"], ["a", "b"]),
     [(P + "/label/enum", "enum_narrowed")])
case("range_max_narrowed", "lower maximum narrows: breaking", setp(PS + ["tol", "maximum"], 5),
     [(P + "/tol/maximum", "range_narrowed")])
case("range_min_widened", "lower minimum widens: still breaking", setp(PS + ["samples", "minimum"], 0),
     [(P + "/samples/minimum", "range_widened")])
case("range_min_narrowed", "higher minimum narrows: breaking", setp(PS + ["samples", "minimum"], 5),
     [(P + "/samples/minimum", "range_narrowed")])
case("range_bound_added", "a new bound narrows", setp(PS + ["label", "minLength"], 1),
     [(P + "/label/minLength", "range_narrowed")])
case("range_bound_removed", "a dropped bound widens", delp(PS + ["points", "maxItems"]),
     [(P + "/points/maxItems", "range_widened")])
case("range_not_number", "a bound that is not a number has no rule", setp(PS + ["tol", "maximum"], "10"),
     [(P + "/tol/maximum", "unclassified_construct")])
case("default_changed", "changed default: breaking", setp(PS + ["mode", "default"], "precise"),
     [(P + "/mode/default", "default_changed")])
case("default_removed", "dropped default: breaking", delp(PS + ["samples", "default"]),
     [(P + "/samples/default", "default_changed")])
case("param_made_required", "optional parameter made required: breaking",
     setp(["params_schema", "required"], ["feature", "mode", "tol"]),
     [("/params_schema/required", "param_made_required")])
case("param_made_optional", "required parameter made optional: not in the additive list",
     setp(["params_schema", "required"], ["feature"]), [("/params_schema/required", "required_changed")])
case("required_reordered", "required compares as a set: no change",
     setp(["params_schema", "required"], ["tol", "feature"]), [])
case("pattern", "changed pattern: breaking", setp(PS + ["label", "pattern"], "^[a-z0-9]+$"),
     [(P + "/label/pattern", "constraint_changed")])
case("additional_properties", "opening the object: breaking", delp(["params_schema", "additionalProperties"]),
     [("/params_schema/additionalProperties", "constraint_changed")])
case("x_ui", "x-ui is a presentation hint: additive", setp(PS + ["samples", "x-ui"], {"widget": "spin"}),
     [(P + "/samples/x-ui", "ui_hint_changed")])
case("param_description", "schema description is documentation: additive",
     setp(PS + ["feature", "description"], "The feature."), [(P + "/feature/description", "doc_changed")])
case("nested_optional_default", "optional field with default in a nested closed object: additive",
     setp(PS + ["points", "items", "properties", "y"], {"type": "number", "default": 0}),
     [(P + "/points/items/properties/y", "optional_param_added")])
case("nested_type", "nested type change: breaking", setp(PS + ["points", "items", "properties", "x", "type"], "integer"),
     [(P + "/points/items/properties/x/type", "type_changed")])
case("items_removed", "dropping items: breaking", delp(PS + ["points", "items"]),
     [(P + "/points/items", "constraint_changed")])
case("bool_schema", "boolean schema has no rule", setp(PS + ["label"], True),
     [(P + "/label", "unclassified_construct")])
case("escaped_name", "pointer tokens are RFC 6901 escaped",
     setp(PS + ["a/b~c"], {"type": "string", "default": "x"}), [(P + "/a~1b~0c", "optional_param_added")])

COND = {"if": {"properties": {"mode": {"const": "precise"}}},
        "then": {"required": ["samples"]},
        "else": {}}


def add_cond(d):
    d["params_schema"].update(copy.deepcopy(COND))


case("if_then_else_changed", "AC-4: changed if/then/else is unclassified",
     setp(["params_schema", "then"], {"required": ["samples", "label"]}),
     [("/params_schema/then", "unclassified_construct")], old_mutate=add_cond)
case("if_then_else_added", "AC-4: added if/then/else is unclassified", add_cond,
     [("/params_schema/else", "unclassified_construct"), ("/params_schema/if", "unclassified_construct"),
      ("/params_schema/then", "unclassified_construct")])
case("if_then_else_taints", "an additive change beside an unknown construct is unclassified",
     setp(PS + ["retries"], {"type": "integer", "default": 0}),
     [(P + "/retries", "unclassified_construct")], old_mutate=add_cond)
case("one_of", "oneOf has no rule", setp(PS + ["label", "oneOf"], [{"maxLength": 3}, {"minLength": 10}]),
     [(P + "/label/oneOf", "unclassified_construct")])
case("ref_keyword", "$ref has no rule", setp(PS + ["feature", "$ref"], "#/$defs/feature"),
     [(P + "/feature/$ref", "unclassified_construct")])

# ---- results
case("result_field_added", "new result field: additive", setp(RS + ["quality"], {"type": "number"}),
     [(R + "/quality", "result_field_added")])
case("result_field_added_required", "new always-present result field: additive",
     both(setp(RS + ["quality"], {"type": "number"}),
          setp(["result_schema", "required"], ["deviation", "quality", "status"])),
     [(R + "/quality", "result_field_added")])
case("result_field_removed", "removed result field: breaking", delp(RS + ["note"]),
     [(R + "/note", "result_field_removed")])
case("result_type", "changed result type: breaking", setp(RS + ["deviation", "type"], "string"),
     [(R + "/deviation/type", "type_changed")])
case("result_unit", "changed result unit: breaking", setp(RS + ["deviation", "x-unit"], "um"),
     [(R + "/deviation/x-unit", "unit_changed")])
case("result_enum_widened", "widened result enum: breaking until AUTO states its policy",
     setp(RS + ["status", "enum"], ["ok", "fail", "warn"]), [(R + "/status/enum", "enum_widened")])
case("result_enum_narrowed", "narrowed result enum: breaking", setp(RS + ["status", "enum"], ["ok"]),
     [(R + "/status/enum", "enum_narrowed")])
case("result_made_optional", "result field no longer guaranteed: breaking",
     setp(["result_schema", "required"], ["deviation"]), [("/result_schema/required", "required_changed")])
case("result_type_whole", "result schema replaced by a non-object type", setp(["result_schema", "type"], "array"),
     [("/result_schema/type", "type_changed")])

# ---- combinations (sorting by pointer)
case("combo_additive", "two additive changes",
     both(setp(["title"], "Measure"), setp(PS + ["retries"], {"type": "integer", "default": 0})),
     [(P + "/retries", "optional_param_added"), ("/title", "doc_changed")])
case("combo_mixed", "additive and breaking together",
     both(setp(["description"], "Changed."), setp(PS + ["tol", "type"], "integer"),
          setp(["side_effect"], "W1")),
     [("/description", "doc_changed"), (P + "/tol/type", "type_changed"), ("/side_effect", "side_effect_changed")])


# ---- review of PR #190 (all hand-labelled from the same rule table)
case("required_malformed", "required that is not an array of strings: added members are unclassified",
     both(setp(PS + ["retries"], {"type": "integer", "default": 0}),
          setp(["params_schema", "required"], ["retries", 5])),
     [(P + "/retries", "unclassified_construct"), ("/params_schema/required", "unclassified_construct")])
case("nullable_added", "adding null to the type is a changed JSON type: breaking",
     setp(PS + ["tol", "type"], ["number", "null"]), [(P + "/tol/type", "type_changed")])
case("integer_to_number", "integer to number is a changed JSON type: breaking",
     setp(PS + ["samples", "type"], "number"), [(P + "/samples/type", "type_changed")])
case("ref_target_changed", "a changed $ref target has no rule",
     setp(PS + ["feature", "$ref"], "#/$defs/datum"), [(P + "/feature/$ref", "unclassified_construct")],
     old_mutate=setp(PS + ["feature", "$ref"], "#/$defs/feature"))
case("open_and_add", "opening the object while adding a parameter: both breaking",
     both(delp(["params_schema", "additionalProperties"]), setp(PS + ["retries"], {"type": "integer", "default": 0})),
     [("/params_schema/additionalProperties", "constraint_changed"), (P + "/retries", "param_added_to_open_object")])
case("optional_plus_default", "required parameter made optional with a new default: breaking",
     both(setp(PS + ["tol", "default"], 1), setp(["params_schema", "required"], ["feature"])),
     [(P + "/tol/default", "default_changed"), ("/params_schema/required", "required_changed")])
case("parent_taints_nested", "an unknown keyword in a parent object taints a nested addition",
     setp(PS + ["points", "items", "properties", "y"], {"type": "number", "default": 0}),
     [(P + "/points/items/properties/y", "unclassified_construct")],
     old_mutate=setp(["params_schema", "patternProperties"], {"^x_": {"type": "string"}}))
case("doc_beside_unknown", "documentation beside an unknown keyword stays additive",
     setp(PS + ["feature", "description"], "The feature."), [(P + "/feature/description", "doc_changed")],
     old_mutate=add_cond)
case("ui_beside_unknown", "an x-ui hint beside an unknown keyword stays additive",
     setp(PS + ["samples", "x-ui"], {"widget": "spin"}), [(P + "/samples/x-ui", "ui_hint_changed")],
     old_mutate=add_cond)

def build_pairs():
    out = {}
    for i, (name, rule, mutate, expect, old_mutate) in enumerate(CASES, 1):
        old = base()
        if old_mutate:
            old_mutate(old)
        new = copy.deepcopy(old)
        mutate(new)
        check_def(old)
        check_def(new)
        assert [p for p, _ in expect] == sorted(p for p, _ in expect), f"{name}: expect not sorted"
        records = []
        for pointer, reason in expect:
            od, nd = digest_at(old, pointer), digest_at(new, pointer)
            assert od != nd, f"{name}: {pointer} does not change"
            records.append({"command": old["id"], "major": old["major"], "pointer": pointer,
                            "old_digest": od, "new_digest": nd, "kind": KIND[reason], "reason": reason})
        doc = {"name": name, "rule": rule, "old": old, "new": new, "expect": records}
        out[f"{i:03d}-{name.replace('_', '-')}.json"] = doc
    return out


# ---- AC-2 manifests
def cmd(id_, **over):
    d = base()
    d["id"] = id_
    d["title"] = f"Demo {id_}"
    for k, v in over.items():
        d[k] = v
    return d


def manifest(commands):
    cs = []
    for c in sorted(commands, key=lambda c: (c["id"], c["major"])):
        check_def(c)
        c = dict(c)
        c["definition_digest"] = sha(c)
        cs.append(c)
    return {"format": 1, "quantity_catalogue_digest": "sha256:" + hashlib.sha256(b"demo catalogue").hexdigest(),
            "commands": cs}


def build_manifests():
    scan_old = cmd("demo.scan")
    scan_old["params_schema"]["properties"]["speed"] = {"type": "number", "default": 5}
    scan_new = copy.deepcopy(scan_old)
    scan_new["params_schema"]["properties"]["speed"]["type"] = "string"
    measure_old = cmd("demo.measure")
    measure_new = copy.deepcopy(measure_old)
    measure_new["params_schema"]["properties"]["retries"] = {"type": "integer", "default": 0}
    probe_old = cmd("demo.probe")
    probe_new = cmd("demo.probe", lifecycle={"state": "deprecated", "since": "2026-09-01",
                                             "replaced_by": "demo.probe@2"})
    stable = cmd("demo.stable")
    legacy = cmd("demo.legacy")
    export = cmd("demo.export")
    v1 = manifest([scan_old, measure_old, probe_old, stable, legacy])
    v2 = manifest([scan_new, measure_new, probe_new, stable, export])

    def rec(old, new, pointer, reason):
        return {"command": old["id"], "major": old["major"], "pointer": pointer,
                "old_digest": digest_at(old, pointer), "new_digest": digest_at(new, pointer),
                "kind": KIND[reason], "reason": reason}

    expected = {
        "added": [{"command": "demo.export", "major": 1}],
        "removed": [{"command": "demo.legacy", "major": 1}],
        "lifecycle_changed": [{
            "command": "demo.probe", "major": 1, "pointer": "/lifecycle",
            "old_state": "active", "new_state": "deprecated",
            "old_digest": sha(probe_old["lifecycle"]), "new_digest": sha(probe_new["lifecycle"]),
        }],
        "changed": [
            rec(measure_old, measure_new, P + "/retries", "optional_param_added"),
            rec(scan_old, scan_new, P + "/speed/type", "type_changed"),
        ],
    }
    return {"v1.json": v1, "v2.json": v2, "v1_v2.diff.json": expected}


def render(doc):
    return (json.dumps(doc, indent=1, ensure_ascii=True) + "\n").encode("ascii")


def main():
    check = "--check" in sys.argv[1:]
    pairs = build_pairs()
    assert len(pairs) >= 40
    targets = [(PAIRS / n, d) for n, d in pairs.items()] + [(MANIFEST / n, d) for n, d in build_manifests().items()]
    bad = []
    if not check:
        PAIRS.mkdir(parents=True, exist_ok=True)
        MANIFEST.mkdir(parents=True, exist_ok=True)
        for old in PAIRS.glob("*.json"):
            if old.name not in pairs:
                old.unlink()
    else:
        extra = sorted(p.name for p in PAIRS.glob("*.json") if p.name not in pairs)
        bad += extra
    for path, doc in targets:
        data = render(doc)
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(str(path.relative_to(HERE.parent.parent)))
        else:
            path.write_bytes(data)
    if bad:
        print("stale fixtures:", *bad, sep="\n  ")
        return 1
    print(f"{len(pairs)} pairs and 3 manifest files {'up to date' if check else 'written'}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
