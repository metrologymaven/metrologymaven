#!/usr/bin/env python3
"""Fixture repositories for spec node ECO.RULES.CHECK (house-rules check).

Writes, next to this script:
- manifest.json: a small manifest; repository ``fix-spec`` gets ``core``, ``fix-code`` gets
  ``core`` and ``rust``; budgets 3072 (core body) and 10240 (AGENTS.md)
- ok/: current ``core`` block, ``CLAUDE.md`` exactly ``@AGENTS.md`` plus LF (AC-1, AC-4)
- edited/: as ok/, then one word of the ``core`` body changed after rendering (AC-2)
- missing/: only ``core``; checked as code repository ``fix-code``, so ``rust`` is missing (AC-3)
- big/: as ok/ plus a long repo-local section, so AGENTS.md is exactly 11 000 bytes (AC-5)
- claude-extra/: as ok/, but CLAUDE.md has an extra line (AC-6)

The block text is rendered here with hashlib from the format stated in ECO.RULES.BLOCK; this
script must never import tools/house_rules, so the check is tested against an independent
rendering. The expected results in the tests are hand-labelled from the ACs, not computed.

Run:  python tools/tests/fixtures/check/make_fixtures.py [--check]
"""

import hashlib
import sys
from pathlib import Path

HERE = Path(__file__).parent
REV = "1111111111111111111111111111111111111111"
SOURCE = "metrologymaven"

CORE = (
    "## House rules (core)\n"
    "\n"
    "1. A person is accountable for everything an agent opens.\n"
    "2. Check, then claim.\n"
    "3. Report only what ran."
)
RUST = "## House rules (Rust)\n\nNo unsafe. No unwrap in library code."


def block(block_id, body):
    sha = hashlib.sha256(body.encode("utf-8")).hexdigest()
    return (
        f"<!-- house-rules:begin block={block_id} source={SOURCE}@{REV} sha256={sha} -->\n"
        f"{body}\n"
        f"<!-- house-rules:end block={block_id} -->\n"
    )


def agents(*blocks, local="Repo-local notes go here.\n"):
    return "# AGENTS.md\n\n" + "".join(b + "\n" for b in blocks) + "## Repo-local\n\n" + local


def big():
    head = agents(block("core", CORE), local="")
    filler = "Long repo-local section line to push the file over its budget.\n"
    out = head
    while len(out.encode("utf-8")) + len(filler) < 11000:
        out += filler
    out += "x" * (11000 - len(out.encode("utf-8")) - 1) + "\n"
    return out


def files():
    ok = agents(block("core", CORE))
    edited = ok.replace("Check, then claim.", "Check, then maybe claim.")
    claude = "@AGENTS.md\n"
    manifest = (
        "{\n"
        '  "budgets": {"core_bytes": 3072, "agents_md_bytes": 10240},\n'
        '  "block": [\n'
        '    {"id": "core", "file": "core.md", "repos": ["fix-spec", "fix-code"]},\n'
        '    {"id": "rust", "file": "rust.md", "repos": ["fix-code"]}\n'
        "  ]\n"
        "}\n"
    )
    out = {
        "manifest.json": manifest,
        "ok/AGENTS.md": ok,
        "ok/CLAUDE.md": claude,
        "edited/AGENTS.md": edited,
        "edited/CLAUDE.md": claude,
        "missing/AGENTS.md": ok,
        "missing/CLAUDE.md": claude,
        "big/AGENTS.md": big(),
        "big/CLAUDE.md": claude,
        "claude-extra/AGENTS.md": ok,
        "claude-extra/CLAUDE.md": claude + "Also follow these extra rules.\n",
        # Not used by an AC: a code repository with both blocks current.
        "code-ok/AGENTS.md": agents(block("core", CORE), block("rust", RUST)),
        "code-ok/CLAUDE.md": claude,
    }
    return {k: v.encode("utf-8") for k, v in out.items()}


def main(argv):
    check = "--check" in argv
    bad = []
    for rel, data in sorted(files().items()):
        p = HERE / rel
        if check:
            if not p.is_file() or p.read_bytes() != data:
                bad.append(rel)
        else:
            p.parent.mkdir(parents=True, exist_ok=True)
            p.write_bytes(data)
    if bad:
        print("fixtures differ from make_fixtures.py: " + ", ".join(bad))
        return 1
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
