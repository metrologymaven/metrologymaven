#!/usr/bin/env python3
"""Generate the fixtures of spec node AUTO.REC.ADOPT.

Independent reference: the draft programs are written here in the AUTO.PROG.FMT canonical
form (RFC 8785: keys sorted, no whitespace; every string is ASCII, so sorting by code point
equals sorting by UTF-16 code units, and json.dumps without ASCII escaping is the JCS form).
Digests come from hashlib. The expected adoption events and results are hand-labelled from
spec/auto/rec/adopt/NODE.md (REQ-003, AC-1, AC-5) and the event format in
crates/fid-program-rec/src/adopt/event.rs; nothing here runs the Rust code.

CC-BY-4.0 project data. Run:  python fixtures/rec/adopt/make_fixtures.py [--check]
"""

import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

PROGRAM = "019a0000-0000-7000-8000-00000000a000"
S1 = "019a0000-0001-7000-8000-00000000a001"  # human
S2 = "019a0000-0002-7000-8000-00000000a002"  # agent
S3 = "019a0000-0003-7000-8000-00000000a003"  # agent
HUMAN = "alice"
SCRIPT = "probe-sweep"
SCRIPT_V1 = "sha256:" + hashlib.sha256(b"probe-sweep script version 1\n").hexdigest()
SCRIPT_V2 = "sha256:" + hashlib.sha256(b"probe-sweep script version 2\n").hexdigest()


def canon(v):
    out = json.dumps(v, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    assert out.isascii(), "fixture strings must be ASCII"
    return out.encode("utf-8")


def program(s3_text):
    return {
        "format": "fid-program",
        "format_version": 1,
        "main": [
            {"kind": "comment", "step_id": S1, "text": "Measure the bore (written by a person)."},
            {
                "args": {"points": {"literal": "pc-17"}},
                "command": "feature.fit_cylinder",
                "kind": "invoke",
                "major": 1,
                "step_id": S2,
            },
            {"kind": "comment", "step_id": S3, "text": s3_text},
        ],
        "meta": {"title": "Mixed provenance draft"},
        "params": [],
        "program_id": PROGRAM,
        "subroutines": [],
        "variables": [],
    }


def digest(b):
    return "sha256:" + hashlib.sha256(b).hexdigest()


def build():
    draft = canon(program("Report the fitted diameter (written by an agent)."))
    edited = canon(program("Report the fitted diameter and roundness (edited after review)."))
    d = digest(draft)
    expected = {
        "program": PROGRAM,
        "steps": {"S1": S1, "S2": S2, "S3": S3},
        "human": HUMAN,
        "draft_digest": d,
        "edited_digest": digest(edited),
        "ac1_event": canon(
            {
                "branch": "working",
                "command": "program.adopt_steps@1",
                "human": HUMAN,
                "program": PROGRAM,
                "program_digest": d,
                "steps": [S2, S3],
            }
        ).decode("ascii"),
        "ac1_result": canon({"adopted": [S2, S3]}).decode("ascii"),
        "script": SCRIPT,
        "script_v1": SCRIPT_V1,
        "script_v2": SCRIPT_V2,
        "ac5_event": canon(
            {
                "branch": "working",
                "command": "script.adopt_version@1",
                "human": HUMAN,
                "script": SCRIPT,
                "version_digest": SCRIPT_V1,
            }
        ).decode("ascii"),
        "ac5_result": canon({"adopted": True}).decode("ascii"),
    }
    return {
        "draft-mixed.json": draft,
        "draft-edited.json": edited,
        "expected.json": json.dumps(expected, indent=2, sort_keys=True).encode("utf-8") + b"\n",
    }


def main():
    check = "--check" in sys.argv[1:]
    bad = 0
    for name, data in build().items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != data:
                print(f"stale: {path}")
                bad += 1
        else:
            path.write_bytes(data)
    if bad:
        sys.exit(1)


if __name__ == "__main__":
    main()
