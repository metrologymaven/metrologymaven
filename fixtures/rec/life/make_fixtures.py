#!/usr/bin/env python3
"""Generate the fixtures of spec node AUTO.REC.LIFE.

Independent reference: nothing here runs the Rust code.

- `fixtures/program/agent-steps.json` (project-authored): a draft program in the AUTO.PROG.FMT
  canonical form (RFC 8785: keys sorted, no whitespace; every string is ASCII, so sorting by
  code point equals sorting by UTF-16 code units, and json.dumps without ASCII escaping is the
  JCS form). Step H1 is written by a person, A1 and A2 by an agent.
- `fixtures/script/hello/main.py`: a one-file script bundle.
- `minimal-edited.json`: `minimal.json` with one comment step added, the content an edit
  makes in a new draft opened from a released `minimal.json` (version 3 of the lineage).
- `expected.json`: digests from hashlib over the canonical bytes; the canonical script bundle
  form is the one documented in crates/fid-program-rec/src/life/bundle.rs; the expected events
  and results are hand-labelled from spec/auto/rec/life/NODE.md (REQ-001..006, AC-1..AC-7) and
  the event format in crates/fid-program-rec/src/life/event.rs.

`fixtures/program/valid/minimal.json` is read as input (written by
fixtures/program/make_fixtures.py) and hashed here.

CC-BY-4.0 project data. Run:  python fixtures/rec/life/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
FIX = HERE.parent.parent

MINIMAL_ID = "018f0000-0000-7a0b-9f63-cc74b8b713d5"
AGENT_PROGRAM = "019a0000-0000-7000-8000-00000000c000"
H1 = "019a0000-0001-7000-8000-00000000c001"  # person
A1 = "019a0000-0002-7000-8000-00000000c002"  # agent
A2 = "019a0000-0003-7000-8000-00000000c003"  # agent
HUMAN = "alice"
BRANCH = "main"
SCRIPT = "hello"
EDIT_STEP = "019a0000-0009-7000-8000-00000000d009"
HELLO_PY = b'print("hello from a Metrology Maven script")\n'


def canon(v):
    out = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert out.isascii(), "fixture strings must be ASCII"
    return out.encode("utf-8")


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def agent_steps():
    return {
        "format": "fid-program",
        "format_version": 1,
        "main": [
            {"kind": "comment", "step_id": H1, "text": "Measure the datum plane (written by a person)."},
            {
                "args": {"points": {"literal": "pc-21"}},
                "command": "feature.fit_plane",
                "kind": "invoke",
                "major": 1,
                "step_id": A1,
            },
            {"kind": "comment", "step_id": A2, "text": "Report the flatness (written by an agent)."},
        ],
        "meta": {"title": "Draft with two agent steps"},
        "params": [],
        "program_id": AGENT_PROGRAM,
        "subroutines": [],
        "variables": [],
    }


def bundle(files):
    return canon({"files": files, "format": "fid-script-bundle", "format_version": 1})


def event(command, artifact, d, state, version, content=None):
    e = {
        "artifact": artifact,
        "branch": BRANCH,
        "by": HUMAN,
        "command": command,
        "digest": d,
        "state": state,
        "version": version,
    }
    if content is not None:
        # Register events carry the frozen canonical bytes as a JSON string (event.rs).
        e["content"] = content.decode("ascii")
    return canon(e).decode("ascii")


def result(d, state, version):
    return canon({"digest": d, "state": state, "version": version}).decode("ascii")


def build():
    minimal = (FIX / "program" / "valid" / "minimal.json").read_bytes()
    assert json.loads(minimal)["program_id"] == MINIMAL_ID
    md = digest(minimal)
    edited_obj = json.loads(minimal)
    edited_obj["main"].append({"kind": "comment", "step_id": EDIT_STEP, "text": "Added in a new draft."})
    edited = canon(edited_obj)
    agent = canon(agent_steps())
    sb = bundle({"main.py": HELLO_PY.decode("ascii")})
    sd = digest(sb)
    expected = {
        "program": MINIMAL_ID,
        "minimal_digest": md,
        "edited_digest": digest(edited),
        "human": HUMAN,
        "branch": BRANCH,
        "ac1_register_event": event("program.register@1", MINIMAL_ID, md, "registered", 1, minimal),
        "ac1_release_event": event("program.release@1", MINIMAL_ID, md, "released", 1),
        "ac1_register_result": result(md, "registered", 1),
        "ac1_release_result": result(md, "released", 1),
        "ac5_deprecate_result": result(md, "deprecated", 1),
        "agent_program": AGENT_PROGRAM,
        "agent_digest": digest(agent),
        "agent_steps": {"H1": H1, "A1": A1, "A2": A2},
        "ac4_unadopted": [A1, A2],
        "script": SCRIPT,
        "script_digest": sd,
        "ac6_register_event": event("script.register@1", SCRIPT, sd, "registered", 1, sb),
        "ac6_release_event": event("script.release@1", SCRIPT, sd, "released", 1),
        "ac6_register_result": result(sd, "registered", 1),
        "ac6_release_result": result(sd, "released", 1),
        "ac6_deprecate_result": result(sd, "deprecated", 1),
    }
    return {
        FIX / "program" / "agent-steps.json": agent,
        FIX / "script" / "hello" / "main.py": HELLO_PY,
        HERE / "minimal-edited.json": edited,
        HERE / "expected.json": json.dumps(expected, indent=2, sort_keys=True).encode("utf-8") + b"\n",
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for path, data in build().items():
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path}")
                bad += 1
        else:
            path.parent.mkdir(parents=True, exist_ok=True)
            path.write_bytes(data)
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
