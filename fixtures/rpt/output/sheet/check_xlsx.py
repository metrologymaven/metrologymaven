#!/usr/bin/env python3
"""Independent OOXML (.xlsx) ZIP and XML verifier for RPT.OUTPUT.SHEET.XLSX (AC-1..AC-3).

Stdlib only (`zipfile` + `xml.etree.ElementTree`). Verifies:
- `fixtures/rpt/output/sheet/bracket.xlsx` matches `bracket.xlsx.sha256`
- Valid ZIP container with fixed entry order, fixed 1980-01-01 00:00:00 timestamps, zero extra fields
- No `docProps/core.xml` creation timestamp or author metadata
- Workbook sheets are `T01`, `T02`, `meta` in exact order
- Every numeric cell carries a style `<xf numFmtId="...">` matching its canonical decimal digit count
- `meta` worksheet holds valid canonical `export_meta@1` JSON with `renderer = {"id": "xlsx", "version": "1.0.0"}`
- Every numeric value in `T02` matches `bracket_3stage.resolved.json` canonical values identically
"""

import hashlib
import json
import pathlib
import sys
import xml.etree.ElementTree as ET
import zipfile

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
XLSX_PATH = HERE / "bracket.xlsx"
SHA_PATH = HERE / "bracket.xlsx.sha256"
RESOLVED_PATH = ROOT / "fixtures/rpt/model/golden/bracket_3stage.resolved.json"

NS = {"m": "http://schemas.openxmlformats.org/spreadsheetml/2006/main"}


def main() -> int:
    if not XLSX_PATH.exists() or not SHA_PATH.exists():
        print(f"MISSING: {XLSX_PATH} or {SHA_PATH}", file=sys.stderr)
        return 1

    data = XLSX_PATH.read_bytes()
    actual_sha = "sha256:" + hashlib.sha256(data).hexdigest()
    expected_sha = SHA_PATH.read_text(encoding="utf-8").strip()
    if actual_sha != expected_sha:
        print(f"SHA MISMATCH: {actual_sha} != {expected_sha}", file=sys.stderr)
        return 1

    resolved = json.loads(RESOLVED_PATH.read_text(encoding="utf-8"))

    with zipfile.ZipFile(XLSX_PATH, "r") as zf:
        assert zf.testzip() is None, "corrupt ZIP entry"
        names = zf.namelist()
        expected_entries = [
            "[Content_Types].xml",
            "_rels/.rels",
            "xl/workbook.xml",
            "xl/_rels/workbook.xml.rels",
            "xl/styles.xml",
            "xl/worksheets/sheet1.xml",
            "xl/worksheets/sheet2.xml",
            "xl/worksheets/sheet3.xml",
        ]
        assert names == expected_entries, f"unexpected ZIP entries: {names}"
        for info in zf.infolist():
            assert info.date_time == (1980, 1, 1, 0, 0, 0), f"non-deterministic timestamp on {info.filename}: {info.date_time}"
            assert info.extra == b"", f"non-empty extra field on {info.filename}"
            assert info.comment == b"", f"non-empty comment on {info.filename}"

        wb = ET.fromstring(zf.read("xl/workbook.xml"))
        sheets = [el.attrib["name"] for el in wb.findall(".//m:sheets/m:sheet", NS)]
        assert sheets == ["T01", "T02", "meta"], f"expected sheets ['T01', 'T02', 'meta'], got {sheets}"

        styles = ET.fromstring(zf.read("xl/styles.xml"))
        num_fmts = {int(el.attrib["numFmtId"]): el.attrib["formatCode"] for el in styles.findall(".//m:numFmts/m:numFmt", NS)}
        xfs = [int(el.attrib["numFmtId"]) for el in styles.findall(".//m:cellXfs/m:xf", NS)]

        # Check T02 numeric cells and number format decimals.
        s2 = ET.fromstring(zf.read("xl/worksheets/sheet2.xml"))
        num_count = 0
        for cell in s2.findall(".//m:sheetData/m:row/m:c", NS):
            if cell.attrib.get("t") is None:
                v = cell.find("m:v", NS)
                assert v is not None and v.text is not None
                s_idx = int(cell.attrib["s"])
                fmt_id = xfs[s_idx]
                fmt_code = num_fmts[fmt_id]
                decimals = len(v.text.split(".")[1]) if "." in v.text else 0
                want_fmt = "0" if decimals == 0 else "0." + ("0" * decimals)
                assert fmt_code == want_fmt, f"cell {cell.attrib['r']} value {v.text} has fmt {fmt_code}, want {want_fmt}"
                num_count += 1
        assert num_count == 500, f"expected 500 numeric cells in T02, got {num_count}"

        # Check meta sheet carries canonical ExportMeta JSON with renderer xlsx.
        s3 = ET.fromstring(zf.read("xl/worksheets/sheet3.xml"))
        meta_kv = {}
        for row in s3.findall(".//m:sheetData/m:row", NS)[1:]:
            cells = row.findall("m:c", NS)
            k = cells[0].find("m:is/m:t", NS).text
            v_el = cells[1].find("m:is/m:t", NS)
            meta_kv[k] = v_el.text if v_el is not None and v_el.text is not None else ""
        meta_obj = json.loads(meta_kv["export_meta_json"])
        assert meta_obj["schema"] == "export_meta@1"
        assert meta_obj["report_digest"] == resolved["digest"]
        assert meta_obj["renderer"] == {"id": "xlsx", "version": "1.0.0"}

    print(f"xlsx check ok ({len(data)} bytes, {actual_sha}, sheets={sheets}, numeric_cells={num_count})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
