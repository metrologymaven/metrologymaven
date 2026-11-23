#!/usr/bin/env python3
"""Deterministic fixture generator for ECO.REPO.TPL.SELFTEST (AC-1..AC-3). CC-BY-4.0.

Writes fixtures under fixtures/repo/tpl/selftest/:
  required-checks.toml       valid required-check set with format, lint and rules (AC-1)
  missing-local.toml         required-check set with `layers` missing `local` (AC-3)
  broken-template/...        template copy whose `code/AGENTS.md` has a hand-edited managed block (AC-2)

Usage: python3 fixtures/repo/tpl/selftest/make_fixtures.py [--check]
"""

from __future__ import annotations

import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[3]
TOOLS = ROOT / "tools"
if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from house_rules import render  # noqa: E402

REV_A = "a" * 40


def build_files() -> dict[str, bytes]:
    required_checks = """# Fixture required-check set for ECO.REPO.TPL.SELFTEST AC-1 and AC-2. CC-BY-4.0.
[[check]]
id = "format"
local = ["{python}", "-c", "import pathlib, sys; bad=[str(p) for p in pathlib.Path('.').rglob('*') if p.is_file() and '.git' not in p.parts and bytes([13, 10]) in p.read_bytes()]; sys.exit(1 if bad else 0)"]

[[check]]
id = "lint"
local = ["{python}", "-c", "import pathlib, sys; req=['AGENTS.md','CLAUDE.md','LICENSE-MIT','LICENSE-APACHE']; miss=[r for r in req if not pathlib.Path(r).is_file()]; sys.exit(1 if miss else 0)"]

[[check]]
id = "rules"
local = ["{python}", "-m", "house_rules", "check", "--target", ".", "--repo-id", "{repo_id}", "--manifest", "{manifest}", "--current-rev", "{rules_rev}"]
"""

    missing_local = """# Fixture required-check set for ECO.REPO.TPL.SELFTEST AC-3: `layers` has no `local` command. CC-BY-4.0.
[[check]]
id = "format"
local = ["{python}", "-c", "import sys; sys.exit(0)"]

[[check]]
id = "layers"
"""

    # Build a broken-template where `code/AGENTS.md` carries a hand-edited managed block (digest mismatch),
    # while `spec/AGENTS.md` is valid (no pre-existing block, synced cleanly).
    core_body = (ROOT / "house-rules" / "core.md").read_text(encoding="utf-8").rstrip("\n")
    valid_block = render("core", "metrologymaven", REV_A, core_body)
    hand_edited_block = valid_block.replace(
        "\n<!-- house-rules:end block=core -->",
        "\n- Hand-edited line that breaks the recorded SHA-256 digest.\n<!-- house-rules:end block=core -->",
        1,
    )
    assert hand_edited_block != valid_block

    tpl_root = ROOT / "templates" / "repo"
    files: dict[str, bytes] = {
        "required-checks.toml": required_checks.encode("utf-8"),
        "missing-local.toml": missing_local.encode("utf-8"),
        "broken-template/variants.toml": (tpl_root / "variants.toml").read_bytes(),
    }

    # Copy common/ excluding AGENTS.md (placed per-variant so code has the hand-edited block and spec stays clean).
    for src in sorted((tpl_root / "common").rglob("*")):
        if src.is_dir():
            continue
        rel = src.relative_to(tpl_root / "common").as_posix()
        if rel == "AGENTS.md":
            continue
        files[f"broken-template/common/{rel}"] = src.read_bytes()

    base_agents = (tpl_root / "common" / "AGENTS.md").read_text(encoding="utf-8")
    lines = base_agents.split("\n")
    heading = next(i for i, ln in enumerate(lines) if ln.startswith("#"))
    broken_agents = "\n".join(lines[: heading + 1]) + "\n\n" + hand_edited_block + "\n".join(lines[heading + 1 :])

    for variant in ("code", "spec"):
        for src in sorted((tpl_root / variant).rglob("*")):
            if src.is_dir():
                continue
            rel = src.relative_to(tpl_root / variant).as_posix()
            files[f"broken-template/{variant}/{rel}"] = src.read_bytes()
    files["broken-template/code/AGENTS.md"] = broken_agents.encode("utf-8")
    files["broken-template/spec/AGENTS.md"] = base_agents.encode("utf-8")

    return files


def main() -> int:
    files = build_files()
    if "--check" in sys.argv[1:]:
        mismatches = []
        for rel, want in sorted(files.items()):
            p = HERE / rel
            if not p.is_file() or p.read_bytes() != want:
                mismatches.append(rel)
        if mismatches:
            print("FAIL: stale fixtures: " + ", ".join(mismatches), file=sys.stderr)
            return 1
        print(f"fixtures/repo/tpl/selftest: ok ({len(files)} files)")
        return 0

    for rel, data in sorted(files.items()):
        p = HERE / rel
        p.parent.mkdir(parents=True, exist_ok=True)
        p.write_bytes(data)
    print(f"wrote {len(files)} fixtures under fixtures/repo/tpl/selftest/")
    return 0


if __name__ == "__main__":
    sys.exit(main())
