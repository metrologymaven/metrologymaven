#!/usr/bin/env python3
"""Generate fixtures/core/bus/pol/proj/registry.json and cases.json (CORE.BUS.POL.PROJ).

registry.json is a synthetic test registry: one command projected exposed, one
request_only, one never, and one human_only W2 command (declared exposed, so the
human_only rule alone makes it request-only on MCP), plus one exposed command
whose surfaces lack mcp. cases.json holds the expected outcomes, stated by hand
from the rules of spec/core/bus/pol/proj/NODE.md (REQ-001, REQ-002, REQ-004) and
CONSTITUTION section 3, and the parameters commitments (sha256 of the RFC 8785
canonical JSON of the parameters), computed here with hashlib and the standard
json module, independently of the Rust code. The parameters use only ASCII
strings, booleans and small integers, for which sorted-key compact json.dumps is
RFC 8785 canonical. CC-BY-4.0 project data.

Run:    python fixtures/core/bus/pol/proj/make_cases.py
Check:  python fixtures/core/bus/pol/proj/make_cases.py --check
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def canonical(v):
    return json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def commitment(v):
    return "sha256:" + hashlib.sha256(canonical(v)).hexdigest()


def command(cid, title, side_effect, projection, human_only=False, exposure=("cli", "gui", "mcp")):
    return {
        "id": cid,
        "major": 1,
        "title": title,
        "description": "Synthetic command of the CORE.BUS.POL.PROJ test registry.",
        "params_schema": {
            "type": "object",
            "additionalProperties": False,
            "required": ["target"],
            "properties": {"target": {"type": "string"}, "count": {"type": "integer"}, "dry": {"type": "boolean"}},
        },
        "result_schema": {"type": "object"},
        "side_effect": side_effect,
        "mcp_projection": projection,
        "determinism": "D0",
        "idempotency": "none",
        "fs_effect": "none",
        "undo": "head_move",
        "preconditions": [],
        "effects": ["TestDone"],
        "algorithms": [],
        "exposure": list(exposure),
        "lifecycle": {"state": "active", "since": "2026-11-20"},
        "human_only": human_only,
        "network": "none",
        "web": "available",
        "cli_bare": False,
    }


REGISTRY = {
    "commands": [
        command("test.run_exposed", "Exposed command", "W1", "exposed"),
        command("test.run_request", "Request-only command", "W2", "request_only"),
        command("test.run_never", "Never-projected command", "R0", "never"),
        command("test.adopt_human", "Human-only command", "W2", "exposed", human_only=True),
        command("test.run_local", "Command without an MCP surface", "W1", "exposed", exposure=("cli", "gui")),
    ]
}

# Expected projection outcome per channel, by hand from REQ-001/REQ-002: off MCP every
# command passes this check; on MCP exposed runs, request_only and human_only file a
# pending request, never (or no mcp surface) is not_found/unknown_command,
# exactly as a command that does not exist (D-035).
ON_MCP = {
    "test.run_exposed": "run",
    "test.run_request": "pending",
    "test.run_never": "not_found/unknown_command",
    "test.adopt_human": "pending",
    "test.run_local": "not_found/unknown_command",
}
MCP_CHANNELS = ["mcp", "server_mcp"]
OTHER_CHANNELS = ["gui", "cli", "python", "control", "server_session", "server_service", "service", "device",
                  "internal"]


def projection_cases():
    out = []
    for c in REGISTRY["commands"]:
        for ch in MCP_CHANNELS:
            out.append({"channel": ch, "command": c["id"] + "@1", "expect": ON_MCP[c["id"]]})
        for ch in OTHER_CHANNELS:
            out.append({"channel": ch, "command": c["id"] + "@1", "expect": "run"})
    return out


PARAMS = [
    {"target": "plane-1"},
    {"target": "plane-1", "count": 3},
    {"count": 3, "target": "plane-1", "dry": True},
    {"target": "plane-2", "count": 3},
    {"target": "Ebene é"},
]

# The parameters filed with the pending request R in AC-1/AC-3/AC-4, and approvals
# against it, by hand from REQ-003/REQ-004.
FILED = {"target": "plane-1", "count": 3}
APPROVALS = [
    {"name": "same parameters by a human", "approver": "gui", "command": "test.run_request@1",
     "params": {"count": 3, "target": "plane-1"}, "expect": "approve"},
    {"name": "one field differs", "approver": "gui", "command": "test.run_request@1",
     "params": {"target": "plane-2", "count": 3}, "expect": "policy/bad_approval"},
    {"name": "extra field", "approver": "gui", "command": "test.run_request@1",
     "params": {"target": "plane-1", "count": 3, "dry": True}, "expect": "policy/bad_approval"},
    {"name": "missing field", "approver": "gui", "command": "test.run_request@1",
     "params": {"target": "plane-1"}, "expect": "policy/bad_approval"},
    {"name": "other command same parameters", "approver": "gui", "command": "test.adopt_human@1",
     "params": {"target": "plane-1", "count": 3}, "expect": "policy/bad_approval"},
    {"name": "agent sets approval_of", "approver": "mcp", "command": "test.run_request@1",
     "params": {"target": "plane-1", "count": 3}, "expect": "policy/bad_approval"},
    {"name": "server agent sets approval_of", "approver": "server_mcp", "command": "test.run_request@1",
     "params": {"target": "plane-1", "count": 3}, "expect": "policy/bad_approval"},
    {"name": "script sets approval_of", "approver": "cli", "command": "test.run_request@1",
     "params": {"target": "plane-1", "count": 3}, "expect": "policy/bad_approval"},
    {"name": "service sets approval_of", "approver": "service", "command": "test.run_request@1",
     "params": {"target": "plane-1", "count": 3}, "expect": "policy/bad_approval"},
    {"name": "server user sets approval_of", "approver": "server_session", "command": "test.run_request@1",
     "params": {"target": "plane-1", "count": 3}, "expect": "policy/bad_approval"},
]

REJECT_REASON = "wrong datum"


def cases():
    return {
        "node": "CORE.BUS.POL.PROJ",
        "projection": projection_cases(),
        "commitments": [{"params": p, "commitment": commitment(p)} for p in PARAMS],
        "filed": {"command": "test.run_request@1", "params": FILED, "commitment": commitment(FILED)},
        "approvals": APPROVALS,
        # AC-6: the rejection reason and its commitment (digest of the reason as a JSON string).
        "reject": {"reason": REJECT_REASON, "commitment": commitment(REJECT_REASON)},
    }


def render(v):
    return json.dumps(v, indent=1, sort_keys=True, ensure_ascii=False) + "\n"


def main():
    outputs = {HERE / "registry.json": render(REGISTRY), HERE / "cases.json": render(cases())}
    check = "--check" in sys.argv[1:]
    bad = 0
    for path, text in outputs.items():
        if check:
            have = path.read_text(encoding="utf-8") if path.exists() else None
            if have != text:
                print(f"STALE {path.name}: run make_cases.py")
                bad += 1
        else:
            path.write_text(text, encoding="utf-8", newline="\n")
    return 1 if bad else 0


if __name__ == "__main__":
    sys.exit(main())
