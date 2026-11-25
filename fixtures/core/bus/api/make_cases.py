#!/usr/bin/env python3
"""Generate fixtures/core/bus/api/errors.json (CORE.BUS.API AC-3).

Every case's expected outcome is labelled by hand below from REQ-CORE.BUS.API-007: a code and a
reason are 1 to 64 bytes of [a-z0-9_], a detail is at most 2,000 characters (Unicode scalar
values). The valid code and reason pairs are copied from schema/bus-errors.json (version 1);
the script checks that each still appears there. A small independent reference check of the
rule must agree with every hand label, or the script fails.

CC-BY-4.0 project data.
Run:  python fixtures/core/bus/api/make_cases.py [--check]
"""

import json
import re
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
OUT = HERE / "errors.json"
CODE = re.compile(r"[a-z0-9_]{1,64}")

STAGES = ["session", "resolve", "validate", "authorise", "precondition", "route", "handler", "effects", "commit"]

# (name, code, reason, detail, stage, expected) -- expected is "ok" or "invalid_api_error".
OK, BAD = "ok", "invalid_api_error"

# Valid pairs copied from schema/bus-errors.json v1, one per code, each with a plausible stage.
PAIRS = [
    ("cancelled", "requested", "handler"),
    ("conflict", "head_moved", "commit"),
    ("hidden_command", "hidden_major", "resolve"),
    ("idempotency_mismatch", "args_digest_differs", "route"),
    ("internal", "invariant_violated", "handler"),
    ("not_found", "unknown_command", "resolve"),
    ("policy", "raw_path_refused", "authorise"),
    ("precondition", "false", "precondition"),
    ("unauthenticated", "no_identity", "session"),
    ("undeclared_effect", "undeclared_event_type", "effects"),
    ("upgrade_refused", "user_mismatch", "session"),
    ("validation", "unit_mismatch", "validate"),
]

CASES = [(f"bus pair {c}/{r}", c, r, f"detail for {c}/{r}", s, OK) for c, r, s in PAIRS]
CASES += [
    ("empty detail", "upgrade_refused", "declined", "", "session", OK),
    ("detail no_presence_provider", "upgrade_refused", "declined", "no_presence_provider", "session", OK),
    ("detail with quotes and newline", "not_found", "unknown_entity", 'no "x"\nhere', "handler", OK),
    ("code of 64 bytes", "c" * 64, "false", "", "route", OK),
    ("reason of 64 bytes", "policy", "r" * 64, "", "authorise", OK),
    ("digits and underscores", "e_404", "_0_", "", "handler", OK),
    ("detail of 2000 ascii characters", "internal", "storage", "a" * 2000, "commit", OK),
    ("detail of 2000 characters ending multi-byte", "internal", "storage", "a" * 1999 + "é", "commit", OK),
    ("detail of 2000 four-byte characters", "internal", "storage", "\U0001F600" * 2000, "commit", OK),
    ("empty code", "", "false", "", "precondition", BAD),
    ("empty reason", "precondition", "", "", "precondition", BAD),
    ("code of 65 bytes", "c" * 65, "false", "", "route", BAD),
    ("reason of 65 bytes", "policy", "r" * 65, "", "authorise", BAD),
    ("upper-case code", "Policy", "raw_path_refused", "", "authorise", BAD),
    ("upper-case reason", "policy", "RAW_PATH_REFUSED", "", "authorise", BAD),
    ("reason with hyphen", "policy", "raw-path-refused", "", "authorise", BAD),
    ("code with space", "not found", "unknown_command", "", "resolve", BAD),
    ("code with dot", "policy.x", "false", "", "authorise", BAD),
    ("non-ascii letter in reason", "validation", "schéma", "", "validate", BAD),
    ("detail of 2001 ascii characters", "internal", "storage", "a" * 2001, "commit", BAD),
    ("detail of 2001 characters, multi-byte at the limit", "internal", "storage", "a" * 2000 + "é", "commit", BAD),
]


def reference(code, reason, detail):
    """Independent statement of REQ-CORE.BUS.API-007."""
    if CODE.fullmatch(code) and CODE.fullmatch(reason) and len(detail) <= 2000:
        return OK
    return BAD


def problems():
    bad = []
    catalog = json.loads((ROOT / "schema" / "bus-errors.json").read_text(encoding="utf-8"))["codes"]
    for c, r, _ in PAIRS:
        if r not in catalog.get(c, []):
            bad.append(f"{c}/{r} is not in schema/bus-errors.json")
    names = set()
    for name, code, reason, detail, stage, expect in CASES:
        if name in names:
            bad.append(f"duplicate case name {name!r}")
        names.add(name)
        if stage not in STAGES:
            bad.append(f"{name}: unknown stage {stage!r}")
        if reference(code, reason, detail) != expect:
            bad.append(f"{name}: hand label {expect} disagrees with the reference rule")
    if set(s for *_, s, _ in CASES) != set(STAGES):
        bad.append("not every stage is used")
    return bad


def render():
    cases = [
        {"name": n, "code": c, "reason": r, "detail": d, "stage": s, "expect": e} for n, c, r, d, s, e in CASES
    ]
    doc = {
        "about": "CORE.BUS.API AC-3: ApiError construction cases, hand-labelled from REQ-CORE.BUS.API-007; "
        "made by make_cases.py. CC-BY-4.0.",
        "cases": cases,
        "schema_version": 1,
    }
    return json.dumps(doc, indent=1, ensure_ascii=False, sort_keys=True) + "\n"


def main():
    bad = problems()
    if bad:
        print("\n".join(bad))
        return 1
    text = render()
    if "--check" in sys.argv[1:]:
        if not OUT.exists() or OUT.read_text(encoding="utf-8") != text:
            print("stale: errors.json")
            return 1
        print(f"ok: {len(CASES)} cases; hand labels agree with the reference rule")
        return 0
    OUT.write_text(text, encoding="utf-8", newline="\n")
    print(f"wrote errors.json, {len(CASES)} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
