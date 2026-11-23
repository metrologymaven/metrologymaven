#!/usr/bin/env python3
"""Parallel runner for ci/fixture-checks.txt.

Validates that ci/fixture-checks.txt is strictly sorted under LC_ALL=C and unique,
runs Cargo-touching commands sequentially to avoid target-dir lock contention, and
runs independent Python fixture scripts concurrently across worker threads (with
known heavy oracle scripts scheduled first to minimise tail latency).

In `--mode affected` (used on narrow pull requests), validates the entire list
format and runs:
  - every entry added/changed in `ci/fixture-checks.txt` in the PR diff;
  - every entry matching any changed fixture/crate/tool subtree;
  - all fast repository-wide structural checkers (`tools/check_*.py`, `tools/tests`).
In `--mode full` (default; always used on `main`, `merge_group`, and full PRs),
runs every single entry in the file.

Usage:
    python3 tools/run_fixture_checks.py --list ci/fixture-checks.txt
                                        [--mode full|affected] [--base SHA] [--head SHA]
                                        [--jobs N]
"""

from __future__ import annotations

import argparse
import os
import shlex
import subprocess
import sys
import time
from concurrent.futures import ThreadPoolExecutor, as_completed
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
ZERO_SHA = "0" * 40

# Scripts known to take >15s; launch them first so they overlap with the ~250 fast scripts.
HEAVY_MARKERS = (
    "make_invert_fixtures.py",
    "geo_dist_batch.py",
    "geo_dist_sign.py",
    "make_eig_fixtures.py",
    "make_svd_fixtures.py",
    "make_qr_fixtures.py",
    "make_la_fixtures.py",
    "nurbs_cert.py",
    "test_scope.py",
    "tools/tests",
)

# Always-fast repo-wide structural checks executed even in affected mode.
ALWAYS_RUN_AFFECTED = (
    "python tools/check_bench_schema.py",
    "python tools/check_command_defs.py",
    "python tools/check_entries.py",
    "python tools/check_eval_suite.py",
    "python tools/check_manifest_schema.py",
    "python tools/check_registry_keywords.py",
)


def parse_and_validate_list(path: Path) -> list[str]:
    """Read non-comment, non-blank entries and enforce strict C-locale sort + uniqueness."""
    entries: list[str] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        entries.append(raw.rstrip("\r\n"))

    for i in range(1, len(entries)):
        prev_b = entries[i - 1].encode("utf-8")
        cur_b = entries[i].encode("utf-8")
        if cur_b <= prev_b:
            raise ValueError(
                f"{path}: lines are not sorted and unique (LC_ALL=C sort -u): "
                f"{entries[i - 1]!r} before {entries[i]!r}"
            )
    return entries


def is_serial_command(cmd: str) -> bool:
    """Commands that invoke Cargo or mutate build artifacts run sequentially."""
    return (
        cmd.startswith("cargo ")
        or cmd.startswith("bash crates/")
        or "rpt_float_lint.py --clippy" in cmd
        or "rpt_float_lint.py --mir" in cmd
    )


def is_heavy_command(cmd: str) -> bool:
    return any(m in cmd for m in HEAVY_MARKERS)


def command_paths(cmd: str) -> list[str]:
    """Extract repo-relative paths referenced by a fixture-check command."""
    paths: list[str] = []
    try:
        tokens = shlex.split(cmd)
    except ValueError:
        tokens = cmd.split()
    for tok in tokens:
        if tok.startswith("-"):
            continue
        if "/" in tok and not tok.startswith("http"):
            paths.append(tok.rstrip("/"))
    return paths


def select_affected_entries(
    entries: list[str],
    changed_files: list[str],
    added_or_changed_lines: set[str],
    affected_crates: set[str],
) -> list[str]:
    """Select fixture checks relevant to changed files/crates, fail-closed on ambiguity."""
    changed_dirs: set[str] = set()
    for f in changed_files:
        p = Path(f)
        for parent in p.parents:
            ps = parent.as_posix()
            if ps and ps != "." and "/" in ps:
                changed_dirs.add(ps)
            elif ps in ("fixtures", "tools", "design", "platform"):
                continue

    selected: list[str] = []
    for cmd in entries:
        if cmd in added_or_changed_lines or cmd in ALWAYS_RUN_AFFECTED:
            selected.append(cmd)
            continue
        if any(f"-p {c}" in cmd or f"crates/{c}/" in cmd for c in affected_crates):
            selected.append(cmd)
            continue
        if "rpt_float_lint.py" in cmd and any(c.startswith("fid-report") for c in affected_crates):
            selected.append(cmd)
            continue
        if "fid-design" in affected_crates and "fid-design" in cmd:
            selected.append(cmd)
            continue
        # Match any path referenced in the command against changed files or their subtrees.
        matched = False
        for cp in command_paths(cmd):
            cp_dir = Path(cp).parent.as_posix() if "." in Path(cp).name else cp
            for cf in changed_files:
                if cf == cp or cf.startswith(cp + "/") or (cp_dir and cp_dir != "." and cf.startswith(cp_dir + "/")):
                    matched = True
                    break
            if matched:
                break
            if cp in changed_dirs or cp_dir in changed_dirs:
                matched = True
                break
        if matched:
            selected.append(cmd)
    return selected


