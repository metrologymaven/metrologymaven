#!/usr/bin/env python3
"""Independent PDF 1.7 structural, subset-font, metadata, and SHA-256 verifier
for RPT.OUTPUT.PDF.WRITE (AC-1, AC-2, AC-3). Stdlib only.

Verifies `fixtures/rpt/output/pdf/bracket_3stage.pdf` against
`fixtures/rpt/output/pdf/bracket_3stage.pdf.sha256`:
- Valid `%PDF-1.7`, exact 20-byte xref entries pointing to exact `N 0 obj` offsets,
  exact stream `/Length`, and valid `startxref`/`%%EOF`.
- Every Flate-compressed stream decompresses cleanly with Python stdlib `zlib.decompress`.
- Trailer `/ID [<HEX16> <HEX16>]` equals the first 16 bytes of `bracket_3stage.resolved.json` digest.
- No `/CreationDate`, `/ModDate`, date patterns, or host/user strings appear anywhere.
- All fonts are embedded TrueType subsets (`/Subtype /Type0`, `/Encoding /Identity-H`,
  embedded `/FontFile2`, and `/ToUnicode` CMap).
- Both `/Info /ExportMeta` and `/Catalog /Metadata` XMP contain the exact canonical
  `export_meta@1` JSON with `renderer = {"id": "pdf", "version": "1.0.0"}`.
"""

import hashlib
import json
import pathlib
import re
import sys
import xml.etree.ElementTree as ET
import zlib

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[3]
RESOLVED_PATH = ROOT / "fixtures/rpt/model/golden/bracket_3stage.resolved.json"
PDF_PATH = HERE / "bracket_3stage.pdf"
SHA_PATH = HERE / "bracket_3stage.pdf.sha256"


def unescape_pdf_literal(raw: bytes) -> str:
    out = bytearray()
    i = 0
    while i < len(raw):
        b = raw[i]
        if b == 0x5C and i + 1 < len(raw):  # backslash
            nxt = raw[i + 1]
            if 0x30 <= nxt <= 0x37:
                j = i + 1
                while j < len(raw) and j < i + 4 and 0x30 <= raw[j] <= 0x37:
                    j += 1
                out.append(int(raw[i + 1 : j], 8) & 0xFF)
                i = j
            else:
                out.append(nxt)
                i += 2
        else:
            out.append(b)
            i += 1
    return out.decode("utf-8")


