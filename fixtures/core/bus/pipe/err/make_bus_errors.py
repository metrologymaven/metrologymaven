#!/usr/bin/env python3
"""Reference generator for the closed bus error code set (CORE.BUS.PIPE.ERR).

Writes, independently of the Rust crate `fid-bus`:
  fixtures/core/bus/pipe/err/bus-errors.v1.json    frozen v1 baseline (canonical JSON + newline)
  fixtures/core/bus/pipe/err/bus-errors.v1.digest  sha256: digest of the canonical bytes
  schema/bus-errors.json                           the currently published file (same bytes as v1 today)
  schema/bus-error.schema.json                     JSON Schema for one serialised BusError

The codes come from REQ-CORE.BUS.PIPE.ERR-001. The reasons are hand-labelled from the
reasons that sibling spec nodes already name for each bus code (cited per code below);
where no node names one, the reason is this leaf's own choice and is listed in the PR.

If `jsonschema` is installed, every (code, reason) pair is also serialised as
{code, reason, detail} and validated against the schema, plus some negative cases.

All keys and strings are ASCII, so Python's sorted keys equal RFC 8785 order.
CC-BY-4.0 project data. Run:  python fixtures/core/bus/pipe/err/make_bus_errors.py
"""

import hashlib
import json
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
    ],
    # CORE.BUS.POL.CLASS, .PROJ, .FS, .EGRESS; TRUST.PROFILE.GATE.ASSIST; replay targets.
    "policy": [
        "class_forbidden", "human_only", "not_job_owner", "not_on_mcp", "bad_approval", "not_requester",
        "raw_path_refused", "outside_root", "bad_root", "extension_refused", "would_overwrite", "bad_handle",
        "bad_destination", "no_approval", "target_protected", "no_assertion", "profile.agent_write_forbidden",
    ],
    # CORE.BUS.PIPE.VALID (`false`, `step_bound_exceeded`) and domain nodes.
    "precondition": [
        "false", "step_bound_exceeded", "capability_missing", "divergent_step", "not_supersedable",
        "not_undoable", "nothing_to_redo", "project_locked", "released_record", "stage_active",
    ],
    # CORE.BUS.ROUTE-004 (expected_head) and CORE.BUS.PIPE.STAGE-005 (head moved during a job).
    "conflict": ["expected_head_mismatch", "head_moved"],
    # CORE.BUS.PIPE-002 (unknown ID, unknown or removed major) and lookups by ID.
    "not_found": [
        "unknown_command", "unknown_major", "removed_major",
        "branch", "entity", "job", "request", "stream",
    ],
    # CORE.BUS.PIPE-003.
    "hidden_command": ["hidden_major"],
    # CORE.BUS.PIPE-007.
    "undeclared_effect": ["event_type"],
    # CORE.JOB cancellation.
    "cancelled": ["requested", "shutdown"],
    # CORE.BUS.PRIN-008.
    "unauthenticated": ["no_identity"],
    # CORE.BUS.ROUTE-007.
    "idempotency_mismatch": ["args_digest_differs"],
    # CORE.BUS.PRIN Errors.
    "upgrade_refused": ["channel", "declined", "timeout", "user_mismatch"],
    # Failures that are bugs or environment faults, never caller mistakes.
    "internal": ["handler_failed", "journal_unavailable", "invariant_violated"],
}

assert len(CODES) == 12
for code, reasons in CODES.items():
    assert len(set(reasons)) == len(reasons), code


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
        {"properties": {"code": {"const": c}, "reason": {"enum": sorted(CODES[c])}}}
        for c in sorted(CODES)
    ],
}


def write(path: Path, data: bytes) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_bytes(data)
    print("wrote", path.relative_to(ROOT).as_posix())


write(HERE / "bus-errors.v1.json", catalog_bytes + b"\n")
write(HERE / "bus-errors.v1.digest", digest.encode("ascii") + b"\n")
write(ROOT / "schema" / "bus-errors.json", catalog_bytes + b"\n")
write(ROOT / "schema" / "bus-error.schema.json", canon(schema) + b"\n")
print(digest)

try:
    from jsonschema import Draft202012Validator
except ImportError:
    print("jsonschema not installed; skipped sample validation")
else:
    Draft202012Validator.check_schema(schema)
    v = Draft202012Validator(schema)
    for c, reasons in CODES.items():
        for r in reasons:
            sample = {"code": c, "reason": r, "detail": "x" * MAX_DETAIL_CHARS}
            assert v.is_valid(sample), sample
    bad = [
        {"code": "conflict", "reason": "schema", "detail": ""},
        {"code": "head_conflict", "reason": "head_moved", "detail": ""},
        {"code": "conflict", "reason": "head_moved"},
        {"code": "conflict", "reason": "head_moved", "detail": "x" * (MAX_DETAIL_CHARS + 1)},
        {"code": "conflict", "reason": "head_moved", "detail": "", "extra": 1},
    ]
    for b in bad:
        assert not v.is_valid(b), b
    print("schema samples ok")
