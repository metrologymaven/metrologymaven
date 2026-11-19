#!/usr/bin/env python3
"""Generate the fixtures of spec node CORE.REG.VER.GATE (AC-1 .. AC-5 and more).

Hand-labelled: every case below states, by hand and from the leaf's rules (REQ-001 ..
REQ-006), the COMPAT reason codes (CORE.REG.VER.COMPAT rule table) and the LIFE codes
(CORE.REG.VER.LIFE transition rules), which (ID, major), JSON pointer and code the gate
must report, and the exit status. This script never runs the Rust gate. It only computes
what cannot be written by hand: each `definition_digest` (SHA-256 of the RFC 8785 form of
the definition, computed with Python's json + hashlib; every number is an integer and every
string is ASCII, so sorted-key compact JSON is exactly RFC 8785), the SHA-256 of each
baseline's exact bytes for its `.sha256` sidecar, and the report text, formatted from the
hand-labelled failures by the documented report format.

Outputs (CC-BY-4.0 project data), all in this directory:
  baseline*.json, baseline*.json.sha256   baselines and their stated digests
  <case>.json                             current declarations, one per case
  expect.json                             every case: inputs, exit, failures, report

Run:    python fixtures/core/reg/ver/gate/make_fixtures.py
Check:  python fixtures/core/reg/ver/gate/make_fixtures.py --check
"""

import copy
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
QCAT = "sha256:" + "ab" * 32


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


