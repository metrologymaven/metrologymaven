"""Acceptance tests AC-1..AC-3 for UNC.RDL.TRACIM.PUBLIC (`tools/rdl/tracim_public.py`)."""

from __future__ import annotations

import importlib.util
import json
import os
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[3]
MOCK_DIR = ROOT / "fixtures" / "rdl" / "tracim" / "mock"
WORKFLOW_PATH = ROOT / ".github" / "workflows" / "tracim-public.yml"

if str(ROOT) not in sys.path:
    sys.path.insert(0, str(ROOT))

from tools.rdl import tracim_public  # noqa: E402


def _load_mock_server():
    spec = importlib.util.spec_from_file_location("tracim_mock_server", MOCK_DIR / "mock_server.py")
    assert spec is not None and spec.loader is not None
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod.TracimMockServer


TracimMockServer = _load_mock_server()
SENTINEL_KEY = "KEY-SENTINEL-PUBLIC-001"
_WIN_RUNTIME_VARS = (
    "SYSTEMROOT",
    "SYSTEMDRIVE",
    "WINDIR",
    "COMSPEC",
    "PATHEXT",
    "TEMP",
    "TMP",
    "USERPROFILE",
)


def _clean_test_env(**extra: str) -> dict[str, str]:
    """Builds a minimal subprocess env preserving OS runtime vars needed on Windows/POSIX."""
    env: dict[str, str] = {}
    if os.name == "nt":
        env_by_upper = {k.upper(): v for k, v in os.environ.items()}
        env["PATH"] = env_by_upper.get("PATH", "")
        for var in _WIN_RUNTIME_VARS:
            if var in env_by_upper:
                env[var] = env_by_upper[var]
    else:
        env["PATH"] = os.environ.get("PATH", "/usr/bin:/bin")
        for var in ("TMPDIR", "TEMP", "TMP"):
            if var in os.environ:
                env[var] = os.environ[var]
    env.update(extra)
    return env


