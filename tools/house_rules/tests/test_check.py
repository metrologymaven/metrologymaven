"""Acceptance tests of spec node ECO.RULES.CHECK (AC-1 .. AC-7) for tools/house_rules/check.py.

Fixture repositories live in tools/tests/fixtures/check/ and are written by its make_fixtures.py,
which renders blocks with its own hashlib code (it never imports house_rules). Every expected
output line below is hand-labelled from the spec's ACs and error list, not produced by check.py.
Run from the repository root:  python -m unittest discover -s tools/house_rules/tests -v
"""

import hashlib
import importlib.util
import os
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
sys.path.insert(0, str(ROOT / "tools"))

from house_rules import render  # noqa: E402
from house_rules.check import MAX_INPUT_BYTES, check  # noqa: E402

FIX = ROOT / "tools" / "tests" / "fixtures" / "check"
MANIFEST = FIX / "manifest.json"
REV = "1111111111111111111111111111111111111111"
OTHER = "2222222222222222222222222222222222222222"
CORE_BODY_BYTES = 127  # hand-counted length of CORE in make_fixtures.py


def run_cli(*args):
    """Run ``python -m house_rules`` as CI would; return (exit code, stdout lines, stderr)."""
    env = dict(os.environ)
    env["PYTHONDONTWRITEBYTECODE"] = "1"
    r = subprocess.run(
        [sys.executable, "-m", "house_rules", *args],
        cwd=ROOT / "tools",
        env=env,
        capture_output=True,
        text=True,
        timeout=60,
    )
    return r.returncode, r.stdout.splitlines(), r.stderr


def run_check(fixture, repo="fix-spec", rev=REV, manifest=MANIFEST, extra=()):
    target = fixture if isinstance(fixture, Path) else FIX / fixture
    return run_cli(
        "check", "--target", str(target), "--repo-id", repo, "--manifest", str(manifest), "--current-rev", rev, *extra
    )


def tree_digest():
    h = {}
    for p in sorted(FIX.rglob("*")):
        if p.is_file() and "__pycache__" not in p.parts:
            h[p.relative_to(FIX).as_posix()] = hashlib.sha256(p.read_bytes()).hexdigest()
    return h


def copy_fixture(name, tmp):
    dst = Path(tmp) / name
    shutil.copytree(FIX / name, dst)
    return dst


class FixturesAreCurrent(unittest.TestCase):
    def test_make_fixtures_check(self):
        spec = importlib.util.spec_from_file_location("check_make_fixtures", FIX / "make_fixtures.py")
        mod = importlib.util.module_from_spec(spec)
        spec.loader.exec_module(mod)
        for rel, data in mod.files().items():
            self.assertEqual((FIX / rel).read_bytes(), data, rel)

    def test_big_is_11000_bytes(self):
        self.assertEqual(len((FIX / "big" / "AGENTS.md").read_bytes()), 11000)

    def test_fixture_blocks_agree_with_block_render(self):
        # The independent rendering and ECO.RULES.BLOCK's render() give the same block text.
        text = (FIX / "ok" / "AGENTS.md").read_text(encoding="utf-8")
        start = text.index("<!-- house-rules:begin")
        end = text.index("<!-- house-rules:end block=core -->\n") + len("<!-- house-rules:end block=core -->\n")
        lines = text[start:end].split("\n")
        body = "\n".join(lines[1:-2])
        self.assertEqual(render("core", "metrologymaven", REV, body), text[start:end])


