#!/usr/bin/env python3
"""Generate the fixtures of spec node CORE.JRN.STREAM.ALOG.

Independent reference, written from the spec text alone (CONSTITUTION §3 Reads, D-031, C4,
and REQ-CORE.JRN.STREAM.ALOG-001..-003), not from the Rust code:

- routing.csv (AC-1): every side-effect class x principal locality x recording flag x
  Command Program step x Access Ledger present, with the expected stream:
  * W1, W2, W3, X1, I1, D1, M1, S1 and A0 commit to the Record Journal, whatever else holds
    (REQ-001; only reads are exempt from the Record Journal, D-031);
  * R0 and R1 by an external principal go to the Access Ledger when one exists, else to no
    stream (REQ-003); recording and program steps do not matter for them;
  * R0 and R1 by a local principal go to the Activity Log when a recording session is active
    or the call is a Command Program step, else to no stream (REQ-002); never the Record
    Journal.
  The expected column is `record`, `activity`, `access_ledger` or `none`.

- lifecycle.json (AC-4): job lifecycle changes and the SHA-256 of the RFC 8785 form of each
  change's transition object {command, error, from, job, principal_kind, to}. The Activity
  Log's lifecycle line names the job as an output entity with this digest. For objects of
  ASCII strings and nulls, RFC 8785 equals `json.dumps` with sorted keys and no whitespace.

CC-BY-4.0 project data.
Run:    python fixtures/core/jrn/stream/alog/make_fixtures.py
Check:  python fixtures/core/jrn/stream/alog/make_fixtures.py --check
"""

import hashlib
import io
import json
import sys
from pathlib import Path

HERE = Path(__file__).parent

CLASSES = ["R0", "R1", "W1", "W2", "W3", "X1", "I1", "D1", "M1", "S1", "A0"]
READS = {"R0", "R1"}
LOCALITIES = ["local", "external"]
BOOLS = ["false", "true"]


def expected_stream(cls, locality, recording, program_step, access_ledger):
    if cls not in READS:
        return "record"
    if locality == "external":
        return "access_ledger" if access_ledger == "true" else "none"
    if recording == "true" or program_step == "true":
        return "activity"
    return "none"


def routing_csv():
    out = io.StringIO()
    out.write("class,principal,recording,program_step,access_ledger,expected\n")
    for cls in CLASSES:
        for loc in LOCALITIES:
            for rec in BOOLS:
                for step in BOOLS:
                    for ledger in BOOLS:
                        exp = expected_stream(cls, loc, rec, step, ledger)
                        out.write(f"{cls},{loc},{rec},{step},{ledger},{exp}\n")
    return out.getvalue()


JOB = "01890a5d-ac96-774b-bcce-b302099a8057"
CHANGES = [
    {"job": JOB, "command": "test.sleep@1", "principal_kind": "human", "from": None, "to": "queued", "error": None},
    {"job": JOB, "command": "test.sleep@1", "principal_kind": "human", "from": "queued", "to": "running", "error": None},
    {"job": JOB, "command": "test.sleep@1", "principal_kind": "human", "from": "running", "to": "succeeded", "error": None},
    {"job": JOB, "command": "test.sleep@1", "principal_kind": "agent", "from": "running", "to": "failed", "error": "timeout"},
    {"job": JOB, "command": "test.sleep@1", "principal_kind": "script", "from": "queued", "to": "cancelled", "error": "cancelled"},
]


def canonical(obj):
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def lifecycle_json():
    cases = []
    for c in CHANGES:
        cases.append({"change": c, "digest": "sha256:" + hashlib.sha256(canonical(c)).hexdigest()})
    doc = {
        "description": "Job lifecycle transition objects and their SHA-256 over RFC 8785, from make_fixtures.py. CC-BY-4.0.",
        "cases": cases,
    }
    return json.dumps(doc, indent=2, sort_keys=True) + "\n"


def main():
    files = {"routing.csv": routing_csv(), "lifecycle.json": lifecycle_json()}
    check = "--check" in sys.argv[1:]
    bad = False
    for name, text in files.items():
        path = HERE / name
        if check:
            have = path.read_bytes().decode("utf-8") if path.exists() else None
            if have != text:
                print(f"{path}: out of date; run make_fixtures.py", file=sys.stderr)
                bad = True
        else:
            path.write_bytes(text.encode("utf-8"))
    if bad:
        sys.exit(1)
    rows = routing_csv().count("\n") - 1
    print(f"ok: routing.csv {rows} rows, lifecycle.json {len(CHANGES)} cases")


if __name__ == "__main__":
    main()
