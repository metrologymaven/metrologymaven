"""House-rules check: edited, stale and missing blocks, budgets, CLAUDE.md. Spec node ECO.RULES.CHECK.

Standard library only. Usage::

    python -m house_rules check --target <dir> --repo-id <id> --manifest <file> \\
        --current-rev <40-hex> [--source-repo <dir>]

Reads ``<target>/AGENTS.md`` and ``<target>/CLAUDE.md`` and the manifest (ECO.RULES.SRC format:
``manifest.toml``, or a JSON file with the same data). It never writes a file (REQ-006). With
``--source-repo`` it runs local, read-only git commands (see *Git hardening*); it never uses the
network.

Output lines, sorted, on stdout; any line not starting with ``warn`` is a failure (exit 1)::

    RULES-EDITED AGENTS.md <block>          body does not match its recorded digest
    RULES-MISSING <block>                   block assigned to --repo-id is absent
    RULES-SOURCE AGENTS.md <block> <repo>   block names a source other than the canonical repo
    RULES-UNKNOWN-REV <block> <rev>         --source-repo: recorded rev not in the current history
    RULES-CLAUDE-MD CLAUDE.md <why>         why: missing, symlink, not-exact
    RULES-PARSE <input> <why>               unreadable or malformed input
    RULES-USAGE <why>                       bad arguments or unusable --source-repo
    warn RULES-STALE <block>                recorded revision differs from --current-rev
    warn RULES-SIZE <name> <bytes> > <budget>   name: core (block body) or AGENTS.md

The canonical repository ID is the manifest's optional ``source_repo`` (default
``metrologymaven``, as in ``tools/repo_template``). Sizes are UTF-8 bytes after normalising CRLF
to LF. Inputs larger than :data:`MAX_INPUT_BYTES` and symbolic links are refused. A body that was
rewritten together with a recomputed digest passes: the digest detects accidental edits, it is
not a signature (the spec asks for no more).

Git hardening (copy this into any tool that runs git on a repository it does not control)
------------------------------------------------------------------------------------------
A repository's own ``.git/config`` can name programs that git runs (``uploadpack``, hooks,
``core.fsmonitor``) and can make a plain read such as ``cat-file`` start a lazy fetch from a
promisor remote. Every git call in this module therefore:

* runs with a minimal environment built from scratch, not inherited: ``PATH``, ``HOME`` and
  ``USERPROFILE``, ``SYSTEMROOT`` on Windows, ``LC_ALL=C``, plus ``GIT_NO_LAZY_FETCH=1``,
  ``GIT_CONFIG_NOSYSTEM=1``, ``GIT_CONFIG_GLOBAL=<os.devnull>``, ``GIT_TERMINAL_PROMPT=0`` and
  ``GIT_OPTIONAL_LOCKS=0``. So ``GIT_DIR``, ``GIT_WORK_TREE``, ``GIT_OBJECT_DIRECTORY``,
  ``GIT_ALTERNATE_OBJECT_DIRECTORIES``, ``GIT_CONFIG_*``, ``GIT_NAMESPACE`` and the rest never
  reach git;
* passes the top-level option ``--no-lazy-fetch`` (git >= 2.44). This is the primary guard
  against lazy fetches. Older git does not know the option and exits with an error, so on those
  versions the check fails closed (see below) instead of running unguarded; the environment
  variable alone would be silently ignored by older git;
* passes, before the subcommand, ``-c protocol.allow=never`` and ``-c protocol.<name>.allow=never``
  for ``file``, ``ext``, ``ssh``, ``git``, ``http`` and ``https``. Command-line ``-c`` outranks the
  repository's own config, so a hostile ``protocol.file.allow=always`` cannot re-enable a
  transport. This is the second, independent guard: if a fetch were attempted anyway, it fails
  before any ``uploadpack`` or ``ext::`` command runs. A protocol not in this list falls back to
  ``protocol.allow=never``;
* passes ``-c core.fsmonitor=false -c core.hooksPath=<os.devnull>``;
* has stdin closed (or fed only our own revision list), output captured, and a timeout;
* never sets ``safe.directory``: git's refusal of a repository owned by someone else is kept.

Any git failure (unknown option on old git, ownership refusal, not a repository, timeout) fails
closed as ``RULES-USAGE --source-repo: git failed (...)``. What is guaranteed is therefore: with
``--source-repo``, either git >= 2.44 runs with lazy fetching disabled and every listed transport
forbidden, or the check fails. It is not guaranteed that git itself has no other way to run a
program configured in the repository; the guards above cover the ones known to be reachable from
``cat-file`` and ``merge-base``.

Revisions are validated as 40 lowercase hex digits before they reach git, so they can never be
read as options. ``--current-rev`` and all recorded revisions are looked up in one
``cat-file --batch-check`` call; each distinct recorded revision is then tested for ancestry once.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import subprocess
import sys
from pathlib import Path

from .block import BlockError, find_blocks, is_valid_block_id, is_valid_repo_id, is_valid_rev

__all__ = ["DEFAULT_SOURCE_REPO", "MAX_INPUT_BYTES", "Finding", "InputError", "check", "git_env", "load_manifest", "main"]

MAX_INPUT_BYTES = 1 << 20  # 1 MiB: AGENTS.md is budgeted at 10 KiB, so this is generous.
GIT_TIMEOUT_S = 30
CLAUDE_OK = (b"@AGENTS.md", b"@AGENTS.md\n", b"@AGENTS.md\r\n")
DEFAULT_SOURCE_REPO = "metrologymaven"
_SAFE_KEY = re.compile(r"[A-Za-z0-9_.-]{1,64}", re.ASCII)


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


def _no_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    out: dict[str, object] = {}
    for k, v in pairs:
        if k in out:
            shown = f" {k}" if _SAFE_KEY.fullmatch(k) else ""
            raise InputError(f"RULES-PARSE manifest duplicate key{shown}")
        out[k] = v
    return out


def load_manifest(path: Path) -> tuple[dict[str, list[str]], int, int, str]:
    """Parse a manifest: ``({block id: [repo ids]}, core_bytes, agents_md_bytes, source_repo)``.

    The format is ECO.RULES.SRC's ``manifest.toml``; a ``.json`` file holds the same data and
    may not repeat a key. ``source_repo`` is optional (default :data:`DEFAULT_SOURCE_REPO`).
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
            m = tomllib.loads(data.decode("utf-8"))  # TOML itself forbids duplicate keys
        elif suffix == ".json":
            m = json.loads(data.decode("utf-8"), object_pairs_hook=_no_duplicate_keys)
        else:
            raise InputError("RULES-PARSE manifest must be .toml or .json")
    except UnicodeDecodeError:
        raise InputError("RULES-PARSE manifest not UTF-8") from None
    except RecursionError:
        raise InputError("RULES-PARSE manifest nested too deeply") from None
    except ValueError as e:  # json.JSONDecodeError and tomllib.TOMLDecodeError
        first = str(e).splitlines()[0] if str(e) else "syntax error"
        raise InputError(f"RULES-PARSE manifest syntax ({first})") from None
    if not isinstance(m, dict):
        raise InputError("RULES-PARSE manifest not a table")
    source_repo = m.get("source_repo", DEFAULT_SOURCE_REPO)
    if not isinstance(source_repo, str) or not is_valid_repo_id(source_repo):
        raise InputError("RULES-PARSE manifest source_repo is not a valid repository ID")
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
    return out, core, agents, source_repo


