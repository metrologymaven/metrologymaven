"""Writes schema/job-status.schema.json for CORE.JOB.CMD, independently of the Rust code.

The schema is written by hand below from the leaf's Design (id, command, state, progress,
stage, submitted_at, started_at, ended_at, error) and serialised as RFC 8785 canonical JSON
(all keys ASCII, so Python's code-point key order equals the UTF-16 order; every number is an
integer). The Rust test `schema_file_matches` checks that `job_status_schema()` produces the
same bytes. CC-BY-4.0.

Usage: python fixtures/core/job/cmd/make_job_status_schema.py [--check]
"""

import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[4]
OUT = ROOT / "schema" / "job-status.schema.json"

UUID = "^[0-9a-f]{8}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{4}-[0-9a-f]{12}$"
COMMAND_ID = "^[a-z0-9]([a-z0-9_.]*[a-z0-9])?@[0-9]{1,5}$"


def time_field(nullable):
    return {
        "type": ["number", "null"] if nullable else "number",
        "minimum": 0,
        "x-quantity": "time",
        "x-unit": "s",
    }


SCHEMA = {
    "$schema": "https://json-schema.org/draft/2020-12/schema",
    "$id": "https://metrologymaven.io/schema/job-status.schema.json",
    "title": "Metrology Maven job status",
    "type": "object",
    "additionalProperties": False,
    "required": [
        "command",
        "ended_at",
        "error",
        "id",
        "progress",
        "stage",
        "started_at",
        "state",
        "submitted_at",
    ],
    "properties": {
        "id": {"type": "string", "pattern": UUID},
        "command": {"type": "string", "pattern": COMMAND_ID},
        "state": {"enum": ["queued", "running", "succeeded", "failed", "cancelled"]},
        "progress": {"type": "number", "minimum": 0, "maximum": 1},
        # CORE.JOB.PROG keeps at most 128 characters of a stage label.
        "stage": {"type": "string", "maxLength": 128},
        "submitted_at": time_field(False),
        "started_at": time_field(True),
        "ended_at": time_field(True),
        "error": {"enum": ["timeout", "cancelled", "handler_failed", "not_found", "queue_full", None]},
    },
}


def render():
    text = json.dumps(SCHEMA, sort_keys=True, separators=(",", ":"), ensure_ascii=False)
    return (text + "\n").encode("utf-8")


def main():
    data = render()
    if "--check" in sys.argv[1:]:
        if OUT.read_bytes() != data:
            sys.exit(f"{OUT} is out of date; run {pathlib.Path(__file__).name}")
        return
    OUT.write_bytes(data)


if __name__ == "__main__":
    main()
