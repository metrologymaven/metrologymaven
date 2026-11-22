#!/usr/bin/env python3
"""Generate fixtures/core/bus/pipe/stage/probes/{registry,cases}.json (CORE.BUS.PIPE.STAGE).

registry.json is a synthetic test registry: a W2 write command, an R0 no-op read, an R1
read, a request_only W2 command and a W3 command whose `reason` parameter is recorded as the
entry's reason. The Rust test registers one probe handler for all of them, whose behaviour
is chosen by the `target` parameter: "refuse" makes the handler refuse with
precondition/false, "undeclared" makes it return an event type the command does not
declare, anything else returns one declared event {"count", "target"}.

cases.json holds the expected outcomes, stated by hand from the rules of
spec/core/bus/pipe/stage/NODE.md (REQ-001 stage order, REQ-002 error path, REQ-003
pending requests) and the sibling leaves each stage calls (CORE.BUS.PIPE.VALID,
CORE.BUS.POL.CLASS, CORE.BUS.POL.PROJ, CORE.BUS.PIPE.HANDLER), never by running the Rust
code:

- "order": the stages a W2 run passes, in order (AC-1);
- "forced": for each stage, the stages that run when a probe refuses at that stage (AC-2):
  the stages up to and including it;
- "real": one real failure per stage (AC-2), with the call, the expected error code and
  reason, and the stages that run;
- "request": the MCP call of the request_only command (AC-3), with the parameters
  commitment of the filed request (sha256 of the RFC 8785 canonical parameters, computed
  here with hashlib; the parameters are ASCII strings and small integers, for which
  sorted-key compact json.dumps is canonical).

CC-BY-4.0 project data.

Run:    python fixtures/core/bus/pipe/stage/make_fixtures.py
Check:  python fixtures/core/bus/pipe/stage/make_fixtures.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

OUT = Path(__file__).resolve().parent / "probes"

STAGES = ["resolve", "validate", "authorise", "precondition", "route", "handler", "effects", "commit"]


def canonical(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def commitment(v):
    return "sha256:" + hashlib.sha256(canonical(v)).hexdigest()


WRITE_PARAMS = {
    "type": "object",
    "additionalProperties": False,
    "required": ["target", "count"],
    "properties": {"target": {"type": "string"}, "count": {"type": "integer"}},
}


def command(cid, title, side_effect, projection, params, effects, preconditions=()):
    return {
        "id": cid,
        "major": 1,
        "title": title,
        "description": "Synthetic command of the CORE.BUS.PIPE.STAGE probe registry.",
        "params_schema": params,
        "result_schema": {"type": "object"},
        "side_effect": side_effect,
        "mcp_projection": projection,
        "determinism": "D0",
        "idempotency": "none",
        "fs_effect": "none",
        "undo": "head_move" if side_effect not in ("R0", "R1") else "not_undoable",
        "preconditions": list(preconditions),
        "effects": list(effects),
        "algorithms": [],
        "exposure": ["cli", "gui", "mcp"],
        "lifecycle": {"state": "active", "since": "2026-11-21"},
        "human_only": False,
        "network": "none",
        "web": "available",
        "cli_bare": False,
    }


POSITIVE = [{"expr": "params.count > 0", "bound": 100, "message_key": "test.count_positive"}]

REGISTRY = {
    "commands": [
        command("test.write", "Probe write", "W2", "exposed", WRITE_PARAMS, ["TestWritten"], POSITIVE),
        command(
            "test.noop_read",
            "Probe no-op read",
            "R0",
            "exposed",
            {"type": "object", "additionalProperties": False, "properties": {}},
            [],
        ),
        command("test.read_r1", "Probe R1 read", "R1", "exposed", WRITE_PARAMS, []),
        command("test.request", "Probe request-only write", "W2", "request_only", WRITE_PARAMS, ["TestWritten"]),
        command(
            "test.release",
            "Probe release with a reason",
            "W3",
            "request_only",
            {
                "type": "object",
                "additionalProperties": False,
                "required": ["target", "count", "reason"],
                "properties": {
                    "target": {"type": "string"},
                    "count": {"type": "integer"},
                    "reason": {"type": "string"},
                },
            },
            ["TestWritten"],
        ),
    ]
}

OK = {"target": "a", "count": 1}


def upto(stage):
    return STAGES[: STAGES.index(stage) + 1]


# One real failure per stage, labelled from the sibling leaves' rules.
REAL = [
    # CORE.BUS.PIPE.VALID REQ-001: an ID the table does not hold.
    {"stage": "resolve", "command": "test.missing@1", "channel": "gui", "params": OK,
     "code": "not_found", "reason": "unknown_command"},
    # CORE.BUS.PIPE.VALID REQ-003: target must be a string.
    {"stage": "validate", "command": "test.write@1", "channel": "gui", "params": {"target": 1, "count": 1},
     "code": "validation", "reason": "schema"},
    # CORE.BUS.POL.CLASS REQ-001: only a human may run a W3 command; cli derives a script.
    {"stage": "authorise", "command": "test.release@1", "channel": "cli",
     "params": {"target": "a", "count": 1, "reason": "r"}, "code": "policy", "reason": "class_forbidden"},
    # CORE.BUS.PIPE.VALID REQ-006: params.count > 0 is false.
    {"stage": "precondition", "command": "test.write@1", "channel": "gui", "params": {"target": "a", "count": 0},
     "code": "precondition", "reason": "false"},
    # STAGE route guard: expected_head is not the branch head.
    {"stage": "route", "command": "test.write@1", "channel": "gui", "params": OK, "expected_head": "wrong",
     "code": "conflict", "reason": "expected_head_mismatch"},
    # The probe handler refuses with precondition/false.
    {"stage": "handler", "command": "test.write@1", "channel": "gui", "params": {"target": "refuse", "count": 1},
     "code": "precondition", "reason": "false"},
    # CORE.BUS.PIPE.HANDLER REQ-003: an undeclared event type.
    {"stage": "effects", "command": "test.write@1", "channel": "gui", "params": {"target": "undeclared", "count": 1},
     "code": "undeclared_effect", "reason": "undeclared_event_type"},
    # STAGE commit: a commit hook's precommit reports a stale decision.
    {"stage": "commit", "command": "test.write@1", "channel": "gui", "params": OK, "hook_refuses": True,
     "code": "conflict", "reason": "head_moved"},
]

for case in REAL:
    case["ran"] = upto(case["stage"])

REQUEST_PARAMS = {"target": "a", "count": 2}

CASES = {
    "order": STAGES,
    "forced": [{"stage": s, "ran": upto(s)} for s in STAGES],
    "real": REAL,
    "request": {
        "command": "test.request@1",
        "channel": "mcp",
        "params": REQUEST_PARAMS,
        # REQ-003: resolve, validate, authorise, then route and commit of the filing only.
        "ran": ["resolve", "validate", "authorise", "route", "commit"],
        "params_commitment": commitment(REQUEST_PARAMS),
        "handler_calls": 0,
        "entries_appended": 1,
    },
}


def render(v):
    return json.dumps(v, indent=1, sort_keys=True, ensure_ascii=False) + "\n"


def main():
    files = {"registry.json": render(REGISTRY), "cases.json": render(CASES)}
    check = "--check" in sys.argv[1:]
    bad = []
    for name, text in files.items():
        path = OUT / name
        if check:
            if not path.exists() or path.read_text(encoding="utf-8") != text:
                bad.append(name)
        else:
            OUT.mkdir(parents=True, exist_ok=True)
            path.write_text(text, encoding="utf-8", newline="\n")
    if bad:
        print("stale: " + ", ".join(bad) + " (run make_fixtures.py)", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main())
