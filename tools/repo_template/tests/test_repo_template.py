"""Acceptance tests of spec node ECO.REPO.TPL.FILES (AC-1 .. AC-5) for tools/repo_template/.

Expected values are hand-labelled from the spec (fixtures under fixtures/repo/tpl/). AC-4's
`cargo clippy` half runs as a CI step in .github/workflows/rust.yml; here we check its file half.
Run from the repository root:  python -m unittest discover -s tools/repo_template/tests -v
"""

import os
import re
import shutil
import subprocess
import sys
import tempfile
import time
import tomllib
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools"
sys.path.insert(0, str(TOOLS))

from house_rules import find_blocks  # noqa: E402
from repo_template import new as tpl_new  # noqa: E402

FIX = ROOT / "fixtures" / "repo" / "tpl"
RULES = FIX / "rules"
TEMPLATES = ROOT / "templates" / "repo"
REV_A = "a" * 40
REV_B = "b" * 40
COMMON = {
    "LICENSE-MIT", "LICENSE-APACHE", "ATTRIBUTION.md", "AGENTS.md", "CLAUDE.md", "CONTRIBUTING.md",
    ".gitattributes", ".github/workflows/ci.yml",
}


def run(*args, rules=RULES, rules_rev=REV_A, ci_rev=REV_B, repo_id="fid-demo", variant="code", year="2026", out):
    cmd = [sys.executable, "-m", "repo_template", "new", "--id", repo_id, "--variant", variant,
           "--rules-source", str(rules), "--rules-rev", rules_rev, "--ci-rev", ci_rev, "--out", str(out), *args]
    if year is not None:
        cmd += ["--year", year]
    env = {**os.environ, "PYTHONPATH": str(TOOLS)}
    return subprocess.run(cmd, capture_output=True, text=True, env=env, cwd=ROOT, timeout=60)


def tree(root):
    """{relative POSIX path: bytes} of every file under root."""
    return {p.relative_to(root).as_posix(): p.read_bytes() for p in sorted(Path(root).rglob("*")) if p.is_file()}


class Base(unittest.TestCase):
    def setUp(self):
        self._tmp = tempfile.TemporaryDirectory()
        self.tmp = Path(self._tmp.name)

    def tearDown(self):
        self._tmp.cleanup()

    def ok(self, **kw):
        out = kw.pop("out", self.tmp / "out")
        r = run(out=out, **kw)
        self.assertEqual(r.returncode, 0, r.stderr)
        return out


