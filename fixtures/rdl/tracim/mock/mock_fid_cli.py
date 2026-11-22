#!/usr/bin/env python3
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