def git_diff_info(base: str, head: str, list_rel: str, root: Path) -> tuple[list[str], set[str]] | None:
    if not base or base == ZERO_SHA:
        return None
    chk = subprocess.run(["git", "cat-file", "-e", f"{base}^{{commit}}"], cwd=root, capture_output=True)
    if chk.returncode != 0:
        return None
    p_files = subprocess.run(
        ["git", "diff", "--name-only", f"{base}...{head}"],
        cwd=root,
        capture_output=True,
        text=True,
    )
    if p_files.returncode != 0:
        return None
    changed_files = [l.strip() for l in p_files.stdout.splitlines() if l.strip()]

    p_list = subprocess.run(
        ["git", "diff", "-U0", f"{base}...{head}", "--", list_rel],
        cwd=root,
        capture_output=True,
        text=True,
    )
    added_lines: set[str] = set()
    if p_list.returncode == 0:
        for line in p_list.stdout.splitlines():
            if line.startswith("+") and not line.startswith("+++"):
                s = line[1:].strip()
                if s and not s.startswith("#"):
                    added_lines.add(s)
    return changed_files, added_lines


def run_one(cmd: str, cwd: Path) -> tuple[str, int, float, str, str]:
    t0 = time.monotonic()
    proc = subprocess.run(
        ["bash", "-euo", "pipefail", "-c", cmd],
        cwd=cwd,
        stdin=subprocess.DEVNULL,
        capture_output=True,
        text=True,
    )
    dt = time.monotonic() - t0
    return cmd, proc.returncode, dt, proc.stdout, proc.stderr


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Run ci/fixture-checks.txt in parallel.")
    ap.add_argument("--list", default="ci/fixture-checks.txt", help="Path to fixture checks list")
    ap.add_argument("--mode", choices=("full", "affected"), default="full")
    ap.add_argument("--base", default="")
    ap.add_argument("--head", default="HEAD")
    ap.add_argument("--crates", default="", help="Affected crate flags (`-p a -p b`) from ci_scope.py")
    ap.add_argument("--jobs", type=int, default=min(os.cpu_count() or 4, 8), help="Parallel workers")
    args = ap.parse_args(argv)

    list_path = Path(args.list)
    if not list_path.is_absolute():
        list_path = ROOT / list_path

    try:
        entries = parse_and_validate_list(list_path)
    except ValueError as err:
        print(f"::error file={args.list}::{err}", file=sys.stderr)
        return 1

    to_run = entries
    if args.mode == "affected":
        diff = git_diff_info(args.base, args.head, args.list, ROOT)
        if diff is not None:
            changed_files, added_lines = diff
            affected_crates = {tok for tok in args.crates.split() if tok != "-p"}
            to_run = select_affected_entries(entries, changed_files, added_lines, affected_crates)

    serial = [c for c in to_run if is_serial_command(c)]
    # Schedule known heavy scripts first so they overlap with the fast majority.
    parallel = sorted((c for c in to_run if not is_serial_command(c)), key=lambda c: (0 if is_heavy_command(c) else 1, c))

    t_start = time.monotonic()
    results: list[tuple[str, int, float, str, str]] = []

    workers = max(1, args.jobs)
    print(
        f"Running {len(to_run)}/{len(entries)} fixture checks "
        f"(mode={args.mode}, {len(parallel)} parallel x{workers}, {len(serial)} serial)...",
        flush=True,
    )

    if parallel:
        with ThreadPoolExecutor(max_workers=workers) as pool:
            futs = {pool.submit(run_one, cmd, ROOT): cmd for cmd in parallel}
            for fut in as_completed(futs):
                results.append(fut.result())

    for cmd in serial:
        results.append(run_one(cmd, ROOT))

    total_dt = time.monotonic() - t_start
    failures = sorted((r for r in results if r[1] != 0), key=lambda r: r[0])

    for cmd, code, dt, out, err in failures:
        print(f"::error file={args.list}::failed (exit {code}, {dt:.2f}s): {cmd}", file=sys.stderr)
        if out.strip():
            print(out.rstrip(), file=sys.stderr)
        if err.strip():
            print(err.rstrip(), file=sys.stderr)

    slowest = sorted(results, key=lambda r: r[2], reverse=True)[:5]
    print(
        f"Fixture checks: {len(results) - len(failures)} passed, {len(failures)} failed in {total_dt:.2f}s.",
        flush=True,
    )
    if slowest:
        print("Slowest 5:", flush=True)
        for cmd, _, dt, _, _ in slowest:
            print(f"  {dt:6.2f}s  {cmd}", flush=True)

    return 1 if failures else 0


if __name__ == "__main__":
    raise SystemExit(main())
