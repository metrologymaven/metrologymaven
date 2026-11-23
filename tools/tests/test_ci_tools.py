"""Unit tests for tools/ci_scope.py, tools/run_fixture_checks.py, and tools/run_wasm32_tests.py."""

from __future__ import annotations

import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import ci_scope
import run_fixture_checks
import run_wasm32_tests


class TestCiScope(unittest.TestCase):
    def setUp(self) -> None:
        self.crate_dirs = {
            "fid-types": Path("crates/fid-types"),
            "fid-num": Path("crates/fid-num"),
            "fid-kernel-wasm": Path("crates/fid-kernel-wasm"),
            "fid-wasm-check": Path("crates/fid-wasm-check"),
            "fid-xart": Path("crates/fid-xart"),
            "fid-report": Path("crates/fid-report"),
            "fid-io-dmis": Path("crates/fid-io-dmis"),
            "fid-overlay": Path("crates/fid-overlay"),
            "fid-geo-nurbs": Path("crates/fid-geo-nurbs"),
            "fid-xtask": Path("crates/fid-xtask"),
        }
        self.fwd_deps = {
            "fid-num": {"fid-types"},
            "fid-kernel-wasm": {"fid-num", "fid-types"},
            "fid-wasm-check": {"fid-types"},
            "fid-xart": {"fid-wasm-check", "fid-types"},
            "fid-report": {"fid-types"},
            "fid-io-dmis": {"fid-types"},
            "fid-overlay": {"fid-types"},
            "fid-geo-nurbs": {"fid-num"},
            "fid-xtask": {"fid-types"},
        }
        self.rev_deps = {
            "fid-types": {
                "fid-num",
                "fid-kernel-wasm",
                "fid-wasm-check",
                "fid-xart",
                "fid-report",
                "fid-io-dmis",
                "fid-overlay",
                "fid-xtask",
            },
            "fid-num": {"fid-kernel-wasm", "fid-geo-nurbs"},
            "fid-wasm-check": {"fid-xart"},
        }
        self.ext_deps = {
            "fid-kernel-wasm": {"libm"},
            "fid-report": {"serde"},
            "fid-overlay": {"wgpu"},
        }
        self.wasm32 = {"fid-num", "fid-geo-nurbs", "fid-types"}

    def _run(self, event: str, files: list[str] | None, ext: set[str] | None = None) -> dict[str, str]:
        return ci_scope.compute_scope(
            event=event,
            files=files,
            crate_dirs=self.crate_dirs,
            rev_deps=self.rev_deps,
            fwd_deps=self.fwd_deps,
            ext_deps=self.ext_deps,
            wasm32_crates=self.wasm32,
            changed_external_deps=ext or set(),
            root=ROOT,
        )

    def test_spec_only_pr_skips_rust_and_kernel_workflows(self) -> None:
        out = self._run("pull_request", ["spec/viz/NODE.md", "decisions/D-052.md", "README.md"])
        self.assertEqual(out["rust"], "false")
        self.assertEqual(out["mode"], "skip")
        self.assertEqual(out["run_wasm32"], "false")
        self.assertEqual(out["run_kernel_repro"], "false")
        self.assertEqual(out["run_xart"], "false")
        self.assertEqual(out["run_deny"], "false")

    def test_leaf_crate_pr_uses_affected_mode_and_skips_unrelated_heavy_jobs(self) -> None:
        out = self._run(
            "pull_request",
            ["crates/fid-overlay/src/lib.rs", "spec/viz/overlay/NODE.md", "ci/fixture-checks.txt"],
        )
        self.assertEqual(out["rust"], "true")
        self.assertEqual(out["mode"], "affected")
        self.assertEqual(out["crates"], "-p fid-overlay")
        self.assertEqual(out["wasm_crates"], "NONE")
        self.assertEqual(out["run_wasm32"], "false")
        self.assertEqual(out["run_kernel_repro"], "false")
        self.assertEqual(out["run_xart"], "false")
        self.assertEqual(out["run_tmpl_fuzz"], "false")
        self.assertEqual(out["run_nurbs_oracle"], "false")
        self.assertEqual(out["run_deny"], "false")

    def test_workspace_only_cargo_lock_does_not_trigger_xart_or_full_mode(self) -> None:
        out = self._run(
            "pull_request",
            ["crates/fid-overlay/Cargo.toml", "crates/fid-overlay/src/lib.rs", "Cargo.lock"],
            ext=set(),
        )
        self.assertEqual(out["mode"], "affected")
        self.assertEqual(out["crates"], "-p fid-overlay")
        self.assertEqual(out["run_deny"], "true")
        self.assertEqual(out["run_xart"], "false")
        self.assertEqual(out["run_kernel_repro"], "false")

    def test_kernel_crate_pr_triggers_kernel_repro_xart_and_wasm32(self) -> None:
        out = self._run("pull_request", ["crates/fid-num/src/lib.rs"])
        self.assertEqual(out["mode"], "affected")
        self.assertIn("-p fid-num", out["crates"])
        self.assertIn("-p fid-kernel-wasm", out["crates"])
        self.assertIn("-p fid-geo-nurbs", out["crates"])
        self.assertEqual(out["run_kernel_repro"], "true")
        self.assertEqual(out["run_xart"], "true")
        self.assertEqual(out["run_wasm32"], "true")
        self.assertEqual(out["wasm_crates"], "fid-geo-nurbs,fid-num")
        self.assertEqual(out["run_nurbs_oracle"], "true")

    def test_wide_core_crate_promotes_to_full_mode(self) -> None:
        out = self._run("pull_request", ["crates/fid-types/src/lib.rs"])
        self.assertEqual(out["mode"], "full")
        self.assertEqual(out["crates"], "")
        self.assertEqual(out["wasm_crates"], "ALL")

    def test_push_and_merge_group_always_run_full(self) -> None:
        for ev in ("push", "merge_group", "workflow_dispatch"):
            out = self._run(ev, ["spec/NODE.md"])
            self.assertEqual(out["mode"], "full", ev)
            self.assertEqual(out["run_kernel_repro"], "true", ev)
            self.assertEqual(out["run_xart"], "true", ev)

    def test_lockfile_external_diff_ignores_workspace_only_entries(self) -> None:
        base = """
[[package]]
name = "fid-a"
version = "0.0.1"

[[package]]
name = "serde"
version = "1.0.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "aaa"
"""
        head_ws_only = base + '\n[[package]]\nname = "fid-b"\nversion = "0.0.1"\n'
        self.assertEqual(ci_scope.external_deps_diff(base, head_ws_only), set())
        head_ext_bump = base.replace('version = "1.0.0"', 'version = "1.0.1"')
        self.assertEqual(ci_scope.external_deps_diff(base, head_ext_bump), {"serde"})


