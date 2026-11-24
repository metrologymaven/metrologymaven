#!/usr/bin/env python3
"""Independent fixture generator for spec node FLOW.RUNNER.UNASSIST.

Generates synthetic golden journals under fixtures/flow/unassist/:
  - human-only.jrn:      20 human and script entries (AC-1, 0 violations)
  - one-agent.jrn:       20 entries with entry index 7 attributed to an agent (AC-2)
  - unattributed.jrn:    10 entries with entry index 4 missing principal (AC-3)
  - mcp-session.jrn:     session opened on MCP channel with human + upgraded entries
  - agent-branch.jrn:    human command targeting an agent/ branch
  - activity-agent.jsonl: Activity Log stream containing one agent entry

Run with `--check` to verify committed fixtures match this generator byte-for-byte.
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent


def _digest(label: str) -> str:
    return "sha256:" + hashlib.sha256(label.encode("utf-8")).hexdigest()


def _canon(obj: dict) -> str:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False)


def _session_entry(seq: int, principal_kind: str, channel: str, branch: str = "main") -> dict:
    return {
        "schema_version": 1,
        "kind": "session_header",
        "stream": "record",
        "seq": str(seq),
        "branch": branch,
        "principal": {"kind": principal_kind, "commit": _digest(f"{principal_kind}-sess-{seq}")},
        "session": {"session_id": f"sess-{seq}", "app_version": "0.1.0", "channel": channel},
    }


def _cmd_entry(
    seq: int,
    cmd_id: str,
    major: int,
    principal_kind: str | None,
    *,
    branch: str = "main",
    stream: str = "record",
) -> dict:
    entry: dict = {
        "schema_version": 1,
        "kind": "command",
        "stream": stream,
        "seq": str(seq),
        "branch": branch,
        "command": {"id": cmd_id, "major": major},
    }
    if principal_kind is not None:
        entry["principal"] = {"kind": principal_kind, "commit": _digest(f"{principal_kind}-{seq}")}
    return entry


def build_fixtures() -> dict[str, bytes]:
    # AC-1: 20 human and script entries (index 0..19)
    human_only = [_session_entry(1, "human", "cli")]
    for i in range(1, 20):
        kind = "human" if i % 2 == 1 else "script"
        cmd = "fit.ls_plane" if i % 3 == 0 else "align.run_bestfit"
        human_only.append(_cmd_entry(i + 1, cmd, 1, kind))

    # AC-2: 20 entries, index 7 attributed to an agent
    one_agent = [_session_entry(1, "human", "cli")]
    for i in range(1, 20):
        if i == 7:
            one_agent.append(_cmd_entry(i + 1, "fit.ls_circle", 1, "agent"))
        else:
            kind = "human" if i % 2 == 1 else "script"
            one_agent.append(_cmd_entry(i + 1, "fit.ls_plane", 1, kind))

    # AC-3: 10 entries, index 4 has no principal attribution
    unattributed = [_session_entry(1, "human", "gui")]
    for i in range(1, 10):
        if i == 4:
            unattributed.append(_cmd_entry(i + 1, "report.export_pdf", 1, None))
        else:
            unattributed.append(_cmd_entry(i + 1, "import.cad_step", 1, "human"))

    # MCP-opened session with human command + principal_upgrade + post-upgrade human command
    mcp_session = [
        _session_entry(1, "human", "mcp"),
        _cmd_entry(2, "fit.ls_plane", 1, "human"),
        {
            "schema_version": 1,
            "kind": "principal_upgrade",
            "stream": "record",
            "seq": "3",
            "branch": "main",
            "principal": {"kind": "human", "commit": _digest("human-up-3")},
            "upgrade": {"method": "gui_prompt"},
        },
        _cmd_entry(4, "report.export_pdf", 1, "human"),
    ]

    # Human command targeting an agent/ branch
    agent_branch = [
        _session_entry(1, "human", "cli"),
        _cmd_entry(2, "fit.ls_plane", 1, "human", branch="main"),
        _cmd_entry(3, "fit.ls_circle", 1, "human", branch="agent/claude/1"),
    ]

    # Activity Log stream with an agent read at index 1
    activity_agent = [
        _cmd_entry(1, "project.list_elements", 1, "human", stream="activity"),
        _cmd_entry(2, "project.describe_element", 1, "agent", stream="activity"),
        _cmd_entry(3, "project.list_elements", 1, "script", stream="activity"),
    ]

    def encode(entries: list[dict]) -> bytes:
        return ("\n".join(_canon(e) for e in entries) + "\n").encode("utf-8")

    return {
        "human-only.jrn": encode(human_only),
        "one-agent.jrn": encode(one_agent),
        "unattributed.jrn": encode(unattributed),
        "mcp-session.jrn": encode(mcp_session),
        "agent-branch.jrn": encode(agent_branch),
        "activity-agent.jsonl": encode(activity_agent),
    }


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="verify committed fixtures match generator")
    args = ap.parse_args()

    expected = build_fixtures()
    if args.check:
        ok = True
        for name, want in sorted(expected.items()):
            p = HERE / name
            if not p.is_file() or p.read_bytes() != want:
                print(f"ERROR: fixtures/flow/unassist/{name} differs from generator", file=sys.stderr)
                ok = False
        return 0 if ok else 1

    for name, data in sorted(expected.items()):
        (HERE / name).write_bytes(data)
    print(f"wrote {len(expected)} fixtures under {HERE}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