class Acceptance(unittest.TestCase):
    """AC-1 .. AC-7, through the command line."""

    def test_ac1_ok(self):
        self.assertEqual(run_check("ok"), (0, [], ""))

    def test_ac2_edited(self):
        self.assertEqual(run_check("edited"), (1, ["RULES-EDITED AGENTS.md core"], ""))

    def test_ac3_missing(self):
        self.assertEqual(run_check("missing", repo="fix-code"), (1, ["RULES-MISSING rust"], ""))

    def test_ac4_stale(self):
        self.assertEqual(run_check("ok", rev=OTHER), (0, ["warn RULES-STALE core"], ""))

    def test_ac5_big(self):
        self.assertEqual(run_check("big"), (0, ["warn RULES-SIZE AGENTS.md 11000 > 10240"], ""))

    def test_ac6_claude_extra(self):
        self.assertEqual(run_check("claude-extra"), (1, ["RULES-CLAUDE-MD CLAUDE.md not-exact"], ""))

    def test_ac7_no_file_changes(self):
        before = tree_digest()
        run_check("ok")
        run_check("edited")
        run_check("missing", repo="fix-code")
        run_check("ok", rev=OTHER)
        run_check("big")
        run_check("claude-extra")
        self.assertEqual(tree_digest(), before)

    def test_code_repository_with_both_blocks(self):
        self.assertEqual(run_check("code-ok", repo="fix-code"), (0, [], ""))

    def test_output_is_sorted_and_combined(self):
        code, out, err = run_check("edited", repo="fix-code", rev=OTHER)
        self.assertEqual(
            (code, out, err),
            (1, ["RULES-EDITED AGENTS.md core", "RULES-MISSING rust", "warn RULES-STALE core"], ""),
        )

    def test_under_one_second(self):
        t = time.perf_counter()
        check(FIX / "big", "fix-spec", MANIFEST, REV)
        self.assertLess(time.perf_counter() - t, 1.0)


