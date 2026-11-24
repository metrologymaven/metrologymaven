#!/usr/bin/env python3
"""Nightly TraCIM public-test runner, config guard, and artifact writer (UNC.RDL.TRACIM.PUBLIC).

Invokes the built `fid` binary artifact with:
    fid rdl submit-tracim --test-kind public --key-ci-secret --json

Publishes `tracim-public-latest.json` (`schema: "tracim-public/v1"`):
  - `schema`: "tracim-public/v1"
  - `test_kind`: "public"
  - `verdict`: "passed" | "failed" | "error"
  - `kernel_semver`: semver string or null
  - `manifest_digest`: "sha256:..." string or null
  - `reason`: null (when passed) | "verdict_failed" | "service_unreachable" | "paid_key_exposed"

Never orders paid tests, fails its config check if any paid-order key secret is reachable,
and never writes the key or raw test data into the published artifact.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import subprocess
import sys
from pathlib import Path
from typing import Any, Mapping, Sequence

SCHEMA_TAG = "tracim-public/v1"
PUBLIC_KEY_ENV = "TRACIM_PUBLIC_KEY"

# Patterns indicating a paid/order key secret or blanket secret exposure in a CI workflow.
FORBIDDEN_WORKFLOW_PATTERNS: tuple[tuple[str, re.Pattern[str]], ...] = (
    ("paid_secret_ref", re.compile(r"secrets\.[A-Za-z0-9_]*(PAID|ORDER)[A-Za-z0-9_]*", re.IGNORECASE)),
    ("paid_env_var", re.compile(r"\bTRACIM_(PAID|ORDER)[A-Za-z0-9_]*\b", re.IGNORECASE)),
    ("broad_secrets_dump", re.compile(r"toJSON\s*\(\s*secrets\s*\)", re.IGNORECASE)),
    ("paid_test_kind", re.compile(r"--test-kind(?:=|\s+)(order|paid)\b", re.IGNORECASE)),
)

# Forbidden runtime environment variable names that indicate a paid/order secret is reachable.
FORBIDDEN_ENV_RE = re.compile(r"(^|_)(TRACIM_PAID|TRACIM_ORDER|PAID_ORDER_KEY|TRACIM_ORDER_KEY)", re.IGNORECASE)

# Allowed secret references inside the nightly workflow.
ALLOWED_SECRETS = frozenset({"TRACIM_PUBLIC_KEY", "GITHUB_TOKEN"})
ANY_SECRET_REF_RE = re.compile(r"secrets\.([A-Za-z0-9_]+)")

# Allowed fields copied from `fid rdl submit-tracim --json` into the published artifact.
# Raw test data, inputs, payloads, and keys are strictly excluded by whitelist.
SHA256_RE = re.compile(r"^sha256:[0-9a-f]{64}$")


def check_workflow_config(
    workflow_text: str,
    env: Mapping[str, str] | None = None,
) -> list[str]:
    """Validates workflow text and optional runtime env against REQ-UNC.RDL.TRACIM.PUBLIC-001.

    Returns a deterministic list of violation strings; empty list means compliant.
    """
    violations: list[str] = []

    # Strip full-line YAML comments before scanning so doc comments explaining the rule
    # don't trigger false positives, while still checking all live YAML keys/values/run lines.
    live_lines = [line for line in workflow_text.splitlines() if not line.lstrip().startswith("#")]
    live_text = "\n".join(live_lines)

    for label, pat in FORBIDDEN_WORKFLOW_PATTERNS:
        if pat.search(live_text):
            violations.append(f"paid_key_exposed: forbidden workflow pattern `{label}` detected")

    for match in ANY_SECRET_REF_RE.finditer(live_text):
        secret_name = match.group(1)
        if secret_name not in ALLOWED_SECRETS:
            violations.append(f"paid_key_exposed: secret `{secret_name}` is not in allowed set {sorted(ALLOWED_SECRETS)}")

    if "--test-kind public" not in live_text and "--test-kind=public" not in live_text:
        violations.append("config_invalid: workflow must explicitly invoke `--test-kind public`")

    if env is not None:
        for k, v in sorted(env.items()):
            if v and FORBIDDEN_ENV_RE.search(k):
                violations.append(f"paid_key_exposed: forbidden environment variable `{k}` is reachable")

    return violations


def build_artifact(
    *,
    verdict: str,
    kernel_semver: str | None,
    manifest_digest: str | None,
    reason: str | None,
) -> dict[str, Any]:
    """Constructs a `tracim-public/v1` artifact containing only whitelisted metadata."""
    return {
        "kernel_semver": kernel_semver,
        "manifest_digest": manifest_digest,
        "reason": reason,
        "schema": SCHEMA_TAG,
        "test_kind": "public",
        "verdict": verdict,
    }


def write_artifact(out_path: Path, artifact: dict[str, Any], secret_value: str | None = None) -> None:
    """Writes canonical sorted-key JSON to `out_path`, refusing to write if `secret_value` appears."""
    payload = json.dumps(artifact, sort_keys=True, indent=2, ensure_ascii=False) + "\n"
    if secret_value and secret_value in payload:
        raise RuntimeError("refusing to write artifact: secret key detected in output")
    out_path.parent.mkdir(parents=True, exist_ok=True)
    out_path.write_text(payload, encoding="utf-8")


def _extract_str(obj: Mapping[str, Any], key: str) -> str | None:
    val = obj.get(key)
    if isinstance(val, str) and val.strip():
        return val.strip()
    return None


def run_public_test(
    *,
    fid_argv: Sequence[str],
    out_path: Path,
    workflow_path: Path | None = None,
    destination: str | None = None,
    env: Mapping[str, str] | None = None,
    timeout_s: int = 300,
) -> tuple[int, dict[str, Any]]:
    """Runs the public-test workflow step, writes `out_path`, and returns `(exit_code, artifact)`."""
    effective_env = dict(os.environ if env is None else env)

    # 1. Fail-closed configuration check (REQ-001, AC-3).
    violations: list[str] = []
    if workflow_path is not None:
        try:
            wf_text = workflow_path.read_text(encoding="utf-8")
        except OSError as exc:
            artifact = build_artifact(
                verdict="error",
                kernel_semver=None,
                manifest_digest=None,
                reason=f"config_unreadable: {exc}",
            )
            write_artifact(out_path, artifact)
            return 1, artifact
        violations.extend(check_workflow_config(wf_text, env=effective_env))
    else:
        for k, v in sorted(effective_env.items()):
            if v and FORBIDDEN_ENV_RE.search(k):
                violations.append(f"paid_key_exposed: forbidden environment variable `{k}` is reachable")

    if violations:
        reason = "paid_key_exposed" if any("paid_key_exposed" in v for v in violations) else violations[0]
        artifact = build_artifact(
            verdict="error",
            kernel_semver=None,
            manifest_digest=None,
            reason=reason,
        )
        write_artifact(out_path, artifact)
        return 1, artifact

    secret_val = effective_env.get(PUBLIC_KEY_ENV, "")
    if not secret_val and os.name == "nt":
        for k, v in effective_env.items():
            if k.upper() == PUBLIC_KEY_ENV:
                secret_val = v
                break

    # 2. Build a minimal child environment carrying only non-secret runtime vars + TRACIM_PUBLIC_KEY.
    keep_vars = (
        "PATH",
        "HOME",
        "TMPDIR",
        "TEMP",
        "TMP",
        "SYSTEMROOT",
        "SYSTEMDRIVE",
        "WINDIR",
        "COMSPEC",
        "PATHEXT",
        "USERPROFILE",
        "LANG",
        "LC_ALL",
        "PYTHONPATH",
    )
    child_env: dict[str, str] = {}
    if os.name == "nt":
        env_by_upper = {k.upper(): v for k, v in effective_env.items()}
        for keep in keep_vars:
            if keep in env_by_upper:
                child_env[keep] = env_by_upper[keep]
    else:
        for keep in keep_vars:
            if keep in effective_env:
                child_env[keep] = effective_env[keep]
    if secret_val:
        child_env[PUBLIC_KEY_ENV] = secret_val
    if destination:
        child_env["FID_TRACIM_ENDPOINT"] = destination

    cmd = [*fid_argv, "rdl", "submit-tracim", "--test-kind", "public", "--key-ci-secret", "--json"]
    if destination:
        cmd.extend(["--destination", destination])

    try:
        proc = subprocess.run(
            cmd,
            env=child_env,
            capture_output=True,
            text=True,
            timeout=timeout_s,
            check=False,
        )
    except (OSError, subprocess.TimeoutExpired):
        artifact = build_artifact(
            verdict="error",
            kernel_semver=None,
            manifest_digest=None,
            reason="service_unreachable",
        )
        write_artifact(out_path, artifact, secret_value=secret_val)
        return 1, artifact

    # 3. Parse JSON output (supporting both flat mock JSON and AUTO.SURF.ENV Envelope) and classify verdict/reason (REQ-002, REQ-003).
    parsed: dict[str, Any] | None = None
    raw_out = proc.stdout.strip()
    if raw_out:
        try:
            candidate = json.loads(raw_out)
            if isinstance(candidate, dict):
                parsed = candidate
        except json.JSONDecodeError:
            parsed = None

    if parsed is None:
        stderr_lower = proc.stderr.lower()
        reason = (
            "service_unreachable"
            if any(tok in stderr_lower for tok in ("unreachable", "connection refused", "timed out", "connect"))
            else "verdict_failed"
        )
        artifact = build_artifact(
            verdict="error",
            kernel_semver=None,
            manifest_digest=None,
            reason=reason,
        )
        write_artifact(out_path, artifact, secret_value=secret_val)
        return 1, artifact

    envelope_outcome: str | None = None
    payload_obj: Mapping[str, Any] = parsed
    if "envelope_version" in parsed and "outcome" in parsed:
        envelope_outcome = _extract_str(parsed, "outcome")
        res_obj = parsed.get("result")
        if isinstance(res_obj, Mapping):
            payload_obj = res_obj

    raw_err = parsed.get("error")
    error_code: str | None = None
    if isinstance(raw_err, Mapping):
        details = raw_err.get("details")
        if isinstance(details, Mapping):
            error_code = _extract_str(details, "tracim_error") or _extract_str(details, "code")
        if not error_code:
            error_code = _extract_str(raw_err, "code")
        err_msg = _extract_str(raw_err, "message") or ""
        if not error_code or error_code in ("precondition_failed", "internal_error", "timeout"):
            if any(
                tok in err_msg.lower()
                for tok in ("service_unreachable", "tracim_service_error", "unreachable", "timed out")
            ):
                error_code = "service_unreachable"
    else:
        error_code = _extract_str(parsed, "error") or _extract_str(parsed, "code")
    if envelope_outcome in ("not_run", "no_reply"):
        error_code = error_code or "service_unreachable"

    verdict = _extract_str(payload_obj, "verdict") or "error"
    kernel_semver = _extract_str(payload_obj, "kernel_semver")
    manifest_digest = _extract_str(payload_obj, "manifest_digest")
    if manifest_digest is not None and not SHA256_RE.match(manifest_digest):
        manifest_digest = None

    if error_code in ("service_unreachable", "tracim_service_error", "timeout"):
        artifact = build_artifact(
            verdict="error",
            kernel_semver=kernel_semver,
            manifest_digest=manifest_digest,
            reason="service_unreachable",
        )
        write_artifact(out_path, artifact, secret_value=secret_val)
        return 1, artifact

    if (
        proc.returncode == 0
        and envelope_outcome in (None, "computed", "committed")
        and verdict == "passed"
        and kernel_semver
        and manifest_digest
    ):
        artifact = build_artifact(
            verdict="passed",
            kernel_semver=kernel_semver,
            manifest_digest=manifest_digest,
            reason=None,
        )
        write_artifact(out_path, artifact, secret_value=secret_val)
        return 0, artifact

    out_verdict = "failed" if (verdict == "failed" and envelope_outcome in (None, "computed", "committed")) else "error"
    artifact = build_artifact(
        verdict=out_verdict,
        kernel_semver=kernel_semver,
        manifest_digest=manifest_digest,
        reason="verdict_failed",
    )
    write_artifact(out_path, artifact, secret_value=secret_val)
    return 1, artifact


def main(argv: Sequence[str] | None = None) -> int:
    parser = argparse.ArgumentParser(prog="tracim_public", description="TraCIM nightly public-test job runner")
    sub = parser.add_subparsers(dest="cmd", required=True)

    p_check = sub.add_parser("check-config", help="Verify workflow never exposes paid-order secrets")
    p_check.add_argument("--workflow", required=True, type=Path)

    p_run = sub.add_parser("run", help="Run public test via built fid binary and emit tracim-public/v1 artifact")
    p_run.add_argument("--out", required=True, type=Path)
    p_run.add_argument("--workflow", type=Path, default=None)
    p_run.add_argument("--destination", type=str, default=None)
    p_run.add_argument("fid_cmd", nargs=argparse.REMAINDER)

    args = parser.parse_args(argv)
    if args.cmd == "check-config":
        wf_text = args.workflow.read_text(encoding="utf-8")
        violations = check_workflow_config(wf_text, env=os.environ)
        if violations:
            for v in violations:
                print(v, file=sys.stderr)
            return 1
        print(f"tracim-public config check: ok ({args.workflow})")
        return 0

    fid_cmd = list(args.fid_cmd)
    if fid_cmd and fid_cmd[0] == "--":
        fid_cmd = fid_cmd[1:]
    if not fid_cmd:
        print("run requires the fid command after `--`", file=sys.stderr)
        return 2

    code, artifact = run_public_test(
        fid_argv=fid_cmd,
        out_path=args.out,
        workflow_path=args.workflow,
        destination=args.destination,
    )
    print(json.dumps(artifact, sort_keys=True))
    return code


if __name__ == "__main__":
    raise SystemExit(main())
