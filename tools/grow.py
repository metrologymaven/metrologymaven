#!/usr/bin/env python3
"""Cross-platform spec-tree growth driver (ECO.COMM.GROW).

Python 3.9+ standard-library replacement for tools/grow.ps1 that runs on
Windows, Linux and macOS:

1. Lists open nodes via ``tools/scope.py open``.
2. Prints the list and a human-review reminder when the target level <= 2.
3. Stops on ``--dry-run`` without starting any agent process.
4. Runs the first node alone as a canary and aborts with exit code 2 if its
   log matches an authentication-failure pattern.
5. Runs remaining nodes with at most ``--parallel`` concurrent agent processes.
6. Optionally runs the level-check prompt unless ``--no-level-check`` is set.
7. Runs ``tools/scope.py check`` and exits with its exit code.

This script never invokes git (no commit, no push).
"""

from __future__ import annotations

import argparse
import re
import subprocess
import sys
import time
from pathlib import Path
from typing import IO, Optional, Sequence

REPO_ROOT = Path(__file__).resolve().parent.parent
SCOPE_PY = Path(__file__).resolve().parent / "scope.py"

AUTH_FAIL_RE = re.compile(
    r"Failed to authenticate|OAuth access token is invalid|not logged in|Invalid API key",
    re.IGNORECASE,
)

POLL_INTERVAL_S = 0.02


def parse_args(argv: Optional[Sequence[str]] = None) -> argparse.Namespace:
    parser = argparse.ArgumentParser(
        prog="grow.py",
        description=(
            "Grow the spec tree by one level: expand open nodes with a coding "
            "agent (canary first, then in parallel), optionally run the level "
            "check, and finish with scope.py check. Never calls git."
        ),
    )
    parser.add_argument(
        "--level",
        type=int,
        default=None,
        help="Target level passed to scope.py open --level.",
    )
    parser.add_argument(
        "--limit",
        type=int,
        default=None,
        help="Maximum number of open nodes passed to scope.py open --limit.",
    )
    parser.add_argument(
        "--parallel",
        type=int,
        default=3,
        help="Maximum concurrent agent processes after the canary (default: 3).",
    )
    parser.add_argument(
        "--agent",
        choices=("claude", "codex"),
        default="claude",
        help="Coding agent executable to invoke (default: claude).",
    )
    parser.add_argument(
        "--model",
        default="",
        help="Optional model name passed to the agent.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="List target nodes and exit without running any agent.",
    )
    parser.add_argument(
        "--no-level-check",
        action="store_true",
        help="Skip the post-expansion level-check agent pass.",
    )
    # Test-only seams (hidden from normal --help output).
    parser.add_argument(
        "--root",
        default=None,
        help=argparse.SUPPRESS,
    )
    parser.add_argument(
        "--agent-cmd",
        default=None,
        help=argparse.SUPPRESS,
    )
    args = parser.parse_args(argv)
    if args.parallel < 1:
        parser.error("--parallel must be at least 1")
    if args.limit is not None and args.limit < 1:
        parser.error("--limit must be at least 1")
    if args.level is not None and args.level < 0:
        parser.error("--level must be >= 0")
    return args


def node_level(node_path: str) -> int:
    """Infer tree level from a repo-relative node path like 'spec/a/b/NODE.md'."""
    norm = node_path.replace("\\", "/")
    if norm.startswith("spec/"):
        norm = norm[len("spec/"):]
    elif norm == "spec":
        norm = ""
    parts = [p for p in norm.split("/") if p]
    return max(0, len(parts) - 1)


def log_path_for_node(log_dir: Path, node_path: str) -> Path:
    """Return the per-node log path under generated/logs/."""
    safe = re.sub(r"[/\\]", "_", node_path)
    return log_dir / f"{safe}.log"


def build_agent_command(
    agent: str,
    message: str,
    model: str,
    agent_cmd: Optional[str],
) -> list[str]:
    """Build the command argv for an agent invocation."""
    if agent == "claude":
        args = [
            "-p",
            message,
            "--permission-mode",
            "acceptEdits",
            "--allowedTools",
            "Read,Write,Edit,Glob,Grep,Bash(python tools/scope.py:*)",
        ]
        if model:
            args.extend(["--model", model])
        exe = "claude"
    elif agent == "codex":
        args = ["exec", "--full-auto"]
        if model:
            args.extend(["--model", model])
        args.append(message)
        exe = "codex"
    else:
        raise ValueError(f"unsupported agent: {agent}")

    if agent_cmd:
        cmd_path = Path(agent_cmd)
        if not cmd_path.is_absolute():
            cmd_path = cmd_path.resolve()
        if cmd_path.suffix.lower() == ".py":
            return [sys.executable, str(cmd_path), *args]
        return [str(cmd_path), *args]

    return [exe, *args]


def check_auth_ok(log_path: Path, agent: str) -> bool:
    """Return False if the agent log contains an authentication-failure pattern."""
    try:
        text = log_path.read_text(encoding="utf-8", errors="replace")
    except OSError:
        return True
    if AUTH_FAIL_RE.search(text):
        print(
            f"Agent authentication failed (see {log_path}). "
            f"Sign in with '{agent}' interactively, then rerun.",
            file=sys.stderr,
        )
        return False
    return True


