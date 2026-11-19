"""House-rules check: edited, stale and missing blocks, budgets, CLAUDE.md. Spec node ECO.RULES.CHECK.

Standard library only. Usage::

    python -m house_rules check --target <dir> --repo-id <id> --manifest <file> \\
        --current-rev <40-hex> [--source-repo <dir>]

Reads ``<target>/AGENTS.md`` and ``<target>/CLAUDE.md`` and the manifest (ECO.RULES.SRC format:
``manifest.toml``, or a JSON file with the same data). It never writes a file (REQ-006). With
``--source-repo`` it runs the local, read-only ``git merge-base --is-ancestor``; it never uses
the network.

Output lines, sorted, on stdout; any line not starting with ``warn`` is a failure (exit 1)::

    RULES-EDITED AGENTS.md <block>          body does not match its recorded digest
    RULES-MISSING <block>                   block assigned to --repo-id is absent
    RULES-UNKNOWN-REV <block> <rev>         --source-repo: recorded rev not in the current history
    RULES-CLAUDE-MD CLAUDE.md <why>         why: missing, symlink, not-exact
    RULES-PARSE <input> <why>               unreadable or malformed input
    RULES-USAGE <why>                       bad arguments
    warn RULES-STALE <block>                recorded revision differs from --current-rev
    warn RULES-SIZE <name> <bytes> > <budget>   name: core (block body) or AGENTS.md

Sizes are UTF-8 bytes after normalising CRLF to LF. Inputs larger than :data:`MAX_INPUT_BYTES`
and symbolic links are refused.
"""

from __future__ import annotations

import argparse
import json
import os
import stat
import subprocess
import sys
from pathlib import Path

from .block import BlockError, find_blocks, is_valid_block_id, is_valid_repo_id, is_valid_rev

__all__ = ["MAX_INPUT_BYTES", "Finding", "InputError", "check", "load_manifest", "main"]

MAX_INPUT_BYTES = 1 << 20  # 1 MiB: AGENTS.md is budgeted at 10 KiB, so this is generous.
GIT_TIMEOUT_S = 30
CLAUDE_OK = (b"@AGENTS.md", b"@AGENTS.md\n", b"@AGENTS.md\r\n")


class InputError(Exception):
    """An input could not be read or understood; ``line`` is the report line."""

    def __init__(self, line: str):
        super().__init__(line)
        self.line = line


Finding = str  # one report line


class _Missing(Exception):
    pass


def _is_link(p: Path) -> bool:
    if p.is_symlink():
        return True
    is_junction = getattr(p, "is_junction", None)  # Python >= 3.12
    return bool(is_junction()) if is_junction is not None else False


def _read(p: Path, label: str) -> bytes:
    """Read a regular file of at most MAX_INPUT_BYTES; refuse links. Raise _Missing if absent."""
    try:
        st = os.lstat(p)
    except FileNotFoundError:
        raise _Missing() from None
    except OSError as e:
        raise InputError(f"RULES-PARSE {label} unreadable ({e.strerror or 'error'})") from None
    if stat.S_ISLNK(st.st_mode) or _is_link(p):
        raise InputError(f"RULES-PARSE {label} symlink refused")
    if not stat.S_ISREG(st.st_mode):
        raise InputError(f"RULES-PARSE {label} not a regular file")
    if st.st_size > MAX_INPUT_BYTES:
        raise InputError(f"RULES-PARSE {label} too large ({st.st_size} > {MAX_INPUT_BYTES} bytes)")
    try:
        with open(p, "rb") as f:
            data = f.read(MAX_INPUT_BYTES + 1)
    except OSError as e:
        raise InputError(f"RULES-PARSE {label} unreadable ({e.strerror or 'error'})") from None
    if len(data) > MAX_INPUT_BYTES:
        raise InputError(f"RULES-PARSE {label} too large (> {MAX_INPUT_BYTES} bytes)")
    return data


def _lf_len(data: bytes) -> int:
    return len(data.replace(b"\r\n", b"\n"))


def _budget(v: object, name: str) -> int:
    if isinstance(v, bool) or not isinstance(v, int) or v < 0:
        raise InputError(f"RULES-PARSE manifest budgets.{name} must be a non-negative integer")
    return v