# --------------------------------------------------------------------------------------------
# Git (see "Git hardening" in the module docstring)


def git_env() -> dict[str, str]:
    """The minimal, non-inherited environment for every git call."""
    env: dict[str, str] = {}
    keep = ["PATH", "HOME", "USERPROFILE"] + (["SYSTEMROOT"] if os.name == "nt" else [])
    for k in keep:
        v = os.environ.get(k)
        if v is not None:
            env[k] = v
    env.update(
        {
            "LC_ALL": "C",
            "GIT_NO_LAZY_FETCH": "1",
            "GIT_CONFIG_NOSYSTEM": "1",
            "GIT_CONFIG_GLOBAL": os.devnull,
            "GIT_TERMINAL_PROMPT": "0",
            "GIT_OPTIONAL_LOCKS": "0",
        }
    )
    return env


GIT_NO_LAZY_FETCH_OPTION = "--no-lazy-fetch"  # top-level option, git >= 2.44
GIT_DENIED_PROTOCOLS = ("file", "ext", "ssh", "git", "http", "https")
GIT_FAILED = "RULES-USAGE --source-repo: git failed (not a readable repository, or git older than 2.44)"


def _git_config_args() -> list[str]:
    out = ["-c", "protocol.allow=never"]
    for proto in GIT_DENIED_PROTOCOLS:
        out += ["-c", f"protocol.{proto}.allow=never"]
    out += ["-c", "core.fsmonitor=false", "-c", f"core.hooksPath={os.devnull}"]
    return out


