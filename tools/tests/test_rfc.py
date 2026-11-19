"""Tests of tools/rfc.py, the RFC comment-period tool of spec node ECO.COMM.RFC (AC-1..AC-5).

Fixtures under tools/tests/fixtures/rfc/ are hand-written with fictional names; the expected
records under expected/ were written by hand from the spec's rules (insert the fields before the
closing `---`, keep every other byte), not produced by running the tool.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import contextlib
import io
import json
import os
import shutil
import subprocess
import sys
import tempfile
import unittest
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import rfc  # noqa: E402

FIX = ROOT / "tools" / "tests" / "fixtures" / "rfc"
CONFIG = str(FIX / "config.json")
START = "2026-12-01T09:00:00Z"
AFTER_CLOSE = "2026-12-09T00:00:00Z"


class RfcTestCase(unittest.TestCase):
    def setUp(self):
        self.tmp = Path(tempfile.mkdtemp(prefix="rfc-test-"))
        self.addCleanup(shutil.rmtree, self.tmp, True)

    def copy(self, name):
        shutil.copyfile(FIX / name, self.tmp / name)
        return self.tmp / name

    def run_rfc(self, *argv, config=CONFIG):
        out, err = io.StringIO(), io.StringIO()
        args = list(argv) + ["--dir", str(self.tmp), "--config", config]
        with contextlib.redirect_stderr(err):
            code = rfc.main(args, out=out)
        return code, out.getvalue(), err.getvalue()

    def start(self, record="D-100", now=START, *extra):
        return self.run_rfc("start", record, "--now", now, *extra)

    def approve(self, approvals, now=AFTER_CLOSE, record="D-100"):
        return self.run_rfc("approve-check", record, "--approvals", str(FIX / approvals), "--now", now)


class TestStart(RfcTestCase):
    def test_ac1_inserts_fields_and_nothing_else(self):
        path = self.copy("D-100-app.md")
        code, out, err = self.start()
        self.assertEqual((code, err), (0, ""))
        self.assertIn("closes 2026-12-08T09:00:00Z", out)
        self.assertEqual(path.read_bytes(), (FIX / "expected" / "D-100-app.md").read_bytes())

    def test_ac2_second_start_refused_and_file_unchanged(self):
        path = self.copy("D-100-app.md")
        self.assertEqual(self.start()[0], 0)
        before = path.read_bytes()
        code, _, err = self.start(now="2026-12-02T09:00:00Z")
        self.assertEqual(code, 1)
        self.assertIn("RFC-ALREADY-STARTED", err)
        self.assertEqual(path.read_bytes(), before)

    def test_crlf_record_without_kind_gets_other_and_short_period(self):
        path = self.copy("D-101-crlf.md")
        code, out, _ = self.start("D-101")
        self.assertEqual(code, 0)
        self.assertIn("days=3", out)
        self.assertEqual(path.read_bytes(), (FIX / "expected" / "D-101-crlf.md").read_bytes())

    def test_kind_option_and_conflict(self):
        self.copy("D-101-crlf.md")
        code, out, _ = self.start("D-101", START, "--kind", "process")
        self.assertEqual(code, 0)
        self.assertIn("days=7", out)
        self.copy("D-100-app.md")
        code, _, err = self.start("D-100", START, "--kind", "other")
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)

    def test_unknown_kind_refused(self):
        path = self.copy("D-101-crlf.md")
        before = path.read_bytes()
        code, _, err = self.start("D-101", START, "--kind", "ap")
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)
        self.assertEqual(path.read_bytes(), before)

    def test_lengths_come_from_config(self):
        self.copy("D-100-app.md")
        cfg = self.tmp / "cfg.json"
        data = json.loads((FIX / "config.json").read_text())
        data["long_days"] = 10
        cfg.write_text(json.dumps(data))
        code, out, _ = self.run_rfc("start", "D-100", "--now", START, config=str(cfg))
        self.assertEqual(code, 0)
        self.assertIn("days=10", out)

    def test_missing_record(self):
        code, _, err = self.start("D-999")
        self.assertEqual(code, 1)
        self.assertIn("RFC-NO-RECORD", err)

    def test_bad_now_and_bad_id(self):
        self.copy("D-100-app.md")
        for args in (("D-100", "2026-12-01 09:00"), ("D-100", "2026-02-30T00:00:00Z"), ("../x", START)):
            code, _, err = self.start(*args)
            self.assertEqual(code, 1, args)
            self.assertIn("RFC-INPUT", err)


class TestConfig(RfcTestCase):
    def test_bad_configs(self):
        self.copy("D-100-app.md")
        base = json.loads((FIX / "config.json").read_text())
        cases = ["not json", "[]", json.dumps(dict(base, short_days=0)),
                 json.dumps(dict(base, short_days=True)), json.dumps(dict(base, extra=1)),
                 json.dumps({k: v for k, v in base.items() if k != "maintainers"}),
                 json.dumps(dict(base, long_kinds=["app", "galaxy"])),
                 json.dumps(dict(base, aliases={"x": "founder"}))]
        for i, text in enumerate(cases):
            cfg = self.tmp / f"cfg{i}.json"
            cfg.write_text(text)
            code, _, err = self.run_rfc("start", "D-100", "--now", START, config=str(cfg))
            self.assertEqual(code, 1, text)
            self.assertIn("RFC-CONFIG", err, text)
        code, _, err = self.run_rfc("start", "D-100", "--now", START, config=str(self.tmp / "none.json"))
        self.assertIn("RFC-CONFIG", err)

    def test_default_config_is_valid(self):
        cfg = rfc.load_config(rfc.DEFAULT_CONFIG)
        self.assertEqual((cfg["short_days"], cfg["long_days"]), (3, 7))
        self.assertEqual(sorted(cfg["long_kinds"]), ["app", "domain", "process"])


class TestStatus(RfcTestCase):
    def test_ac3_open_then_closed(self):
        self.copy("D-100-app.md")
        self.copy("D-101-crlf.md")
        self.assertEqual(self.start()[0], 0)
        code, out, _ = self.run_rfc("status", "--now", "2026-12-05T00:00:00Z")
        self.assertEqual((code, out), (0, "D-100 open closes 2026-12-08T09:00:00Z\n"))
        code, out, _ = self.run_rfc("status", "--now", "2026-12-08T09:00:01Z")
        self.assertEqual((code, out), (0, "D-100 closed closes 2026-12-08T09:00:00Z\n"))

    def test_sorted_and_boundary(self):
        self.copy("D-100-app.md")
        self.copy("D-101-crlf.md")
        self.start("D-101")
        self.start("D-100")
        code, out, _ = self.run_rfc("status", "--now", "2026-12-04T09:00:00Z")
        self.assertEqual(out, "D-100 open closes 2026-12-08T09:00:00Z\n"
                              "D-101 closed closes 2026-12-04T09:00:00Z\n")

    def test_malformed_record_reported_not_fatal(self):
        self.copy("D-100-app.md")
        self.start()
        (self.tmp / "D-102-bad.md").write_bytes(b"---\nid: D-102\ncomment_period_start: soon\n"
                                                 b"comment_period_days: 3\n---\n")
        (self.tmp / "D-103-bin.md").write_bytes(b"\xff\xfe---\n")
        code, out, err = self.run_rfc("status", "--now", AFTER_CLOSE)
        self.assertEqual(code, 1)
        self.assertEqual(out, "D-100 closed closes 2026-12-08T09:00:00Z\n")
        self.assertEqual(err.count("RFC-INPUT"), 2)


class TestApproveCheck(RfcTestCase):
    def setUp(self):
        super().setUp()
        self.copy("D-100-app.md")
        self.assertEqual(self.start()[0], 0)

    def test_ac4_self_approval_fails_other_passes(self):
        code, _, err = self.approve("approvals-self.json")
        self.assertEqual(code, 1)
        self.assertIn("RFC-SELF-APPROVAL", err)
        code, out, err = self.approve("approvals-other.json")
        self.assertEqual((code, err), (0, ""))
        self.assertIn("grace-example", out)

    def test_ac5_period_open(self):
        code, _, err = self.approve("approvals-other.json", now="2026-12-03T00:00:00Z")
        self.assertEqual(code, 1)
        self.assertIn("RFC-PERIOD-OPEN", err)

    def test_not_started_is_period_open(self):
        self.copy("D-101-crlf.md")
        code, _, err = self.approve("approvals-other.json", record="D-101")
        self.assertEqual(code, 1)
        self.assertIn("RFC-PERIOD-OPEN", err)

    def test_raw_author_login_is_self(self):
        # D-101 is raised by grace-example directly; only ada-example's approval counts.
        self.copy("D-101-crlf.md")
        self.start("D-101")
        code, out, _ = self.approve("approvals-other.json", record="D-101")
        self.assertEqual(code, 0)
        self.assertIn("ada-example", out)
        self.assertNotIn("grace-example", out)

    def test_ignored_approvals(self):
        for name in ("approvals-early.json", "approvals-nonmaintainer.json", "approvals-withdrawn.json"):
            code, _, err = self.approve(name)
            self.assertEqual(code, 1, name)
            self.assertIn("RFC-SELF-APPROVAL", err, name)

    def test_review_after_now_ignored(self):
        code, _, err = self.approve("approvals-other.json", now="2026-12-08T11:00:00Z")
        self.assertIn("RFC-SELF-APPROVAL", err)

    def test_malformed_approvals(self):
        code, _, err = self.approve("approvals-bad.json")
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)
        code, _, err = self.approve("missing.json")
        self.assertIn("RFC-INPUT", err)
        code, _, err = self.run_rfc("approve-check", "D-100", "--now", AFTER_CLOSE)
        self.assertIn("RFC-INPUT", err)


class TestHostileInput(RfcTestCase):
    """Review fixes for PR #169: deep JSON, year-9999 overflow, symlinks, odd line breaks, ids."""

    def test_deeply_nested_json(self):
        self.copy("D-100-app.md")
        self.assertEqual(self.start()[0], 0)
        deep = "[" * 100000 + "]" * 100000
        (self.tmp / "deep.json").write_text(deep)
        code, _, err = self.run_rfc("approve-check", "D-100", "--approvals", str(self.tmp / "deep.json"),
                                    "--now", AFTER_CLOSE)
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)
        code, _, err = self.run_rfc("start", "D-100", "--now", START, config=str(self.tmp / "deep.json"))
        self.assertEqual(code, 1)
        self.assertIn("RFC-CONFIG", err)

    def test_overflow_on_start_leaves_record_untouched(self):
        path = self.copy("D-100-app.md")
        before = path.read_bytes()
        code, _, err = self.start(now="9999-12-30T00:00:00Z")
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)
        self.assertEqual(path.read_bytes(), before)

    def test_overflow_in_recorded_period(self):
        (self.tmp / "D-104-far.md").write_bytes(b"---\nid: D-104\nraised_by: founder\n"
                                                 b"comment_period_start: 9999-12-30T00:00:00Z\n"
                                                 b"comment_period_days: 7\n---\n")
        code, out, err = self.run_rfc("status", "--now", AFTER_CLOSE)
        self.assertEqual((code, out), (1, ""))
        self.assertIn("RFC-INPUT", err)
        code, _, err = self.approve("approvals-other.json", record="D-104")
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)

    def _symlink(self, target, link):
        try:
            os.symlink(target, link)
        except (OSError, NotImplementedError):
            self.skipTest("symbolic links are not available here")

    def test_symlinked_record_refused(self):
        outside = Path(tempfile.mkdtemp(prefix="rfc-outside-"))
        self.addCleanup(shutil.rmtree, outside, True)
        shutil.copyfile(FIX / "D-100-app.md", outside / "D-100-app.md")
        self._symlink(outside / "D-100-app.md", self.tmp / "D-100-app.md")
        code, _, err = self.start()
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)
        self.assertTrue((self.tmp / "D-100-app.md").is_symlink())
        self.assertEqual((outside / "D-100-app.md").read_bytes(), (FIX / "D-100-app.md").read_bytes())
        code, _, err = self.run_rfc("status", "--now", AFTER_CLOSE)
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)

    def test_symlinked_approvals_and_config_refused(self):
        self.copy("D-100-app.md")
        self.assertEqual(self.start()[0], 0)
        self._symlink(FIX / "approvals-other.json", self.tmp / "link.json")
        code, _, err = self.run_rfc("approve-check", "D-100", "--approvals", str(self.tmp / "link.json"),
                                    "--now", AFTER_CLOSE)
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)
        self._symlink(FIX / "config.json", self.tmp / "cfg-link.json")
        code, _, err = self.run_rfc("approve-check", "D-100", "--approvals", str(FIX / "approvals-other.json"),
                                    "--now", AFTER_CLOSE, config=str(self.tmp / "cfg-link.json"))
        self.assertEqual(code, 1)
        self.assertIn("RFC-INPUT", err)

    def test_odd_line_breaks_refused(self):
        # U+2028 would make scope.parse_front see a line break that bytes.splitlines does not.
        for sep in (" ", "\x85", "\x0c"):
            rec = self.tmp / "D-105-odd.md"
            data = ("---\nid: D-105\nraised_by: founder\ntitle: x" + sep
                    + "comment_period_start: 2026-12-01T09:00:00Z\n---\n").encode("utf-8")
            rec.write_bytes(data)
            code, _, err = self.start("D-105")
            self.assertEqual(code, 1, repr(sep))
            self.assertIn("RFC-INPUT", err)
            self.assertEqual(rec.read_bytes(), data)

    def test_missing_or_wrong_id(self):
        (self.tmp / "D-106-noid.md").write_bytes(b"---\ntitle: no id\n---\n")
        (self.tmp / "D-107-wrong.md").write_bytes(b"---\nid: D-999\n---\n")
        for rid in ("D-106", "D-107"):
            code, _, err = self.start(rid)
            self.assertEqual(code, 1, rid)
            self.assertIn("RFC-INPUT", err)
        code, out, err = self.run_rfc("status", "--now", AFTER_CLOSE)
        self.assertEqual((code, out), (1, ""))
        self.assertEqual(err.count("RFC-INPUT"), 1)  # D-107's id is valid; it just has no period


class TestCli(unittest.TestCase):
    def test_no_traceback_on_usage_or_input_error(self):
        tool = str(ROOT / "tools" / "rfc.py")
        for args, want in (([], 2), (["status"], 2), (["status", "--now", "x", "--dir", str(FIX)], 1)):
            p = subprocess.run([sys.executable, tool] + args, capture_output=True, text=True)
            self.assertEqual(p.returncode, want, args)
            self.assertNotIn("Traceback", p.stderr)


if __name__ == "__main__":
    unittest.main()
