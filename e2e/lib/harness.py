"""E2E test registry, isolated execution harness, assertions, and e2e-report/v1 writer."""

from __future__ import annotations

import json
import shutil
import subprocess
import sys
import tempfile
import time
import traceback
from dataclasses import dataclass
from pathlib import Path
from typing import Callable, Mapping, Sequence

from e2e.lib.binaries import (
    BUILD_ERROR,
    BUILT,
    HOST_C_TOOLCHAIN_BLOCKED,
    MISSING_CRATE_OR_BIN,
    BinaryManager,
)
from e2e.lib.env import Capabilities, clean_env

REPORT_SCHEMA = "e2e-report/v1"


class SkipCapability(Exception):
    """Raised when a test requires a host capability unavailable in `--mode auto`."""

    def __init__(self, reason: str, *, os_unsupported: bool = False) -> None:
        super().__init__(reason)
        self.reason = reason
        self.os_unsupported = os_unsupported


class TestFailure(AssertionError):
    """Raised on explicit requirement/contract assertion failures."""


@dataclass(frozen=True)
class TestSpec:
    test_id: str
    tier: int
    feature: str
    description: str
    capabilities_required: tuple[str, ...]
    fn: Callable[[TestContext], None]


_REGISTRY: list[TestSpec] = []


def e2e_test(
    *,
    test_id: str,
    tier: int,
    feature: str,
    description: str,
    capabilities_required: Sequence[str] = (),
) -> Callable[[Callable[[TestContext], None]], Callable[[TestContext], None]]:
    """Register an opaque-box E2E test function."""

    def decorator(fn: Callable[[TestContext], None]) -> Callable[[TestContext], None]:
        if any(t.test_id == test_id for t in _REGISTRY):
            raise ValueError(f"duplicate test_id: {test_id}")
        _REGISTRY.append(
            TestSpec(
                test_id=test_id,
                tier=tier,
                feature=feature,
                description=description,
                capabilities_required=tuple(capabilities_required),
                fn=fn,
            )
        )
        return fn

    return decorator


def registered_tests() -> list[TestSpec]:
    return sorted(_REGISTRY, key=lambda t: (t.tier, t.feature, t.test_id))


@dataclass
class RunResult:
    returncode: int
    stdout: str
    stderr: str


class TestContext:
    """Per-test isolated context passed to every registered E2E test."""

    def __init__(
        self,
        *,
        repo_root: Path,
        workdir: Path,
        binaries: BinaryManager,
        caps: Capabilities,
        mode: str,
    ) -> None:
        self.repo_root = repo_root
        self.workdir = workdir
        self.binaries = binaries
        self.caps = caps
        self.mode = mode

    def require_bin(self, bin_name: str) -> Path:
        res = self.binaries.build_bin(bin_name)
        if res.status == BUILT and res.path is not None:
            return res.path
        if res.status == MISSING_CRATE_OR_BIN:
            raise TestFailure(f"required binary `{bin_name}` missing from workspace: {res.detail}")
        if res.status == HOST_C_TOOLCHAIN_BLOCKED:
            raise SkipCapability(res.detail, os_unsupported=False)
        raise TestFailure(f"failed building `{bin_name}` ({res.status}): {res.detail}")

    def require_c_toolchain(self, what: str) -> None:
        if not self.caps.c_toolchain_available:
            raise SkipCapability(
                f"{what} requires host C/SQLite compilation: {self.caps.c_toolchain_detail}",
                os_unsupported=False,
            )

    def require_linux(self, what: str) -> None:
        if not self.caps.is_linux:
            raise SkipCapability(f"{what} is Linux-only (host={self.caps.platform})", os_unsupported=True)

    def require_windows(self, what: str) -> None:
        if not self.caps.is_windows:
            raise SkipCapability(f"{what} is Windows-only (host={self.caps.platform})", os_unsupported=True)

    def require_node(self) -> str:
        if not self.caps.node_path:
            raise TestFailure("node executable is required but was not found on PATH")
        return self.caps.node_path

    def run(
        self,
        cmd: Sequence[str | Path],
        *,
        cwd: Path | None = None,
        stdin_text: str | bytes | None = None,
        env: Mapping[str, str] | None = None,
        strip_env: Sequence[str] = (),
        timeout: int = 120,
    ) -> RunResult:
        argv = [str(c) for c in cmd]
        child_env = clean_env(extra=env, strip=strip_env)
        kwargs: dict[str, object] = {
            "cwd": cwd or self.repo_root,
            "env": child_env,
            "capture_output": True,
            "timeout": timeout,
            "check": False,
        }
        if isinstance(stdin_text, bytes):
            kwargs["input"] = stdin_text
            proc = subprocess.run(argv, **kwargs)  # type: ignore[arg-type]
            stdout = proc.stdout.decode("utf-8", errors="replace")
            stderr = proc.stderr.decode("utf-8", errors="replace")
        else:
            kwargs["text"] = True
            kwargs["encoding"] = "utf-8"
            kwargs["errors"] = "replace"
            if stdin_text is not None:
                kwargs["input"] = stdin_text
            proc = subprocess.run(argv, **kwargs)  # type: ignore[arg-type]
            stdout = proc.stdout
            stderr = proc.stderr
        return RunResult(returncode=proc.returncode, stdout=stdout, stderr=stderr)

    def copy_repo_subset(self, dest: Path, rel_paths: Sequence[str]) -> Path:
        """Copy selected repo directories/files into `dest` for isolated mutation tests."""
        dest.mkdir(parents=True, exist_ok=True)
        for rel in rel_paths:
            src = self.repo_root / rel
            dst = dest / rel
            if src.is_dir():
                shutil.copytree(src, dst, dirs_exist_ok=True)
            elif src.is_file():
                dst.parent.mkdir(parents=True, exist_ok=True)
                shutil.copy2(src, dst)
            else:
                raise TestFailure(f"copy_repo_subset: missing repo path `{rel}`")
        return dest