def sha_bytes(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def definition(cid, major, side_effect="R0", projection="exposed", lifecycle=None, **extra):
    d = {
        "id": cid,
        "major": major,
        "title": f"Demo {cid}",
        "description": "Demo command for the registry gate fixtures.",
        "params_schema": {
            "type": "object",
            "additionalProperties": False,
            "required": ["tol"],
            "properties": {"tol": {"type": "number", "minimum": 0}},
        },
        "result_schema": {
            "type": "object",
            "additionalProperties": False,
            "required": ["deviation"],
            "properties": {"deviation": {"type": "number"}},
        },
        "side_effect": side_effect,
        "mcp_projection": projection,
        "determinism": "D0",
        "idempotency": "natural",
        "fs_effect": "none",
        "undo": "head_move",
        "preconditions": [],
        "effects": [],
        "algorithms": ["geo.fit.lsq@1.0.0"],
        "exposure": ["cli", "mcp"],
        "lifecycle": lifecycle or {"state": "active", "since": "2026-01-01"},
        "human_only": False,
        "network": "none",
        "web": "available",
        "cli_bare": False,
    }
    d.update(extra)
    return d


# The baseline: the manifest of the last release.
BASE = {
    ("demo.export", 1): definition("demo.export", 1, side_effect="W2", projection="request_only"),
    ("demo.legacy", 1): definition("demo.legacy", 1, lifecycle={
        "state": "deprecated", "since": "2025-01-15", "replaced_by": "demo.legacy@2"}),
    ("demo.legacy", 2): definition("demo.legacy", 2, lifecycle={"state": "active", "since": "2025-01-15"}),
    ("demo.measure", 1): definition("demo.measure", 1),
    ("demo.old", 1): definition("demo.old", 1, lifecycle={
        "state": "hidden", "since": "2025-06-01", "remove_after": "2026-01-01"}),
    ("demo.private", 1): definition("demo.private", 1),
}


def manifest(defs, fmt=1, digest_override=None):
    commands = []
    for key in sorted(defs):
        d = copy.deepcopy(defs[key])
        d["definition_digest"] = sha_bytes(jcs(d))
        if digest_override and key in digest_override:
            d["definition_digest"] = digest_override[key]
        commands.append(d)
    doc = {"format": fmt, "quantity_catalogue_digest": QCAT, "commands": commands}
    return (json.dumps(doc, indent=1, ensure_ascii=True) + "\n").encode("ascii")


def manifest_raw(defs, fmt=1):
    """A manifest of `defs` in the given order, repeats kept."""
    commands = []
    for d in defs:
        d = copy.deepcopy(d)
        d["definition_digest"] = sha_bytes(jcs(d))
        commands.append(d)
    doc = {"format": fmt, "quantity_catalogue_digest": QCAT, "commands": commands}
    return (json.dumps(doc, indent=1, ensure_ascii=True) + "\n").encode("ascii")


def edit(changes=None, drop=(), add=None):
    """The baseline with `changes` (key -> function mutating a copy), `drop`ped keys and
    `add`ed definitions."""
    defs = copy.deepcopy(BASE)
    for key, fn in (changes or {}).items():
        fn(defs[key])
    for key in drop:
        del defs[key]
    for d in add or []:
        defs[(d["id"], d["major"])] = d
    return defs


def setter(path, value):
    def fn(d):
        cur = d
        for p in path[:-1]:
            cur = cur[p]
        cur[path[-1]] = value
    return fn


def chain(*fns):
    def fn(d):
        for f in fns:
            f(d)
    return fn


def fail(cid, major, pointer, code):
    return {"code": code, "command": cid, "major": major, "pointer": pointer}


def report(failures):
    if not failures:
        return "registry-gate: ok\n"
    ordered = sorted(failures, key=lambda f: (f["command"], f["major"], f["pointer"], f["code"]))
    assert ordered == failures, "list failures in report order"
    lines = [jcs(f).decode("ascii") for f in failures]
    return "\n".join(lines) + f"\nregistry-gate: fail ({len(failures)})\n"


FILES = {}
CASES = []


def case(name, current_defs, failures, exit_code=None, baseline="baseline.json", release_date=None,
         current_bytes=None, current_file=None):
    current = current_file or f"{name}.json"
    if current_file is None:
        FILES[current] = current_bytes if current_bytes is not None else manifest(current_defs)
    c = {"name": name, "baseline": baseline, "current": current}
    if release_date:
        c["release_date"] = release_date
    if exit_code == 2:
        c["exit"] = 2
        c["code"] = failures
    else:
        c["exit"] = 1 if failures else 0
        c["failures"] = failures
        c["report"] = report(failures)
    CASES.append(c)


def baseline(name, data, sidecar=None):
    FILES[name] = data
    if sidecar is not False:
        FILES[name + ".sha256"] = (sidecar if sidecar is not None else sha_bytes(data) + "\n").encode("ascii")


BASE_BYTES = manifest(BASE)
baseline("baseline.json", BASE_BYTES)

M = ("demo.measure", 1)
E = ("demo.export", 1)
L1 = ("demo.legacy", 1)
OLD = ("demo.old", 1)
P = ("demo.private", 1)

# AC-1: additive changes to baseline majors, plus new (ID, major)s with anything in them.
measure2 = definition("demo.measure", 2)
measure2["params_schema"]["properties"]["tol"]["type"] = "string"
case("additive", edit(
    changes={M: chain(
        setter(["title"], "Measure a feature"),  # doc_changed (additive)
        setter(["params_schema", "properties", "samples"], {"type": "integer", "default": 25}),  # optional_param_added
        setter(["result_schema", "properties", "note"], {"type": "string"}),  # result_field_added
    )},
    add=[measure2, definition("demo.newcmd", 1, side_effect="W2")],
), [])

# AC-2: a parameter type changed under the same major (COMPAT type_changed).
case("type_change", edit(changes={M: setter(["params_schema", "properties", "tol", "type"], "string")}),
     [fail("demo.measure", 1, "/params_schema/properties/tol/type", "type_changed")])

# AC-3: class W2 -> W1, nothing else. COMPAT and REQ-002 name the same failure once.
case("class_change", edit(changes={E: setter(["side_effect"], "W1")}),
     [fail("demo.export", 1, "/side_effect", "side_effect_changed")])

# AC-4: a baseline (ID, major) missing without a transition to removed.
case("silent_removal", edit(drop=[E]), [fail("demo.export", 1, "", "silent_removal")])

# REQ-002: human_only true lowers the effective projection exposed -> request_only.
case("projection_change", edit(changes={P: setter(["human_only"], True)}),
     [fail("demo.private", 1, "/human_only", "human_only_changed"),
      fail("demo.private", 1, "/mcp_projection", "mcp_projection_changed")])

# COMPAT fails closed on a keyword without a rule; so does the gate.
case("unclassified", edit(changes={M: setter(["params_schema", "oneOf"], [{"required": ["tol"]}])}),
     [fail("demo.measure", 1, "/params_schema/oneOf", "unclassified_construct")])

# Several failures, sorted by command, major, pointer, code.
case("multi", edit(
    changes={E: setter(["side_effect"], "W1"), M: setter(["params_schema", "properties", "tol", "type"], "string")},
    drop=[P],
), [fail("demo.export", 1, "/side_effect", "side_effect_changed"),
    fail("demo.measure", 1, "/params_schema/properties/tol/type", "type_changed"),
    fail("demo.private", 1, "", "silent_removal")])

# Legal lifecycle moves, dated 2026-02-01: active -> deprecated; deprecated -> hidden after
# 12 months (2025-01-15 -> 2026-02-01); hidden -> removed on or after remove_after 2026-01-01.
LEGAL = edit(
    changes={
        M: setter(["lifecycle"], {"state": "deprecated", "since": "2026-02-01", "replaced_by": "demo.measure@2"}),
        L1: setter(["lifecycle"], {"state": "hidden", "since": "2026-02-01", "replaced_by": "demo.legacy@2",
                                   "remove_after": "2026-08-01"}),
        OLD: setter(["lifecycle"], {"state": "removed", "since": "2026-02-01"}),
    },
    add=[definition("demo.measure", 2, lifecycle={"state": "active", "since": "2026-02-01"})],
)
# A state change needs --release-date (maintainer decision on PR #199): without it, exit 2.
case("lifecycle_legal", LEGAL, [], release_date="2026-02-01")
case("lifecycle_legal_undated", None, "release_date_required", exit_code=2, current_file="lifecycle_legal.json")
# --release-date names another release: every changed lifecycle's `since` is wrong.
case("lifecycle_wrong_release_date", None, [
    fail("demo.legacy", 1, "/lifecycle/since", "bad_lifecycle"),
    fail("demo.measure", 1, "/lifecycle/since", "bad_lifecycle"),
    fail("demo.old", 1, "/lifecycle/since", "bad_lifecycle"),
], release_date="2026-03-01", current_file="lifecycle_legal.json")

# REQ-004: transitions the lifecycle check refuses.
case("illegal_transition", edit(changes={M: setter(["lifecycle"], {"state": "hidden", "since": "2026-02-01"})}),
     [fail("demo.measure", 1, "/lifecycle", "illegal_transition")], release_date="2026-02-01")
case("deprecation_too_short", edit(changes={L1: setter(["lifecycle"], {
    "state": "hidden", "since": "2025-12-01", "replaced_by": "demo.legacy@2"})}),
    [fail("demo.legacy", 1, "/lifecycle", "deprecation_too_short")], release_date="2025-12-01")
case("removal_too_early", edit(changes={OLD: setter(["lifecycle"], {"state": "removed", "since": "2025-12-01"})}),
     [fail("demo.old", 1, "/lifecycle", "removal_too_early")], release_date="2025-12-01")

# A `since` far in the future cannot stand in for the release date: undated it is exit 2,
# dated it is a `since` that is not the release date (exit 1). Both bypasses, both ways.
case("deprecation_bypass", edit(changes={L1: setter(["lifecycle"], {
    "state": "hidden", "since": "2099-01-01", "replaced_by": "demo.legacy@2"})}),
    "release_date_required", exit_code=2)
case("deprecation_bypass_dated", None, [fail("demo.legacy", 1, "/lifecycle/since", "bad_lifecycle")],
     release_date="2025-12-01", current_file="deprecation_bypass.json")
case("removal_bypass", edit(changes={OLD: setter(["lifecycle"], {"state": "removed", "since": "2099-01-01"})}),
     "release_date_required", exit_code=2)
case("removal_bypass_dated", None, [fail("demo.old", 1, "/lifecycle/since", "bad_lifecycle")],
     release_date="2025-12-01", current_file="removal_bypass.json")

# Staying in the same state with new metadata is not one of LIFE's legal moves; it is
# checked before any date rule, so it is illegal_transition with or without a release date.
case("same_state_change", edit(changes={L1: setter(["lifecycle"], {
    "state": "deprecated", "since": "2026-02-01", "replaced_by": "demo.legacy@2", "remove_after": "2027-01-01"})}),
    [fail("demo.legacy", 1, "/lifecycle", "illegal_transition")])
case("same_state_change_dated", None, [fail("demo.legacy", 1, "/lifecycle", "illegal_transition")],
     release_date="2026-02-01", current_file="same_state_change.json")
case("same_state_old_since", edit(changes={L1: setter(["lifecycle"], {
    "state": "deprecated", "since": "2025-01-15", "replaced_by": "demo.legacy@2", "remove_after": "2027-01-01"})}),
    [fail("demo.legacy", 1, "/lifecycle", "illegal_transition")])
# A state change that keeps the old date would start the deprecation clock in the past.
case("stale_since", edit(changes={M: setter(["lifecycle"], {"state": "deprecated", "since": "2026-01-01"})}),
     [fail("demo.measure", 1, "/lifecycle/since", "bad_lifecycle")], release_date="2026-01-01")
# An unreadable lifecycle fails even on an (ID, major) absent from the baseline.
case("bad_lifecycle_new", edit(add=[definition("demo.newcmd", 1, lifecycle={"state": "retired", "since": "2026-02-01"})]),
     [fail("demo.newcmd", 1, "/lifecycle", "bad_lifecycle")])

# An empty current set: every baseline (ID, major) is a silent removal.
case("empty_current", {}, [fail(cid, major, "", "silent_removal") for (cid, major) in sorted(BASE)])
# A zero-command baseline: everything current is new, so it passes.
baseline("baseline_empty.json", manifest({}))
case("baseline_zero_commands", None, [], baseline="baseline_empty.json", current_file="additive.json")
# A major bump that drops the old major instead of retiring it through the lifecycle.
case("major_bump_drop", edit(drop=[M], add=[definition("demo.measure", 2)]),
     [fail("demo.measure", 1, "", "silent_removal")])
# A duplicate (ID, major), in the declarations or in the baseline.
case("duplicate_current", None, "declarations_invalid", exit_code=2,
     current_bytes=manifest_raw([BASE[k] for k in sorted(BASE)] + [BASE[M]]))
baseline("baseline_duplicate_command.json", manifest_raw([BASE[k] for k in sorted(BASE)] + [BASE[M]]))
case("baseline_duplicate_command", None, "baseline_invalid", exit_code=2,
     baseline="baseline_duplicate_command.json", current_file="additive.json")

# A manifest never lists a removed major (LIFE retained()): such a baseline is invalid
# (maintainer decision on PR #199).
GONE = dict(BASE)
GONE[("demo.gone", 1)] = definition("demo.gone", 1, lifecycle={"state": "removed", "since": "2025-06-01"})
baseline("baseline_lists_removed.json", manifest(GONE))
case("baseline_lists_removed", None, "baseline_invalid", exit_code=2, baseline="baseline_lists_removed.json",
     current_file="baseline.json")

# Exit 2: inputs the gate cannot trust (REQ-006, AC-5).
baseline("baseline_tampered.json", BASE_BYTES.replace(b'"format": 1', b'"format":  1'),
         sidecar=sha_bytes(BASE_BYTES) + "\n")
case("baseline_tampered", None, "baseline_invalid", exit_code=2, baseline="baseline_tampered.json",
     current_file="additive.json")
baseline("baseline_nosidecar.json", BASE_BYTES, sidecar=False)
case("baseline_no_sidecar", None, "baseline_invalid", exit_code=2, baseline="baseline_nosidecar.json",
     current_file="additive.json")
baseline("baseline_upper.json", BASE_BYTES, sidecar=sha_bytes(BASE_BYTES).upper().replace("SHA256:", "sha256:"))
case("baseline_upper_sidecar", None, "baseline_invalid", exit_code=2, baseline="baseline_upper.json",
     current_file="additive.json")
# Exactly one digest: a repeated line is refused.
baseline("baseline_two_lines.json", BASE_BYTES, sidecar=(sha_bytes(BASE_BYTES) + "\n") * 2)
case("baseline_two_line_sidecar", None, "baseline_invalid", exit_code=2, baseline="baseline_two_lines.json",
     current_file="additive.json")
baseline("baseline_dupkey.json", BASE_BYTES.replace(b'"format": 1,', b'"format": 1,\n "format": 1,', 1))
case("baseline_duplicate_key", None, "baseline_invalid", exit_code=2, baseline="baseline_dupkey.json",
     current_file="additive.json")
BADLIFE = edit(changes={M: setter(["lifecycle"], {"state": "active"})})
baseline("baseline_badlife.json", manifest(BADLIFE))
case("baseline_bad_lifecycle", None, "baseline_invalid", exit_code=2, baseline="baseline_badlife.json",
     current_file="additive.json")
case("baseline_missing", None, "baseline_invalid", exit_code=2, baseline="baseline_does_not_exist.json",
     current_file="additive.json")
case("declarations_bad_digest", BASE, "declarations_invalid", exit_code=2,
     current_bytes=manifest(BASE, digest_override={M: "sha256:" + "00" * 32}))
case("format_mismatch", BASE, "format_mismatch", exit_code=2, current_bytes=manifest(BASE, fmt=2))

FILES["expect.json"] = (json.dumps({"cases": CASES}, indent=1, ensure_ascii=True) + "\n").encode("ascii")


def main():
    check = "--check" in sys.argv[1:]
    bad = []
    for name, data in sorted(FILES.items()):
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                bad.append(name)
        else:
            path.write_bytes(data)
    if check:
        if bad:
            print("stale fixtures: " + ", ".join(bad))
            return 1
        print(f"ok: {len(FILES)} files")
    else:
        print(f"wrote {len(FILES)} files")
    return 0


if __name__ == "__main__":
    sys.exit(main())
