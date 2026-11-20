#!/usr/bin/env python3
"""Reference generator for the closed bus error code set (CORE.BUS.PIPE.ERR).

Independently of the Rust crate `fid-bus`, a plain run writes the currently
published files:
  schema/bus-errors.json         the published catalog (canonical JSON + newline)
  schema/bus-error.schema.json   JSON Schema for one serialised BusError

and checks that the catalog keeps every code and reason of every frozen baseline
fixtures/core/bus/pipe/err/bus-errors.vN.json (REQ-003: additions only).

Frozen baselines are never overwritten. A new baseline is written only when asked:
  python fixtures/core/bus/pipe/err/make_bus_errors.py --new-version N
writes bus-errors.vN.json and bus-errors.vN.digest, and refuses if either file
exists or if N is not the next version after the newest existing baseline.

The codes come from REQ-CORE.BUS.PIPE.ERR-001. The reasons are hand-labelled from the
reasons that sibling spec nodes already name for each bus code (cited per code below);
where no node names one, the reason is this leaf's own choice and is listed in the PR.

If `jsonschema` is installed, every (code, reason) pair is also serialised as
{code, reason, detail} and validated against the schema, plus some negative cases.

All keys and strings are ASCII, so Python's sorted keys equal RFC 8785 order.
CC-BY-4.0 project data.
"""

import argparse
import hashlib
import json
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[5]
HERE = Path(__file__).resolve().parent

MAX_DETAIL_CHARS = 2000

CODES = {
    # CORE.BUS.PIPE.VALID, CORE.BUS.UNIT, CORE.BUS.PIPE.HANDLER, CORE.BUS.POL.EGRESS and domain nodes.
    "validation": [
        "schema", "bad_reference", "unit_mismatch", "literal_out_of_range", "literal_syntax",
        "input_limit", "effect_limit", "bad_artifact", "bad_branch_name", "bad_stage",
        "hierarchy", "label", "name", "unregistered_type", "bad_token", "stale_token",
        "bad_range",
    ],
    # CORE.BUS.POL.CLASS, .PROJ, .FS, .EGRESS; TRUST.PROFILE.GATE.ASSIST; replay targets.
    "policy": [
        "class_forbidden", "human_only", "not_job_owner", "not_on_mcp", "bad_approval", "not_requester",
        "raw_path_refused", "outside_root", "bad_root", "extension_refused", "would_overwrite", "bad_handle",
        "bad_destination", "no_approval", "target_protected", "no_assertion", "agent_write_forbidden",
        "system_only",
    ],
    # CORE.BUS.PIPE.VALID (`false`, `step_bound_exceeded`) and domain nodes. v3:
    # `invalid_transition`, a commit's event asks for an element state change the tree does
    # not allow (CORE.PRJ.TREE.MODEL REQ-003; the commit is aborted).
    "precondition": [
        "false", "step_bound_exceeded", "capability_missing", "divergent_step", "not_supersedable",
        "not_undoable", "nothing_to_redo", "project_locked", "released_record", "stage_active",
        "invalid_transition",
    ],
    # CORE.BUS.ROUTE-004 (expected_head), CORE.BUS.PIPE.STAGE-005 (head moved during a job),
    # writer lock held, expected content digest differs; v2: `id_collision`, an entity ID
    # (imported or drawn) already in the project (CORE.PRJ.CONT.ID REQ-004, CORE.PRJ.CONT.HIER,
    # GEO.TYPES.REG).
    "conflict": ["expected_head_mismatch", "head_moved", "writer_locked", "expected_digest_mismatch", "id_collision"],
    # CORE.BUS.PIPE-002 (unknown ID, unknown or removed major) and lookups by ID.
    "not_found": [
        "unknown_command", "unknown_major", "removed_major",
        "unknown_branch", "unknown_entity", "unknown_job", "unknown_request", "unknown_stream",
    ],
    # CORE.BUS.PIPE-003.
    "hidden_command": ["hidden_major"],
    # CORE.BUS.PIPE-007.
    "undeclared_effect": ["undeclared_event_type"],
    # CORE.JOB cancellation; `timeout`: the command's time budget ran out and nothing was committed.
    "cancelled": ["requested", "shutdown", "timeout"],
    # CORE.BUS.PRIN-008.
    "unauthenticated": ["no_identity"],
    # CORE.BUS.ROUTE-007.
    "idempotency_mismatch": ["args_digest_differs"],
    # CORE.BUS.PRIN Errors.
    "upgrade_refused": ["channel", "declined", "timeout", "user_mismatch"],
    # Failures that are bugs or environment faults, never caller mistakes.
    "internal": ["handler_failed", "journal_unavailable", "invariant_violated", "storage"],
}

