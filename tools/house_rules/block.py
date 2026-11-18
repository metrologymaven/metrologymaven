"""Generated house-rules blocks inside Markdown files. Spec node ECO.RULES.BLOCK.

Standard library only, so SYNC and CHECK run in every repository's CI without a Rust toolchain.

A block is the begin marker line, the body lines, and the end marker line::

    <!-- house-rules:begin block=<id> source=<repo-id>@<40-hex rev> sha256=<64-hex> -->
    ...body...
    <!-- house-rules:end block=<id> -->

* Block ID: ``^[a-z][a-z0-9-]{1,31}$``. Repo ID: 1 to 100 characters of ``[A-Za-z0-9._/-]``
  starting with a letter or digit. Revision: 40 lowercase hex digits. Digest: 64 lowercase hex.
* The digest is the SHA-256 of the body as UTF-8 with every CRLF replaced by LF, with no
  trailing newline added or removed (:func:`digest`).
* Lines are split on LF; one trailing CR on a line is part of its line ending, so a file with
  CRLF endings gives the same blocks and digests. The body is every line between the markers,
  with their own line endings, except the line ending of the last body line. Zero body lines
  and one empty body line both give the empty body.
* One U+FEFF (byte order mark) at the very start of the text is ignored.

Fail closed: a *marker line* is any line whose text, after leading spaces and tabs, starts with
``<!-- house-rules:``. Every marker line must be an exact begin or end marker (single spaces,
nothing before or after, no trailing whitespace), otherwise parsing fails with ``bad-marker``.
This holds everywhere in the file: a marker line inside a fenced code block, an indented marker,
or a marker with trailing whitespace is not skipped or tolerated. Documentation that shows the
format must quote it so that the line does not start with ``<!-- house-rules:``.

A digest mismatch is not a parse error: :func:`is_intact` reports it and ECO.RULES.CHECK decides.
"""

from __future__ import annotations

import hashlib
import re
from dataclasses import dataclass

__all__ = [
    "MARKER_PREFIX",
    "Block",
    "BlockError",
    "RenderError",
    "digest",
    "render",
    "find_blocks",
    "is_intact",
    "is_valid_block_id",
    "is_valid_repo_id",
    "is_valid_rev",
]

MARKER_PREFIX = "<!-- house-rules:"
_ID = r"[a-z][a-z0-9-]{1,31}"
_REPO = r"[A-Za-z0-9][A-Za-z0-9._/-]{0,99}"
_REV = r"[0-9a-f]{40}"
_BEGIN = re.compile(
    rf"<!-- house-rules:begin block=({_ID}) source=({_REPO})@({_REV}) sha256=([0-9a-f]{{64}}) -->", re.ASCII
)
_END = re.compile(rf"<!-- house-rules:end block=({_ID}) -->", re.ASCII)
_ID_RE = re.compile(_ID, re.ASCII)
_REPO_RE = re.compile(_REPO, re.ASCII)
_REV_RE = re.compile(_REV, re.ASCII)
_BOM = "﻿"


@dataclass(frozen=True)
class Block:
    """One generated block found in a text."""

    id: str
    source_repo: str
    source_rev: str
    sha256: str  # recorded digest, 64 lowercase hex digits
    body: str
    begin_line: int  # 1-based line of the begin marker
    end_line: int  # 1-based line of the end marker

    def computed_digest(self) -> str:
        """Digest of the body as it is now."""
        return digest(self.body)

    def is_intact(self) -> bool:
        """Whether the body still matches the recorded digest."""
        return self.computed_digest() == self.sha256


class BlockError(ValueError):
    """Parse error: ``kind`` is unbalanced, nested, id-mismatch or bad-marker; ``line`` is 1-based."""

    KINDS = ("unbalanced", "nested", "id-mismatch", "bad-marker")

    def __init__(self, kind: str, line: int):
        super().__init__(f"{kind} at line {line}")
        self.kind = kind
        self.line = line

    def __eq__(self, other: object) -> bool:
        return isinstance(other, BlockError) and (self.kind, self.line) == (other.kind, other.line)

    def __hash__(self) -> int:
        return hash((self.kind, self.line))

    def __repr__(self) -> str:
        return f"BlockError({self.kind!r}, line={self.line})"


