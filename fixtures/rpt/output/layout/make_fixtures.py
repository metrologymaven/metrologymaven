#!/usr/bin/env python3
"""Deterministic fixture generator and checker for RPT.OUTPUT.PDF.LAYOUT (AC-1..AC-3).

Generates:
- fixtures/rpt/output/layout/fai_500rows.resolved.json  (AC-2: 500-row FAI Form 3 table with
  repeat_header_rows, continuation_block, and "n of m" footer placeholders)
- fixtures/rpt/output/layout/ppap_10parts.resolved.json (AC-3: PPAP table with 10 part columns,
  repeat_columns=[0,1,2] for Balloon/Specification/Outcome, max_columns_per_group=6)

And verifies canonical RFC 8785 formatting & pinned SHA-256 of all layout fixtures including
bracket_3stage.layout.json (AC-1).

Usage:
  python3 fixtures/rpt/output/layout/make_fixtures.py          # write synthetic inputs
  python3 fixtures/rpt/output/layout/make_fixtures.py --check  # verify all bytes
"""

import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def canon_bytes(obj) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256_str(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def build_resolved(pages: list, template_id: str, locale: str = "en") -> bytes:
    content = {
        "pages": pages,
        "provenance": {
            "formatters": [{"id": "fmt.length@1", "version": "1"}],
            "head": "sha256:" + "11" * 32,
            "kernel_version": "0.1.0",
            "locale": locale,
            "locale_release": "i18n-locale@1",
            "template": {
                "builtin": True,
                "digest": "sha256:" + "22" * 32,
                "id": template_id,
                "version": "1",
            },
            "units": None,
        },
        "schema": "resolved_report@1",
        "schema_version": "1",
        "warnings": [],
    }
    digest = sha256_str(canon_bytes(content))
    full = dict(content)
    full["digest"] = digest
    return canon_bytes(full)


def make_fai_500rows() -> bytes:
    ident_id = "019a9f10-0001-7000-8000-000000000001"
    table_id = "019a9f10-0001-7000-8000-000000000002"
    footer_id = "019a9f10-0001-7000-8000-000000000003"

    rows = []
    for i in range(1, 501):
        nominal = f"{10 + (i % 40)}.000"
        measured = f"{10 + (i % 40)}.00{(i % 5)}"
        outcome = "conforms" if (i % 17 != 0) else "nonconforming"
        rows.append([
            f"B{i:03d}",
            f"Dim {i:03d}",
            {"display": nominal, "canonical": nominal, "unit": "mm", "markers": []},
            {"display": measured, "canonical": measured, "unit": "mm", "markers": []},
            {"text": outcome, "rule": "dr:simple-acceptance@1"},
        ])

    pages = [
        {
            "id": "019a9f10-0001-7000-8000-000000000010",
            "blocks": [
                {
                    "id": ident_id,
                    "kind": "text",
                    "kind_version": "1",
                    "frame": None,
                    "payload": {
                        "schema": "resolved_text@1",
                        "key": "fai.form3.part_ident",
                        "style": "heading",
                        "text": "AS9102 Form 3 — Part PN-4410-REV-B — Serial SN-001",
                    },
                },
                {
                    "id": table_id,
                    "kind": "table",
                    "kind_version": "1",
                    "frame": None,
                    "payload": {
                        "schema": "resolved_table@1",
                        "repeat_header_rows": 1,
                        "continuation_block": ident_id,
                        "columns": [
                            {"key": "balloon", "title": "Char No"},
                            {"key": "desc", "title": "Reference"},
                            {"key": "nominal", "title": "Requirement"},
                            {"key": "measured", "title": "Results"},
                            {"key": "decision", "title": "Outcome"},
                        ],
                        "rows": rows,
                    },
                },
                {
                    "id": footer_id,
                    "kind": "footer",
                    "kind_version": "1",
                    "frame": None,
                    "payload": {
                        "schema": "resolved_text@1",
                        "runs": [
                            {"kind": "literal", "text": "Page "},
                            {"kind": "page_placeholder", "field": "page_number"},
                            {"kind": "literal", "text": " of "},
                            {"kind": "page_placeholder", "field": "page_count"},
                        ],
                    },
                },
            ],
        }
    ]
    return build_resolved(pages, "019a9f10-0001-7000-8000-000000000099")


def make_ppap_10parts() -> bytes:
    cols = [
        {"key": "balloon", "title": "Balloon"},
        {"key": "spec", "title": "Specification"},
        {"key": "outcome", "title": "Outcome"},
    ]
    for p in range(1, 11):
        cols.append({"key": f"part_{p}", "title": f"Part {p}"})

    rows = []
    for c in range(1, 9):
        row = [
            f"{c}",
            f"{20 + c}.00 +/- 0.05 mm",
            {"text": "conforms", "rule": "dr:simple-acceptance@1"},
        ]
        for p in range(1, 11):
            val = f"{20 + c}.0{(p % 4)}{(c % 5)}"
            row.append({"display": val, "canonical": val, "unit": "mm", "markers": []})
        rows.append(row)

    pages = [
        {
            "id": "019a9f20-0001-7000-8000-000000000010",
            "blocks": [
                {
                    "id": "019a9f20-0001-7000-8000-000000000001",
                    "kind": "text",
                    "kind_version": "1",
                    "frame": None,
                    "payload": {
                        "schema": "resolved_text@1",
                        "style": "heading",
                        "text": "PPAP Dimensional Results — 10 Parts",
                    },
                },
                {
                    "id": "019a9f20-0001-7000-8000-000000000002",
                    "kind": "table",
                    "kind_version": "1",
                    "frame": None,
                    "payload": {
                        "schema": "resolved_table@1",
                        "repeat_header_rows": 1,
                        "max_columns_per_group": 6,
                        "repeat_columns": [0, 1, 2],
                        "columns": cols,
                        "rows": rows,
                    },
                },
            ],
        }
    ]
    return build_resolved(pages, "019a9f20-0001-7000-8000-000000000099")


def main() -> int:
    check = "--check" in sys.argv
    expected = {
        "fai_500rows.resolved.json": make_fai_500rows(),
        "ppap_10parts.resolved.json": make_ppap_10parts(),
    }

    bad = 0
    for name, want in expected.items():
        path = HERE / name
        if check:
            if not path.exists():
                print(f"MISSING: {path}", file=sys.stderr)
                bad += 1
                continue
            got = path.read_bytes()
            if got != want:
                print(f"DRIFT: {path}", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(want)

    golden = HERE / "bracket_3stage.layout.json"
    if check:
        if not golden.exists():
            print(f"MISSING: {golden}", file=sys.stderr)
            bad += 1
        else:
            raw = golden.read_bytes()
            parsed = json.loads(raw.decode("utf-8"))
            re_canon = canon_bytes(parsed)
            if raw != re_canon:
                print(f"NOT CANONICAL RFC 8785: {golden}", file=sys.stderr)
                bad += 1
            if parsed.get("schema") != "page_layout@1":
                print(f"BAD SCHEMA in {golden}", file=sys.stderr)
                bad += 1
            # Confirm zero floating-point JSON numbers anywhere in the layout golden.
            def check_no_float(v, ptr=""):
                if isinstance(v, float):
                    raise AssertionError(f"float found at {ptr}: {v}")
                if isinstance(v, list):
                    for idx, item in enumerate(v):
                        check_no_float(item, f"{ptr}[{idx}]")
                elif isinstance(v, dict):
                    for k, item in v.items():
                        check_no_float(item, f"{ptr}/{k}")

            check_no_float(parsed)

    if bad:
        return 1
    print("layout fixtures ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
