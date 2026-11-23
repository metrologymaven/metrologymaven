#!/usr/bin/env python3
"""Deterministic stdlib-only fixture generator and verifier for IO.REG.PLUGIN.DRAW (AC-1..AC-7).

Generates under fixtures/io/reg/plugin/draw/:
- a3_landscape.pdf  (AC-1: 1 page, 420 x 297 mm MediaBox -> 42000 x 29700 cmm)
- rotated.pdf       (AC-2: A4 portrait MediaBox + /Rotate 90 -> 29700 x 21000 cmm)
- three_pages.pdf   (AC-3: 3 pages with CropBox on page 2)
- scan_300dpi.png   (AC-4: 3508 x 2480 px Gray8 PNG with pHYs 11811 px/m = 300 dpi)
- nodpi.png         (AC-4: 64 x 48 px RGB PNG with no pHYs chunk -> resolution: none)
- g4_two_pages.tif  (AC-5: 2-page ITU-T T.6 / CCITT Group 4 bilevel TIFF)
- g4_two_pages.sha256 (pinned Gray8 pixel SHA-256 digests for both pages of g4_two_pages.tif)
- encrypted.pdf     (AC-6: trailer /Encrypt -> pdf_encrypted)
- js_action.pdf     (AC-6: /OpenAction /JavaScript + /AcroForm -> warnings, never executed)
- pages201.pdf      (AC-7: 201 pages -> limit_exceeded)

Usage:
  python3 fixtures/io/reg/plugin/draw/make_fixtures.py          # write fixtures
  python3 fixtures/io/reg/plugin/draw/make_fixtures.py --check  # verify byte-for-byte
"""

import hashlib
import pathlib
import struct
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent


def build_pdf(objects: list[bytes], trailer_extra: bytes = b"") -> bytes:
    out = bytearray(b"%PDF-1.7\n%\xe2\xe3\xcf\xd3\n")
    offsets = []
    for idx, body in enumerate(objects, start=1):
        offsets.append(len(out))
        out += f"{idx} 0 obj\n".encode("ascii")
        out += body + b"\nendobj\n"
    xref_pos = len(out)
    n = len(objects) + 1
    out += f"xref\n0 {n}\n0000000000 65535 f \n".encode("ascii")
    for off in offsets:
        out += f"{off:010d} 00000 n \n".encode("ascii")
    out += f"trailer\n<< /Size {n} /Root 1 0 R".encode("ascii") + trailer_extra + b" >>\n"
    out += f"startxref\n{xref_pos}\n%%EOF\n".encode("ascii")
    return bytes(out)


def make_a3_landscape() -> bytes:
    # 420 mm = 1190.551181 pt, 297 mm = 841.889764 pt
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1190.551181 841.889764] >>",
    ])


def make_rotated() -> bytes:
    # A4 portrait 210 x 297 mm with /Rotate 90 -> 29700 x 21000
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.275591 841.889764] /Rotate 90 >>",
    ])


def make_three_pages() -> bytes:
    # Page 1: A4 (210 x 297 mm), Page 2: MediaBox A3 but CropBox 150 x 100 mm (425.19685 x 283.464567 pt),
    # Page 3: Letter (215.9 x 279.4 mm = 612 x 792 pt)
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [3 0 R 4 0 R 5 0 R] /Count 3 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.275591 841.889764] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 1190.551181 841.889764] /CropBox [10 20 435.196850 303.464567] >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 612 792] >>",
    ])


def make_encrypted() -> bytes:
    return build_pdf(
        [
            b"<< /Type /Catalog /Pages 2 0 R >>",
            b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
            b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.275591 841.889764] >>",
            b"<< /Filter /Standard /V 2 /R 3 /Length 128 >>",
        ],
        trailer_extra=b" /Encrypt 4 0 R",
    )


def make_js_action() -> bytes:
    return build_pdf([
        b"<< /Type /Catalog /Pages 2 0 R /OpenAction 4 0 R /AcroForm << /Fields [] >> >>",
        b"<< /Type /Pages /Kids [3 0 R] /Count 1 >>",
        b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.275591 841.889764] >>",
        b"<< /Type /Action /S /JavaScript /JS (app.alert\\(1\\);) >>",
    ])


def make_pages201() -> bytes:
    kids = " ".join(f"{i} 0 R" for i in range(3, 204)).encode("ascii")
    objs = [
        b"<< /Type /Catalog /Pages 2 0 R >>",
        b"<< /Type /Pages /Kids [" + kids + b"] /Count 201 >>",
    ]
    for _ in range(201):
        objs.append(b"<< /Type /Page /Parent 2 0 R /MediaBox [0 0 595.275591 841.889764] >>")
    return build_pdf(objs)