class RenderError(ValueError):
    """Why :func:`render` refused its input.

    ``kind`` is bad-block-id, bad-source-repo, bad-source-rev, marker-in-body (``line`` is the
    1-based body line) or trailing-cr (the body ends in CR, which parsing would read as part of
    a CRLF line ending).
    """

    def __init__(self, kind: str, line: int | None = None):
        super().__init__(kind if line is None else f"{kind} at body line {line}")
        self.kind = kind
        self.line = line


def is_valid_block_id(s: str) -> bool:
    return _ID_RE.fullmatch(s) is not None


def is_valid_repo_id(s: str) -> bool:
    return _REPO_RE.fullmatch(s) is not None


def is_valid_rev(s: str) -> bool:
    return _REV_RE.fullmatch(s) is not None


def digest(body: str) -> str:
    """SHA-256 (64 lowercase hex) of ``body`` as UTF-8 with CRLF normalised to LF."""
    return hashlib.sha256(body.replace("\r\n", "\n").encode("utf-8")).hexdigest()


def is_intact(block: Block) -> bool:
    """Whether ``block``'s body matches its recorded digest."""
    return block.is_intact()


def _strip_cr(line: str) -> str:
    return line[:-1] if line.endswith("\r") else line


def _is_marker_line(line: str) -> bool:
    return line.lstrip(" \t").startswith(MARKER_PREFIX)


def render(block_id: str, source_repo: str, source_rev: str, body: str) -> str:
    """Render a block (begin marker, body, end marker, each ending in LF).

    Parsing the result with :func:`find_blocks` returns the same ID, repository, revision,
    digest and body. Raises :class:`RenderError` for invalid IDs or revisions, a body containing
    a marker line, or a body ending in CR.
    """
    if not is_valid_block_id(block_id):
        raise RenderError("bad-block-id")
    if not is_valid_repo_id(source_repo):
        raise RenderError("bad-source-repo")
    if not is_valid_rev(source_rev):
        raise RenderError("bad-source-rev")
    for i, line in enumerate(body.split("\n")):
        if _is_marker_line(_strip_cr(line)):
            raise RenderError("marker-in-body", i + 1)
    if body.endswith("\r"):
        raise RenderError("trailing-cr")
    return (
        f"<!-- house-rules:begin block={block_id} source={source_repo}@{source_rev} sha256={digest(body)} -->\n"
        f"{body}\n<!-- house-rules:end block={block_id} -->\n"
    )


def find_blocks(text: str) -> list[Block]:
    """Find and parse every block in ``text``, in order.

    Raises :class:`BlockError` naming the 1-based line when markers are unbalanced (a stray end
    marker, or a begin marker never closed: reported at the begin line), nested, carry different
    block IDs, or are malformed.
    """
    if text.startswith(_BOM):
        text = text[1:]
    lines = text.split("\n")
    blocks: list[Block] = []
    open_: tuple[int, re.Match[str]] | None = None
    for i, raw in enumerate(lines):
        line = _strip_cr(raw)
        if not _is_marker_line(line):
            continue
        n = i + 1
        begin = _BEGIN.fullmatch(line)
        if begin is not None:
            if open_ is not None:
                raise BlockError("nested", n)
            open_ = (n, begin)
            continue
        end = _END.fullmatch(line)
        if end is None:
            raise BlockError("bad-marker", n)
        if open_ is None:
            raise BlockError("unbalanced", n)
        begin_line, m = open_
        if end.group(1) != m.group(1):
            raise BlockError("id-mismatch", n)
        body_lines = lines[begin_line : n - 1]
        if body_lines:
            body_lines[-1] = _strip_cr(body_lines[-1])
        blocks.append(
            Block(
                id=m.group(1),
                source_repo=m.group(2),
                source_rev=m.group(3),
                sha256=m.group(4),
                body="\n".join(body_lines),
                begin_line=begin_line,
                end_line=n,
            )
        )
        open_ = None
    if open_ is not None:
        raise BlockError("unbalanced", open_[0])
    return blocks
