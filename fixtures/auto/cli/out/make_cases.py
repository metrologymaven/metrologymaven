#!/usr/bin/env python3
"""Generate fixtures/auto/cli/out/cases.json (AUTO.CLI.OUT AC-1 to AC-4).

Each case scripts the fake bus for `fid demo show-thing`: diagnostics it emits while
running (warnings, progress, logs) and the final envelope it replies with. Expected
exit codes are written by hand from REQ-AUTO.CLI.OUT-002 (0 committed, computed or
dry_run; 1 internal error; 2 usage error; 3 refused by side-effect policy; 4 command
failed; 5 cancelled; 6 not_run timeout; 7 no_reply timeout; 8 request filed for a
human), not computed by the Rust code. `stdout_lines` is the number of stdout lines
under --json; `human` lists substrings REQ-AUTO.CLI.OUT-004 requires in the
human-readable output. CC-BY-4.0 project data.
Run:  python fixtures/auto/cli/out/make_cases.py [--check]
"""

import json
import sys
from pathlib import Path

CMD = "demo.show_thing@1"
HASH = "sha256:" + "ab" * 32
NOISE = [["warn", "calibration is 3 days old"], ["progress", "probing 1/2"], ["log", "bus: sent"],
         ["progress", "probing 2/2"], ["warn", "second warning"]]
cases = []


def env(outcome, **extra):
    e = {"command_id": CMD, "envelope_version": 1, "outcome": outcome}
    e.update(extra)
    return e


def failed(code, message):
    return env("failed", error={"code": code, "message": message})


def case(name, reply, exit_code, human=(), emit=(), stdout_lines=1):
    cases.append({"name": name, "emit": [list(x) for x in emit], "reply": reply,
                  "expect": {"exit": exit_code, "stdout_lines": stdout_lines, "human": list(human)}})


# AC-1: the R0 fixture command computes; exit 0.
case("computed", env("computed", result={"thing": "probe-1"}), 0, human=[CMD, "computed"])
# AC-4: warnings and progress under --json never reach stdout.
case("computed with warnings and progress", env("computed", result={"thing": "probe-1"}), 0,
     human=["computed"], emit=NOISE)
# AC-3: one script per outcome in the table.
case("committed", env("committed", journal_entry_hash=HASH), 0, human=["committed", HASH])
case("dry_run", env("dry_run", result={"would_change": 1}), 0, human=["dry_run"])
case("internal error", failed("internal_error", "kernel crashed"), 1, human=["internal_error", "kernel crashed"])
case("usage error", failed("usage_error", "missing --plane"), 2, human=["usage_error", "missing --plane"])
case("dry run unsupported", failed("dry_run_unsupported", "no dry run"), 2, human=["dry_run_unsupported"])
# AC-2: policy refusal.
case("policy refused", failed("policy_refused", "M1 needs the approval station"), 3,
     human=["policy_refused", "M1 needs the approval station"])
case("raw path refused", failed("raw_path_refused", "raw path"), 3, human=["raw_path_refused"])
case("command failed", failed("precondition_failed", "no alignment"), 4,
     human=["precondition_failed", "no alignment"], emit=NOISE)
case("validation error", failed("validation_error", "bad value"), 4, human=["validation_error"])
case("timeout error", failed("timeout", "probe did not answer"), 4, human=["timeout", "probe did not answer"])
case("root escape", failed("root_escape", "path leaves the project"), 4, human=["root_escape"])
case("cancelled", env("cancelled"), 5, human=["cancelled"])
case("failed with cancelled", failed("cancelled", "operator stopped it"), 5, human=["cancelled", "operator stopped it"])
# Judgement call stated in the PR: the CLI waits for a final state (AUTO.CLI.JOB), so
# ending on `accepted` is an internal error, never success (REQ-AUTO.CLI-006).
case("accepted", env("accepted", job_id="job-7"), 1, human=["accepted", "job-7"])
case("not_run timeout", env("not_run"), 6, human=["not_run"])
case("no_reply timeout", env("no_reply"), 7, human=["no_reply"])
case("request filed", env("request_filed", request_id="req-42"), 8, human=["request_filed", "req-42"])
# Design, Errors: an envelope that cannot be encoded (committed without a journal hash)
# is exit 1, nothing on stdout, an internal_error envelope on stderr.
case("unencodable envelope", env("committed"), 1, stdout_lines=0)

out = json.dumps({"command_id": CMD, "cases": cases}, indent=1, sort_keys=True) + "\n"
path = Path(__file__).with_name("cases.json")
if "--check" in sys.argv:
    if path.read_text(encoding="utf-8") != out:
        sys.exit(f"{path} is stale; run {__file__}")
else:
    path.write_text(out, encoding="utf-8", newline="\n")