class AC1CodeVariant(Base):
    def test_ac1_claude_agents_and_ci_pin(self):
        t0 = time.monotonic()
        out = self.ok()
        self.assertLess(time.monotonic() - t0, 5.0)  # performance budget
        # REQ-001: CLAUDE.md is exactly "@AGENTS.md\n".
        self.assertEqual((out / "CLAUDE.md").read_bytes(), b"@AGENTS.md\n")
        # REQ-002: a valid core block (and rust, also assigned) at the rules revision.
        agents = (out / "AGENTS.md").read_text(encoding="utf-8")
        blocks = {b.id: b for b in find_blocks(agents)}
        self.assertEqual(sorted(blocks), ["core", "rust"])  # python belongs to another repository
        core = blocks["core"]
        self.assertEqual(core.source_rev, REV_A)
        self.assertTrue(core.is_intact())
        self.assertEqual(core.body, "## House rules (fixture core)\n\n- Check before you claim.\n- Never crash: return errors instead.")
        # Blocks sit right after the first heading; the skeleton text after them is kept.
        self.assertTrue(agents.startswith("# AGENTS.md: rules for coding agents in fid-demo\n\n<!-- house-rules:begin block=core "))
        self.assertIn("## Repository-specific notes\n", agents)
        # REQ-004: the caller workflow references the shared workflows by the CI revision only.
        wf = (out / ".github/workflows/ci.yml").read_text(encoding="utf-8")
        uses = re.findall(r"^\s*uses:\s*(\S+)", wf, re.M)
        self.assertTrue(uses)
        for u in uses:
            self.assertTrue(u.endswith("@" + REV_B), u)
        # Placeholders filled; nothing staged is left behind.
        self.assertIn("Copyright (c) 2026 The fid-demo contributors", (out / "LICENSE-MIT").read_text(encoding="utf-8"))
        self.assertFalse((out / ".repo-template.partial").exists())
        for name, data in tree(out).items():
            self.assertNotRegex(data.decode("utf-8"), r"\{\{[A-Za-z_][A-Za-z0-9_]*\}\}", name)

    def test_deterministic(self):
        a = self.ok(out=self.tmp / "a")
        b = self.ok(out=self.tmp / "b")
        self.assertEqual(tree(a), tree(b))

    def test_empty_existing_out_is_used(self):
        out = self.tmp / "empty"
        out.mkdir()
        self.ok(out=out)
        self.assertTrue((out / "Cargo.toml").is_file())

    @unittest.skipIf(shutil.which("git") is None, "git not installed")
    def test_year_from_rules_revision_commit_date(self):
        src = self.tmp / "rules-git"
        shutil.copytree(RULES, src)
        env = {**os.environ, "GIT_AUTHOR_DATE": "2031-06-01T12:00:00+00:00",
               "GIT_COMMITTER_DATE": "2031-06-01T12:00:00+00:00"}
        g = ["git", "-C", str(src), "-c", "user.name=fixture", "-c", "user.email=fixture@example.invalid",
             "-c", "commit.gpgsign=false", "-c", "core.autocrlf=false"]
        subprocess.run(g + ["init", "-q"], check=True, env=env)
        subprocess.run(g + ["add", "-A"], check=True, env=env)
        subprocess.run(g + ["commit", "-q", "-m", "fixture"], check=True, env=env)
        rev = subprocess.run(g + ["rev-parse", "HEAD"], check=True, env=env, capture_output=True, text=True).stdout.strip()
        if len(rev) != 40:
            self.skipTest("repository uses a non-SHA-1 object format")
        out = self.ok(rules=src, rules_rev=rev, year=None)
        self.assertIn("Copyright (c) 2031 The fid-demo contributors",
                      (out / "LICENSE-MIT").read_text(encoding="utf-8"))
        core = {b.id: b for b in find_blocks((out / "AGENTS.md").read_text(encoding="utf-8"))}["core"]
        self.assertEqual(core.source_rev, rev)


