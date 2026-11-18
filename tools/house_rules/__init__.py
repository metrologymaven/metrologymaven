"""House-rules tooling (spec node ECO.RULES). Standard library only.

- :mod:`house_rules.block`: generated block format and digest library (ECO.RULES.BLOCK).
"""

from .block import (
    MARKER_PREFIX,
    Block,
    BlockError,
    RenderError,
    digest,
    find_blocks,
    is_intact,
    is_valid_block_id,
    is_valid_repo_id,
    is_valid_rev,
    render,
)

__all__ = [
    "MARKER_PREFIX",
    "Block",
    "BlockError",
    "RenderError",
    "digest",
    "find_blocks",
    "is_intact",
    "is_valid_block_id",
    "is_valid_repo_id",
    "is_valid_rev",
    "render",
]
