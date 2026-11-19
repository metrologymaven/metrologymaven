"""Tests of tools/rpt_float_lint.py, the float-to-text lint of spec node RPT.MODEL.FMT
(REQ-RPT.MODEL.FMT-002, AC-7). The expected findings are hand-labelled from the rules in the
tool's documentation, line by line in the fixture crates under fixtures/rpt/model/fmt/lint/.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import io
import sys
import tempfile
import unittest
from contextlib import redirect_stderr, redirect_stdout
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools"))

import rpt_float_lint as lint  # noqa: E402

FIX = ROOT / "fixtures" / "rpt" / "model" / "fmt" / "lint"


def quiet_main(argv):
    out, err = io.StringIO(), io.StringIO()
    with redirect_stdout(out), redirect_stderr(err):
        code = lint.main(argv)
    return code, out.getvalue()


class BadCrate(unittest.TestCase):
    def test_ac7_and_every_rule_fire(self):
        found = [(f.line, f.rule) for f in lint.run(ROOT, [FIX / "bad"])]
        self.assertEqual(
            found,
            [
                (7, "RPT-FLOAT-ARG"),  # format!("{}", 1.5_f64): AC-7
                (11, "RPT-FLOAT-SPEC"),
                (15, "RPT-FLOAT-SPEC"),
                (19, "RPT-FLOAT-ARG"),
                (23, "RPT-FLOAT-TOSTRING"),
                (27, "RPT-FLOAT-TOSTRING"),
                (31, "RPT-FLOAT-TOSTRING"),
                (36, "RPT-FLOAT-TRAIT"),
                (41, "RPT-FLOAT-CRATE"),
                (45, "RPT-FLOAT-ARG"),
            ],
        )

    def test_cli_fails_on_bad_crate(self):
        code, out = quiet_main(["--crate", str(FIX / "bad")])
        self.assertEqual(code, 1)
        self.assertIn("lib.rs:7:", out)
        self.assertIn("RPT-FLOAT-ARG", out)

    def test_ac7_alone(self):
        # The exact AC-7 snippet in a fresh test crate.
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "src"
            src.mkdir()
            (src / "lib.rs").write_text('pub fn f() -> String {\n    format!("{}", 1.5_f64)\n}\n', encoding="utf-8")
            code, out = quiet_main(["--crate", d])
        self.assertEqual(code, 1)
        self.assertIn("lib.rs:2:19: RPT-FLOAT-ARG", out)


class GoodCode(unittest.TestCase):
    def test_good_crate_passes(self):
        self.assertEqual(lint.run(ROOT, [FIX / "good"]), [])
        self.assertEqual(quiet_main(["--crate", str(FIX / "good")])[0], 0)

    def test_unlexable_file_fails_closed(self):
        found = [(f.line, f.rule) for f in lint.run(ROOT, [FIX / "unlexable"])]
        self.assertEqual(found, [(4, "RPT-LEX")])

    def test_not_utf8_fails_closed(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "src"
            src.mkdir()
            (src / "lib.rs").write_bytes(b"pub fn f() {}\xff\n")
            self.assertEqual([f.rule for f in lint.run(Path(d), [Path(d)])], ["RPT-LEX"])

    def test_repository_report_crates_pass(self):
        # The lint as CI runs it: every crates/fid-report* crate, no finding.
        crates = lint.crate_dirs(ROOT)
        self.assertIn("fid-report", [c.name for c in crates])
        self.assertEqual([str(f) for f in lint.run(ROOT)], [])


class Specs(unittest.TestCase):
    def test_bad_spec(self):
        for fmt in ["{:.3}", "{x:>8.2}", "{:.*}", "{:e}", "{0:E}", "a {:+.1} b"]:
            self.assertIsNotNone(lint.bad_spec(fmt), fmt)
        for fmt in ["{}", "{:?}", "{:#?}", "{:>8}", "{:08}", "{:#x}", "{{:.3}}", "{name}"]:
            self.assertIsNone(lint.bad_spec(fmt), fmt)


if __name__ == "__main__":
    unittest.main()