def load_manifest(path: Path) -> tuple[dict[str, list[str]], int, int]:
    """Parse a manifest: ``({block id: [repo ids]}, core_bytes, agents_md_bytes)``.

    The format is ECO.RULES.SRC's ``manifest.toml``; a ``.json`` file holds the same data.
    """
    try:
        data = _read(path, "manifest")
    except _Missing:
        raise InputError("RULES-PARSE manifest missing") from None
    suffix = path.suffix.lower()
    try:
        if suffix == ".toml":
            try:
                import tomllib
            except ImportError:
                raise InputError("RULES-PARSE manifest .toml needs Python >= 3.11; use manifest.json") from None
            m = tomllib.loads(data.decode("utf-8"))
        elif suffix == ".json":
            m = json.loads(data.decode("utf-8"))
        else:
            raise InputError("RULES-PARSE manifest must be .toml or .json")
    except UnicodeDecodeError:
        raise InputError("RULES-PARSE manifest not UTF-8") from None
    except ValueError as e:  # json.JSONDecodeError and tomllib.TOMLDecodeError
        first = str(e).splitlines()[0] if str(e) else "syntax error"
        raise InputError(f"RULES-PARSE manifest syntax ({first})") from None
    if not isinstance(m, dict):
        raise InputError("RULES-PARSE manifest not a table")
    budgets = m.get("budgets")
    if not isinstance(budgets, dict):
        raise InputError("RULES-PARSE manifest budgets missing")
    core = _budget(budgets.get("core_bytes"), "core_bytes")
    agents = _budget(budgets.get("agents_md_bytes"), "agents_md_bytes")
    blocks = m.get("block")
    if not isinstance(blocks, list):
        raise InputError("RULES-PARSE manifest block list missing")
    out: dict[str, list[str]] = {}
    for b in blocks:
        bid = b.get("id") if isinstance(b, dict) else None
        if not isinstance(bid, str) or not is_valid_block_id(bid):
            raise InputError("RULES-PARSE manifest block with invalid id")
        if bid in out:
            raise InputError(f"RULES-PARSE manifest duplicate block {bid}")
        repos = b.get("repos")
        if not isinstance(repos, list) or not all(isinstance(r, str) and is_valid_repo_id(r) for r in repos):
            raise InputError(f"RULES-PARSE manifest block {bid} has invalid repos")
        out[bid] = list(repos)
    return out, core, agents


def _git_ok(source_repo: Path, *args: str) -> bool | None:
    """Run a read-only local git command; True on exit 0, False on 1, None on anything else."""
    env = dict(os.environ)
    env.update({"GIT_TERMINAL_PROMPT": "0", "GIT_OPTIONAL_LOCKS": "0", "LC_ALL": "C"})
    try:
        r = subprocess.run(
            ["git", "-C", str(source_repo), *args],
            stdin=subprocess.DEVNULL,
            stdout=subprocess.DEVNULL,
            stderr=subprocess.DEVNULL,
            env=env,
            timeout=GIT_TIMEOUT_S,
            check=False,
        )
    except (OSError, subprocess.SubprocessError):
        return None
    if r.returncode in (0, 1):
        return r.returncode == 0
    return None


def _has_commit(source_repo: Path, rev: str) -> bool | None:
    r = _git_ok(source_repo, "cat-file", "-e", f"{rev}^{{commit}}")
    if r is None:
        # cat-file -e exits 128 for an unknown object; tell that apart from a broken repo.
        ok = _git_ok(source_repo, "rev-parse", "--git-dir")
        return False if ok else None
    return r


