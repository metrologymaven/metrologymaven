"""Tests of tools/scope.py, spec node ECO.COMM.SCOPE (AC-1..AC-8, REQ-001..REQ-006).

Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import hashlib
import shutil
import subprocess
import sys
import tempfile
import time
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "scope.py"
FIX = Path(__file__).resolve().parent / "fixtures" / "scope"

GENERATED_REPORTS = ("INDEX.md", "COVERAGE.md", "DECISIONS.md", "BUILT.md", "EXPERT-QUEUE.md")


def run_scope(*args: str) -> tuple[int, str, str]:
    r = subprocess.run(
        [sys.executable, str(TOOL), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
    )
    return r.returncode, r.stdout, r.stderr


class ScopeFixtureTest(unittest.TestCase):
    def copy_fixture(self, name: str) -> Path:
        tmp = Path(tempfile.mkdtemp(prefix=f"scope-{name}-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        dst = tmp / name
        shutil.copytree(FIX / name, dst)
        return dst

    def test_ac1_valid_tree(self) -> None:
        tree = self.copy_fixture("valid")
        code, out, err = run_scope("--root", str(tree), "check")
        self.assertEqual(code, 0, out + err)
        self.assertIn("0 errors", out)
        self.assertIn("0 warnings", out)
        for name in GENERATED_REPORTS:
            report = tree / "generated" / name
            self.assertTrue(report.is_file(), f"missing generated/{name}")
            self.assertIn("Do not edit", report.read_text(encoding="utf-8"))

        # Also verify `--root` accepted after the subcommand.
        code2, out2, err2 = run_scope("check", "--root", str(tree))
        self.assertEqual(code2, 0, out2 + err2)
        self.assertIn("0 errors", out2)

    def test_ac2_unsatisfied_parent_requirement(self) -> None:
        tree = self.copy_fixture("unsatisfied")
        code, out, err = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 1, out + err)
        self.assertIn("requirement REQ-X-002 is not satisfied by any child", out)

    def test_ac3_dependency_cycle(self) -> None:
        tree = self.copy_fixture("cycle")
        code, out, err = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 1, out + err)
        self.assertIn("cycle:", out)
        cycle_lines = [line for line in out.splitlines() if "cycle:" in line]
        self.assertTrue(cycle_lines, out)
        self.assertIn("A", cycle_lines[0])
        self.assertIn("B", cycle_lines[0])

    def test_ac4_reserved_windows_name(self) -> None:
        tree = self.copy_fixture("reserved")
        code, out, err = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 1, out + err)
        self.assertIn("reserved Windows device name", out)
        self.assertIn("'CON'", out)

    def test_ac5_open_skips_claimed_nodes(self) -> None:
        tree = self.copy_fixture("claimed")
        code, out, err = run_scope("open", "--level", "2", "--root", str(tree))
        self.assertEqual(code, 0, err)
        lines = [line for line in out.splitlines() if line.strip()]
        self.assertEqual(lines, ["spec/a/aa/NODE.md", "spec/a/cc/NODE.md"])

        # Default shallowest open level also finds level 2 and skips claimed.
        code_def, out_def, err_def = run_scope("--root", str(tree), "open")
        self.assertEqual(code_def, 0, err_def)
        self.assertEqual([line for line in out_def.splitlines() if line.strip()], lines)

        # --limit 1 returns only the first unclaimed node in ID order.
        code_lim, out_lim, err_lim = run_scope("open", "--level", "2", "--limit", "1", "--root", str(tree))
        self.assertEqual(code_lim, 0, err_lim)
        self.assertEqual([line for line in out_lim.splitlines() if line.strip()], ["spec/a/aa/NODE.md"])

    def test_ac6_phase_order_warning_only(self) -> None:
        tree = self.copy_fixture("phase")
        code, out, err = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 0, out + err)
        self.assertIn("0 errors, 1 warnings", out)
        warn_lines = [line for line in out.splitlines() if line.startswith("warn  ")]
        self.assertEqual(len(warn_lines), 1, out)
        self.assertIn("phase P0 is earlier than parent A (P1a)", warn_lines[0])

    def test_ac7_lf_and_cross_os_byte_identical_reports(self) -> None:
        tree_lf = self.copy_fixture("valid")
        tree_crlf = self.copy_fixture("valid")
        # Convert all fixture inputs in tree_crlf to CRLF to prove generated output stays pure LF & byte-identical.
        for md in tree_crlf.rglob("*.md"):
            raw = md.read_bytes().replace(b"\r\n", b"\n").replace(b"\n", b"\r\n")
            md.write_bytes(raw)

        code1, out1, err1 = run_scope("check", "--root", str(tree_lf))
        code2, out2, err2 = run_scope("check", "--root", str(tree_crlf))
        self.assertEqual(code1, 0, out1 + err1)
        self.assertEqual(code2, 0, out2 + err2)

        for name in GENERATED_REPORTS:
            b1 = (tree_lf / "generated" / name).read_bytes()
            b2 = (tree_crlf / "generated" / name).read_bytes()
            self.assertEqual(b1, b2, f"generated/{name} differed between LF and CRLF inputs")
            self.assertNotIn(b"\r", b1, f"generated/{name} contains CR bytes")
            self.assertNotIn(b"\\", b1, f"generated/{name} contains backslash paths")
            # Sanity: SHA-256 is non-empty and repeatable.
            self.assertEqual(len(hashlib.sha256(b1).hexdigest()), 64)

    def test_ac8_stdlib_only_import(self) -> None:
        code_str = f"import sys; sys.path.insert(0, {str(ROOT / 'tools')!r}); import scope"
        r = subprocess.run(
            [sys.executable, "-I", "-S", "-c", code_str],
            stdout=subprocess.PIPE,
            stderr=subprocess.PIPE,
            text=True,
            encoding="utf-8",
        )
        self.assertEqual(r.returncode, 0, r.stderr)
        self.assertEqual(r.stderr, "")

    def test_req001_uncovered_and_dropped_seeds(self) -> None:
        tree = self.copy_fixture("valid")
        seeds_file = tree / "seeds" / "vendor.md"
        seeds_file.write_text(
            seeds_file.read_text(encoding="utf-8") + "| VEND.ALN.99 | Uncovered seed |\n",
            encoding="utf-8",
            newline="\n",
        )
        code, out, _ = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 1, out)
        self.assertIn("VEND.ALN.99", out)

    def test_req002_feature_leaf_and_decision_and_leaf_size_errors(self) -> None:
        tree = self.copy_fixture("valid")
        # Remove cli from feature leaf ALPHA.ONE, change BETA.ONE to size L, set BETA.TWO to decision without file.
        a1 = tree / "spec" / "alpha" / "one" / "NODE.md"
        a1.write_text(a1.read_text(encoding="utf-8").replace("cli: [fid alpha one]", "cli: []"), encoding="utf-8", newline="\n")
        b1 = tree / "spec" / "beta" / "one" / "NODE.md"
        b1.write_text(b1.read_text(encoding="utf-8").replace("size: S", "size: L"), encoding="utf-8", newline="\n")
        b2 = tree / "spec" / "beta" / "two" / "NODE.md"
        b2.write_text(
            b2.read_text(encoding="utf-8").replace("status: leaf", "status: decision\ndecision: D-999"),
            encoding="utf-8",
            newline="\n",
        )
        code, out, _ = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 1, out)
        self.assertIn("feature leaf needs 'cli'", out)
        self.assertIn("a leaf must be size S or M", out)
        self.assertIn("status decision needs 'decision: D-NNN' with a file in decisions/", out)

    def test_req003_warnings_never_fail(self) -> None:
        tree = self.copy_fixture("valid")
        b2 = tree / "spec" / "beta" / "two" / "NODE.md"
        b2.write_text(
            b2.read_text(encoding="utf-8").replace(
                "depends_on: [BETA.ONE]",
                "depends_on: [BETA.ONE, NO.SUCH.NODE]\ncovers: [NO.MATCH.*]",
            ),
            encoding="utf-8",
            newline="\n",
        )
        code, out, _ = run_scope("check", "--root", str(tree))
        self.assertEqual(code, 0, out)
        self.assertIn("depends_on 'NO.SUCH.NODE' does not exist yet", out)
        self.assertIn("covers pattern 'NO.MATCH.*' matches no seed row", out)

    def test_stats_subcommand(self) -> None:
        tree = self.copy_fixture("valid")
        code, out, err = run_scope("stats", "--root", str(tree))
        self.assertEqual(code, 0, err)
        self.assertIn("L0: split=1", out)
        self.assertIn("L1: split=2", out)
        self.assertIn("L2: leaf=4", out)
        self.assertIn("built: 0 leaves", out)

    def test_req006_real_repo_performance_under_10s(self) -> None:
        # Copy generated reports only into a temp copy? Run validate directly via import to avoid dirtying repo generated/.
        sys.path.insert(0, str(ROOT / "tools"))
        import scope  # noqa: E402

        t0 = time.monotonic()
        scope.set_root(ROOT)
        rep = scope.Report()
        seeds, oos = scope.load_seeds(rep)
        nodes = scope.load_nodes(rep)
        scope.validate(nodes, seeds, oos, rep)
        elapsed = time.monotonic() - t0
        self.assertEqual(rep.errors, [], rep.errors)
        self.assertGreaterEqual(len(nodes), 2000)
        self.assertLess(elapsed, 10.0, f"validation of {len(nodes)} nodes took {elapsed:.2f}s (> 10s)")

    def test_d053_commercial_repo_guardrails(self) -> None:
        # 1. open skips nodes whose ancestor sets commercial_repo, unless --include-commercial is set.
        tree = self.copy_fixture("claimed")
        parent_a = tree / "spec" / "a" / "NODE.md"
        parent_a.write_text(
            parent_a.read_text(encoding="utf-8").replace(
                "status: split", "status: split\ncommercial_repo: metrologymaven-enterprise"
            ),
            encoding="utf-8",
            newline="\n",
        )
        code, out, err = run_scope("open", "--level", "2", "--root", str(tree))
        self.assertEqual(code, 0, err)
        self.assertEqual([line for line in out.splitlines() if line.strip()], [])

        code_inc, out_inc, err_inc = run_scope("open", "--level", "2", "--include-commercial", "--root", str(tree))
        self.assertEqual(code_inc, 0, err_inc)
        self.assertEqual(
            [line for line in out_inc.splitlines() if line.strip()],
            ["spec/a/aa/NODE.md", "spec/a/cc/NODE.md"],
        )

        # 2. check rejects built_in on any descendant of a commercial_repo node, and rejects bad slugs.
        vtree = self.copy_fixture("valid")
        alpha = vtree / "spec" / "alpha" / "NODE.md"
        alpha.write_text(
            alpha.read_text(encoding="utf-8").replace(
                "status: split", "status: split\ncommercial_repo: metrologymaven-enterprise"
            ),
            encoding="utf-8",
            newline="\n",
        )
        code_ok, out_ok, err_ok = run_scope("check", "--root", str(vtree))
        self.assertEqual(code_ok, 0, out_ok + err_ok)
        self.assertIn("(commercial: metrologymaven-enterprise)", (vtree / "generated" / "INDEX.md").read_text(encoding="utf-8"))
        self.assertIn("commercial leaves live in private repositories under D-053", (vtree / "generated" / "BUILT.md").read_text(encoding="utf-8"))

        # Setting built_in on a commercial descendant must fail with an error.
        (vtree / "crates" / "mm-fake").mkdir(parents=True, exist_ok=True)
        (vtree / "crates" / "mm-fake" / "Cargo.toml").write_text("[package]\nname = 'mm-fake'\n", encoding="utf-8")
        a1 = vtree / "spec" / "alpha" / "one" / "NODE.md"
        a1.write_text(
            a1.read_text(encoding="utf-8").replace("status: leaf", "status: leaf\nbuilt_in: [mm-fake]"),
            encoding="utf-8",
            newline="\n",
        )
        code_bad, out_bad, _ = run_scope("check", "--root", str(vtree))
        self.assertEqual(code_bad, 1, out_bad)
        self.assertIn("built_in is forbidden in the public repository", out_bad)

        # Invalid commercial_repo slug must fail with an error.
        alpha.write_text(
            alpha.read_text(encoding="utf-8").replace("commercial_repo: metrologymaven-enterprise", "commercial_repo: BAD/SLUG"),
            encoding="utf-8",
            newline="\n",
        )
        code_slug, out_slug, _ = run_scope("check", "--root", str(vtree))
        self.assertEqual(code_slug, 1, out_slug)
        self.assertIn("commercial_repo must be a repository slug", out_slug)


if __name__ == "__main__":
    unittest.main()
