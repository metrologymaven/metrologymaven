"""Tests of tools/house_rules/sync.py, spec node ECO.RULES.SYNC (AC-1..AC-6).

Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOOLS = ROOT / "tools"
FIX = Path(__file__).resolve().parent / "fixtures" / "sync"
SRC = FIX / "source"
REV = "a" * 40
REV2 = "b" * 40

sys.path.insert(0, str(TOOLS))

from house_rules import find_blocks  # noqa: E402
from house_rules.check import check as check_rules  # noqa: E402


def run_cli(*args: str) -> tuple[int, str, str]:
    env = dict(os.environ)
    env["PYTHONPATH"] = str(TOOLS) + (os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else "")
    r = subprocess.run(
        [sys.executable, "-m", "house_rules", "sync", *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        env=env,
    )
    return r.returncode, r.stdout, r.stderr


class SyncTest(unittest.TestCase):
    def copy_tree(self, src: Path) -> Path:
        tmp = Path(tempfile.mkdtemp(prefix="hr-sync-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        dst = tmp / src.name
        shutil.copytree(src, dst)
        return dst

    def test_ac1_inserts_blocks_after_heading_and_writes_claude_md(self) -> None:
        target = self.copy_tree(FIX / "target-empty")
        code, out, err = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 0, out + err)
        self.assertIn("added core", out)
        self.assertIn("added rust", out)

        agents = (target / "AGENTS.md").read_text(encoding="utf-8")
        lines = agents.splitlines()
        self.assertEqual(lines[0], "# Sample repo")
        self.assertEqual(lines[1], "")
        self.assertTrue(lines[2].startswith("<!-- house-rules:begin block=core "))

        blocks = find_blocks(agents)
        self.assertEqual([b.id for b in blocks], ["core", "rust"])
        for b in blocks:
            self.assertTrue(b.is_intact(), b.id)
            self.assertEqual(b.source_rev, REV)

        # Paragraph outside blocks is unchanged.
        self.assertTrue(agents.endswith("\nLocal notes paragraph.\n"))

        # CLAUDE.md is exact.
        self.assertEqual((target / "CLAUDE.md").read_bytes(), b"@AGENTS.md\n")

        # Cross-check with ECO.RULES.CHECK: zero findings!
        findings = check_rules(target, "repo-rust", SRC / "manifest.toml", REV)
        self.assertEqual(findings, [])

    def test_ac2_idempotent_second_run_writes_nothing(self) -> None:
        target = self.copy_tree(FIX / "target-empty")
        code1, out1, err1 = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code1, 0, out1 + err1)
        agents_before = (target / "AGENTS.md").read_bytes()
        claude_before = (target / "CLAUDE.md").read_bytes()

        code2, out2, err2 = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code2, 0, out2 + err2)
        self.assertEqual(out2.strip(), "no changes")
        self.assertEqual((target / "AGENTS.md").read_bytes(), agents_before)
        self.assertEqual((target / "CLAUDE.md").read_bytes(), claude_before)

    def test_ac3_restores_hand_edited_block(self) -> None:
        target = self.copy_tree(FIX / "target-edited")
        code, out, err = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 0, out + err)
        self.assertIn("restored core", out)
        agents = (target / "AGENTS.md").read_text(encoding="utf-8")
        self.assertNotIn("EDITED BY HAND.", agents)
        self.assertIn("1. Be accountable.", agents)
        self.assertTrue(agents.endswith("\nTail note.\n"))
        blocks = find_blocks(agents)
        self.assertEqual([b.id for b in blocks], ["core", "rust"])
        self.assertTrue(all(b.is_intact() for b in blocks))

    def test_ac4_removes_unassigned_block(self) -> None:
        target = self.copy_tree(FIX / "target-extra")
        code, out, err = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 0, out + err)
        self.assertIn("removed python", out)
        agents = (target / "AGENTS.md").read_text(encoding="utf-8")
        blocks = find_blocks(agents)
        self.assertEqual([b.id for b in blocks], ["core", "rust"])
        self.assertNotIn("Python rules", agents)
        self.assertTrue(agents.startswith("# Extra repo\n"))
        self.assertTrue(agents.endswith("\nKeep this.\n"))

    def test_ac5_dry_run_writes_nothing(self) -> None:
        target = self.copy_tree(FIX / "target-empty")
        agents_before = (target / "AGENTS.md").read_bytes()
        code, out, err = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target), "--dry-run"
        )
        self.assertEqual(code, 0, out + err)
        self.assertIn("added core", out)
        self.assertIn("added rust", out)
        self.assertIn("created CLAUDE.md", out)
        self.assertEqual((target / "AGENTS.md").read_bytes(), agents_before)
        self.assertFalse((target / "CLAUDE.md").exists())

    def test_ac6_preserves_crlf_outside_blocks(self) -> None:
        target = self.copy_tree(FIX / "target-crlf")
        # Ensure CRLF input bytes regardless of git checkout line-ending normalization.
        agents_path = target / "AGENTS.md"
        agents_path.write_bytes(agents_path.read_bytes().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n"))
        code, out, err = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 0, out + err)
        raw = agents_path.read_bytes()
        self.assertTrue(raw.startswith(b"# CRLF repo\r\n\r\n<!-- house-rules:begin block=core "))
        self.assertTrue(raw.endswith(b"\nKeep CRLF outside blocks.\r\nSecond line.\r\n"))
        # Running again is idempotent.
        code2, out2, _ = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code2, 0)
        self.assertEqual(out2.strip(), "no changes")

    def test_revision_update_updates_blocks(self) -> None:
        target = self.copy_tree(FIX / "target-empty")
        run_cli("--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target))
        code, out, err = run_cli(
            "--source", str(SRC), "--source-rev", REV2, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 0, out + err)
        self.assertIn("updated core", out)
        self.assertIn("updated rust", out)
        blocks = find_blocks((target / "AGENTS.md").read_text(encoding="utf-8"))
        self.assertTrue(all(b.source_rev == REV2 for b in blocks))

    def test_json_manifest_fallback(self) -> None:
        src = self.copy_tree(SRC)
        (src / "manifest.toml").unlink()
        target = self.copy_tree(FIX / "target-empty")
        code, out, err = run_cli(
            "--source", str(src), "--source-rev", REV, "--repo-id", "repo-py", "--target", str(target)
        )
        self.assertEqual(code, 0, out + err)
        blocks = find_blocks((target / "AGENTS.md").read_text(encoding="utf-8"))
        self.assertEqual([b.id for b in blocks], ["core", "python"])

    def test_error_codes_sync_repo_source_target(self) -> None:
        target = self.copy_tree(FIX / "target-empty")
        # SYNC-REPO
        code, out, _ = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "unknown-repo", "--target", str(target)
        )
        self.assertEqual(code, 1)
        self.assertIn("SYNC-REPO", out)

        # SYNC-SOURCE (missing block file)
        src = self.copy_tree(SRC)
        (src / "rust.md").unlink()
        code, out, _ = run_cli(
            "--source", str(src), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 1)
        self.assertIn("SYNC-SOURCE", out)

        # SYNC-TARGET (malformed block marker in AGENTS.md)
        (target / "AGENTS.md").write_text("# Broken\n<!-- house-rules:begin bad -->\n", encoding="utf-8")
        code, out, _ = run_cli(
            "--source", str(SRC), "--source-rev", REV, "--repo-id", "repo-rust", "--target", str(target)
        )
        self.assertEqual(code, 1)
        self.assertIn("SYNC-TARGET", out)


if __name__ == "__main__":
    unittest.main()
