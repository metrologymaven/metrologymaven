#!/usr/bin/env python3
"""Independent fixture generator for spec node UNC.RDL.TRACIM.ARCH.

Generates fixtures/rdl/tracim/arch/exchange1.json containing:
  - synthetic order_key (never a real credential),
  - request exchange with protocol-marked secret_spans covering the X-TraCIM-Order-Key header,
  - returned report exchange (stored verbatim, zero secret spans),
  - leaked_request exchange where the key also appears in an unmarked JSON body field (AC-2),
  - expected redacted request text and independent SHA-256 digests of redacted request & report.

Run with `--check` to verify committed fixtures match this generator byte-for-byte.
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
OUT = HERE / "exchange1.json"
MARKER = "[REDACTED]"


def _sha256(data: bytes) -> str:
    return "sha256:" + hashlib.sha256(data).hexdigest()


def build_fixture() -> bytes:
    key = "TRACIM-SYNTH-ORDER-KEY-0001-EXAMPLE"
    timestamp = "2026-11-22T06:00:00Z"
    destination = "https://tracim.example.invalid/v1/orders/submit"

    req_prefix = "POST /v1/orders/submit HTTP/1.1\r\nHost: tracim.example.invalid\r\nX-TraCIM-Order-Key: "
    req_suffix = "\r\nContent-Type: application/json\r\n\r\n{\"suite\":\"gaussian-line\",\"order_ref\":\"ORD-42\"}\n"
    req_raw = req_prefix + key + req_suffix
    key_start = len(req_prefix.encode("utf-8"))
    key_end = key_start + len(key.encode("utf-8"))
    req_redacted = req_prefix + MARKER + req_suffix

    # Unmarked leak variant (AC-2): header span marked, but key also echoed inside unmarked body field.
    leak_suffix = (
        "\r\nContent-Type: application/json\r\n\r\n"
        f"{{\"suite\":\"gaussian-line\",\"debug_echo\":\"{key}\"}}\n"
    )
    leak_raw = req_prefix + key + leak_suffix

    report_raw = (
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n"
        "<TraCIMReport suite=\"gaussian-line\" orderRef=\"ORD-42\" status=\"PASS\">\n"
        "  <TestCase id=\"line3-0001\" maxDeviationUm=\"0.012\"/>\n"
        "</TraCIMReport>\n"
    )

    doc = {
        "schema": "tracim-archive-fixture/v1",
        "licence": "CC-BY-4.0",
        "order_key": key,
        "request": {
            "direction": "request",
            "timestamp": timestamp,
            "destination": destination,
            "payload": req_raw,
            "secret_spans": [[key_start, key_end]],
            "expected_redacted": req_redacted,
            "expected_digest": _sha256(req_redacted.encode("utf-8")),
        },
        "report": {
            "direction": "report",
            "timestamp": "2026-11-22T06:00:05Z",
            "destination": destination,
            "payload": report_raw,
            "secret_spans": [],
            "expected_digest": _sha256(report_raw.encode("utf-8")),
        },
        "leaked_request": {
            "direction": "request",
            "timestamp": timestamp,
            "destination": destination,
            "payload": leak_raw,
            "secret_spans": [[key_start, key_end]],
        },
    }
    return (json.dumps(doc, indent=2, sort_keys=True, ensure_ascii=False) + "\n").encode("utf-8")


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("--check", action="store_true", help="verify committed fixture matches generator")
    args = ap.parse_args()

    expected = build_fixture()
    if args.check:
        if not OUT.is_file() or OUT.read_bytes() != expected:
            print(f"ERROR: {OUT} differs from generator", file=sys.stderr)
            return 1
        return 0

    OUT.write_bytes(expected)
    print(f"wrote {OUT} ({len(expected)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main())
