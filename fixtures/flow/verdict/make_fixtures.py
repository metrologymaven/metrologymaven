#!/usr/bin/env python3
"""Independent stdlib fixture generator for FLOW.RUNNER.VERDICT (fixtures/flow/verdict/).

Generates:
- r1.json: canonical RFC 8785 acceptance-report@1 with gating, tracked, and assisted packages
- r1.sha256: independent SHA-256 digest of r1.json
- pii-host-path.json: report containing a host path (AC-4 VERDICT-PII)
- pii-email.json: report containing an email address (VERDICT-PII)
- trend-40.jsonl: 40 nights of JOB.ACC.02 interleaved with JOB.ACC.01 records (AC-2); each
  record names the digest of its night's report
- reports-40/<hex>.json: the 40 nightly reports the trend records name (canonical, one per night)

Trend record rules (hand-labelled from the spec node and the #440 review): night = UTC date of the
report's started_at; machine_s = the largest machine time of the package; report = the report's
SHA-256 digest of its RFC 8785 form.

Usage:
  python3 fixtures/flow/verdict/make_fixtures.py         # write fixtures
  python3 fixtures/flow/verdict/make_fixtures.py --check # verify fixtures are up to date
"""

from __future__ import annotations

import datetime as dt
import hashlib
import json
import pathlib
import sys

HERE = pathlib.Path(__file__).resolve().parent


def _jcs_norm(obj: object) -> object:
    if isinstance(obj, float) and obj.is_integer():
        return int(obj)
    if isinstance(obj, dict):
        return {k: _jcs_norm(v) for k, v in obj.items()}
    if isinstance(obj, list):
        return [_jcs_norm(v) for v in obj]
    return obj


def canon_bytes(obj: object) -> bytes:
    """RFC 8785 canonical JSON for plain ASCII-safe objects/integers/strings/booleans/simple floats."""
    return json.dumps(_jcs_norm(obj), sort_keys=True, separators=(",", ":"), ensure_ascii=False).encode("utf-8")


def base_report() -> dict:
    return {
        "format": "acceptance-report@1",
        "build_id": "2026.11.20-nightly.1",
        "manifest_digest": "sha256:" + "11" * 32,
        "suite_digest": "sha256:" + "22" * 32,
        "started_at": "2026-11-20T01:00:00Z",
        "ended_at": "2026-11-20T01:14:32Z",
        "machines": [
            {"id": "ref-linux-01", "host": "x86_64-linux-ref01"},
            {"id": "ref-arm-01", "host": "aarch64-darwin-ref01"},
        ],
        "packages": [
            {
                "job": "JOB.ACC.01",
                "variant": "nominal-mm",
                "gating": "gating",
                "assistance": "unassisted",
                "non_gating": False,
                "replay_outcome": "identical",
                "checks": [
                    {"id": "chk-replay", "kind": "replay_outcome", "outcome": "pass"},
                    {"id": "chk-u", "kind": "value_within_u", "outcome": "pass"},
                ],
                "machine_times": {"ref-arm-01": 14.5, "ref-linux-01": 12.25},
                "verdict": "pass",
                "reasons": [],
            },
            {
                "job": "JOB.ACC.02",
                "variant": "assisted-draft",
                "gating": "gating",
                "assistance": "assisted",
                "non_gating": True,
                "replay_outcome": "equivalent",
                "checks": [
                    {"id": "chk-replay", "kind": "replay_outcome", "outcome": "pass"},
                    {
                        "id": "chk-decision",
                        "kind": "decision_equals",
                        "outcome": "fail",
                        "reason": "characteristic C3 expected pass got fail",
                    },
                ],
                "machine_times": {"ref-arm-01": 21.0, "ref-linux-01": 18.75},
                "verdict": "fail",
                "reasons": ["characteristic C3 expected pass got fail"],
            },
            {
                "job": "JOB.ACC.03",
                "variant": "tracked-preview",
                "gating": "tracked",
                "assistance": "unassisted",
                "non_gating": True,
                "replay_outcome": "not_replayable",
                "checks": [
                    {
                        "id": "chk-replay",
                        "kind": "replay_outcome",
                        "outcome": "skipped",
                        "reason": "journal requires newer kernel minor",
                    }
                ],
                "machine_times": {"ref-linux-01": 0.5},
                "verdict": "not_replayable",
                "reasons": ["journal requires newer kernel minor"],
            },
        ],
        "reliability": {
            "total_packages": 3,
            "passed": 1,
            "failed": 1,
            "not_replayable": 1,
            "gating_passed": 1,
            "gating_failed": 0,
            "non_gating_count": 2,
            "crash_recovery_passed": True,
        },
    }