def parse_single_line_json(stdout: str) -> dict[str, object]:
    """Verify stdout is exactly one newline-terminated JSON object and return it."""
    if not stdout.endswith("\n"):
        raise TestFailure(f"expected trailing newline on JSON stdout, got: {stdout!r}")
    lines = stdout.splitlines()
    if len(lines) != 1:
        raise TestFailure(f"expected exactly 1 JSON line on stdout, got {len(lines)}: {stdout!r}")
    try:
        obj = json.loads(lines[0])
    except json.JSONDecodeError as exc:
        raise TestFailure(f"invalid JSON on stdout ({exc}): {lines[0]!r}") from exc
    if not isinstance(obj, dict):
        raise TestFailure(f"expected JSON object on stdout, got {type(obj).__name__}")
    return obj


def assert_envelope(
    obj: Mapping[str, object],
    *,
    outcome: str | None = None,
    command_id: str | None = None,
    error_code: str | None = None,
) -> None:
    if obj.get("envelope_version") != 1:
        raise TestFailure(f"expected envelope_version=1, got {obj.get('envelope_version')!r}")
    if not isinstance(obj.get("command_id"), str) or "@" not in str(obj["command_id"]):
        raise TestFailure(f"invalid command_id: {obj.get('command_id')!r}")
    if command_id is not None and obj.get("command_id") != command_id:
        raise TestFailure(f"expected command_id={command_id!r}, got {obj.get('command_id')!r}")
    if outcome is not None and obj.get("outcome") != outcome:
        raise TestFailure(f"expected outcome={outcome!r}, got {obj.get('outcome')!r} (full={obj})")
    if error_code is not None:
        err = obj.get("error")
        if not isinstance(err, dict) or err.get("code") != error_code:
            raise TestFailure(f"expected error.code={error_code!r}, got {err!r}")


