#!/usr/bin/env python3
"""Generate CORE.REG.VAL fixtures (CC-BY-4.0 project data). Stdlib only.

Every expected result below is hand-labelled from the rules in spec/core/reg/val/NODE.md;
nothing is computed by the Rust validator.

- valid.json: {"decls": [...]}, at least one declaration per side-effect class; no errors.
- invalid/<case>.json: {"expect": <rule code>, "decls": [...]}, exactly one rule broken.
- many_errors.json: {"expect_errors": [[command, major, rule, field], ...], "decls": [...]}
  with 12 errors; the declarations are in shuffled order and the expected list is sorted
  by (command, major, rule, field) in byte order, the order REQ-008 pins.

The `expect` / `expect_errors` keys are test metadata: the test removes them before
validating, because the file format admits only `decls`.
Run:  python fixtures/core/reg/decls/make_fixtures.py
"""

import copy
import json
from pathlib import Path

HERE = Path(__file__).parent

LENGTH = {"type": "number", "x-quantity": "length", "x-unit": "mm"}
ANGLE = {"type": "number", "x-quantity": "angle", "x-unit": "deg"}


def decl(cid, major, side, proj, **extra):
    d = {
        "id": cid,
        "major": major,
        "title": f"Fixture {cid}@{major}",
        "description": f"Fixture declaration for side-effect class {side}.",
        "params_schema": {
            "type": "object",
            "properties": {"tolerance": copy.deepcopy(LENGTH)},
            "required": ["tolerance"],
            "additionalProperties": False,
        },
        "result_schema": {"type": "object", "properties": {"angle": copy.deepcopy(ANGLE)}},
        "side_effect": side,
        "mcp_projection": proj,
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "not_undoable",
        "preconditions": [],
        "effects": [],
        "algorithms": [],
        "exposure": ["cli"],
        "lifecycle": {"state": "active", "since": "2026-01-01"},
        "web": "available",
    }
    d.update(extra)
    return d


def write(path, value):
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(json.dumps(value, indent=1, ensure_ascii=True) + "\n", encoding="utf-8", newline="\n")


# ---------------------------------------------------------------- valid.json
# One declaration per side-effect class at its loosest allowed projection, plus stricter
# projections, a second major (tool name *_v2), every subschema position the rule walks,
# and both catalogue kinds.
rich_params = {
    "type": "object",
    "$defs": {"len": copy.deepcopy(LENGTH)},
    "properties": {
        "nominal": {"$ref": "#/$defs/len"},
        "offsets": {"type": "array", "items": copy.deepcopy(LENGTH), "minItems": 1, "maxItems": 8},
        "pair": {"type": "array", "prefixItems": [copy.deepcopy(LENGTH), copy.deepcopy(ANGLE)]},
        "limit": {"anyOf": [{"type": "null"}, {"type": ["number", "null"], "x-quantity": "length",
                                                "minimum": 0, "maximum": 1}]},
        "count": {"type": "integer", "minimum": 1},
        "label": {"type": "string", "minLength": 1, "maxLength": 64},
    },
    "patternProperties": {"^[a-z]+$": {"type": "string"}, "^[A-Z]+$": {"type": "string"}},
    "required": ["nominal"],
}
valid = [
    decl("gdt.evaluate_flatness", 1, "R0", "exposed", params_schema=rich_params),
    decl("project.list_features", 1, "R1", "exposed", determinism="D1"),
    decl("project.list_features", 2, "R1", "request_only"),
    decl("sim.read_probe", 1, "I1", "exposed", determinism="N"),
    decl("report.set_title", 1, "W1", "exposed", human_only=False, network="none", cli_bare=False),
    decl("align.run_bestfit", 1, "W2", "exposed", determinism="D3", human_only=True),
    decl("record.release_fai", 1, "W3", "request_only"),
    decl("egress.send", 1, "X1", "request_only", network="egress"),
    decl("hal.connect_device", 1, "D1", "never", network="device"),
    decl("cmm.move_to", 1, "M1", "never"),
    decl("trust.sign_record", 1, "S1", "never"),
    decl("settings.set_admin_value", 1, "A0", "never", web="unavailable", web_note="Desktop only."),
]
write(HERE / "valid.json", {"decls": valid})