assert len(CODES) == 12
for _code, _reasons in CODES.items():
    assert len(set(_reasons)) == len(_reasons), _code
    for _r in _reasons:
        assert re.fullmatch(r"[a-z][a-z0-9_]*", _r), _r


def canon(obj) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


catalog = {"schema_version": 1, "codes": {c: sorted(r) for c, r in CODES.items()}}
catalog_bytes = canon(catalog)
digest = "sha256:" + hashlib.sha256(catalog_bytes).hexdigest()

schema = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "https://metrologymaven.io/schema/bus-error.schema.json",
    "title": "Metrology Maven bus error, schema version 1",
    "type": "object",
    "additionalProperties": False,
    "required": ["code", "detail", "reason"],
    "properties": {
        "code": {"enum": sorted(CODES)},
        "reason": {"type": "string"},
        "detail": {"type": "string", "maxLength": MAX_DETAIL_CHARS},
    },
    "oneOf": [
        {"properties": {"code": {"const": c}, "reason": {"enum": sorted(CODES[c])}}} for c in sorted(CODES)
    ],
}


def write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    print("wrote", path.relative_to(ROOT).as_posix())


def baselines() -> dict:
    out = {}
    for f in HERE.glob("bus-errors.v*.json"):
        m = re.fullmatch(r"bus-errors\.v([1-9][0-9]*)\.json", f.name)
        if m:
            out[int(m.group(1))] = f
    return out


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--new-version", type=int, help="write a new frozen baseline vN (never overwrites)")
    args = ap.parse_args()

    # REQ-003: the current catalog keeps every code and reason of every frozen baseline.
    bad = 0
    for n, f in sorted(baselines().items()):
        old = json.loads(f.read_text(encoding="utf-8"))["codes"]
        for code, reasons in old.items():
            if code not in CODES:
                bad += 1
                print(f"FAIL v{n}: code `{code}` was removed or renamed")
                continue
            for r in reasons:
                if r not in CODES[code]:
                    bad += 1
                    print(f"FAIL v{n}: reason `{r}` of code `{code}` was removed or renamed")
    if bad:
        return 1

    if args.new_version is not None:
        n = args.new_version
        existing = baselines()
        want = max(existing, default=0) + 1
        jf, df = HERE / f"bus-errors.v{n}.json", HERE / f"bus-errors.v{n}.digest"
        if jf.exists() or df.exists():
            print(f"REFUSED: baseline v{n} exists and is frozen; never overwrite it")
            return 1
        if n != want:
            print(f"REFUSED: the next baseline version is v{want}, not v{n}")
            return 1
        write(jf, catalog_bytes + b"\n")
        write(df, digest.encode("ascii") + b"\n")

    write(ROOT / "schema" / "bus-errors.json", catalog_bytes + b"\n")
    write(ROOT / "schema" / "bus-error.schema.json", canon(schema) + b"\n")
    print(digest)

    try:
        from jsonschema import Draft202012Validator
    except ImportError:
        print("jsonschema not installed; skipped sample validation")
        return 0
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    for c, reasons in CODES.items():
        for r in reasons:
            sample = {"code": c, "reason": r, "detail": "x" * MAX_DETAIL_CHARS}
            assert v.is_valid(sample), sample
    negatives = [
        {"code": "conflict", "reason": "schema", "detail": ""},
        {"code": "head_conflict", "reason": "head_moved", "detail": ""},
        {"code": "conflict", "reason": "head_moved"},
        {"code": "conflict", "reason": "head_moved", "detail": "x" * (MAX_DETAIL_CHARS + 1)},
        {"code": "conflict", "reason": "head_moved", "detail": "", "extra": 1},
    ]
    for b in negatives:
        assert not v.is_valid(b), b
    print("schema samples ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