class Rules(unittest.TestCase):
    """Further error paths, in process and hand-labelled."""

    def setUp(self):
        self.tmp = tempfile.mkdtemp()
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def manifest(self, text):
        p = Path(self.tmp) / "m.json"
        p.write_text(text, encoding="utf-8")
        return p

    def test_core_over_budget(self):
        m = self.manifest(
            '{"budgets": {"core_bytes": 100, "agents_md_bytes": 10240},'
            ' "block": [{"id": "core", "file": "core.md", "repos": ["fix-spec"]}]}'
        )
        self.assertEqual(check(FIX / "ok", "fix-spec", m, REV), [f"warn RULES-SIZE core {CORE_BODY_BYTES} > 100"])

    def test_crlf_sizes_are_lf_normalised(self):
        d = copy_fixture("big", self.tmp)
        p = d / "AGENTS.md"
        p.write_bytes(p.read_bytes().replace(b"\n", b"\r\n"))
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["warn RULES-SIZE AGENTS.md 11000 > 10240"])

    def test_claude_md_missing_and_newline_forms(self):
        d = copy_fixture("ok", self.tmp)
        for content, want in [
            (b"@AGENTS.md", []),
            (b"@AGENTS.md\r\n", []),
            (b"@AGENTS.md\n\n", ["RULES-CLAUDE-MD CLAUDE.md not-exact"]),
            (b" @AGENTS.md\n", ["RULES-CLAUDE-MD CLAUDE.md not-exact"]),
            (b"\xef\xbb\xbf@AGENTS.md\n", ["RULES-CLAUDE-MD CLAUDE.md not-exact"]),
        ]:
            (d / "CLAUDE.md").write_bytes(content)
            self.assertEqual(check(d, "fix-spec", MANIFEST, REV), want, content)
        (d / "CLAUDE.md").unlink()
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["RULES-CLAUDE-MD CLAUDE.md missing"])

    def test_agents_md_missing(self):
        d = copy_fixture("code-ok", self.tmp)
        (d / "AGENTS.md").unlink()
        self.assertEqual(check(d, "fix-code", MANIFEST, REV), ["RULES-MISSING core", "RULES-MISSING rust"])

    def test_parse_errors(self):
        d = copy_fixture("ok", self.tmp)
        p = d / "AGENTS.md"
        p.write_bytes(b"# AGENTS.md\n\n<!-- house-rules:begin block=core -->\n")
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["RULES-PARSE AGENTS.md bad-marker at line 3"])
        p.write_bytes(b"# AGENTS.md\n\xff\n")
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["RULES-PARSE AGENTS.md not UTF-8"])
        ok = (FIX / "ok" / "AGENTS.md").read_bytes()
        start = ok.index(b"<!-- house-rules:begin")
        end = ok.index(b"## Repo-local")
        p.write_bytes(ok[:end] + ok[start:end])
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["RULES-PARSE AGENTS.md duplicate block core at line 11"])

    def test_input_size_bound(self):
        d = copy_fixture("ok", self.tmp)
        with open(d / "AGENTS.md", "ab") as f:
            f.write(b"x" * MAX_INPUT_BYTES)
        [line] = check(d, "fix-spec", MANIFEST, REV)
        self.assertTrue(line.startswith("RULES-PARSE AGENTS.md too large"), line)

    def test_symlinks_refused(self):
        d = copy_fixture("ok", self.tmp)
        real = Path(self.tmp) / "real-agents.md"
        shutil.move(str(d / "AGENTS.md"), str(real))
        try:
            os.symlink(real, d / "AGENTS.md")
        except (OSError, NotImplementedError):
            self.skipTest("cannot create symlinks here")
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["RULES-PARSE AGENTS.md symlink refused"])
        shutil.move(str(real), str(d / "x"))
        (d / "AGENTS.md").unlink()
        shutil.move(str(d / "x"), str(d / "AGENTS.md"))
        (d / "CLAUDE.md").unlink()
        os.symlink(d / "AGENTS.md", d / "CLAUDE.md")
        self.assertEqual(check(d, "fix-spec", MANIFEST, REV), ["RULES-CLAUDE-MD CLAUDE.md symlink"])
        link = Path(self.tmp) / "link"
        os.symlink(FIX / "ok", link, target_is_directory=True)
        self.assertEqual(check(link, "fix-spec", MANIFEST, REV), ["RULES-USAGE --target symlink refused"])
        mlink = Path(self.tmp) / "m.json"
        os.symlink(MANIFEST, mlink)
        self.assertEqual(check(FIX / "ok", "fix-spec", mlink, REV), ["RULES-PARSE manifest symlink refused"])

    def test_manifest_errors(self):
        cases = [
            ("{", "RULES-PARSE manifest syntax"),
            ("[]", "RULES-PARSE manifest not a table"),
            ('{"block": []}', "RULES-PARSE manifest budgets missing"),
            ('{"budgets": {"core_bytes": true, "agents_md_bytes": 1}, "block": []}', "RULES-PARSE manifest budgets.core_bytes"),
            ('{"budgets": {"core_bytes": 1, "agents_md_bytes": 1}, "block": [{"id": "X"}]}', "RULES-PARSE manifest block with invalid id"),
        ]
        for text, prefix in cases:
            [line] = check(FIX / "ok", "fix-spec", self.manifest(text), REV)
            self.assertTrue(line.startswith(prefix), (text, line))
        [line] = check(FIX / "ok", "fix-spec", Path(self.tmp) / "absent.json", REV)
        self.assertEqual(line, "RULES-PARSE manifest missing")

    def test_usage_errors_exit_1_without_traceback(self):
        for args, want in [
            (["check"], "RULES-USAGE the following arguments are required"),
            (["check", "--target", str(FIX / "ok"), "--repo-id", "fix-spec", "--manifest", str(MANIFEST), "--current-rev", "abc"],
             "RULES-USAGE --current-rev must be 40 lowercase hex digits"),
            (["check", "--target", str(FIX / "ok"), "--repo-id", "nobody", "--manifest", str(MANIFEST), "--current-rev", REV],
             "RULES-USAGE repository nobody has no blocks in the manifest"),
            (["check", "--target", str(FIX / "nope"), "--repo-id", "fix-spec", "--manifest", str(MANIFEST), "--current-rev", REV],
             "RULES-USAGE --target is not a directory"),
            (["frobnicate"], "RULES-USAGE python -m house_rules"),
        ]:
            code, out, err = run_cli(*args)
            self.assertEqual(code, 1, args)
            self.assertEqual(len(out), 1, out)
            self.assertTrue(out[0].startswith(want), out)
            self.assertEqual(err, "")

    def test_real_manifest_loads(self):
        try:
            import tomllib  # noqa: F401
        except ImportError:
            self.skipTest("needs tomllib")
        # The project's own repository checked against the canonical manifest never crashes.
        lines = check(ROOT, "metrologymaven", ROOT / "house-rules" / "manifest.toml", REV)
        self.assertTrue(all(line.split(" ")[0] in ("RULES-MISSING", "RULES-CLAUDE-MD", "RULES-EDITED", "warn") for line in lines), lines)