def execute_suite(
    *,
    repo_root: Path,
    caps: Capabilities,
    binaries: BinaryManager,
    mode: str,
    tier_filter: int | None = None,
    feature_filter: str | None = None,
    keep_workdirs: bool = False,
) -> dict[str, object]:
    """Run filtered tests and return an `e2e-report/v1` dictionary."""
    tests = registered_tests()
    if tier_filter is not None:
        tests = [t for t in tests if t.tier == tier_filter]
    if feature_filter is not None:
        tests = [t for t in tests if t.feature == feature_filter]

    results: list[dict[str, object]] = []
    by_tier: dict[str, dict[str, int]] = {}
    by_feature: dict[str, dict[str, int]] = {}

    def _bump(bucket: dict[str, dict[str, int]], key: str, status: str) -> None:
        entry = bucket.setdefault(key, {"total": 0, "passed": 0, "failed": 0, "skipped_capability": 0})
        entry["total"] += 1
        entry[status] += 1

    passed = 0
    failed = 0
    skipped_capability = 0
    skipped_os_unsupported = 0

    for spec in tests:
        t0 = time.monotonic()
        tmp_mgr = None
        if keep_workdirs:
            workdir = Path(tempfile.mkdtemp(prefix=f"e2e-{spec.test_id}-"))
        else:
            tmp_mgr = tempfile.TemporaryDirectory(prefix=f"e2e-{spec.test_id}-")
            workdir = Path(tmp_mgr.name)

        ctx = TestContext(
            repo_root=repo_root,
            workdir=workdir,
            binaries=binaries,
            caps=caps,
            mode=mode,
        )
        status = "passed"
        detail = "ok"
        try:
            spec.fn(ctx)
        except SkipCapability as skip:
            if mode == "strict" and not skip.os_unsupported:
                status = "failed"
                detail = f"[strict] capability skipped: {skip.reason}"
            else:
                status = "skipped_capability"
                detail = skip.reason
                if skip.os_unsupported:
                    skipped_os_unsupported += 1
        except (AssertionError, TestFailure) as exc:
            status = "failed"
            detail = str(exc) or type(exc).__name__
        except Exception as exc:  # noqa: BLE001
            status = "failed"
            tb = traceback.format_exc(limit=4)
            detail = f"unexpected {type(exc).__name__}: {exc}\n{tb}"
        finally:
            if tmp_mgr is not None:
                try:
                    tmp_mgr.cleanup()
                except OSError:
                    pass

        dur_ms = int((time.monotonic() - t0) * 1000)
        if status == "passed":
            passed += 1
        elif status == "failed":
            failed += 1
        else:
            skipped_capability += 1

        _bump(by_tier, str(spec.tier), status)
        _bump(by_feature, spec.feature, status)

        results.append(
            {
                "id": spec.test_id,
                "tier": spec.tier,
                "feature": spec.feature,
                "description": spec.description,
                "capabilities_required": list(spec.capabilities_required),
                "status": status,
                "duration_ms": dur_ms,
                "detail": detail,
            }
        )

    return {
        "schema": REPORT_SCHEMA,
        "mode": mode,
        "platform": caps.platform,
        "capabilities": caps.to_dict(),
        "summary": {
            "total": len(results),
            "passed": passed,
            "failed": failed,
            "skipped_capability": skipped_capability,
            "skipped_os_unsupported": skipped_os_unsupported,
            "by_tier": dict(sorted(by_tier.items())),
            "by_feature": dict(sorted(by_feature.items())),
        },
        "results": results,
    }


def print_summary(report: Mapping[str, object]) -> None:
    summary = report["summary"]
    assert isinstance(summary, dict)
    print(
        f"\nMetrology Maven E2E Report ({report['schema']}) | mode={report['mode']} | platform={report['platform']}"
    )
    print(
        f"Total: {summary['total']}  Passed: {summary['passed']}  "
        f"Failed: {summary['failed']}  Skipped(cap): {summary['skipped_capability']} "
        f"(os_unsupported={summary['skipped_os_unsupported']})"
    )
    print("\nBy Tier:")
    print(f"{'Tier':<8} {'Total':>6} {'Pass':>6} {'Fail':>6} {'Skip':>6}")
    for tier, counts in summary["by_tier"].items():
        print(
            f"T{tier:<7} {counts['total']:>6} {counts['passed']:>6} "
            f"{counts['failed']:>6} {counts['skipped_capability']:>6}"
        )
    print("\nBy Feature:")
    print(f"{'Feature':<8} {'Total':>6} {'Pass':>6} {'Fail':>6} {'Skip':>6}")
    for feat, counts in summary["by_feature"].items():
        print(
            f"{feat:<8} {counts['total']:>6} {counts['passed']:>6} "
            f"{counts['failed']:>6} {counts['skipped_capability']:>6}"
        )
    failures = [r for r in report["results"] if r["status"] == "failed"]
    if failures:
        print("\nFailures:")
        for r in failures:
            first_line = str(r["detail"]).splitlines()[0] if r["detail"] else ""
            print(f"  [FAIL] {r['id']} (T{r['tier']} {r['feature']}): {first_line}")