def list_open_nodes(
    root: Path,
    level: Optional[int],
    limit: Optional[int],
) -> tuple[int, list[str], str]:
    """Invoke scope.py open and return (exit_code, node_paths, stderr)."""
    cmd = [sys.executable, str(SCOPE_PY), "--root", str(root), "open"]
    if level is not None:
        cmd.extend(["--level", str(level)])
    if limit is not None:
        cmd.extend(["--limit", str(limit)])
    proc = subprocess.run(
        cmd,
        cwd=str(root),
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    nodes = [line.strip() for line in proc.stdout.splitlines() if line.strip()]
    return proc.returncode, nodes, proc.stderr


def start_agent_process(
    root: Path,
    cmd: list[str],
    log_path: Path,
) -> tuple[subprocess.Popen[bytes], IO[bytes], Path]:
    """Launch an agent subprocess writing combined stdout/stderr to log_path."""
    log_path.parent.mkdir(parents=True, exist_ok=True)
    fh = open(log_path, "wb")
    try:
        proc = subprocess.Popen(
            cmd,
            cwd=str(root),
            stdout=fh,
            stderr=subprocess.STDOUT,
        )
    except Exception:
        fh.close()
        raise
    return proc, fh, log_path


def finalize_process(
    proc: subprocess.Popen[bytes],
    fh: IO[bytes],
    log_path: Path,
) -> int:
    """Record process exit code in the log file and close its handle."""
    rc = proc.wait()
    try:
        fh.flush()
        fh.write(f"exit={rc}\n".encode("utf-8"))
    finally:
        fh.close()
    return rc


def run_single_agent(
    root: Path,
    agent: str,
    message: str,
    model: str,
    agent_cmd: Optional[str],
    log_path: Path,
) -> int:
    """Run one agent process synchronously to completion."""
    cmd = build_agent_command(agent, message, model, agent_cmd)
    proc, fh, path = start_agent_process(root, cmd, log_path)
    return finalize_process(proc, fh, path)


def run_parallel_pool(
    root: Path,
    nodes: Sequence[str],
    parallel: int,
    agent: str,
    model: str,
    agent_cmd: Optional[str],
    log_dir: Path,
) -> None:
    """Run remaining nodes keeping at most ``parallel`` agent processes active."""
    active: list[tuple[subprocess.Popen[bytes], IO[bytes], Path]] = []
    pending = list(nodes)

    try:
        while pending or active:
            # Launch new processes while slots remain.
            while pending and len(active) < parallel:
                node = pending.pop(0)
                log_path = log_path_for_node(log_dir, node)
                print(f"-> expanding {node}", flush=True)
                msg = f"Follow prompts/expand-node.md exactly. TARGET: {node}"
                cmd = build_agent_command(agent, msg, model, agent_cmd)
                active.append(start_agent_process(root, cmd, log_path))

            # Reap finished processes.
            still_running: list[tuple[subprocess.Popen[bytes], IO[bytes], Path]] = []
            reaped = False
            for proc, fh, path in active:
                if proc.poll() is not None:
                    finalize_process(proc, fh, path)
                    reaped = True
                else:
                    still_running.append((proc, fh, path))
            active = still_running

            if active and not reaped:
                time.sleep(POLL_INTERVAL_S)
    except BaseException:
        for proc, fh, _ in active:
            try:
                proc.terminate()
            except OSError:
                pass
            try:
                fh.close()
            except OSError:
                pass
        raise


def run_scope_check(root: Path) -> int:
    """Run scope.py check on root and return its exit code."""
    cmd = [sys.executable, str(SCOPE_PY), "--root", str(root), "check"]
    proc = subprocess.run(cmd, cwd=str(root))
    return proc.returncode


def main(argv: Optional[Sequence[str]] = None) -> int:
    args = parse_args(argv)
    root = Path(args.root).resolve() if args.root else REPO_ROOT

    rc, nodes, err = list_open_nodes(root, args.level, args.limit)
    if rc != 0:
        if err:
            print(err, file=sys.stderr, end="")
        return rc

    if not nodes:
        print("No open nodes. The tree is fully expanded (or blocked on decisions).")
        return 0

    target_level = node_level(nodes[0])
    print(
        f"Level {target_level} : {len(nodes)} open node(s) to expand "
        f"with '{args.agent}' (parallel {args.parallel})."
    )
    for node in nodes:
        print(f"  {node}")
    if target_level <= 2:
        print(
            f"NOTE: level {target_level} splits need human review in a PR before merging."
        )

    if args.dry_run:
        return 0

    log_dir = root / "generated" / "logs"
    log_dir.mkdir(parents=True, exist_ok=True)

    # Canary: run the first node alone so broken auth/setup fails fast.
    canary = nodes[0]
    canary_log = log_path_for_node(log_dir, canary)
    print(f"-> expanding {canary}", flush=True)
    canary_msg = f"Follow prompts/expand-node.md exactly. TARGET: {canary}"
    run_single_agent(
        root,
        args.agent,
        canary_msg,
        args.model,
        args.agent_cmd,
        canary_log,
    )
    if not check_auth_ok(canary_log, args.agent):
        return 2

    # Run remaining nodes up to --parallel concurrently.
    if len(nodes) > 1:
        run_parallel_pool(
            root,
            nodes[1:],
            args.parallel,
            args.agent,
            args.model,
            args.agent_cmd,
            log_dir,
        )

    print("All expansions finished. Logs: generated/logs/", flush=True)

    if not args.no_level_check:
        next_level = target_level + 1
        print(f"-> level check for level {next_level}", flush=True)
        lc_log = log_dir / f"level-check-{next_level}.log"
        lc_msg = f"Follow prompts/level-check.md exactly. LEVEL: {next_level}"
        run_single_agent(
            root,
            args.agent,
            lc_msg,
            args.model,
            args.agent_cmd,
            lc_log,
        )
        check_auth_ok(lc_log, args.agent)

    return run_scope_check(root)


if __name__ == "__main__":
    raise SystemExit(main())