@unittest.skipIf(shutil.which("git") is None, "needs git")
class SourceRepo(unittest.TestCase):
    """REQ-003 with --source-repo: stale only for ancestors, RULES-UNKNOWN-REV otherwise."""

    @classmethod
    def setUpClass(cls):
        cls.tmp = tempfile.mkdtemp()
        cls.src = Path(cls.tmp) / "src"
        cls.src.mkdir()
        env = dict(os.environ)
        env.update({"GIT_AUTHOR_DATE": "2026-01-01T00:00:00Z", "GIT_COMMITTER_DATE": "2026-01-01T00:00:00Z"})

        def git(*a):
            return subprocess.run(
                ["git", "-c", "user.name=t", "-c", "user.email=t@example.invalid", "-c", "commit.gpgsign=false",
                 "-C", str(cls.src), *a],
                check=True, capture_output=True, text=True, env=env,
            ).stdout.strip()

        git("init", "-q")
        git("commit", "-q", "--allow-empty", "-m", "a")
        cls.rev_a = git("rev-parse", "HEAD")
        git("commit", "-q", "--allow-empty", "-m", "b")
        cls.rev_b = git("rev-parse", "HEAD")

    @classmethod
    def tearDownClass(cls):
        shutil.rmtree(cls.tmp, True)

    def target_at(self, rev):
        d = Path(tempfile.mkdtemp(dir=self.tmp))
        (d / "AGENTS.md").write_text("# AGENTS.md\n\n" + render("core", "metrologymaven", rev, "rules"), encoding="utf-8")
        (d / "CLAUDE.md").write_bytes(b"@AGENTS.md\n")
        return d

    def test_ancestor_is_stale(self):
        d = self.target_at(self.rev_a)
        self.assertEqual(check(d, "fix-spec", MANIFEST, self.rev_b, self.src), ["warn RULES-STALE core"])

    def test_current_is_ok(self):
        d = self.target_at(self.rev_b)
        self.assertEqual(check(d, "fix-spec", MANIFEST, self.rev_b, self.src), [])

    def test_unknown_rev_fails(self):
        d = self.target_at(OTHER)
        self.assertEqual(check(d, "fix-spec", MANIFEST, self.rev_b, self.src), [f"RULES-UNKNOWN-REV core {OTHER}"])

    def test_descendant_is_not_in_history(self):
        d = self.target_at(self.rev_b)
        self.assertEqual(check(d, "fix-spec", MANIFEST, self.rev_a, self.src), [f"RULES-UNKNOWN-REV core {self.rev_b}"])

    def test_current_rev_not_in_source(self):
        d = self.target_at(self.rev_a)
        self.assertEqual(
            check(d, "fix-spec", MANIFEST, OTHER, self.src), ["RULES-USAGE --current-rev is not a commit in --source-repo"]
        )

    def test_cli_exit_code(self):
        d = self.target_at(OTHER)
        code, out, err = run_check(d, rev=self.rev_b, extra=("--source-repo", str(self.src)))
        self.assertEqual((code, out, err), (1, [f"RULES-UNKNOWN-REV core {OTHER}"], ""))


if __name__ == "__main__":
    unittest.main()
