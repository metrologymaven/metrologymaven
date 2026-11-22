#!/usr/bin/env python3
"""Generate and verify built-in starter templates for RPT.MODEL.TMPL.STARTER.

Writes canonical RFC 8785 JSON template documents under crates/fid-report/templates/starter/:
- char-table.json       (tmpl:char-table)
- deviation-summary.json (tmpl:deviation-summary)
- multistage-table.json (tmpl:multistage-table)
- labels.en.json        (catalog keys -> English source text)

Run with --check in CI to verify all files match byte for byte and satisfy format invariants.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent.parent.parent.parent.parent
OUT_DIR = REPO / "crates/fid-report/templates/starter"

MEDIA_TYPE = "application/vnd.fid.report-template+json"

LABELS_EN: dict[str, str] = {
    "rpt.starter.char_table.heading": "Characteristic Table",
    "rpt.starter.char_table.title": "Characteristic Table",
    "rpt.starter.col.count": "Count",
    "rpt.starter.col.decision": "Decision",
    "rpt.starter.col.deviation": "Deviation",
    "rpt.starter.col.id": "Characteristic ID",
    "rpt.starter.col.limits": "Limits",
    "rpt.starter.col.measured": "Measured",
    "rpt.starter.col.nominal": "Nominal",
    "rpt.starter.col.outcome": "Outcome",
    "rpt.starter.col.stage_measured": "Stage Measured",
    "rpt.starter.deviation_summary.heading": "Deviation Summary",
    "rpt.starter.deviation_summary.nonconforming_heading": "Nonconforming Characteristics",
    "rpt.starter.deviation_summary.outcomes_heading": "Outcome Counts",
    "rpt.starter.deviation_summary.title": "Deviation Summary",
    "rpt.starter.multistage_table.heading": "Multi-Stage Comparison",
    "rpt.starter.multistage_table.title": "Multi-Stage Comparison Table",
}


def untrusted_text(text: str) -> dict[str, object]:
    return {"text": text, "untrusted": True}


def untrusted_val(value: object) -> dict[str, object]:
    return {"untrusted": True, "value": value}


def col(key: str, title_key: str) -> dict[str, str]:
    return {"key": key, "title_key": title_key}


def build_char_table() -> dict[str, object]:
    return {
        "clearance": None,
        "locked": False,
        "media_type": MEDIA_TYPE,
        "pages": [
            {
                "blocks": [
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0001-7000-8000-000000000101",
                        "kind": "text",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {"text": "rpt.starter.char_table.heading"}
                        ),
                        "repeat_header_rows": "0",
                        "slot": None,
                    },
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0001-7000-8000-000000000102",
                        "kind": "table",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {
                                "columns": [
                                    col("char_id", "rpt.starter.col.id"),
                                    col("nominal", "rpt.starter.col.nominal"),
                                    col("limits", "rpt.starter.col.limits"),
                                    col("measured", "rpt.starter.col.measured"),
                                    col("deviation", "rpt.starter.col.deviation"),
                                    col("decision", "rpt.starter.col.decision"),
                                ],
                                "starter_id": "tmpl:char-table",
                            }
                        ),
                        "repeat_header_rows": "1",
                        "slot": {
                            "kind": "characteristics",
                            "spec": untrusted_val(
                                {
                                    "repeat": {"kind": "row"},
                                    "rule": {"characteristics": "all"},
                                    "sort": {
                                        "direction": "asc",
                                        "key": "characteristic_id",
                                    },
                                }
                            ),
                        },
                    },
                ],
                "id": "019a9f00-0001-7000-8000-000000000011",
            }
        ],
        "schema_version": "1",
        "template_id": "019a9f00-0001-7000-8000-000000000001",
        "title": untrusted_text("rpt.starter.char_table.title"),
    }


def build_deviation_summary() -> dict[str, object]:
    return {
        "clearance": None,
        "locked": False,
        "media_type": MEDIA_TYPE,
        "pages": [
            {
                "blocks": [
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0002-7000-8000-000000000101",
                        "kind": "text",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {"text": "rpt.starter.deviation_summary.heading"}
                        ),
                        "repeat_header_rows": "0",
                        "slot": None,
                    },
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0002-7000-8000-000000000102",
                        "kind": "table",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {
                                "columns": [
                                    col("outcome", "rpt.starter.col.outcome"),
                                    col("count", "rpt.starter.col.count"),
                                ],
                                "heading_key": "rpt.starter.deviation_summary.outcomes_heading",
                                "section": "outcome_counts",
                                "starter_id": "tmpl:deviation-summary",
                            }
                        ),
                        "repeat_header_rows": "1",
                        "slot": {
                            "kind": "characteristics",
                            "spec": untrusted_val(
                                {
                                    "repeat": {"kind": "row"},
                                    "rule": {"characteristics": "all"},
                                    "sort": {
                                        "direction": "asc",
                                        "key": "characteristic_id",
                                    },
                                }
                            ),
                        },
                    },
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0002-7000-8000-000000000103",
                        "kind": "table",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {
                                "columns": [
                                    col("char_id", "rpt.starter.col.id"),
                                    col("nominal", "rpt.starter.col.nominal"),
                                    col("limits", "rpt.starter.col.limits"),
                                    col("measured", "rpt.starter.col.measured"),
                                    col("deviation", "rpt.starter.col.deviation"),
                                    col("decision", "rpt.starter.col.decision"),
                                ],
                                "heading_key": "rpt.starter.deviation_summary.nonconforming_heading",
                                "section": "nonconforming_rows",
                                "starter_id": "tmpl:deviation-summary",
                            }
                        ),
                        "repeat_header_rows": "1",
                        "slot": {
                            "kind": "characteristics",
                            "spec": untrusted_val(
                                {
                                    "repeat": {"kind": "row"},
                                    "rule": {
                                        "characteristics": "all",
                                        "outcome": ["nonconforms"],
                                    },
                                    "sort": {
                                        "direction": "asc",
                                        "key": "characteristic_id",
                                    },
                                }
                            ),
                        },
                    },
                ],
                "id": "019a9f00-0002-7000-8000-000000000011",
            }
        ],
        "schema_version": "1",
        "template_id": "019a9f00-0002-7000-8000-000000000001",
        "title": untrusted_text("rpt.starter.deviation_summary.title"),
    }


def build_multistage_table() -> dict[str, object]:
    return {
        "clearance": None,
        "locked": False,
        "media_type": MEDIA_TYPE,
        "pages": [
            {
                "blocks": [
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0003-7000-8000-000000000101",
                        "kind": "text",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {"text": "rpt.starter.multistage_table.heading"}
                        ),
                        "repeat_header_rows": "0",
                        "slot": None,
                    },
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0003-7000-8000-000000000102",
                        "kind": "table",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {
                                "columns": [
                                    col("stage_measured", "rpt.starter.col.stage_measured"),
                                ],
                                "section": "stage_columns",
                                "starter_id": "tmpl:multistage-table",
                            }
                        ),
                        "repeat_header_rows": "1",
                        "slot": {
                            "kind": "stages",
                            "spec": untrusted_val(
                                {
                                    "repeat": {
                                        "kind": "column",
                                        "max_columns_per_group": "8",
                                        "repeat_columns": ["char_id", "nominal"],
                                    },
                                    "rule": {"stages": "all"},
                                    "sort": None,
                                }
                            ),
                        },
                    },
                    {
                        "continuation_block": None,
                        "id": "019a9f00-0003-7000-8000-000000000103",
                        "kind": "table",
                        "kind_version": "1",
                        "payload": untrusted_val(
                            {
                                "columns": [
                                    col("char_id", "rpt.starter.col.id"),
                                    col("nominal", "rpt.starter.col.nominal"),
                                    col("measured", "rpt.starter.col.measured"),
                                    col("deviation", "rpt.starter.col.deviation"),
                                    col("decision", "rpt.starter.col.decision"),
                                ],
                                "section": "characteristic_rows",
                                "starter_id": "tmpl:multistage-table",
                            }
                        ),
                        "repeat_header_rows": "1",
                        "slot": {
                            "kind": "characteristics",
                            "spec": untrusted_val(
                                {
                                    "repeat": {"kind": "row"},
                                    "rule": {"characteristics": "all"},
                                    "sort": {
                                        "direction": "asc",
                                        "key": "characteristic_id",
                                    },
                                }
                            ),
                        },
                    },
                ],
                "id": "019a9f00-0003-7000-8000-000000000011",
            }
        ],
        "schema_version": "1",
        "template_id": "019a9f00-0003-7000-8000-000000000001",
        "title": untrusted_text("rpt.starter.multistage_table.title"),
    }


def canonical_bytes(obj: object) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def desired_files() -> dict[Path, bytes]:
    return {
        OUT_DIR / "char-table.json": canonical_bytes(build_char_table()),
        OUT_DIR / "deviation-summary.json": canonical_bytes(build_deviation_summary()),
        OUT_DIR / "multistage-table.json": canonical_bytes(build_multistage_table()),
        OUT_DIR / "labels.en.json": (
            json.dumps(LABELS_EN, indent=2, sort_keys=True, ensure_ascii=False) + "\n"
        ).encode("utf-8"),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv
    desired = desired_files()
    if check:
        ok = True
        for path, expected in sorted(desired.items()):
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                ok = False
                continue
            if path.read_bytes() != expected:
                print(f"DRIFT: {path}", file=sys.stderr)
                ok = False
        if not ok:
            return 1
        print(f"ok: {len(desired)} RPT.MODEL.TMPL.STARTER files match")
        return 0

    for path, content in sorted(desired.items()):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_bytes(content)
    print(f"wrote {len(desired)} RPT.MODEL.TMPL.STARTER files")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv[1:]))
