#!/usr/bin/env python3
"""Generate CORE.REG.DEF.FIELD fixtures (CC-BY-4.0 project data).

defs_valid.json: one definition per side-effect class, each enum value used at least once.
defs_invalid/*.json: {"expect": <error code>, "def": {...}} per failure case.
Run:  python fixtures/core/reg/def/field/make_fixtures.py
"""

import copy
import json
from pathlib import Path

HERE = Path(__file__).parent
SIDE = ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]
PROJ = ["exposed", "request_only", "never"]
DET = ["D0", "D1", "D2", "D3", "N"]
IDEM = ["natural", "keyed", "none"]
FS = ["none", "read_root", "write_root"]
UNDO = ["head_move", "not_undoable"]
NET = ["none", "egress", "device"]
WEB = ["available", "partial", "unavailable"]
SURF = ["cli", "mcp", "python", "gui", "control"]
NAMES = ["gdt.evaluate_flatness", "project.list_features", "report.set_title", "align.run_321",
         "record.release_fai", "egress.send", "hal.read_probe", "hal.connect_device", "cmm.move_to",
         "trust.sign_record", "settings.set_admin_value"]

valid = []
for i, side in enumerate(SIDE):
    d = {
        "id": NAMES[i],
        "major": 1 + (i % 3),
        "title": f"Example command {i}",
        "description": f"Fixture definition for side-effect class {side}.",
        "params_schema": {"type": "object", "properties": {"tolerance_mm": {"type": "number"}}},
        "result_schema": {"type": "object"},
        "side_effect": side,
        "mcp_projection": PROJ[i % 3],
        "determinism": DET[i % 5],
        "idempotency": IDEM[i % 3],
        "fs_effect": FS[i % 3],
        "undo": UNDO[i % 2],
        "preconditions": ["project.open"] if i % 2 else [],
        "effects": [f"{NAMES[i]}@1"] if side.startswith("W") else [],
        "algorithms": ["gdt.flatness.mz@0.1.0"] if i == 0 else [],
        "exposure": sorted({SURF[i % 5], SURF[(i + 2) % 5]}),
        "lifecycle": {"state": "experimental"},
        "web": WEB[i % 3],
    }
    if d["web"] != "available":
        d["web_note"] = "Needs local instrument or file access."
    if i % 4 == 1:
        d["human_only"] = True
    if i % 5 != 0:
        d["network"] = NET[i % 3]
    if i == 3:
        d["cli_bare"] = True
    valid.append(d)

for field, values in [("mcp_projection", PROJ), ("determinism", DET), ("idempotency", IDEM), ("fs_effect", FS),
                      ("undo", UNDO), ("web", WEB)]:
    assert {d[field] for d in valid} == set(values), field
assert {d.get("network", "none") for d in valid} == set(NET)
assert {s for d in valid for s in d["exposure"]} == set(SURF)
assert any(d.get("human_only") for d in valid) and any(d.get("cli_bare") for d in valid)
(HERE / "defs_valid.json").write_text(json.dumps(valid, indent=1) + "\n", encoding="utf-8", newline="\n")

base = valid[3]
inv = HERE / "defs_invalid"
inv.mkdir(exist_ok=True)


def bad(name, expect, change):
    d = copy.deepcopy(base)
    change(d)
    (inv / f"{name}.json").write_text(json.dumps({"expect": expect, "def": d}, indent=1) + "\n",
                                      encoding="utf-8", newline="\n")


bad("bad_side_effect", "bad_enum", lambda d: d.update(side_effect="W9"))
bad("bad_fs_effect", "bad_enum", lambda d: d.update(fs_effect="write_anywhere"))
bad("bad_undo", "bad_enum", lambda d: d.update(undo="maybe"))
bad("bad_network", "bad_enum", lambda d: d.update(network="internet"))
bad("web_missing", "web_missing", lambda d: d.pop("web"))
bad("web_note_missing", "web_note_missing", lambda d: (d.update(web="partial"), d.pop("web_note", None)))
bad("entitlement_field", "unknown_field", lambda d: d.update(entitlement="pro"))
bad("unknown_field", "unknown_field", lambda d: d.update(colour="blue"))
print(f"wrote {len(valid)} valid definitions and {len(list(inv.glob('*.json')))} invalid cases")