def png_chunk(ctype: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(ctype + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + ctype + data + struct.pack(">I", crc)


def make_scan_300dpi_png() -> bytes:
    # 3508 x 2480 px (A4 landscape at 300 dpi), 8-bit grayscale, pHYs = 11811 px/m -> 300 dpi.
    w, h = 3508, 2480
    white_row = b"\x00" + (b"\xff" * w)
    border_row = b"\x00" + (b"\x00" * w)
    mid_row = b"\x00" + (b"\xff" * 40) + (b"\x00" * 4) + (b"\xff" * (w - 88)) + (b"\x00" * 4) + (b"\xff" * 40)
    comp = zlib.compressobj(level=6)
    idat = bytearray()
    for y in range(h):
        if y in (40, 41, h - 42, h - 41):
            idat += comp.compress(border_row)
        elif 40 < y < h - 41:
            idat += comp.compress(mid_row)
        else:
            idat += comp.compress(white_row)
    idat += comp.flush()
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 0, 0, 0, 0)
    phys = struct.pack(">IIB", 11811, 11811, 1)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"pHYs", phys)
        + png_chunk(b"IDAT", bytes(idat))
        + png_chunk(b"IEND", b"")
    )


def make_nodpi_png() -> bytes:
    w, h = 64, 48
    raw = bytearray()
    for y in range(h):
        raw.append(0)
        for x in range(w):
            raw.extend(((x * 4) & 255, (y * 5) & 255, 128))
    ihdr = struct.pack(">IIBBBBB", w, h, 8, 2, 0, 0, 0)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", zlib.compress(bytes(raw), level=6))
        + png_chunk(b"IEND", b"")
    )


# Pure-stdlib ITU-T T.6 (CCITT Group 4) encoder for deterministic multi-page TIFF generation.
WHITE_TERM = [
    "00110101", "000111", "0111", "1000", "1011", "1100", "1110", "1111",
    "10011", "10100", "00111", "01000", "001000", "000011", "110100", "110101",
    "101010", "101011", "0100111", "0001100", "0001000", "0010111", "0000011", "0000100",
    "0101000", "0101011", "0010011", "0100100", "0011000", "00000010", "00000011", "00011010",
    "00011011", "00010010", "00010011", "00010100", "00010101", "00010110", "00010111", "00101000",
    "00101001", "00101010", "00101011", "00101100", "00101101", "00000100", "00000101", "00001010",
    "00001011", "01010010", "01010011", "01010100", "01010101", "00100100", "00100101", "01011000",
    "01011001", "01011010", "01011011", "01001010", "01001011", "00110010", "00110011", "00110100",
]
BLACK_TERM = [
    "0000110111", "010", "11", "10", "011", "0011", "0010", "00011",
    "000101", "000100", "0000100", "0000101", "0000111", "00000100", "00000111", "000011000",
    "0000010111", "0000011000", "0000001000", "00001100111", "00001101000", "00001101100", "00000110111", "00000101000",
    "00000010111", "00000011000", "000011001010", "000011001011", "000011001100", "000011001101", "000001101000", "000001101001",
    "000001101010", "000001101011", "000011010010", "000011010011", "000011010100", "000011010101", "000011010110", "000011010111",
    "000001101100", "000001101101", "000011011010", "000011011011", "000001010100", "000001010101", "000001010110", "000001010111",
    "000001100100", "000001100101", "000001010010", "000001010011", "000000100100", "000000110111", "000000111000", "000000100111",
    "000000101000", "000001011000", "000001011001", "000000101011", "000000101100", "000001011010", "000001100110", "000001100111",
]
WHITE_MAKEUP = {64: "11011", 128: "10010", 192: "010111"}
BLACK_MAKEUP = {64: "0000001111", 128: "000011001000", 192: "000011001001"}
VERT_CODES = {0: "1", 1: "011", 2: "000011", 3: "0000011", -1: "010", -2: "000010", -3: "0000010"}