class AC2Placeholder(Base):
    def test_ac2_unknown_placeholder_fails_and_leaves_nothing(self):
        out = self.tmp / "out"
        r = run("--templates", str(FIX / "bad-placeholder"), out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("TPL-PLACEHOLDER", r.stderr)
        self.assertIn("NOTICE.md", r.stderr)
        self.assertIn("{{owner}}", r.stderr)
        self.assertFalse(out.exists())

    def test_sync_failure_fails_and_leaves_nothing(self):
        out = self.tmp / "out"
        r = run(repo_id="fid-unassigned", out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("TPL-SYNC-FAILED", r.stderr)
        self.assertFalse(out.exists())

    def test_year_unknown_without_flag_fails(self):
        out = self.tmp / "out"
        r = run(year=None, out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("TPL-YEAR", r.stderr)
        self.assertIn("--year", r.stderr)
        self.assertFalse(out.exists())

    def test_placeholder_in_file_name_fails(self):
        tpl = self.tmp / "tpl"
        shutil.copytree(FIX / "bad-placeholder", tpl)
        (tpl / "common" / "NOTICE.md").unlink()
        (tpl / "common" / "{{owner}}.md").write_text("clean contents.", encoding="utf-8")
        out = self.tmp / "out"
        r = run("--templates", str(tpl), out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("TPL-PLACEHOLDER", r.stderr)
        self.assertIn("{{owner}}.md: {{owner}}", r.stderr)
        self.assertFalse(out.exists())

    def test_manifest_repos_must_be_a_list(self):
        rules = self.tmp / "rules"
        shutil.copytree(RULES, rules)
        toml = (rules / "manifest.toml").read_text(encoding="utf-8")
        (rules / "manifest.toml").write_text(toml.replace('repos = ["fid-demo"]', 'repos = "fid-demo"'), encoding="utf-8")
        out = self.tmp / "out"
        r = run(rules=rules, out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("TPL-SYNC-FAILED", r.stderr)
        self.assertIn("repos must be a list", r.stderr)
        self.assertFalse(out.exists())

    def test_failed_move_leaves_existing_empty_out_empty(self):
        out = self.tmp / "empty"
        out.mkdir()
        real_replace = os.replace
        calls = []

        def flaky_replace(src, dst):
            calls.append(src)
            if len(calls) == 3:
                raise OSError("simulated failure while moving into place")
            return real_replace(src, dst)

        tpl_new.os.replace = flaky_replace
        try:
            with self.assertRaises(OSError):
                tpl_new.instantiate("fid-demo", "code", RULES, REV_A, REV_B, out, year="2026")
        finally:
            tpl_new.os.replace = real_replace
        self.assertEqual(len(calls), 3)
        self.assertEqual(list(out.iterdir()), [])

    def test_bad_revision_refused(self):
        out = self.tmp / "out"
        r = run(ci_rev="main", out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("--ci-rev", r.stderr)
        self.assertFalse(out.exists())


class AC3NonEmptyOut(Base):
    def test_ac3_non_empty_out_refused_unchanged(self):
        out = self.tmp / "nonempty-out"
        shutil.copytree(FIX / "nonempty-out", out)
        before = tree(out)
        r = run(out=out)
        self.assertEqual(r.returncode, 1)
        self.assertIn("TPL-OUT-NOT-EMPTY", r.stderr)
        self.assertEqual(tree(out), before)
        self.assertEqual(sorted(p.name for p in self.tmp.iterdir()), ["nonempty-out"])

    def test_out_is_a_file_refused(self):
        out = self.tmp / "file"
        out.write_bytes(b"x")
        r = run(out=out)
        self.assertEqual(r.returncode, 1)
        self.assertEqual(out.read_bytes(), b"x")


class AC4CodeVariantFiles(Base):
    def test_ac4_layers_and_lints(self):
        out = self.ok()
        with open(out / "layers.toml", "rb") as f:
            layers = tomllib.load(f)
        with open(out / "crates/starter/Cargo.toml", "rb") as f:
            starter = tomllib.load(f)
        name = starter["package"]["name"]
        self.assertIn(layers["crates"][name], layers["order"])
        self.assertEqual(starter["lints"], {"workspace": True})
        with open(out / "Cargo.toml", "rb") as f:
            ws = tomllib.load(f)
        lints = ws["workspace"]["lints"]
        self.assertIn(lints["rust"]["unsafe_code"], ("deny", "forbid"))
        for lint in ("unwrap_used", "expect_used", "panic", "todo", "unimplemented", "unreachable"):
            self.assertIn(lints["clippy"][lint], ("deny", "forbid"), lint)
        self.assertTrue((out / "rust-toolchain.toml").is_file())


class AC5SpecVariant(Base):
    def test_ac5_spec_has_no_cargo_and_same_common_files(self):
        code = tree(self.ok(out=self.tmp / "code"))
        spec = tree(self.ok(out=self.tmp / "spec", variant="spec"))
        self.assertNotIn("Cargo.toml", spec)
        self.assertFalse(any(p.endswith("Cargo.toml") for p in spec))
        self.assertIn("spec/NODE.md", spec)
        self.assertTrue(COMMON <= set(code))
        for name in COMMON:
            self.assertEqual(spec.get(name), code[name], name)


class TemplateHygiene(unittest.TestCase):
    def test_no_emails_or_secret_like_strings(self):
        email = re.compile(r"[A-Za-z0-9._%+-]+@[A-Za-z0-9-]+\.[A-Za-z.]{2,}")
        secret = re.compile(r"(?i)(api[_-]?key|secret|password|token)\s*[:=]")
        for name, data in tree(TEMPLATES).items():
            text = data.decode("utf-8")
            self.assertIsNone(email.search(text), name)
            self.assertIsNone(secret.search(text), name)

    def test_claude_md_template(self):
        self.assertEqual((TEMPLATES / "common" / "CLAUDE.md").read_bytes(), b"@AGENTS.md\n")


if __name__ == "__main__":
    unittest.main()
