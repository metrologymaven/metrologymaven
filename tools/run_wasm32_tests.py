#!/usr/bin/env python3
"""Batched runner for ci/wasm32-crates.txt.

Validates LC_ALL=C sort order, uniqueness and workspace membership, supports
filtering to affected crates on PRs, and batches all plain crate entries into a
single Cargo invocation instead of spawning 30+ separate `cargo test` commands.

Usage:
    python3 tools/run_wasm32_tests.py --list ci/wasm32-crates.txt
                                      [--only ALL|NONE|crate1,crate2]
                                      [--use-nextest]
"""

from __future__ import annotations

import argparse
import json
import shlex
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]


def parse_and_validate_wasm_list(path: Path, workspace_members: set[str]) -> list[tuple[str, list[str]]]:
    """Validate C-locale sorting, uniqueness and workspace membership."""
    raw_lines: list[str] = []
    parsed: list[tuple[str, list[str]]] = []
    for raw in path.read_text(encoding="utf-8").splitlines():
        s = raw.strip()
        if not s or s.startswith("#"):
            continue
        raw_lines.append(s)
        parts = shlex.split(s)
        parsed.append((parts[0], parts[1:]))

    for i in range(1, len(raw_lines)):
        if raw_lines[i].encode("utf-8") <= raw_lines[i - 1].encode("utf-8"):
            raise ValueError(
                f"{path}: lines are not sorted and unique (LC_ALL=C sort -u): "
                f"{raw_lines[i - 1]!r} before {raw_lines[i]!r}"
            )

    missing = [crate for crate, _ in parsed if crate not in workspace_members]
    if missing:
        raise ValueError(f"{path}: not workspace crates: {', '.join(missing)}")

    return parsed


def workspace_member_names(root: Path = ROOT) -> set[str]:
    proc = subprocess.run(
        ["cargo", "metadata", "--no-deps", "--format-version", "1"],
        cwd=root,
        capture_output=True,
        text=True,
        check=True,
    )
    meta = json.loads(proc.stdout)
    return {p["name"] for p in meta.get("packages", [])}


def build_commands(
    entries: list[tuple[str, list[str]]],
    only: str,
    use_nextest: bool,
) -> list[tuple[str, list[str]]]:
    """Group plain entries into one batched invocation and preserve custom-arg entries."""
    if only == "NONE":
        return []
    if only != "ALL":
        allow = {c.strip() for c in only.split(",") if c.strip()}
        entries = [(c, a) for c, a in entries if c in allow]
    if not entries:
        return []

    plain = [c for c, a in entries if not a]
    custom = [(c, a) for c, a in entries if a]

    cmds: list[tuple[str, list[str]]] = []
    if plain:
        if use_nextest:
            cmd = ["cargo", "nextest", "run", "--no-fail-fast", "--no-tests=pass", "--target", "wasm32-wasip1"]
        else:
            cmd = ["cargo", "test", "--no-fail-fast", "--target", "wasm32-wasip1"]
        for c in plain:
            cmd.extend(["-p", c])
        cmds.append((f"batched wasm32 ({len(plain)} crates)", cmd))

    for crate, extra in custom:
        cmd = ["cargo", "test", "--target", "wasm32-wasip1", "-p", crate, *extra]
        cmds.append((f"{crate} {' '.join(extra)}", cmd))

    return cmds


def main(argv: list[str] | None = None) -> int:
    ap = argparse.ArgumentParser(description="Run batched wasm32-wasip1 crate tests.")
    ap.add_argument("--list", default="ci/wasm32-crates.txt")
    ap.add_argument("--only", default="ALL", help="ALL, NONE, or comma-separated crate names")
    ap.add_argument("--use-nextest", action="store_true", help="Use cargo-nextest for batched plain crates")
    args = ap.parse_args(argv)

    list_path = Path(args.list)
    if not list_path.is_absolute():
        list_path = ROOT / list_path

    try:
        members = workspace_member_names(ROOT)
        entries = parse_and_validate_wasm_list(list_path, members)
    except ValueError as err:
        print(f"::error file={args.list}::{err}", file=sys.stderr)
        return 1

    cmds = build_commands(entries, args.only, args.use_nextest)
    if not cmds:
        print(f"No wasm32 crates selected (only={args.only}); list validation passed.", flush=True)
        return 0

    failed: list[str] = []
    for label, cmd in cmds:
        print(f"::group::{label}: {' '.join(cmd)}", flush=True)
        proc = subprocess.run(cmd, cwd=ROOT, stdin=subprocess.DEVNULL)
        print("::endgroup::", flush=True)
        if proc.returncode != 0:
            failed.append(label)

    if failed:
        print(f"::error::wasm32 tests failed: {', '.join(failed)}", file=sys.stderr)
        return 1
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