def check(
    target: Path,
    repo_id: str,
    manifest: Path,
    current_rev: str,
    source_repo: Path | None = None,
) -> list[Finding]:
    """Run the check and return the sorted report lines (failures and ``warn`` lines)."""
    if not is_valid_repo_id(repo_id):
        return ["RULES-USAGE --repo-id is not a valid repository ID"]
    if not is_valid_rev(current_rev):
        return ["RULES-USAGE --current-rev must be 40 lowercase hex digits"]
    if _is_link(target):
        return ["RULES-USAGE --target symlink refused"]
    if not target.is_dir():
        return ["RULES-USAGE --target is not a directory"]
    if source_repo is not None:
        if _is_link(source_repo):
            return ["RULES-USAGE --source-repo symlink refused"]
        if not source_repo.is_dir():
            return ["RULES-USAGE --source-repo is not a directory"]
    try:
        assignment, core_budget, agents_budget = load_manifest(manifest)
    except InputError as e:
        return [e.line]
    required = sorted(b for b, repos in assignment.items() if repo_id in repos)
    if not required:
        return [f"RULES-USAGE repository {repo_id} has no blocks in the manifest"]

    out: set[str] = set()
    out.update(_check_claude(target))

    try:
        agents = _read(target / "AGENTS.md", "AGENTS.md")
    except _Missing:
        agents = None
    except InputError as e:
        out.add(e.line)
        return sorted(out)

    if agents is None:
        out.update(f"RULES-MISSING {b}" for b in required)
        return sorted(out)

    size = _lf_len(agents)
    if size > agents_budget:
        out.add(f"warn RULES-SIZE AGENTS.md {size} > {agents_budget}")
    try:
        text = agents.decode("utf-8")
    except UnicodeDecodeError:
        out.add("RULES-PARSE AGENTS.md not UTF-8")
        return sorted(out)
    try:
        blocks = find_blocks(text)
    except BlockError as e:
        out.add(f"RULES-PARSE AGENTS.md {e.kind} at line {e.line}")
        return sorted(out)

    seen: set[str] = set()
    for b in blocks:
        if b.id in seen:
            out.add(f"RULES-PARSE AGENTS.md duplicate block {b.id} at line {b.begin_line}")
            continue
        seen.add(b.id)
        if not b.is_intact():
            out.add(f"RULES-EDITED AGENTS.md {b.id}")
        if b.id == "core":
            core_size = len(b.body.replace("\r\n", "\n").encode("utf-8"))
            if core_size > core_budget:
                out.add(f"warn RULES-SIZE core {core_size} > {core_budget}")
        if b.source_rev != current_rev:
            out.update(_check_rev(b.id, b.source_rev, current_rev, source_repo))
    out.update(f"RULES-MISSING {b}" for b in required if b not in seen)
    return sorted(out)


def _check_rev(block_id: str, rev: str, current: str, source_repo: Path | None) -> list[str]:
    if source_repo is None:
        return [f"warn RULES-STALE {block_id}"]
    have_current = _has_commit(source_repo, current)
    if have_current is None:
        return ["RULES-USAGE --source-repo is not a readable git repository"]
    if not have_current:
        return ["RULES-USAGE --current-rev is not a commit in --source-repo"]
    if _has_commit(source_repo, rev) and _git_ok(source_repo, "merge-base", "--is-ancestor", rev, current):
        return [f"warn RULES-STALE {block_id}"]
    return [f"RULES-UNKNOWN-REV {block_id} {rev}"]


def _check_claude(target: Path) -> list[str]:
    try:
        data = _read(target / "CLAUDE.md", "CLAUDE.md")
    except _Missing:
        return ["RULES-CLAUDE-MD CLAUDE.md missing"]
    except InputError as e:
        if "symlink" in e.line:
            return ["RULES-CLAUDE-MD CLAUDE.md symlink"]
        return [e.line]
    if data not in CLAUDE_OK:
        return ["RULES-CLAUDE-MD CLAUDE.md not-exact"]
    return []


class _Parser(argparse.ArgumentParser):
    def error(self, message: str):  # type: ignore[override]
        raise InputError(f"RULES-USAGE {message}")

    def exit(self, status: int = 0, message: str | None = None):  # type: ignore[override]
        if message:
            sys.stdout.write(message)
        raise SystemExit(status)


def build_parser() -> argparse.ArgumentParser:
    p = _Parser(prog="python -m house_rules check", description="House-rules check (ECO.RULES.CHECK).")
    p.add_argument("--target", required=True, help="repository working tree to check")
    p.add_argument("--repo-id", required=True, help="repository ID in the manifest")
    p.add_argument("--manifest", required=True, help="house-rules manifest (.toml or .json)")
    p.add_argument("--current-rev", required=True, help="current canonical revision (40 hex)")
    p.add_argument("--source-repo", help="local clone of the canonical source, for ancestry")
    return p


def main(argv: list[str]) -> int:
    """Entry point: print the sorted report and return 0 (ok, warnings allowed) or 1 (fail)."""
    try:
        a = build_parser().parse_args(argv)
        lines = check(
            Path(a.target),
            a.repo_id,
            Path(a.manifest),
            a.current_rev,
            Path(a.source_repo) if a.source_repo else None,
        )
    except InputError as e:
        lines = [e.line]
    except Exception as e:  # never a traceback (CONSTITUTION §8 never crash)
        lines = [f"RULES-PARSE internal error ({type(e).__name__})"]
    for line in lines:
        sys.stdout.write(line + "\n")
    return 1 if any(not line.startswith("warn ") for line in lines) else 0
