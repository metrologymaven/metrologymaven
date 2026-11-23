#!/usr/bin/env python3
"""Deterministic fixture generator for UNC.RDL.TRACIM.PUBLIC loopback mock (AC-1..AC-3).

Generates CC-BY-4.0 synthetic fixtures under `fixtures/rdl/tracim/mock/` (no real TraCIM data):
  - `cases.json`: 3 synthetic LS cases (plane, circle, cylinder)
  - `mock_server.py`: loopback-only (`127.0.0.1`) HTTP mock supporting `pass` and `fail` verdicts
  - `mock_fid_cli.py`: stand-in `fid` binary adapter for CI/unit testing of `tools/rdl/tracim_public.py`
  - `workflow_paid_exposed.yml`: non-compliant workflow exposing a paid-order key secret (AC-3)

Usage: python3 fixtures/rdl/tracim/mock/make_fixtures.py [--check]
Licence: CC-BY-4.0.
"""

from __future__ import annotations

import json
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent

CASES = {
    "schema": "tracim-mock-cases/v1",
    "licence": "CC-BY-4.0",
    "cases": [
        {
            "aim": "ls.plane.svd",
            "id": "mock-plane-01",
            "points_mm": [[0.0, 0.0, 1.0], [10.0, 0.0, 1.0], [0.0, 10.0, 1.0], [10.0, 10.0, 1.0]],
        },
        {
            "aim": "ls.circle.gn",
            "id": "mock-circle-02",
            "points_mm": [[5.0, 0.0, 0.0], [0.0, 5.0, 0.0], [-5.0, 0.0, 0.0], [0.0, -5.0, 0.0]],
        },
        {
            "aim": "ls.cylinder.gn",
            "id": "mock-cylinder-03",
            "points_mm": [[3.0, 0.0, 0.0], [0.0, 3.0, 1.0], [-3.0, 0.0, 2.0], [0.0, -3.0, 3.0]],
        },
    ],
}

MOCK_SERVER_PY = '''#!/usr/bin/env python3
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
'''

MOCK_FID_CLI_PY = '''#!/usr/bin/env python3
"""Stand-in `fid` CLI artifact for UNC.RDL.TRACIM.PUBLIC loopback mock testing (CC-BY-4.0).

Accepts: `rdl submit-tracim --test-kind public --key-ci-secret --json [--destination URL]`
Reads `TRACIM_PUBLIC_KEY` from the environment into memory only, calls `/fetch` and `/submit`
on the loopback mock, and prints a `TracimRunRecord` JSON object without ever emitting the key.
"""

from __future__ import annotations

import argparse
import hashlib
import json
import os
import sys
import urllib.error
import urllib.request

KERNEL_SEMVER = "0.1.0-rc.1"
MANIFEST_DIGEST = "sha256:" + hashlib.sha256(b"fiducial-kernel-manifest-mock-v1").hexdigest()


def post_json(url: str, payload: dict) -> tuple[int, dict]:
    data = json.dumps(payload, sort_keys=True).encode("utf-8")
    req = urllib.request.Request(url, data=data, headers={"Content-Type": "application/json"}, method="POST")
    try:
        with urllib.request.urlopen(req, timeout=10) as resp:
            return resp.status, json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as exc:
        body = exc.read().decode("utf-8", errors="replace")
        try:
            return exc.code, json.loads(body)
        except json.JSONDecodeError:
            return exc.code, {"error": "tracim_service_error"}
    except (urllib.error.URLError, TimeoutError, OSError):
        return 0, {"error": "service_unreachable"}


def main(argv: list[str]) -> int:
    if len(argv) < 3 or argv[1] != "rdl" or argv[2] != "submit-tracim":
        print(json.dumps({"error": "bad_subcommand"}))
        return 2

    parser = argparse.ArgumentParser(prog="mock_fid_cli")
    parser.add_argument("--test-kind", required=True)
    parser.add_argument("--key-ci-secret", action="store_true")
    parser.add_argument("--json", action="store_true")
    parser.add_argument("--destination", default=os.environ.get("FID_TRACIM_ENDPOINT", ""))
    args = parser.parse_args(argv[3:])

    if args.test_kind != "public":
        print(json.dumps({"error": "paid_test_forbidden"}))
        return 1

    key = os.environ.get("TRACIM_PUBLIC_KEY", "") if args.key_ci_secret else ""
    if not key or not args.destination:
        print(json.dumps({"error": "service_unreachable"}))
        return 1

    status, fetched = post_json(f"{args.destination}/fetch", {"key": key, "test_kind": "public"})
    if status == 0:
        print(json.dumps({"error": "service_unreachable"}))
        return 1
    if status != 200:
        print(json.dumps({"error": fetched.get("error", "tracim_service_error")}))
        return 1

    status, submitted = post_json(
        f"{args.destination}/submit",
        {
            "key": key,
            "software_id": f"fiducial-kernel {KERNEL_SEMVER} km:{MANIFEST_DIGEST}",
            "case_count": len(fetched.get("cases", [])),
        },
    )
    if status == 0:
        print(json.dumps({"error": "service_unreachable"}))
        return 1
    if status != 200:
        print(json.dumps({"error": submitted.get("error", "tracim_service_error")}))
        return 1

    # Emit run record (also include a synthetic `raw_cases` field to verify tracim_public strips it).
    out = {
        "kernel_semver": KERNEL_SEMVER,
        "manifest_digest": MANIFEST_DIGEST,
        "raw_cases": fetched.get("cases", []),
        "report_digest": submitted.get("report_digest"),
        "test_id": submitted.get("test_id"),
        "verdict": submitted.get("verdict"),
    }
    print(json.dumps(out, sort_keys=True))
    return 0 if submitted.get("verdict") == "passed" else 1


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
'''

WORKFLOW_PAID_EXPOSED_YML = """name: tracim-public-bad
on:
  workflow_dispatch:
permissions:
  contents: read
jobs:
  nightly-public-test:
    runs-on: ubuntu-latest
    steps:
      - name: Bad step exposing paid-order key secret
        env:
          TRACIM_PUBLIC_KEY: ${{ secrets.TRACIM_PUBLIC_KEY }}
          TRACIM_PAID_ORDER_KEY: ${{ secrets.TRACIM_PAID_ORDER_KEY }}
        run: fid rdl submit-tracim --test-kind public --key-ci-secret --json
"""


def build_all() -> dict[str, bytes]:
    return {
        "cases.json": (json.dumps(CASES, indent=2, sort_keys=True) + "\n").encode("utf-8"),
        "mock_server.py": MOCK_SERVER_PY.encode("utf-8"),
        "mock_fid_cli.py": MOCK_FID_CLI_PY.encode("utf-8"),
        "workflow_paid_exposed.yml": WORKFLOW_PAID_EXPOSED_YML.encode("utf-8"),
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    files = build_all()
    if check:
        drift = []
        for name, expected in files.items():
            p = HERE / name
            if not p.exists() or p.read_bytes() != expected:
                drift.append(name)
        if drift:
            print(f"tracim mock fixtures out of date: {', '.join(drift)}", file=sys.stderr)
            return 1
        print(f"tracim mock fixtures: ok ({len(files)} files)")
        return 0

    for name, content in files.items():
        (HERE / name).write_bytes(content)
    print(f"wrote {len(files)} tracim mock fixtures")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