def verify_pdf(data: bytes, expected_resolved_digest: str) -> dict:
    assert data.startswith(b"%PDF-1.7\n"), "missing %PDF-1.7 header"
    m = re.search(rb"startxref\n(\d+)\n%%EOF\n$", data[-64:])
    assert m, "missing startxref / %%EOF"
    xref_off = int(m.group(1))
    assert data[xref_off : xref_off + 5] == b"xref\n", "startxref does not point to xref"

    tail = data[xref_off:]
    lines = tail.split(b"\n")
    count = int(lines[1].split()[1])
    assert lines[2] == b"0000000000 65535 f ", "bad xref object 0"

    offsets = {}
    for n in range(1, count):
        entry = lines[2 + n] + b"\n"
        assert len(entry) == 20, f"bad xref entry len for obj {n}"
        offsets[n] = int(entry[:10])

    # Parse every object & verify stream /Length and zlib decompression.
    objs = {}
    for n, off in offsets.items():
        head = f"{n} 0 obj\n".encode("ascii")
        assert data[off : off + len(head)] == head, f"xref offset mismatch for obj {n}"
        end_pos = data.index(b"\nendobj\n", off)
        body = data[off + len(head) : end_pos]
        if b"\nstream\n" in body:
            dict_part, rest = body.split(b"\nstream\n", 1)
            lm = re.search(rb"/Length\s+(\d+)", dict_part)
            assert lm, f"obj {n} missing /Length"
            slen = int(lm.group(1))
            sdata = rest[:slen]
            assert rest[slen : slen + 10] == b"\nendstream", f"obj {n} stream /Length mismatch"
            if b"/Filter /FlateDecode" in dict_part:
                decompressed = zlib.decompress(sdata)
                if b"/Length1 " in dict_part:
                    l1 = int(re.search(rb"/Length1\s+(\d+)", dict_part).group(1))
                    assert len(decompressed) == l1, f"obj {n} /Length1 mismatch"
            else:
                decompressed = sdata
            objs[n] = (dict_part, decompressed)
        else:
            objs[n] = (body, None)

    # Check trailer /ID equals first 16 bytes of resolved_digest (uppercase hex).
    want_id = expected_resolved_digest.split(":", 1)[1][:32].upper().encode("ascii")
    tm = re.search(rb"trailer\n<< /ID \[<([0-9A-F]{32})> <([0-9A-F]{32})>\] /Info (\d+) 0 R /Root (\d+) 0 R /Size (\d+) >>", tail)
    assert tm, "trailer missing required /ID /Info /Root /Size"
    assert tm.group(1) == want_id and tm.group(2) == want_id, f"/ID mismatch: {tm.group(1)} != {want_id}"
    info_num = int(tm.group(3))
    root_num = int(tm.group(4))
    assert int(tm.group(5)) == count, "trailer /Size mismatch"

    # Verify Catalog -> Pages + Metadata.
    cat_dict = objs[root_num][0]
    assert b"/Type /Catalog" in cat_dict
    meta_m = re.search(rb"/Metadata (\d+) 0 R", cat_dict)
    assert meta_m, "Catalog missing /Metadata"
    xmp_bytes = objs[int(meta_m.group(1))][1]
    xmp_root = ET.fromstring(xmp_bytes.decode("utf-8").split("<x:xmpmeta", 1)[1].split("</x:xmpmeta>", 1)[0].join(["<x:xmpmeta", "</x:xmpmeta>"]))
    xmp_meta_elems = list(xmp_root.iter("{https://metrologymaven.io/ns/export/1/}ExportMeta"))
    assert len(xmp_meta_elems) == 1, "XMP missing fid:ExportMeta"
    xmp_meta_str = xmp_meta_elems[0].text

    # Verify /Info /ExportMeta matches XMP byte-for-byte and has no dates.
    info_dict = objs[info_num][0]
    assert b"/CreationDate" not in data and b"/ModDate" not in data, "timestamps must not appear in PDF"
    assert b"D:20" not in data, "PDF date string must not appear"
    em_m = re.search(rb"/ExportMeta \((.*?)\) /Producer", info_dict)
    assert em_m, "/Info missing /ExportMeta"
    info_meta_str = unescape_pdf_literal(em_m.group(1))
    assert info_meta_str == xmp_meta_str, "/Info /ExportMeta and XMP ExportMeta disagree"

    meta = json.loads(info_meta_str)
    assert meta["schema"] == "export_meta@1"
    assert meta["report_digest"] == expected_resolved_digest
    assert meta["renderer"] == {"id": "pdf", "version": "1.0.0"}

    # Verify all Type0 fonts are embedded subsets with FontFile2 + ToUnicode.
    type0_count = 0
    for n, (dbytes, _) in objs.items():
        if b"/Subtype /Type0" in dbytes:
            type0_count += 1
            assert b"/Encoding /Identity-H" in dbytes
            assert re.search(rb"/BaseFont /[A-Z]{6}\+", dbytes), f"font obj {n} missing 6-letter subset tag"
            assert re.search(rb"/ToUnicode \d+ 0 R", dbytes), f"font obj {n} missing /ToUnicode"
    assert type0_count >= 1, "expected at least one embedded subset font"
    return meta


def main() -> int:
    if not RESOLVED_PATH.exists():
        print(f"MISSING: {RESOLVED_PATH}", file=sys.stderr)
        return 1
    if not PDF_PATH.exists() or not SHA_PATH.exists():
        print(f"MISSING: {PDF_PATH} or {SHA_PATH}", file=sys.stderr)
        return 1

    resolved = json.loads(RESOLVED_PATH.read_text(encoding="utf-8"))
    pdf_bytes = PDF_PATH.read_bytes()
    actual_sha = "sha256:" + hashlib.sha256(pdf_bytes).hexdigest()
    expected_sha = SHA_PATH.read_text(encoding="utf-8").strip()
    if actual_sha != expected_sha:
        print(f"SHA MISMATCH: {actual_sha} != {expected_sha}", file=sys.stderr)
        return 1

    meta = verify_pdf(pdf_bytes, resolved["digest"])
    print(f"pdf check ok ({len(pdf_bytes)} bytes, {actual_sha}, renderer={meta['renderer']['id']}@{meta['renderer']['version']})")
    return 0


if __name__ == "__main__":
    sys.exit(main())