def make_files() -> dict[str, bytes]:
    r1 = base_report()
    r1_bytes = canon_bytes(r1)
    r1_sha = f"sha256:{hashlib.sha256(r1_bytes).hexdigest()}\n".encode("utf-8")

    pii_path = base_report()
    pii_path["packages"][1]["reasons"] = ["missing fixture at /home/alice/metrology/part.step"]
    pii_path["packages"][1]["checks"][1]["reason"] = "missing fixture at /home/alice/metrology/part.step"
    pii_path_bytes = canon_bytes(pii_path)

    pii_email = base_report()
    pii_email["packages"][1]["reasons"] = ["contact alice@partner.example for data"]
    pii_email["packages"][1]["checks"][1]["reason"] = "contact alice@partner.example for data"
    pii_email_bytes = canon_bytes(pii_email)

    # 40 nights of JOB.ACC.02 (2026-10-12 .. 2026-11-20) interleaved with JOB.ACC.01, each night
    # with its own stored report.
    start = dt.date(2026, 10, 12)
    trend_lines: list[bytes] = []
    reports: dict[str, bytes] = {}
    for i in range(40):
        day = (start + dt.timedelta(days=i)).isoformat()
        report = nightly_report(day, i)
        report_bytes = canon_bytes(report)
        hexd = hashlib.sha256(report_bytes).hexdigest()
        reports[f"{REPORTS_40}/{hexd}.json"] = report_bytes
        for pkg in report["packages"]:
            rec = {
                "night": day,
                "job": pkg["job"],
                "variant": pkg["variant"],
                "verdict": pkg["verdict"],
                "machine_s": max(pkg["machine_times"].values()),
                "report": f"sha256:{hexd}",
            }
            trend_lines.append(canon_bytes(rec))
    trend_bytes = b"\n".join(trend_lines) + b"\n"

    files = {
        "r1.json": r1_bytes,
        "r1.sha256": r1_sha,
        "pii-host-path.json": pii_path_bytes,
        "pii-email.json": pii_email_bytes,
        "trend-40.jsonl": trend_bytes,
    }
    files.update(reports)
    return files


REPORTS_40 = "reports-40"


def nightly_report(day: str, i: int) -> dict:
    """One synthetic night: JOB.ACC.01 always passes; JOB.ACC.02 fails on every 7th night (i % 7 == 3).

    Verdicts follow the evidence rules: a pass has an identical replay, every check passing and a
    machine time for every declared machine; a fail names its failing check as the reason.
    """
    pkg01 = {
        "job": "JOB.ACC.01",
        "variant": "nominal-mm",
        "gating": "gating",
        "assistance": "unassisted",
        "non_gating": False,
        "replay_outcome": "identical",
        "checks": [{"id": "chk-replay", "kind": "replay_outcome", "outcome": "pass"}],
        "machine_times": {"ref-linux-01": 12.0 + (i % 5) * 0.25},
        "verdict": "pass",
        "reasons": [],
    }
    fails = i % 7 == 3
    checks_02 = [{"id": "chk-replay", "kind": "replay_outcome", "outcome": "pass"}]
    if fails:
        checks_02.append(
            {"id": "chk-u", "kind": "value_within_u", "outcome": "fail", "reason": "value outside uncertainty band"}
        )
    else:
        checks_02.append({"id": "chk-u", "kind": "value_within_u", "outcome": "pass"})
    pkg02 = {
        "job": "JOB.ACC.02",
        "variant": "nominal-mm",
        "gating": "gating",
        "assistance": "unassisted",
        "non_gating": False,
        "replay_outcome": "identical",
        "checks": checks_02,
        "machine_times": {"ref-linux-01": 20.0 + i * 0.5},
        "verdict": "fail" if fails else "pass",
        "reasons": ["value outside uncertainty band"] if fails else [],
    }
    passed = 1 if fails else 2
    failed = 1 if fails else 0
    return {
        "format": "acceptance-report@1",
        "build_id": f"{day}-nightly.1",
        "manifest_digest": "sha256:" + "11" * 32,
        "suite_digest": "sha256:" + "22" * 32,
        "started_at": f"{day}T01:00:00Z",
        "ended_at": f"{day}T01:20:00Z",
        "machines": [{"id": "ref-linux-01", "host": "x86_64-linux-ref01"}],
        "packages": [pkg01, pkg02],
        "reliability": {
            "total_packages": 2,
            "passed": passed,
            "failed": failed,
            "not_replayable": 0,
            "gating_passed": passed,
            "gating_failed": failed,
            "non_gating_count": 0,
        },
    }


def main(argv: list[str]) -> int:
    check = "--check" in argv[1:]
    files = make_files()
    mismatches: list[str] = []
    reports_dir = HERE / REPORTS_40
    stale = sorted(
        f"{REPORTS_40}/{p.name}" for p in (reports_dir.iterdir() if reports_dir.is_dir() else []) if f"{REPORTS_40}/{p.name}" not in files
    )
    if check:
        mismatches.extend(stale)
    else:
        reports_dir.mkdir(exist_ok=True)
        for name in stale:
            (HERE / name).unlink()
    for name, expected in sorted(files.items()):
        path = HERE / name
        if check:
            if not path.is_file() or path.read_bytes() != expected:
                mismatches.append(name)
        else:
            path.write_bytes(expected)
    if check:
        if mismatches:
            sys.stderr.write(f"fixtures/flow/verdict drift: {', '.join(mismatches)}\n")
            return 1
        print("fixtures/flow/verdict: ok")
    return 0


if __name__ == "__main__":
    raise SystemExit(main(sys.argv))