def _git_args(source_repo: Path, *args: str) -> list[str]:
    return [
        "git",
        GIT_NO_LAZY_FETCH_OPTION,
        *_git_config_args(),
        "-C",
        str(source_repo),
        *args,
    ]


def _run_git(source_repo: Path, args: list[str], stdin: bytes | None = None) -> tuple[int, bytes] | None:
    """Run hardened git; return (exit code, stdout) or None if it could not run."""
    try:
        r = subprocess.run(
            _git_args(source_repo, *args),
            input=stdin if stdin is not None else b"",
            stdout=subprocess.PIPE,
            stderr=subprocess.DEVNULL,
            env=git_env(),
            timeout=GIT_TIMEOUT_S,
            check=False,
        )
    except (OSError, subprocess.SubprocessError, ValueError):
        return None
    return r.returncode, r.stdout


def _existing_commits(source_repo: Path, revs: list[str]) -> set[str] | None:
    """Which of ``revs`` name commits in ``source_repo`` (one git call); None if git fails."""
    query = "".join(f"{r}^{{commit}}\n" for r in revs).encode("ascii")
    res = _run_git(source_repo, ["cat-file", "--batch-check=%(objectname)"], query)
    if res is None or res[0] != 0:
        return None
    lines = res[1].decode("ascii", "replace").splitlines()
    if len(lines) != len(revs):
        return None
    return {rev for rev, line in zip(revs, lines) if line.strip() == rev}


def _is_ancestor(source_repo: Path, rev: str, current: str) -> bool | None:
    res = _run_git(source_repo, ["merge-base", "--is-ancestor", rev, current])
    if res is None or res[0] not in (0, 1):
        return None
    return res[0] == 0


# --------------------------------------------------------------------------------------------


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
        assignment, core_budget, agents_budget, canonical = load_manifest(manifest)
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
    stale: list[tuple[str, str]] = []  # (block id, recorded rev), revs differing from current
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
        if b.source_repo != canonical:
            # A revision of some other repository says nothing about the canonical history.
            out.add(f"RULES-SOURCE AGENTS.md {b.id} {b.source_repo}")
            continue
        if b.source_rev != current_rev:
            stale.append((b.id, b.source_rev))
    out.update(f"RULES-MISSING {b}" for b in required if b not in seen)
    if stale:
        out.update(_check_revs(stale, current_rev, source_repo))
    return sorted(out)


def _check_revs(stale: list[tuple[str, str]], current: str, source_repo: Path | None) -> list[str]:
    if source_repo is None:
        return [f"warn RULES-STALE {bid}" for bid, _ in stale]
    distinct = sorted({rev for _, rev in stale})
    existing = _existing_commits(source_repo, [current] + distinct)
    if existing is None:
        return [GIT_FAILED]
    if current not in existing:
        return ["RULES-USAGE --current-rev is not a commit in --source-repo"]
    ancestor: dict[str, bool] = {}
    for rev in distinct:
        if rev not in existing:
            ancestor[rev] = False
            continue
        r = _is_ancestor(source_repo, rev, current)
        if r is None:
            return [GIT_FAILED]
        ancestor[rev] = r
    return [f"warn RULES-STALE {bid}" if ancestor[rev] else f"RULES-UNKNOWN-REV {bid} {rev}" for bid, rev in stale]


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