class TestTracimPublicJob(unittest.TestCase):
    def test_ac1_loopback_mock_passes_and_emits_clean_artifact_without_key_or_raw_data(self) -> None:
        server = TracimMockServer(expected_key=SENTINEL_KEY, verdict_mode="passed")
        url = server.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                out_path = Path(tmp) / "tracim-public-latest.json"
                code, artifact = tracim_public.run_public_test(
                    fid_argv=[sys.executable, str(MOCK_DIR / "mock_fid_cli.py")],
                    out_path=out_path,
                    workflow_path=WORKFLOW_PATH,
                    destination=url,
                    env=_clean_test_env(TRACIM_PUBLIC_KEY=SENTINEL_KEY),
                )
                self.assertEqual(code, 0)
                self.assertTrue(out_path.exists())
                raw_text = out_path.read_text(encoding="utf-8")
                # Sentinel key and raw test data must never appear in the published artifact.
                self.assertNotIn(SENTINEL_KEY, raw_text)
                self.assertNotIn("raw_cases", raw_text)
                self.assertNotIn("points_mm", raw_text)
                self.assertNotIn("mock-plane-01", raw_text)

                parsed = json.loads(raw_text)
                self.assertEqual(parsed, artifact)
                self.assertEqual(parsed["schema"], "tracim-public/v1")
                self.assertEqual(parsed["test_kind"], "public")
                self.assertEqual(parsed["verdict"], "passed")
                self.assertEqual(parsed["kernel_semver"], "0.1.0-rc.1")
                self.assertTrue(parsed["manifest_digest"].startswith("sha256:"))
                self.assertEqual(len(parsed["manifest_digest"]), 7 + 64)
                self.assertIsNone(parsed["reason"])
        finally:
            server.stop()

    def test_ac2_failed_verdict_fails_job_with_reason_verdict_failed(self) -> None:
        server = TracimMockServer(expected_key=SENTINEL_KEY, verdict_mode="failed")
        url = server.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                out_path = Path(tmp) / "tracim-public-latest.json"
                code, artifact = tracim_public.run_public_test(
                    fid_argv=[sys.executable, str(MOCK_DIR / "mock_fid_cli.py")],
                    out_path=out_path,
                    workflow_path=WORKFLOW_PATH,
                    destination=url,
                    env=_clean_test_env(TRACIM_PUBLIC_KEY=SENTINEL_KEY),
                )
                self.assertEqual(code, 1)
                self.assertEqual(artifact["verdict"], "failed")
                self.assertEqual(artifact["reason"], "verdict_failed")
                self.assertEqual(artifact["kernel_semver"], "0.1.0-rc.1")
                self.assertNotIn(SENTINEL_KEY, out_path.read_text(encoding="utf-8"))
        finally:
            server.stop()

    def test_ac3_paid_key_exposed_fails_config_check(self) -> None:
        # 1. Shipped workflow passes config check.
        wf_text = WORKFLOW_PATH.read_text(encoding="utf-8")
        self.assertEqual(tracim_public.check_workflow_config(wf_text, env={}), [])

        # 2. Workflow exposing paid-order secret fails with paid_key_exposed.
        bad_wf_path = MOCK_DIR / "workflow_paid_exposed.yml"
        violations = tracim_public.check_workflow_config(bad_wf_path.read_text(encoding="utf-8"), env={})
        self.assertTrue(any("paid_key_exposed" in v for v in violations), f"got {violations}")

        with tempfile.TemporaryDirectory() as tmp:
            out_path = Path(tmp) / "tracim-public-latest.json"
            code, artifact = tracim_public.run_public_test(
                fid_argv=[sys.executable, str(MOCK_DIR / "mock_fid_cli.py")],
                out_path=out_path,
                workflow_path=bad_wf_path,
                destination="http://127.0.0.1:1",
                env=_clean_test_env(TRACIM_PUBLIC_KEY=SENTINEL_KEY),
            )
            self.assertEqual(code, 1)
            self.assertEqual(artifact["reason"], "paid_key_exposed")

        # 3. Runtime environment exposing TRACIM_PAID_ORDER_KEY also fails with paid_key_exposed.
        env_violations = tracim_public.check_workflow_config(
            wf_text,
            env={"TRACIM_PUBLIC_KEY": SENTINEL_KEY, "TRACIM_PAID_ORDER_KEY": "secret-paid"},
        )
        self.assertTrue(any("paid_key_exposed" in v for v in env_violations), f"got {env_violations}")

    def test_unreachable_service_fails_with_service_unreachable(self) -> None:
        # Start and immediately stop a loopback server so the port is guaranteed closed.
        server = TracimMockServer(expected_key=SENTINEL_KEY, verdict_mode="passed")
        url = server.start()
        server.stop()

        with tempfile.TemporaryDirectory() as tmp:
            out_path = Path(tmp) / "tracim-public-latest.json"
            code, artifact = tracim_public.run_public_test(
                fid_argv=[sys.executable, str(MOCK_DIR / "mock_fid_cli.py")],
                out_path=out_path,
                workflow_path=WORKFLOW_PATH,
                destination=url,
                env=_clean_test_env(TRACIM_PUBLIC_KEY=SENTINEL_KEY),
            )
            self.assertEqual(code, 1)
            self.assertEqual(artifact["verdict"], "error")
            self.assertEqual(artifact["reason"], "service_unreachable")
            self.assertNotIn(SENTINEL_KEY, out_path.read_text(encoding="utf-8"))

    def test_child_env_forwards_mixed_case_windows_runtime_vars_and_strips_unrelated(self) -> None:
        server = TracimMockServer(expected_key=SENTINEL_KEY, verdict_mode="passed")
        url = server.start()
        try:
            with tempfile.TemporaryDirectory() as tmp:
                probe_script = Path(tmp) / "env_probe_cli.py"
                probe_out = Path(tmp) / "child_env.json"
                probe_script.write_text(
                    "import json, os, subprocess, sys\n"
                    f"Path = __import__('pathlib').Path({str(probe_out)!r})\n"
                    "Path.write_text(json.dumps(dict(os.environ), sort_keys=True), encoding='utf-8')\n"
                    "import importlib.util\n"
                    f"spec = importlib.util.spec_from_file_location('mock_cli', {str(MOCK_DIR / 'mock_fid_cli.py')!r})\n"
                    "mod = importlib.util.module_from_spec(spec)\n"
                    "spec.loader.exec_module(mod)\n"
                    "raise SystemExit(mod.main(sys.argv))\n",
                    encoding="utf-8",
                )

                base_env = _clean_test_env(TRACIM_PUBLIC_KEY=SENTINEL_KEY)
                hostile_env: dict[str, str] = {
                    "TRACIM_PUBLIC_KEY": SENTINEL_KEY,
                    "UNRELATED_HOST_VAR": "must-not-leak",
                    "FID_EXTRA_SECRET": "also-must-not-leak",
                }
                if os.name == "nt":
                    # Explicitly pass mixed-case keys to verify case-insensitive Windows forwarding.
                    hostile_env["Path"] = base_env.get("PATH", "")
                    hostile_env["SystemRoot"] = base_env["SYSTEMROOT"]
                    for k in ("SYSTEMDRIVE", "WINDIR", "COMSPEC", "PATHEXT", "TEMP", "TMP", "USERPROFILE"):
                        if k in base_env:
                            hostile_env[k.title()] = base_env[k]
                else:
                    hostile_env["PATH"] = base_env["PATH"]
                    hostile_env["SystemRoot"] = "C:\\Windows"

                out_path = Path(tmp) / "tracim-public-latest.json"
                code, artifact = tracim_public.run_public_test(
                    fid_argv=[sys.executable, str(probe_script)],
                    out_path=out_path,
                    workflow_path=WORKFLOW_PATH,
                    destination=url,
                    env=hostile_env,
                )
                self.assertEqual(code, 0, f"artifact={artifact}")
                self.assertEqual(artifact["verdict"], "passed")

                child_env = json.loads(probe_out.read_text(encoding="utf-8"))
                child_keys_upper = {k.upper() for k in child_env}
                self.assertNotIn("UNRELATED_HOST_VAR", child_keys_upper)
                self.assertNotIn("FID_EXTRA_SECRET", child_keys_upper)
                self.assertIn("TRACIM_PUBLIC_KEY", child_keys_upper)
                if os.name == "nt":
                    self.assertIn("SYSTEMROOT", child_keys_upper)
                    self.assertEqual(
                        {k.upper(): v for k, v in child_env.items()}["SYSTEMROOT"],
                        base_env["SYSTEMROOT"],
                    )

                # paid_key_exposed guard remains strict even with mixed-case runtime vars present.
                bad_env = dict(hostile_env)
                bad_env["TRACIM_PAID_ORDER_KEY"] = "secret-paid"
                bad_out = Path(tmp) / "tracim-paid-exposed.json"
                bad_code, bad_artifact = tracim_public.run_public_test(
                    fid_argv=[sys.executable, str(probe_script)],
                    out_path=bad_out,
                    workflow_path=WORKFLOW_PATH,
                    destination=url,
                    env=bad_env,
                )
                self.assertEqual(bad_code, 1)
                self.assertEqual(bad_artifact["reason"], "paid_key_exposed")
        finally:
            server.stop()


if __name__ == "__main__":
    unittest.main()
