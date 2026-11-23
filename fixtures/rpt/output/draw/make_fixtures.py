#!/usr/bin/env python3
"""Deterministic fixture generator and verifier for RPT.OUTPUT.PDF.DRAW (AC-3, AC-4).

Generates under fixtures/rpt/output/draw/:
- ballooned_sheet.png: synthetic 160x100 RGB PNG technical drawing sheet
- ballooned_sheet.resolved.json: resolved_report@1 with a `drawing` block placing 12 balloons
  at exact 0.01 mm coordinates on a 160x100 mm sheet (1:1 µm mapping on A4 portrait)

Usage:
  python3 fixtures/rpt/output/draw/make_fixtures.py          # write fixtures
  python3 fixtures/rpt/output/draw/make_fixtures.py --check  # verify byte-for-byte
"""

import hashlib
import json
import pathlib
import struct
import sys
import zlib

HERE = pathlib.Path(__file__).resolve().parent


def canon_bytes(obj) -> bytes:
    return json.dumps(obj, sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def sha256_str(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def png_chunk(ctype: bytes, data: bytes) -> bytes:
    crc = zlib.crc32(ctype + data) & 0xFFFFFFFF
    return struct.pack(">I", len(data)) + ctype + data + struct.pack(">I", crc)


def make_sheet_png(width: int = 160, height: int = 100) -> bytes:
    raw = bytearray()
    for y in range(height):
        raw.append(0)  # Filter None
        for x in range(width):
            # Subtle blueprint border and centerlines
            is_border = x in (4, width - 5) or y in (4, height - 5)
            is_part = (30 <= x <= 130 and 25 <= y <= 75) and (x in (30, 130) or y in (25, 75))
            if is_border or is_part:
                raw.extend((32, 48, 72))
            else:
                raw.extend((248, 250, 252))
    ihdr = struct.pack(">IIBBBBB", width, height, 8, 2, 0, 0, 0)
    compressed = zlib.compress(bytes(raw), level=6)
    return (
        b"\x89PNG\r\n\x1a\n"
        + png_chunk(b"IHDR", ihdr)
        + png_chunk(b"IDAT", compressed)
        + png_chunk(b"IEND", b"")
    )


def make_ballooned_sheet_resolved(sheet_digest: str) -> bytes:
    # 12 balloons positioned at exact 0.01 mm coordinates on a 160.00 x 100.00 mm sheet.
    balloons = []
    coords_01mm = [
        (2000, 1500),
        (4500, 1500),
        (7000, 1500),
        (9500, 1500),
        (12000, 1500),
        (14000, 1500),
        (2000, 8500),
        (4500, 8500),
        (7000, 8500),
        (9500, 8500),
        (12000, 8500),
        (14000, 8500),
    ]
    for num, (x, y) in enumerate(coords_01mm, start=1):
        balloons.append({"number": num, "x_01mm": x, "y_01mm": y, "radius_01mm": 250})

    content = {
        "pages": [
            {
                "id": "019a9f50-0001-7000-8000-000000000010",
                "blocks": [
                    {
                        "id": "019a9f50-0001-7000-8000-000000000001",
                        "kind": "text",
                        "kind_version": "1",
                        "frame": None,
                        "payload": {
                            "schema": "resolved_text@1",
                            "style": "heading",
                            "text": "Ballooned Inspection Drawing — Sheet A",
                        },
                    },
                    {
                        "id": "019a9f50-0001-7000-8000-000000000002",
                        "kind": "drawing",
                        "kind_version": "1",
                        "frame": {"x": 1500, "y": 3000, "width": 16000, "height": 10000},
                        "payload": {
                            "schema": "resolved_drawing@1",
                            "sheet_digest": sheet_digest,
                            "media_type": "image/png",
                            "sheet_width_01mm": 16000,
                            "sheet_height_01mm": 10000,
                            "balloons": balloons,
                        },
                    },
                ],
            }
        ],
        "provenance": {
            "formatters": [{"id": "fmt.length@1", "version": "1"}],
            "head": "sha256:" + "33" * 32,
            "kernel_version": "0.1.0",
            "locale": "en",
            "locale_release": "i18n-locale@1",
            "template": None,
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


def main() -> int:
    check = "--check" in sys.argv
    png_bytes = make_sheet_png()
    sheet_digest = sha256_str(png_bytes)
    resolved_bytes = make_ballooned_sheet_resolved(sheet_digest)

    expected = {
        "ballooned_sheet.png": png_bytes,
        "ballooned_sheet.resolved.json": resolved_bytes,
    }
    bad = 0
    for name, want in expected.items():
        path = HERE / name
        if check:
            if not path.exists() or path.read_bytes() != want:
                print(f"DRIFT: {path}", file=sys.stderr)
                bad += 1
        else:
            path.write_bytes(want)

    if bad:
        return 1
    print("draw fixtures ok")
    return 0


if __name__ == "__main__":
    sys.exit(main())