# ---------------------------------------------------------------- invalid/
no_det = decl("gdt.evaluate_flatness", 1, "R0", "exposed")
del no_det["determinism"]
unquantified = decl("gdt.evaluate_flatness", 1, "R0", "exposed")
unquantified["params_schema"]["properties"]["tolerance"] = {"type": "number"}
invalid = {
    "missing_field": ("missing_field", [no_det]),
    "bad_id": ("bad_id", [decl("gdt.Evaluate_flatness", 1, "R0", "exposed")]),
    # "gdt." + 61 characters: tool name "gdt_" + 61 = 65 characters.
    "long_tool_name": ("tool_name_too_long", [decl("gdt." + "a" * 61, 1, "R0", "exposed")]),
    "m1_exposed": ("projection_too_loose", [decl("cmm.move_to", 1, "M1", "exposed")]),
    "a0_request_only": ("projection_too_loose", [decl("settings.set_admin_value", 1, "A0", "request_only")]),
    "w3_exposed": ("projection_too_loose", [decl("record.release_fai", 1, "W3", "exposed")]),
    "number_without_quantity": ("missing_quantity", [unquantified]),
    "duplicate_major": ("duplicate_command", [decl("align.run_bestfit", 1, "W2", "exposed"),
                                              decl("align.run_bestfit", 1, "W2", "exposed", title="Again")]),
    # align.run_v2 @1 and align.run @2 both derive the tool name align_run_v2.
    "tool_name_collision": ("tool_name_collision", [decl("align.run_v2", 1, "W2", "exposed"),
                                                    decl("align.run", 2, "W2", "exposed")]),
}
for name, (expect, decls) in invalid.items():
    write(HERE / "invalid" / f"{name}.json", {"expect": expect, "decls": decls})

# ---------------------------------------------------------------- many_errors.json
alpha = decl("alpha.read_x", 1, "R0", "exposed")
del alpha["determinism"]
del alpha["result_schema"]
beta = decl("beta.calc", 1, "R1", "exposed")
beta["params_schema"]["properties"]["tolerance"] = {"type": "number"}
beta["result_schema"]["properties"]["angle"] = {"type": "number", "x-quantity": "time"}
lam = decl("lambda.cfg", 1, "A0", "never")
lam["colour"] = "blue"
many = [
    decl("zeta.move_axis", 1, "M1", "exposed"),
    lam,
    decl("eps.a", 2, "W2", "exposed"),
    beta,
    decl("gamma.dup", 1, "R0", "exposed"),
    decl("Bad.Id", 1, "R0", "exposed"),
    decl("kappa.write", 0, "W1", "exposed"),
    alpha,
    decl("delta.sign", 1, "S1", "request_only"),
    decl("gamma.dup", 1, "R0", "exposed"),
    decl("eps.a_v2", 1, "W2", "exposed"),
]
expect_errors = [
    ["zeta.move_axis", 1, "projection_too_loose", "/mcp_projection"],
    ["lambda.cfg", 1, "unknown_field", "/colour"],
    ["eps.a", 2, "tool_name_collision", "/id"],
    ["eps.a_v2", 1, "tool_name_collision", "/id"],
    ["beta.calc", 1, "missing_quantity", "/params_schema/properties/tolerance"],
    ["beta.calc", 1, "unknown_quantity", "/result_schema/properties/angle/x-quantity"],
    ["gamma.dup", 1, "duplicate_command", None],
    # An invalid id is never echoed as the command: command is empty.
    ["", 1, "bad_id", "/id"],
    ["kappa.write", 0, "bad_id", "/major"],
    ["alpha.read_x", 1, "missing_field", "/determinism"],
    ["alpha.read_x", 1, "missing_field", "/result_schema"],
    ["delta.sign", 1, "projection_too_loose", "/mcp_projection"],
]
assert len(expect_errors) == 12
expect_errors.sort(key=lambda e: (e[0].encode(), e[1], e[2].encode(), (e[3] or "").encode()))
write(HERE / "many_errors.json", {"expect_errors": expect_errors, "decls": many})
