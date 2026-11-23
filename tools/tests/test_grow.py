"""Tests for tools/grow.py (spec node ECO.COMM.GROW, AC-1..AC-5).

Run from the repository root:
    python -m unittest discover -s tools/tests -p "test_grow.py" -v
"""

from __future__ import annotations

import json
import os
import shutil
import stat
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
TOOL = ROOT / "tools" / "grow.py"
FIX = Path(__file__).resolve().parent / "fixtures" / "grow"
FIX_TREE = FIX / "tree"
FAKE_AGENT = FIX / "fake_agent.py"
FAKE_AGENT_AUTHFAIL = FIX / "fake_agent_authfail.py"

EXPECTED_NODES = [
    "spec/a/one/NODE.md",
    "spec/a/three/NODE.md",
    "spec/a/two/NODE.md",
]


def run_grow(
    *args: str,
    env: dict[str, str] | None = None,
) -> tuple[int, str, str]:
    proc = subprocess.run(
        [sys.executable, str(TOOL), *args],
        stdout=subprocess.PIPE,
        stderr=subprocess.PIPE,
        text=True,
        encoding="utf-8",
        env=env,
    )
    return proc.returncode, proc.stdout, proc.stderr


class GrowDriverTest(unittest.TestCase):
    def copy_tree(self) -> Path:
        tmp = Path(tempfile.mkdtemp(prefix="grow-tree-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        dst = tmp / "tree"
        shutil.copytree(FIX_TREE, dst)
        return dst

    def make_state_dir(self) -> Path:
        tmp = Path(tempfile.mkdtemp(prefix="grow-state-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        return tmp

    def make_failing_git_shim(self) -> tuple[Path, Path]:
        """Put a failing git shim ahead of PATH on POSIX and Windows."""
        tmp = Path(tempfile.mkdtemp(prefix="grow-git-shim-"))
        self.addCleanup(shutil.rmtree, tmp, True)
        marker = tmp / "git_was_called"

        if os.name == "nt":
            shim = tmp / "git.cmd"
            shim.write_text(
                f"@echo off\r\necho called > \"{marker}\"\r\nexit /b 99\r\n",
                encoding="utf-8",
            )
            shim_bat = tmp / "git.bat"
            shim_bat.write_text(
                f"@echo off\r\necho called > \"{marker}\"\r\nexit /b 99\r\n",
                encoding="utf-8",
            )
        else:
            shim = tmp / "git"
            shim.write_text(
                f"#!/bin/sh\necho called > \"{marker}\"\nexit 99\n",
                encoding="utf-8",
            )
            shim.chmod(shim.stat().st_mode | stat.S_IXUSR | stat.S_IXGRP | stat.S_IXOTH)

        return tmp, marker

    def test_ac1_dry_run_lists_nodes_and_human_review_note(self) -> None:
        tree = self.copy_tree()
        state_dir = self.make_state_dir()
        env = dict(os.environ, GROW_TEST_STATE_DIR=str(state_dir))

        code, out, err = run_grow(
            "--dry-run",
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT),
            env=env,
        )
        self.assertEqual(code, 0, out + err)
        for node in EXPECTED_NODES:
            self.assertIn(node, out)
        self.assertIn("human review", out.lower())
        self.assertFalse((tree / "generated" / "logs").exists())
        self.assertFalse((state_dir / "events").exists())

    def test_ac2_canary_first_parallel_limit_and_logs(self) -> None:
        tree = self.copy_tree()
        state_dir = self.make_state_dir()
        env = dict(os.environ, GROW_TEST_STATE_DIR=str(state_dir))

        code, out, err = run_grow(
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT),
            "--parallel",
            "2",
            "--no-level-check",
            env=env,
        )
        self.assertEqual(code, 0, out + err)

        log_dir = tree / "generated" / "logs"
        self.assertTrue(log_dir.is_dir())
        for node in EXPECTED_NODES:
            log_file = log_dir / (node.replace("/", "_") + ".log")
            self.assertTrue(log_file.is_file(), f"missing log {log_file}")
            text = log_file.read_text(encoding="utf-8")
            self.assertIn("ok", text)
            self.assertIn("exit=0", text)

        event_files = sorted((state_dir / "events").glob("*.json"))
        self.assertEqual(len(event_files), 3)
        events = [json.loads(p.read_text(encoding="utf-8")) for p in event_files]
        events.sort(key=lambda e: e["start_ns"])

        # Maximum concurrency never exceeds --parallel 2.
        max_active = max(e["active_at_start"] for e in events)
        self.assertLessEqual(max_active, 2, f"observed concurrency {max_active} > 2")

        # Canary finished strictly before the remaining two started.
        canary_events = [e for e in events if not e["canary_done_at_start"]]
        after_events = [e for e in events if e["canary_done_at_start"]]
        self.assertEqual(len(canary_events), 1, f"expected 1 canary, got {events}")
        self.assertEqual(len(after_events), 2, f"expected 2 post-canary runs, got {events}")
        canary = canary_events[0]
        self.assertIn(f"TARGET: {EXPECTED_NODES[0]}", " ".join(canary["argv"]))
        for ev in after_events:
            self.assertGreaterEqual(
                ev["start_ns"],
                canary["end_ns"],
                f"post-canary event started before canary ended: {canary} vs {ev}",
            )

        # Verify expand-node prompt wording.
        for ev in events:
            joined = " ".join(ev["argv"])
            self.assertIn("Follow prompts/expand-node.md exactly. TARGET:", joined)

    def test_ac3_auth_failure_stops_after_canary_with_exit_2(self) -> None:
        tree = self.copy_tree()
        state_dir = self.make_state_dir()
        env = dict(os.environ, GROW_TEST_STATE_DIR=str(state_dir))

        code, out, err = run_grow(
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT_AUTHFAIL),
            "--parallel",
            "2",
            "--no-level-check",
            env=env,
        )
        self.assertEqual(code, 2, f"expected exit 2, got {code}\nout={out}\nerr={err}")
        self.assertIn("authentication failed", err.lower())

        event_files = list((state_dir / "events").glob("*.txt"))
        self.assertEqual(len(event_files), 1, "only canary should have run")

        log_dir = tree / "generated" / "logs"
        logs = list(log_dir.glob("*.log"))
        self.assertEqual(len(logs), 1)
        self.assertIn("Invalid API key", logs[0].read_text(encoding="utf-8"))

    def test_ac4_scope_check_runs_last_and_git_never_invoked(self) -> None:
        tree = self.copy_tree()
        state_dir = self.make_state_dir()
        shim_dir, git_marker = self.make_failing_git_shim()

        env = dict(os.environ)
        env["GROW_TEST_STATE_DIR"] = str(state_dir)
        env["PATH"] = str(shim_dir) + os.pathsep + env.get("PATH", "")

        code, out, err = run_grow(
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT),
            "--parallel",
            "2",
            "--no-level-check",
            env=env,
        )
        self.assertEqual(code, 0, out + err)
        self.assertFalse(git_marker.exists(), "git shim must never be invoked")
        self.assertIn("0 errors", out)
        # scope.py check wrote generated/INDEX.md after all agents finished.
        index_md = tree / "generated" / "INDEX.md"
        self.assertTrue(index_md.is_file(), "scope.py check did not run")

        events = [
            json.loads(p.read_text(encoding="utf-8"))
            for p in (state_dir / "events").glob("*.json")
        ]
        last_agent_end = max(e["end_ns"] for e in events)
        index_mtime_ns = index_md.stat().st_mtime_ns
        # Allow minor filesystem timestamp granularity differences while checking order.
        self.assertGreaterEqual(index_mtime_ns + 50_000_000, last_agent_end)

    def test_level_check_codex_and_limit_options(self) -> None:
        tree = self.copy_tree()
        state_dir = self.make_state_dir()
        env = dict(os.environ, GROW_TEST_STATE_DIR=str(state_dir))

        code, out, err = run_grow(
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT),
            "--agent",
            "codex",
            "--model",
            "test-model",
            "--level",
            "2",
            "--limit",
            "2",
            "--parallel",
            "2",
            env=env,
        )
        self.assertEqual(code, 0, out + err)

        # Two nodes expanded + 1 level-check run = 3 total invocations.
        events = [
            json.loads(p.read_text(encoding="utf-8"))
            for p in sorted((state_dir / "events").glob("*.json"))
        ]
        self.assertEqual(len(events), 3)
        self.assertTrue((tree / "generated" / "logs" / "level-check-3.log").is_file())

        # Check codex argv shape and model flag.
        for ev in events:
            self.assertEqual(ev["argv"][:2], ["exec", "--full-auto"])
            self.assertIn("--model", ev["argv"])
            self.assertIn("test-model", ev["argv"])
        lc_events = [
            ev for ev in events if "Follow prompts/level-check.md exactly. LEVEL: 3" in ev["argv"]
        ]
        self.assertEqual(len(lc_events), 1)

    def test_scope_check_failure_exit_code_propagates(self) -> None:
        tree = self.copy_tree()
        # Break tree traceability so scope.py check exits 1 after agents finish.
        (tree / "spec" / "a" / "two" / "NODE.md").unlink()
        state_dir = self.make_state_dir()
        env = dict(os.environ, GROW_TEST_STATE_DIR=str(state_dir))

        code, out, err = run_grow(
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT),
            "--parallel",
            "2",
            "--no-level-check",
            env=env,
        )
        self.assertEqual(code, 1, out + err)
        self.assertIn("REQ-A-002", out)

    def test_parallel_limit_enforced_with_four_open_nodes(self) -> None:
        tree = self.copy_tree()
        # Add a fourth open level-2 node so 3 nodes remain after the canary.
        a_node = tree / "spec" / "a" / "NODE.md"
        a_node.write_text(
            a_node.read_text(encoding="utf-8").replace(
                "- **REQ-A-003** The system shall satisfy REQ-A-003.",
                "- **REQ-A-003** The system shall satisfy REQ-A-003.\n"
                "- **REQ-A-004** The system shall satisfy REQ-A-004.",
            ),
            encoding="utf-8",
        )
        four_dir = tree / "spec" / "a" / "four"
        four_dir.mkdir(parents=True, exist_ok=True)
        (four_dir / "NODE.md").write_text(
            "---\n"
            "id: A.FOUR\n"
            "title: Open Node Four\n"
            "level: 2\n"
            "parent: A\n"
            "kind: subsystem\n"
            "status: open\n"
            "phase: P0\n"
            "size: L\n"
            "owners: []\n"
            "claimed_by:\n"
            "satisfies: [REQ-A-004]\n"
            "updated: 2026-11-22\n"
            "---\n\n"
            "# A.FOUR: Open Node Four\n\n"
            "## Purpose\n\nFixture open level-2 node A.FOUR.\n\n"
            "## Scope\n\n**In:** fixture scope.\n**Out:** none.\n\n"
            "## Requirements\n\n"
            "- **REQ-A.FOUR-001** The system shall satisfy REQ-A.FOUR-001.\n\n"
            "## Children\n\n_See child folders._\n",
            encoding="utf-8",
        )

        state_dir = self.make_state_dir()
        env = dict(os.environ, GROW_TEST_STATE_DIR=str(state_dir))

        code, out, err = run_grow(
            "--root",
            str(tree),
            "--agent-cmd",
            str(FAKE_AGENT),
            "--parallel",
            "2",
            "--no-level-check",
            env=env,
        )
        self.assertEqual(code, 0, out + err)

        events = [
            json.loads(p.read_text(encoding="utf-8"))
            for p in (state_dir / "events").glob("*.json")
        ]
        self.assertEqual(len(events), 4)
        self.assertLessEqual(max(e["active_at_start"] for e in events), 2)


if __name__ == "__main__":
    unittest.main()
