"""Acceptance tests for spec node ECO.REPO.TPL.SELFTEST (AC-1..AC-4).

Run from repository root:
    python -m unittest discover -s tools/repo_template/tests -v
"""

from __future__ import annotations

import os
import re
import subprocess
import sys
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
TOOLS = ROOT / "tools"
FIX = ROOT / "fixtures" / "repo" / "tpl" / "selftest"
WORKFLOW = ROOT / ".github" / "workflows" / "repo-template-selftest.yml"


def run_selftest_cli(*args: str) -> subprocess.CompletedProcess[str]:
    env = {**os.environ, "PYTHONPATH": str(TOOLS)}
    return subprocess.run(
        [sys.executable, "-m", "repo_template", "selftest", *args],
        cwd=ROOT,
        env=env,
        capture_output=True,
        text=True,
        timeout=120,
    )


def extract_workflow_paths(yaml_text: str) -> dict[str, set[str]]:
    """Minimal stdlib YAML path-filter extractor for trigger sections (pull_request, push)."""
    result: dict[str, set[str]] = {}
    in_on = False
    current_event: str | None = None
    in_paths = False
    paths_indent = -1

    for raw_line in yaml_text.splitlines():
        line = raw_line.split("#", 1)[0].rstrip()
        if not line.strip():
            continue
        indent = len(line) - len(line.lstrip(" "))
        stripped = line.strip()

        if indent == 0:
            in_on = stripped == "on:"
            current_event = None
            in_paths = False
            continue
        if not in_on:
            continue

        if indent == 2 and stripped.endswith(":"):
            current_event = stripped[:-1].strip()
            result.setdefault(current_event, set())
            in_paths = False
            continue

        if current_event is not None:
            if stripped == "paths:":
                in_paths = True
                paths_indent = indent
                continue
            if in_paths:
                if indent <= paths_indent:
                    in_paths = False
                elif stripped.startswith("- "):
                    val = stripped[2:].strip().strip("'\"")
                    result[current_event].add(val)
    return result


class TestSelftest(unittest.TestCase):
    def test_ac1_real_template_passes_format_lint_rules(self) -> None:
        r = run_selftest_cli("--checks", str(FIX / "required-checks.toml"))
        self.assertEqual(r.returncode, 0, f"stdout:\n{r.stdout}\nstderr:\n{r.stderr}")
        lines = [ln.strip() for ln in r.stdout.splitlines() if ln.strip()]
        for variant in ("code", "spec"):
            for check_id in ("format", "lint", "rules"):
                self.assertIn(f"PASS {variant}/{check_id}", lines)

    def test_ac2_hand_edited_agents_block_fails_naming_code_rules(self) -> None:
        r = run_selftest_cli(
            "--checks",
            str(FIX / "required-checks.toml"),
            "--templates",
            str(FIX / "broken-template"),
        )
        self.assertEqual(r.returncode, 1, f"expected exit 1; stdout:\n{r.stdout}\nstderr:\n{r.stderr}")
        self.assertIn("FAIL code/rules", r.stdout)
        self.assertIn("SELFTEST-CHECK-FAILED code/rules", r.stderr)
        # Spec variant remains clean and passes all three checks.
        self.assertIn("PASS spec/rules", r.stdout)

    def test_ac3_missing_local_command_fails_with_code(self) -> None:
        r = run_selftest_cli("--checks", str(FIX / "missing-local.toml"))
        self.assertEqual(r.returncode, 1, f"expected exit 1; stdout:\n{r.stdout}\nstderr:\n{r.stderr}")
        self.assertIn("SELFTEST-NO-LOCAL-COMMAND layers", r.stderr)

    def test_ac4_workflow_path_filter_contains_all_four_groups(self) -> None:
        self.assertTrue(WORKFLOW.is_file(), f"missing workflow {WORKFLOW}")
        text = WORKFLOW.read_text(encoding="utf-8")
        # Fork safety: read-only contents permission.
        self.assertRegex(text, re.compile(r"contents:\s*read"))
        paths_by_event = extract_workflow_paths(text)
        required_groups = {
            "templates/repo/**",
            "tools/repo_template/**",
            "required-checks.toml",
            "house-rules/**",
        }
        for event in ("pull_request", "push"):
            self.assertIn(event, paths_by_event, f"workflow missing on.{event}")
            missing = required_groups - paths_by_event[event]
            self.assertFalse(missing, f"workflow on.{event}.paths missing: {sorted(missing)}")


if __name__ == "__main__":
    unittest.main()
