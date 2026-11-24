#!/usr/bin/env python3
"""Deterministic fixture generator for RPT.BALLOON.AUTO.TEXT (CC-BY-4.0).

Generates:
- vector_bracket.pdf        (AC-1: A3 landscape vector PDF with 12 dimensions and 3 GD&T frames + ToUnicode CMap)
- vector_bracket.truth.json (AC-1: ground-truth integer 0.01 mm SheetRect bounds for all 15 callouts)
- scanned_only.pdf          (AC-2: A4 PDF containing a raster Image XObject and zero vector text runs)

Usage:
  python3 fixtures/rpt/balloon/auto/make_fixtures.py          # write fixtures
  python3 fixtures/rpt/balloon/auto/make_fixtures.py --check  # verify byte-for-byte
"""

import json
import pathlib
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent

# Exact A3 landscape dimensions in PDF points: 420 x 297 mm
PT_PER_MM = 72.0 / 25.4
A3_W_MM = 420.0
A3_H_MM = 297.0
A3_W_PT = A3_W_MM * PT_PER_MM  # 1190.551181...
A3_H_PT = A3_H_MM * PT_PER_MM  # 841.889764...


def build_pdf(objects: list[bytes]) -> bytes:
    """Builds a minimal deterministic PDF 1.7 with exact xref offsets."""
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = [0]
    for idx, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out.extend(f"{idx} 0 obj\n".encode("ascii"))
        out.extend(body)
        out.extend(b"\nendobj\n")
    xref_pos = len(out)
    out.extend(f"xref\n0 {len(objects) + 1}\n".encode("ascii"))
    out.extend(b"0000000000 65535 f \n")
    for off in offsets[1:]:
        out.extend(f"{off:010d} 00000 n \n".encode("ascii"))
    out.extend(
        f"trailer\n<< /Size {len(objects) + 1} /Root 1 0 R >>\nstartxref\n{xref_pos}\n%%EOF\n".encode("ascii")
    )
    return bytes(out)


def pdf_stream_obj(dict_entries: str, payload: bytes, compress: bool = True) -> bytes:
    if compress:
        comp = zlib.compress(payload, level=6)
        hdr = f"<< {dict_entries} /Filter /FlateDecode /Length {len(comp)} >>\nstream\n".encode("ascii")
        return hdr + comp + b"\nendstream"
    hdr = f"<< {dict_entries} /Length {len(payload)} >>\nstream\n".encode("ascii")
    return hdr + payload + b"\nendstream"


# Custom ToUnicode CMap mapping control bytes 0x01..0x06 to engineering Unicode symbols:
#   0x01 -> U+2300 (diameter)
#   0x02 -> U+00B1 (plus-minus)
#   0x03 -> U+2316 (position)
#   0x04 -> U+22A5 (perpendicularity)
#   0x05 -> U+23E5 (flatness)
#   0x06 -> U+00B0 (degree)
SYMBOL_MAP = {
    "\u2300": 0x01,  # ⌀
    "\u00d8": 0x01,  # Ø -> mapped via 0x01 -> Ø (U+00D8) below for readability
    "\u00b1": 0x02,  # ±
    "\u2316": 0x03,  # ⌖
    "\u22a5": 0x04,  # ⊥
    "\u23e5": 0x05,  # ⏥
    "\u00b0": 0x06,  # °
}

TO_UNICODE_CMAP = (
    b"/CIDInit /ProcSet findresource begin\n"
    b"12 dict begin\n"
    b"begincmap\n"
    b"/CIDSystemInfo << /Registry (Fid) /Ordering (Metro) /Supplement 0 >> def\n"
    b"/CMapName /FidMetro-UTF16 def\n"
    b"/CMapType 2 def\n"
    b"1 begincodespacerange\n"
    b"<00> <FF>\n"
    b"endcodespacerange\n"
    b"6 beginbfchar\n"
    b"<01> <00D8>\n"
    b"<02> <00B1>\n"
    b"<03> <2316>\n"
    b"<04> <22A5>\n"
    b"<05> <23E5>\n"
    b"<06> <00B0>\n"
    b"endbfchar\n"
    b"endcmap\n"
    b"CMapName currentdict /CMap defineresource pop\n"
    b"end\n"
    b"end\n"
)