class TestFixtureAndWasmRunners(unittest.TestCase):
    def test_fixture_list_sort_and_uniqueness_validation(self) -> None:
        with tempfile.TemporaryDirectory() as td:
            p = Path(td) / "checks.txt"
            p.write_text("# comment\npython a.py\npython b.py\n", encoding="utf-8")
            self.assertEqual(run_fixture_checks.parse_and_validate_list(p), ["python a.py", "python b.py"])
            p.write_text("python b.py\npython a.py\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                run_fixture_checks.parse_and_validate_list(p)
            p.write_text("python a.py\npython a.py\n", encoding="utf-8")
            with self.assertRaises(ValueError):
                run_fixture_checks.parse_and_validate_list(p)

    def test_wasm_batching_and_filtering(self) -> None:
        entries = [
            ("fid-a", []),
            ("fid-b", ["--test", "t1"]),
            ("fid-c", []),
        ]
        cmds = run_wasm32_tests.build_commands(entries, "ALL", use_nextest=True)
        self.assertEqual(len(cmds), 2)
        self.assertEqual(
            cmds[0][1],
            [
                "cargo",
                "nextest",
                "run",
                "--no-fail-fast",
                "--no-tests=pass",
                "--target",
                "wasm32-wasip1",
                "-p",
                "fid-a",
                "-p",
                "fid-c",
            ],
        )
        self.assertEqual(
            cmds[1][1],
            ["cargo", "test", "--target", "wasm32-wasip1", "-p", "fid-b", "--test", "t1"],
        )
        filtered = run_wasm32_tests.build_commands(entries, "fid-c", use_nextest=False)
        self.assertEqual(len(filtered), 1)
        self.assertEqual(filtered[0][1], ["cargo", "test", "--no-fail-fast", "--target", "wasm32-wasip1", "-p", "fid-c"])
        self.assertEqual(run_wasm32_tests.build_commands(entries, "NONE", use_nextest=True), [])

    def test_fixture_affected_selection(self) -> None:
        entries = [
            "python fixtures/geo/nurbs/invert/make_invert_fixtures.py --check",
            "python fixtures/viz/labels/make_part20.py --check",
            "python tools/check_command_defs.py",
            "python tools/rpt_float_lint.py",
        ]
        sel = run_fixture_checks.select_affected_entries(
            entries,
            changed_files=["crates/fid-overlay/src/lib.rs", "fixtures/viz/labels/part20/project.json"],
            added_or_changed_lines={"python fixtures/viz/labels/make_part20.py --check"},
            affected_crates={"fid-overlay"},
        )
        self.assertIn("python fixtures/viz/labels/make_part20.py --check", sel)
        self.assertIn("python tools/check_command_defs.py", sel)
        self.assertNotIn("python fixtures/geo/nurbs/invert/make_invert_fixtures.py --check", sel)
        self.assertNotIn("python tools/rpt_float_lint.py", sel)

    def test_repo_lists_validate_cleanly(self) -> None:
        entries = run_fixture_checks.parse_and_validate_list(ROOT / "ci" / "fixture-checks.txt")
        self.assertGreater(len(entries), 100)
        members = run_wasm32_tests.workspace_member_names(ROOT)
        wasm_entries = run_wasm32_tests.parse_and_validate_wasm_list(ROOT / "ci" / "wasm32-crates.txt", members)
        self.assertGreater(len(wasm_entries), 20)


if __name__ == "__main__":
    unittest.main()