def encode_run_bits(run: int, is_black: bool) -> str:
    bits = []
    makeup = BLACK_MAKEUP if is_black else WHITE_MAKEUP
    term = BLACK_TERM if is_black else WHITE_TERM
    while run >= 64:
        m = min(192, (run // 64) * 64)
        bits.append(makeup[m])
        run -= m
    bits.append(term[run])
    return "".join(bits)


def encode_ccitt_g4(rows: list[list[int]]) -> bytes:
    w = len(rows[0])
    ref = [0] * w
    bitstr = []
    for line in rows:
        a0 = -1
        color = 0
        while max(a0, 0) < w:
            start = max(a0, 0)
            a1 = w
            for p in range(start, w):
                if line[p] != color:
                    a1 = p
                    break
            target = 1 - color
            b1 = w
            for p in range(0 if a0 < 0 else a0 + 1, w):
                prev = 0 if p == 0 else ref[p - 1]
                if ref[p] == target and (p == 0 or ref[p] != prev):
                    b1 = p
                    break
            b2 = w
            if b1 < w:
                for p in range(b1 + 1, w):
                    if ref[p] != ref[b1]:
                        b2 = p
                        break
            if b2 < a1:
                bitstr.append("0001")  # Pass
                a0 = b2
            elif -3 <= a1 - b1 <= 3:
                bitstr.append(VERT_CODES[a1 - b1])
                a0 = a1
                color = 1 - color
            else:
                a2 = w
                for p in range(a1 + 1, w):
                    if line[p] != (1 - color):
                        a2 = p
                        break
                bitstr.append("001")  # Horizontal
                bitstr.append(encode_run_bits(a1 - start, color == 1))
                bitstr.append(encode_run_bits(a2 - a1, color == 0))
                a0 = a2
        ref = line
    bitstr.append("000000000001000000000001")  # EOFB
    full = "".join(bitstr)
    while len(full) % 8 != 0:
        full += "0"
    return bytes(int(full[i : i + 8], 2) for i in range(0, len(full), 8))


def make_g4_two_pages_tiff() -> tuple[bytes, bytes]:
    w, h = 96, 64
    page1_bits = []
    page2_bits = []
    for y in range(h):
        r1 = []
        r2 = []
        for x in range(w):
            # Page 1: outer technical frame + crosshairs
            b1 = 1 if (x in (8, w - 9) or y in (8, h - 9) or (y == h // 2 and 16 <= x < w - 16)) else 0
            # Page 2: stepped bracket silhouette + diagonal hatching
            b2 = 1 if (12 <= x <= 78 and 10 <= y <= 50 and (x in (12, 78) or y in (10, 50) or (x + y) % 11 == 0)) else 0
            r1.append(b1)
            r2.append(b2)
        page1_bits.append(r1)
        page2_bits.append(r2)

    strip1 = encode_ccitt_g4(page1_bits)
    strip2 = encode_ccitt_g4(page2_bits)

    # WhiteIsZero (photometric=0): bit 0 -> Gray8 255, bit 1 -> Gray8 0.
    gray1 = bytes(0 if b else 255 for row in page1_bits for b in row)
    gray2 = bytes(0 if b else 255 for row in page2_bits for b in row)
    sha1 = "sha256:" + hashlib.sha256(gray1).hexdigest()
    sha2 = "sha256:" + hashlib.sha256(gray2).hexdigest()
    sha_txt = f"page0 {sha1}\npage1 {sha2}\n".encode("ascii")

    # Assemble 2-page Little-Endian TIFF.
    buf = bytearray(b"II\x2a\x00")
    buf += struct.pack("<I", 8)  # IFD0 at offset 8

    def write_ifd(strip_data: bytes, has_next: bool) -> int:
        # 12 tags per IFD -> 2 + 12*12 + 4 = 150 bytes; followed by 2 rationals (16 bytes) + strip
        ifd_off = len(buf)
        xres_off = ifd_off + 150
        yres_off = xres_off + 8
        strip_off = yres_off + 8
        next_off = strip_off + len(strip_data) if has_next else 0
        if has_next and next_off % 2 == 1:
            next_off += 1

        entries = [
            (256, 4, 1, w),               # ImageWidth
            (257, 4, 1, h),               # ImageLength
            (258, 3, 1, 1),               # BitsPerSample = 1
            (259, 3, 1, 4),               # Compression = 4 (CCITT G4)
            (262, 3, 1, 0),               # Photometric = 0 (WhiteIsZero)
            (273, 4, 1, strip_off),       # StripOffsets
            (277, 3, 1, 1),               # SamplesPerPixel = 1
            (278, 4, 1, h),               # RowsPerStrip
            (279, 4, 1, len(strip_data)), # StripByteCounts
            (282, 5, 1, xres_off),        # XResolution = 300/1
            (283, 5, 1, yres_off),        # YResolution = 300/1
            (296, 3, 1, 2),               # ResolutionUnit = 2 (inch)
        ]
        buf.extend(struct.pack("<H", len(entries)))
        for tag, typ, cnt, val in entries:
            buf.extend(struct.pack("<HHII", tag, typ, cnt, val))
        buf.extend(struct.pack("<I", next_off))
        buf.extend(struct.pack("<II", 300, 1))
        buf.extend(struct.pack("<II", 300, 1))
        buf.extend(strip_data)
        if has_next and len(buf) % 2 == 1:
            buf.append(0)
        return next_off

    write_ifd(strip1, has_next=True)
    write_ifd(strip2, has_next=False)
    return bytes(buf), sha_txt


def main() -> int:
    check = "--check" in sys.argv
    g4_tif, g4_sha = make_g4_two_pages_tiff()
    files = {
        "a3_landscape.pdf": make_a3_landscape(),
        "rotated.pdf": make_rotated(),
        "three_pages.pdf": make_three_pages(),
        "scan_300dpi.png": make_scan_300dpi_png(),
        "nodpi.png": make_nodpi_png(),
        "g4_two_pages.tif": g4_tif,
        "g4_two_pages.sha256": g4_sha,
        "encrypted.pdf": make_encrypted(),
        "js_action.pdf": make_js_action(),
        "pages201.pdf": make_pages201(),
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
    print(f"io-draw fixtures ok ({len(files)} files)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