def encode_pdf_hex_string(text: str) -> str:
    out = bytearray()
    for ch in text:
        if ch in SYMBOL_MAP:
            out.append(SYMBOL_MAP[ch])
        else:
            b = ord(ch)
            assert 0x20 <= b <= 0x7E, f"unexpected char {ch!r}"
            out.append(b)
    return "<" + out.hex().upper() + ">"


def make_vector_bracket() -> tuple[bytes, bytes]:
    # 12 dimensions + 3 GD&T feature control frames = 15 callouts on A3 landscape (420 x 297 mm).
    # Coordinates given in top-left sheet millimetres (x_mm, y_baseline_mm, font_mm, angle_deg, text, boxed)
    # Note: in PDF user space (origin bottom-left), y_pdf_pt = (A3_H_MM - y_baseline_mm) * PT_PER_MM.
    callouts = [
        # 12 Dimensions
        (45.0, 52.0, 3.5, 0, "120.0 \u00b10.1", False),
        (140.0, 52.0, 3.5, 0, "60.00 \u00b10.05", False),
        (235.0, 52.0, 3.5, 0, "\u00d812.00 \u00b10.05", False),
        (330.0, 52.0, 3.5, 0, "2X \u00d86.50 H7", False),
        (45.0, 102.0, 3.5, 0, "45.00 +0.02/-0.01", False),
        (140.0, 102.0, 3.5, 0, "30.00", False),
        (235.0, 102.0, 3.5, 0, "18.50 \u00b10.05", False),
        (330.0, 102.0, 3.5, 0, "8.00 \u00b10.02", False),
        (45.0, 152.0, 3.5, 0, "R4.0", False),
        (140.0, 152.0, 3.5, 0, "M6x1.0-6H", False),
        (235.0, 152.0, 3.5, 0, "2x45\u00b0", False),
        (330.0, 152.0, 3.5, 0, "Ra 1.6", False),
        # 3 GD&T Feature Control Frames (with vector boxes)
        (45.0, 212.0, 3.5, 0, "\u2316 | \u00d80.05 M | A | B | C", True),
        (185.0, 212.0, 3.5, 0, "\u22a5 | 0.02 | A", True),
        (295.0, 212.0, 3.5, 0, "\u23e5 | 0.01", True),
    ]

    lines = [
        "0.5 w",
        # Outer drawing border (10 mm margin)
        f"{10.0 * PT_PER_MM:.3f} {10.0 * PT_PER_MM:.3f} {(A3_W_MM - 20.0) * PT_PER_MM:.3f} {(A3_H_MM - 20.0) * PT_PER_MM:.3f} re S",
    ]

    truth_runs = []
    truth_boxes = []

    for idx, (x_mm, y_base_mm, font_mm, angle_deg, text, boxed) in enumerate(callouts):
        x_pt = x_mm * PT_PER_MM
        y_pt = (A3_H_MM - y_base_mm) * PT_PER_MM
        font_pt = font_mm * PT_PER_MM

        # Approximate glyph width (0.60 * font_size per character matches our extractor's Helvetica metric).
        n_chars = len(text)
        w_mm = n_chars * font_mm * 0.60
        ascent_mm = font_mm * 0.80
        descent_mm = font_mm * 0.20

        x0_cmm = round(x_mm * 100)
        y0_cmm = round((y_base_mm - ascent_mm) * 100)
        x1_cmm = round((x_mm + w_mm) * 100)
        y1_cmm = round((y_base_mm + descent_mm) * 100)

        truth_runs.append(
            {
                "text": text,
                "bbox": {"x0": x0_cmm, "y0": y0_cmm, "x1": x1_cmm, "y1": y1_cmm},
                "font_h_cmm": round(font_mm * 100),
                "angle_mdeg": angle_deg * 1000,
                "kind": "gdt" if boxed else "dimension",
            }
        )

        if boxed:
            pad_mm = 2.0
            bx0_mm = x_mm - pad_mm
            by0_mm = y_base_mm - ascent_mm - pad_mm
            bw_mm = w_mm + 2.0 * pad_mm
            bh_mm = font_mm + 2.0 * pad_mm
            bx0_pt = bx0_mm * PT_PER_MM
            by_bot_pt = (A3_H_MM - (by0_mm + bh_mm)) * PT_PER_MM
            lines.append(f"{bx0_pt:.3f} {by_bot_pt:.3f} {bw_mm * PT_PER_MM:.3f} {bh_mm * PT_PER_MM:.3f} re S")
            truth_boxes.append(
                {
                    "x0": round(bx0_mm * 100),
                    "y0": round(by0_mm * 100),
                    "x1": round((bx0_mm + bw_mm) * 100),
                    "y1": round((by0_mm + bh_mm) * 100),
                }
            )

        hex_s = encode_pdf_hex_string(text)
        # Exercise both Tj and TJ operators across callouts
        if idx % 3 == 1:
            lines.append(f"BT /F1 {font_pt:.4f} Tf 1 0 0 1 {x_pt:.4f} {y_pt:.4f} Tm [{hex_s}] TJ ET")
        else:
            lines.append(f"BT /F1 {font_pt:.4f} Tf {x_pt:.4f} {y_pt:.4f} Td {hex_s} Tj ET")

    stream_bytes = ("\n".join(lines) + "\n").encode("ascii")

    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            f"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {A3_W_PT:.6f} {A3_H_PT:.6f}] "
            f"/Resources << /Font << /F1 4 0 R >> >> /Contents 6 0 R >>"
        ).encode("ascii"),
        b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica /ToUnicode 5 0 R >>",
        pdf_stream_obj("", TO_UNICODE_CMAP, compress=True),
        pdf_stream_obj("", stream_bytes, compress=True),
    ]
    pdf_bytes = build_pdf(objs)

    truth = {
        "sheet_width_cmm": 42000,
        "sheet_height_cmm": 29700,
        "callout_count": len(truth_runs),
        "runs": truth_runs,
        "frame_boxes": truth_boxes,
    }
    truth_bytes = (json.dumps(truth, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")
    return pdf_bytes, truth_bytes


def make_scanned_only() -> bytes:
    # 16x16 solid light-gray raster XObject placed on A4 portrait page with zero text runs.
    img_pixels = bytes([240] * (16 * 16))
    content = b"q\n595.2756 0 0 841.8898 0 0 cm\n/Im1 Do\nQ\n"
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        (
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.275591 841.889764] "
            b"/Resources << /XObject << /Im1 4 0 R >> >> /Contents 5 0 R >>"
        ),
        pdf_stream_obj(
            "/Type /XObject /Subtype /Image /Width 16 /Height 16 /ColorSpace /DeviceGray /BitsPerComponent 8",
            img_pixels,
            compress=True,
        ),
        pdf_stream_obj("", content, compress=True),
    ]
    return build_pdf(objs)


def main() -> int:
    check = "--check" in sys.argv[1:]
    vec_pdf, truth_json = make_vector_bracket()
    scan_pdf = make_scanned_only()
    files = {
        "vector_bracket.pdf": vec_pdf,
        "vector_bracket.truth.json": truth_json,
        "scanned_only.pdf": scan_pdf,
    }

    bad = 0
    for name, want in sorted(files.items()):
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != want:
                print(f"DRIFT: {path}", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(want)

    if bad:
        return 1
    print(f"rpt-balloon-auto fixtures ok ({len(files)} files)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
