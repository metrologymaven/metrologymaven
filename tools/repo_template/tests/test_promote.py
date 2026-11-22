"""Tests of tools/repo_template/promote.py (spec node ECO.REPO.TPL.PROMOTE, AC-1..AC-4).

Run from the repository root:  python -m unittest discover -s tools/repo_template/tests -v
"""

from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools"
FIX = ROOT / "fixtures" / "repo" / "tpl" / "promote"
BUNDLE = FIX / "mono.bundle"
REPOS_TOML = FIX / "repos.toml"
AUTHOR = "Maintainer One <maint@example.invalid>"
DATE = "2026-11-22T00:00:00Z"
REV_ZERO = "0" * 40

if str(TOOLS) not in sys.path:
    sys.path.insert(0, str(TOOLS))

from repo_template.promote import compute_git_tree_digest  # noqa: E402


def _run_tool(*args: str) -> subprocess.CompletedProcess[str]:
    env = dict(os.environ)
    env["PYTHONPATH"] = str(TOOLS) + (os.pathsep + env["PYTHONPATH"] if env.get("PYTHONPATH") else "")
    return subprocess.run(
        [sys.executable, "-m", "repo_template", *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        env=env,
    )


def _git(repo: Path, *args: str) -> str:
    r = subprocess.run(
        ["git", "-C", str(repo), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        check=True,
    )
    return r.stdout.strip()


@unittest.skipIf(shutil.which("git") is None, "git not installed")
class PromoteTest(unittest.TestCase):
    def clone_bundle(self) -> Path:
        tmp = Path(tempfile.mkdtemp(prefix="promote-test-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        mono = tmp / "mono"
        subprocess.run(
            ["git", "-c", "core.autocrlf=false", "clone", "-q", str(BUNDLE), str(mono)],
            check=True,
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
        )
        return mono

    def test_ac1_ac2_ac4_promote_repin_and_xtask_pins(self) -> None:
        mono = self.clone_bundle()
        out = mono.parent / "fid-b"

        # Verify source fixture shape: 12 commits total, 5 touching crates/fid-b.
        self.assertEqual(int(_git(mono, "rev-list", "--count", "HEAD")), 12)
        self.assertEqual(int(_git(mono, "rev-list", "--count", "HEAD", "--", "crates/fid-b")), 5)
        src_digest = compute_git_tree_digest(mono, "HEAD:crates/fid-b")
        src_tree_oid = _git(mono, "rev-parse", "HEAD:crates/fid-b")

        # AC-1: promote crates/fid-b -> 5 extracted + 1 template = 6 commits, matching tree digest.
        r = _run_tool(
            "promote",
            "--source", str(mono),
            "--path", "crates/fid-b",
            "--id", "fid-b",
            "--author", AUTHOR,
            "--date", DATE,
            "--out", str(out),
        )
        self.assertEqual(r.returncode, 0, r.stdout + r.stderr)
        self.assertEqual(int(_git(out, "rev-list", "--count", "HEAD")), 6)

        # Last extracted commit (HEAD~1) matches source tree OID and content digest byte-for-byte.
        self.assertEqual(_git(out, "rev-parse", "HEAD~1^{tree}"), src_tree_oid)
        self.assertEqual(compute_git_tree_digest(out, "HEAD~1^{tree}"), src_digest)

        # Original authorship and messages preserved on extracted commits.
        authors = _git(out, "log", "--reverse", "--format=%an|%ae|%aI|%s", "HEAD~1").splitlines()
        self.assertEqual(len(authors), 5)
        self.assertIn("Bob Lib|bob@example.invalid|2026-01-02T10:00:00Z|feat(fid-b): initial crate scaffold", authors[0])
        self.assertIn("Carol Docs|carol@example.invalid|2026-01-06T10:00:00Z|docs(fid-b): document b()", authors[2])

        # Template commit did not overwrite any extracted library file (REQ-002).
        self.assertEqual((out / "Cargo.toml").read_bytes(), (mono / "crates" / "fid-b" / "Cargo.toml").read_bytes())
        self.assertEqual((out / "src" / "lib.rs").read_bytes(), (mono / "crates" / "fid-b" / "src" / "lib.rs").read_bytes())
        self.assertEqual((out / "src" / "extra.rs").read_bytes(), (mono / "crates" / "fid-b" / "src" / "extra.rs").read_bytes())
        # Template files such as CLAUDE.md and AGENTS.md were added in the template commit.
        self.assertEqual((out / "CLAUDE.md").read_bytes(), b"@AGENTS.md\n")
        self.assertTrue((out / "AGENTS.md").is_file())

        # AC-4: output repository has zero remotes.
        self.assertEqual(_git(out, "remote"), "")

        # Determinism (D0): a second promote with identical inputs yields the exact same HEAD commit hash.
        out2 = mono.parent / "fid-b-2"
        r2 = _run_tool(
            "promote",
            "--source", str(mono),
            "--path", "crates/fid-b",
            "--id", "fid-b",
            "--author", AUTHOR,
            "--date", DATE,
            "--out", str(out2),
        )
        self.assertEqual(r2.returncode, 0, r2.stdout + r2.stderr)
        new_rev = _git(out, "rev-parse", "HEAD")
        self.assertEqual(_git(out2, "rev-parse", "HEAD"), new_rev)

        # AC-2: repin monorepo consumers to https://example.invalid/fid-b @ new_rev.
        rr = _run_tool(
            "repin",
            "--workspace", str(mono),
            "--crate", "fid-b",
            "--url", "https://example.invalid/fid-b",
            "--rev", new_rev,
        )
        self.assertEqual(rr.returncode, 0, rr.stdout + rr.stderr)
        fid_a_toml = (mono / "crates" / "fid-a" / "Cargo.toml").read_text(encoding="utf-8")
        self.assertIn(f'fid-b = {{ git = "https://example.invalid/fid-b", rev = "{new_rev}" }}', fid_a_toml)
        self.assertNotIn('path = "../fid-b"', fid_a_toml)

        # Run real `cargo xtask pins --workspace <mono> --repos <REPOS_TOML>` if cargo is available.
        if shutil.which("cargo"):
            xp = subprocess.run(
                ["cargo", "xtask", "pins", "--workspace", str(mono), "--repos", str(REPOS_TOML)],
                cwd=str(ROOT),
                stdout=subprocess.PIPE,
                stderr=subprocess.PIPE,
                text=True,
                encoding="utf-8",
            )
            self.assertEqual(xp.returncode, 0, xp.stdout + xp.stderr)
            self.assertIn("pins: ok", xp.stdout)

    def test_ac3_dirty_source_refused_with_promote_dirty(self) -> None:
        mono = self.clone_bundle()
        (mono / "README.md").write_text("dirty change\n", encoding="utf-8")
        out = mono.parent / "fid-b-dirty"
        r = _run_tool(
            "promote",
            "--source", str(mono),
            "--path", "crates/fid-b",
            "--id", "fid-b",
            "--author", AUTHOR,
            "--date", DATE,
            "--out", str(out),
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("PROMOTE-DIRTY", r.stderr)
        self.assertFalse(out.exists())

    def test_error_paths_missing_path_and_no_dependents(self) -> None:
        mono = self.clone_bundle()
        out = mono.parent / "fid-missing"
        r = _run_tool(
            "promote",
            "--source", str(mono),
            "--path", "crates/does-not-exist",
            "--id", "fid-missing",
            "--author", AUTHOR,
            "--date", DATE,
            "--out", str(out),
        )
        self.assertEqual(r.returncode, 1)
        self.assertIn("PROMOTE-PATH-NOT-FOUND", r.stderr)

        rr = _run_tool(
            "repin",
            "--workspace", str(mono),
            "--crate", "fid-unused",
            "--url", "https://example.invalid/fid-unused",
            "--rev", REV_ZERO,
        )
        self.assertEqual(rr.returncode, 1)
        self.assertIn("REPIN-NO-DEPENDENTS", rr.stderr)


if __name__ == "__main__":
    unittest.main()
