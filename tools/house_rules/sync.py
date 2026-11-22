"""House-rules sync tool. Spec node ECO.RULES.SYNC.

Standard library only. Usage::

    python -m house_rules sync --source <dir> --source-rev <40-hex> \\
        --repo-id <id> --target <dir> [--dry-run]

Writes or refreshes generated house-rules blocks in ``<target>/AGENTS.md`` and ensures
``<target>/CLAUDE.md`` is ``@AGENTS.md\\n``. Preserves every byte outside generated blocks
(including CRLF line endings), writes via temp file + atomic rename only when bytes differ,
and prints ``no changes`` when already up to date.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import stat
import sys
import tempfile
from dataclasses import dataclass
from pathlib import Path
from typing import TextIO

from .block import (
    Block,
    BlockError,
    RenderError,
    find_blocks,
    is_valid_block_id,
    is_valid_repo_id,
    is_valid_rev,
    render,
)
from .check import DEFAULT_SOURCE_REPO, MAX_INPUT_BYTES

__all__ = ["SyncError", "SyncResult", "load_sync_manifest", "sync_repo", "main"]

CLAUDE_CONTENT = b"@AGENTS.md\n"
_HEADING_RE = re.compile(r"^#{1,6}[ \t]+\S")
_BOM = "\ufeff"


class SyncError(Exception):
    """Typed sync failure whose message begins with SYNC-SOURCE, SYNC-TARGET, SYNC-REPO or SYNC-USAGE."""

    def __init__(self, code: str, detail: str) -> None:
        super().__init__(f"{code} {detail}")
        self.code = code
        self.detail = detail


@dataclass(frozen=True)
class BlockSpec:
    id: str
    file: str
    repos: tuple[str, ...]


@dataclass(frozen=True)
class SyncResult:
    actions: tuple[str, ...]
    agents_changed: bool
    claude_changed: bool
    wrote: bool


def _is_link(p: Path) -> bool:
    if p.is_symlink():
        return True
    is_junction = getattr(p, "is_junction", None)
    return bool(is_junction()) if is_junction is not None else False


def _read_regular(p: Path, code: str, label: str) -> bytes | None:
    """Read a regular file up to MAX_INPUT_BYTES; return None if absent; refuse symlinks."""
    try:
        st = os.lstat(p)
    except FileNotFoundError:
        return None
    except OSError as e:
        raise SyncError(code, f"{label} unreadable ({e.strerror or 'error'})") from None
    if stat.S_ISLNK(st.st_mode) or _is_link(p):
        raise SyncError(code, f"{label} symlink refused")
    if not stat.S_ISREG(st.st_mode):
        raise SyncError(code, f"{label} not a regular file")
    if st.st_size > MAX_INPUT_BYTES:
        raise SyncError(code, f"{label} too large ({st.st_size} > {MAX_INPUT_BYTES} bytes)")
    try:
        with open(p, "rb") as f:
            data = f.read(MAX_INPUT_BYTES + 1)
    except OSError as e:
        raise SyncError(code, f"{label} unreadable ({e.strerror or 'error'})") from None
    if len(data) > MAX_INPUT_BYTES:
        raise SyncError(code, f"{label} too large (> {MAX_INPUT_BYTES} bytes)")
    return data


def _no_duplicate_keys(pairs: list[tuple[str, object]]) -> dict[str, object]:
    out: dict[str, object] = {}
    for k, v in pairs:
        if k in out:
            raise SyncError("SYNC-SOURCE", f"manifest duplicate key {k!r}")
        out[k] = v
    return out


def load_sync_manifest(source_dir: Path) -> tuple[list[BlockSpec], str, set[str]]:
    """Load manifest.toml (or manifest.json on Python < 3.11 / when only json exists)."""
    if not source_dir.is_dir() or _is_link(source_dir):
        raise SyncError("SYNC-SOURCE", f"source directory {source_dir} is not an accessible directory")

    toml_path = source_dir / "manifest.toml"
    json_path = source_dir / "manifest.json"

    try:
        import tomllib  # type: ignore[import-not-found]
    except ImportError:
        tomllib = None

    raw: bytes | None = None
    use_toml = False
    if tomllib is not None and toml_path.exists():
        raw = _read_regular(toml_path, "SYNC-SOURCE", "manifest.toml")
        use_toml = True
    elif json_path.exists():
        raw = _read_regular(json_path, "SYNC-SOURCE", "manifest.json")
        use_toml = False
    elif toml_path.exists():
        raise SyncError("SYNC-SOURCE", "manifest.toml requires Python >= 3.11 and manifest.json is missing")

    if raw is None:
        raise SyncError("SYNC-SOURCE", "manifest.toml / manifest.json missing")

    try:
        text = raw.decode("utf-8")
        if use_toml:
            m = tomllib.loads(text)
        else:
            m = json.loads(text, object_pairs_hook=_no_duplicate_keys)
    except UnicodeDecodeError:
        raise SyncError("SYNC-SOURCE", "manifest is not valid UTF-8") from None
    except ValueError as e:
        first = str(e).splitlines()[0] if str(e) else "syntax error"
        raise SyncError("SYNC-SOURCE", f"manifest syntax error ({first})") from None

    if not isinstance(m, dict):
        raise SyncError("SYNC-SOURCE", "manifest root must be a table/object")

    source_repo = m.get("source_repo", DEFAULT_SOURCE_REPO)
    if not isinstance(source_repo, str) or not is_valid_repo_id(source_repo):
        raise SyncError("SYNC-SOURCE", "manifest source_repo is not a valid repository ID")

    raw_blocks = m.get("block")
    if not isinstance(raw_blocks, list):
        raise SyncError("SYNC-SOURCE", "manifest missing block list")

    specs: list[BlockSpec] = []
    seen_ids: set[str] = set()
    known_repos: set[str] = set()

    extra_repos = m.get("repos")
    if extra_repos is not None:
        if not isinstance(extra_repos, list) or not all(isinstance(r, str) and is_valid_repo_id(r) for r in extra_repos):
            raise SyncError("SYNC-SOURCE", "manifest repos must be a list of valid repository IDs")
        known_repos.update(extra_repos)

    for entry in raw_blocks:
        if not isinstance(entry, dict):
            raise SyncError("SYNC-SOURCE", "manifest block entry must be a table/object")
        bid = entry.get("id")
        rel_file = entry.get("file")
        repos = entry.get("repos")
        if not isinstance(bid, str) or not is_valid_block_id(bid):
            raise SyncError("SYNC-SOURCE", f"invalid block id {bid!r}")
        if bid in seen_ids:
            raise SyncError("SYNC-SOURCE", f"duplicate block id {bid}")
        if not isinstance(rel_file, str) or not rel_file or rel_file.startswith("/") or ".." in Path(rel_file).parts:
            raise SyncError("SYNC-SOURCE", f"block {bid} has invalid file path {rel_file!r}")
        if not isinstance(repos, list) or not all(isinstance(r, str) and is_valid_repo_id(r) for r in repos):
            raise SyncError("SYNC-SOURCE", f"block {bid} has invalid repos list")
        seen_ids.add(bid)
        known_repos.update(repos)
        specs.append(BlockSpec(id=bid, file=rel_file, repos=tuple(repos)))

    return specs, source_repo, known_repos


def _read_block_body(source_dir: Path, spec: BlockSpec) -> str:
    src_root = source_dir.resolve()
    p = source_dir / spec.file
    raw = _read_regular(p, "SYNC-SOURCE", spec.file)
    if raw is None:
        raise SyncError("SYNC-SOURCE", f"missing block file {spec.file} for block {spec.id}")
    resolved = p.resolve()
    if src_root not in resolved.parents:
        raise SyncError("SYNC-SOURCE", f"block file {spec.file} escapes source directory")
    try:
        text = raw.decode("utf-8")
    except UnicodeDecodeError:
        raise SyncError("SYNC-SOURCE", f"block file {spec.file} is not valid UTF-8") from None
    text = text.replace("\r\n", "\n")
    if text.endswith("\n"):
        text = text[:-1]
    return text


def _split_lines_keep_lf(text: str) -> list[str]:
    """Split text into 1-based lines matching block.find_blocks line numbering, preserving exact line bytes."""
    if not text:
        return []
    parts = text.split("\n")
    lines = [p + "\n" for p in parts[:-1]]
    if parts[-1] != "":
        lines.append(parts[-1])
    return lines


def _splice_agents(
    existing_text: str,
    desired_order: list[str],
    rendered_blocks: dict[str, str],
) -> tuple[str, list[str]]:
    """Splice desired rendered blocks into existing AGENTS.md without changing any byte outside blocks."""
    bom = ""
    body_text = existing_text
    if body_text.startswith(_BOM):
        bom = _BOM
        body_text = body_text[1:]

    try:
        found = find_blocks(body_text)
    except BlockError as e:
        raise SyncError("SYNC-TARGET", f"AGENTS.md parse error ({e})") from None

    seen_found: set[str] = set()
    for b in found:
        if b.id in seen_found:
            raise SyncError("SYNC-TARGET", f"AGENTS.md contains duplicate block {b.id}")
        seen_found.add(b.id)

    desired_set = set(desired_order)
    lines = _split_lines_keep_lf(body_text)

    # Index existing blocks by 0-based start line.
    by_start: dict[int, Block] = {b.begin_line - 1: b for b in found}
    blocked_indices: set[int] = set()
    for b in found:
        for idx in range(b.begin_line - 1, b.end_line):
            blocked_indices.add(idx)

    # Determine missing blocks (in manifest order).
    missing_ids = [bid for bid in desired_order if bid not in seen_found]

    # Locate insertion point for missing blocks: right after the first Markdown heading outside any block.
    insert_after: int | None = None
    if missing_ids:
        for idx, raw_line in enumerate(lines):
            if idx in blocked_indices:
                continue
            stripped = raw_line.rstrip("\r\n")
            if _HEADING_RE.match(stripped):
                insert_after = idx
                break

    actions: list[str] = []
    out_chunks: list[str] = []

    if missing_ids and insert_after is None:
        for bid in missing_ids:
            out_chunks.append(rendered_blocks[bid])
            actions.append(f"added {bid}")

    idx = 0
    n_lines = len(lines)
    while idx < n_lines:
        block = by_start.get(idx)
        if block is not None:
            old_slice = "".join(lines[block.begin_line - 1 : block.end_line])
            if block.id not in desired_set:
                actions.append(f"removed {block.id}")
            else:
                want = rendered_blocks[block.id]
                if not block.is_intact():
                    out_chunks.append(want)
                    actions.append(f"restored {block.id}")
                elif old_slice != want:
                    out_chunks.append(want)
                    actions.append(f"updated {block.id}")
                else:
                    out_chunks.append(old_slice)
            idx = block.end_line
            continue

        out_chunks.append(lines[idx])
        if missing_ids and insert_after == idx:
            # Ensure the heading line ends with a newline before inserting generated blocks.
            if not lines[idx].endswith("\n"):
                out_chunks.append("\n")
            for bid in missing_ids:
                out_chunks.append(rendered_blocks[bid])
                actions.append(f"added {bid}")
        idx += 1

    new_text = bom + "".join(out_chunks)
    return new_text, actions


def _atomic_write(target_path: Path, data: bytes) -> None:
    parent = target_path.parent
    tmp_name: str | None = None
    try:
        with tempfile.NamedTemporaryFile(dir=parent, prefix=f".{target_path.name}.", suffix=".tmp", delete=False) as tf:
            tmp_name = tf.name
            tf.write(data)
            tf.flush()
        os.replace(tmp_name, target_path)
        tmp_name = None
    except OSError as e:
        raise SyncError("SYNC-TARGET", f"failed writing {target_path.name} ({e.strerror or 'error'})") from None
    finally:
        if tmp_name is not None:
            try:
                os.unlink(tmp_name)
            except OSError:
                pass


def sync_repo(
    source_dir: Path,
    source_rev: str,
    repo_id: str,
    target_dir: Path,
    *,
    dry_run: bool = False,
) -> SyncResult:
    """Sync house-rules blocks and CLAUDE.md in target_dir from source_dir at source_rev."""
    if not is_valid_rev(source_rev):
        raise SyncError("SYNC-USAGE", "--source-rev must be 40 lowercase hex digits")
    if not is_valid_repo_id(repo_id):
        raise SyncError("SYNC-REPO", f"invalid repository ID {repo_id!r}")
    if not target_dir.is_dir() or _is_link(target_dir):
        raise SyncError("SYNC-TARGET", f"target directory {target_dir} is not an accessible directory")

    specs, source_repo, known_repos = load_sync_manifest(source_dir)
    if repo_id not in known_repos:
        raise SyncError("SYNC-REPO", f"repository {repo_id} not in manifest")

    assigned = [s for s in specs if repo_id in s.repos]
    desired_order = [s.id for s in assigned]
    rendered_blocks: dict[str, str] = {}
    for spec in assigned:
        body = _read_block_body(source_dir, spec)
        try:
            rendered_blocks[spec.id] = render(spec.id, source_repo, source_rev, body)
        except RenderError as e:
            raise SyncError("SYNC-SOURCE", f"cannot render block {spec.id} ({e})") from None

    agents_path = target_dir / "AGENTS.md"
    old_agents_bytes = _read_regular(agents_path, "SYNC-TARGET", "AGENTS.md")
    if old_agents_bytes is None:
        old_agents_text = ""
    else:
        try:
            old_agents_text = old_agents_bytes.decode("utf-8")
        except UnicodeDecodeError:
            raise SyncError("SYNC-TARGET", "AGENTS.md is not valid UTF-8") from None

    new_agents_text, actions = _splice_agents(old_agents_text, desired_order, rendered_blocks)
    new_agents_bytes = new_agents_text.encode("utf-8")
    agents_changed = old_agents_bytes != new_agents_bytes

    claude_path = target_dir / "CLAUDE.md"
    old_claude_bytes = _read_regular(claude_path, "SYNC-TARGET", "CLAUDE.md")
    claude_changed = old_claude_bytes != CLAUDE_CONTENT
    if claude_changed:
        actions.append("created CLAUDE.md" if old_claude_bytes is None else "updated CLAUDE.md")

    wrote = False
    if not dry_run:
        if agents_changed:
            _atomic_write(agents_path, new_agents_bytes)
            wrote = True
        if claude_changed:
            _atomic_write(claude_path, CLAUDE_CONTENT)
            wrote = True

    return SyncResult(
        actions=tuple(actions),
        agents_changed=agents_changed,
        claude_changed=claude_changed,
        wrote=wrote,
    )


def _parse_args(argv: list[str]) -> argparse.Namespace:
    ap = argparse.ArgumentParser(prog="python -m house_rules sync", add_help=True)
    ap.add_argument("--source", type=Path, required=True, help="canonical house-rules source directory")
    ap.add_argument("--source-rev", required=True, help="40-hex canonical source revision")
    ap.add_argument("--repo-id", required=True, help="target repository ID from manifest")
    ap.add_argument("--target", type=Path, required=True, help="target repository working tree")
    ap.add_argument("--dry-run", action="store_true", help="print planned changes without writing")
    return ap.parse_args(argv)


def main(argv: list[str], out: TextIO | None = None) -> int:
    stream = sys.stdout if out is None else out
    try:
        args = _parse_args(argv)
    except SystemExit as e:
        return int(e.code) if isinstance(e.code, int) else 1
    try:
        res = sync_repo(
            args.source,
            args.source_rev,
            args.repo_id,
            args.target,
            dry_run=args.dry_run,
        )
    except SyncError as e:
        stream.write(f"{e}\n")
        return 1
    if not res.actions:
        stream.write("no changes\n")
    else:
        for line in res.actions:
            stream.write(f"{line}\n")
    return 0
