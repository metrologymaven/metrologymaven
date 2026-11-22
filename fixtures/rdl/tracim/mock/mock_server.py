#!/usr/bin/env python3
"""Loopback-only HTTP mock service for TraCIM public tests (CC-BY-4.0, no real TraCIM data)."""

from __future__ import annotations

import hashlib
import json
import threading
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

CASES_PATH = Path(__file__).resolve().parent / "cases.json"


class TracimMockServer:
    """In-process loopback HTTP server bound to `127.0.0.1:0`."""

    def __init__(self, *, expected_key: str, verdict_mode: str = "passed") -> None:
        self.expected_key = expected_key
        self.verdict_mode = verdict_mode
        self.requests: list[tuple[str, str]] = []
        self._cases = json.loads(CASES_PATH.read_text(encoding="utf-8"))

        outer = self

        class Handler(BaseHTTPRequestHandler):
            def log_message(self, format: str, *args: object) -> None:
                return

            def do_POST(self) -> None:
                length = int(self.headers.get("Content-Length", "0"))
                body = self.rfile.read(min(length, 1_048_576))
                outer.requests.append((self.path, body.decode("utf-8", errors="replace")))
                try:
                    data = json.loads(body.decode("utf-8"))
                except json.JSONDecodeError:
                    self._send_json(400, {"error": "malformed"})
                    return

                key = data.get("key", "")
                if key != outer.expected_key:
                    self._send_json(401, {"error": "tracim_key_invalid"})
                    return

                if self.path == "/fetch":
                    self._send_json(
                        200,
                        {
                            "test_id": "tracim-public-mock-001",
                            "test_kind": data.get("test_kind", "public"),
                            "cases": outer._cases["cases"],
                        },
                    )
                    return

                if self.path == "/submit":
                    report_digest = "sha256:" + hashlib.sha256(body).hexdigest()
                    self._send_json(
                        200,
                        {
                            "test_id": "tracim-public-mock-001",
                            "verdict": outer.verdict_mode,
                            "report_digest": report_digest,
                        },
                    )
                    return

                self._send_json(404, {"error": "not_found"})

            def _send_json(self, code: int, payload: dict) -> None:
                raw = json.dumps(payload, sort_keys=True).encode("utf-8")
                self.send_response(code)
                self.send_header("Content-Type", "application/json")
                self.send_header("Content-Length", str(len(raw)))
                self.end_headers()
                self.wfile.write(raw)

        self._httpd = ThreadingHTTPServer(("127.0.0.1", 0), Handler)
        host, port = self._httpd.server_address
        self.url = f"http://{host}:{port}"
        self._thread = threading.Thread(target=self._httpd.serve_forever, daemon=True)

    def start(self) -> str:
        self._thread.start()
        return self.url

    def stop(self) -> None:
        self._httpd.shutdown()
        self._httpd.server_close()
        self._thread.join(timeout=5)
