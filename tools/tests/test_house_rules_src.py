"""Acceptance tests of spec node ECO.RULES.SRC (AC-1 .. AC-4) for the house-rules/ directory.

Expected values are hand-labelled from the spec (block IDs, budgets, headings, the 3 073-byte
oversize fixture written by fixtures/house-rules/make_oversize_core.py).
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import fnmatch
import json
import re
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

from house_rules import find_blocks, render  # noqa: E402

try:
    import tomllib
except ImportError:  # Python < 3.11
    tomllib = None

HR = ROOT / "house-rules"
OVERSIZE = ROOT / "fixtures" / "house-rules" / "oversize-core.md"
CORE_BUDGET = 3072
AGENTS_BUDGET = 10240
HEADINGS = [
    "accountability",
    "check then claim",
    "verify before citing",
    "report only what ran",
    "ask only for high-stakes actions",
    "main is pr-only",
    "never crash",
    "clean room",
    "never paste standards text",
]
REV = "0" * 40


def lf_bytes(path):
    """Size in UTF-8 bytes after normalising CRLF to LF."""
    return len(path.read_bytes().replace(b"\r\n", b"\n"))


def size_failure(path, budget):
    """None if ``path`` fits ``budget``, else a message naming the measured size."""
    size = lf_bytes(path)
    if size <= budget:
        return None
    return f"{path.name}: {size} bytes exceeds the {budget}-byte budget"


def load_manifest():
    if tomllib is not None:
        with open(HR / "manifest.toml", "rb") as f:
            return tomllib.load(f)
    return json.loads((HR / "manifest.json").read_text(encoding="utf-8"))


def repo_ids():
    path = ROOT / "repos.toml"
    if not path.exists():
        path = HR / "repos.fixture.toml"
    if tomllib is None:
        raise unittest.SkipTest("reading the repository map needs tomllib (Python >= 3.11)")
    with open(path, "rb") as f:
        return [r["id"] for r in tomllib.load(f)["repo"]]


def headings(text):
    out = []
    for line in text.replace("\r\n", "\n").split("\n"):
        m = re.match(r"#{1,6}\s+(.*?)\s*$", line)
        if m:
            out.append(m.group(1).replace("`", "").lower())
    return out


class Manifest(unittest.TestCase):
    """AC-1 and REQ-001, REQ-002."""

    def test_parses_and_lists_blocks(self):
        m = load_manifest()
        ids = [b["id"] for b in m["block"]]
        self.assertEqual(sorted(ids), ["core", "python", "rust"])
        self.assertEqual(len(ids), len(set(ids)))
        for b in m["block"]:
            self.assertTrue((HR / b["file"]).is_file(), b["file"])
            self.assertTrue(b["repos"], b["id"])

    def test_budgets(self):
        self.assertEqual(load_manifest()["budgets"], {"core_bytes": CORE_BUDGET, "agents_md_bytes": AGENTS_BUDGET})

    def test_core_goes_to_every_repository(self):
        repos = repo_ids()
        self.assertTrue(repos)
        blocks = {b["id"]: b for b in load_manifest()["block"]}
        self.assertEqual(sorted(blocks["core"]["repos"]), sorted(repos))
        for b in blocks.values():
            for r in b["repos"]:
                self.assertIn(r, repos, f"block {b['id']} names unknown repository {r}")

    @unittest.skipIf(tomllib is None, "comparing with manifest.toml needs tomllib")
    def test_json_fallback_equals_toml(self):
        with open(HR / "manifest.toml", "rb") as f:
            toml_m = tomllib.load(f)
        self.assertEqual(json.loads((HR / "manifest.json").read_text(encoding="utf-8")), toml_m)


class Core(unittest.TestCase):
    """AC-2 and REQ-003."""

    def test_core_fits_budget(self):
        self.assertIsNone(size_failure(HR / "core.md", CORE_BUDGET))

    def test_core_has_every_heading(self):
        found = headings((HR / "core.md").read_text(encoding="utf-8"))
        for h in HEADINGS:
            self.assertIn(h, found)

    def test_every_block_renders_and_round_trips(self):
        # The sources must be valid block bodies for ECO.RULES.BLOCK (no marker lines).
        for b in load_manifest()["block"]:
            body = (HR / b["file"]).read_text(encoding="utf-8").replace("\r\n", "\n").rstrip("\n")
            [parsed] = find_blocks(render(b["id"], "spec", REV, body))
            self.assertEqual(parsed.body, body)


class RestrictedPaths(unittest.TestCase):
    """AC-3."""

    def test_patterns_cover_standards_excerpts_and_customer_data(self):
        lines = (HR / "restricted-paths.txt").read_text(encoding="utf-8").splitlines()
        self.assertTrue(lines)
        for line in lines:
            self.assertEqual(line, line.strip())
            self.assertTrue(line and not line.startswith("#"), "one pattern per line, no comments")
        for sample in ["docs/standards-excerpts/iso-1101/clause-8.md", "fixtures/customer-data/acme/part.stl"]:
            self.assertTrue(any(fnmatch.fnmatch(sample, p) for p in lines), sample)
        self.assertFalse(any(fnmatch.fnmatch("spec/eco/rules/src/NODE.md", p) for p in lines))


class Oversize(unittest.TestCase):
    """AC-4 (error path)."""

    def test_oversize_fixture_fails_with_measured_size(self):
        msg = size_failure(OVERSIZE, CORE_BUDGET)
        self.assertIsNotNone(msg)
        self.assertIn("3073 bytes", msg)

    def test_fixture_size_is_hand_labelled(self):
        self.assertEqual(lf_bytes(OVERSIZE), 3073)


class Readme(unittest.TestCase):
    """REQ-004."""

    def test_readme_says_canonical_and_generated(self):
        text = (HR / "README.md").read_text(encoding="utf-8").lower()
        self.assertIn("canonical source", text)
        self.assertIn("generated", text)
        self.assertIn("by hand", text)


if __name__ == "__main__":
    unittest.main()
