"""Tests of tools/rpt_float_lint.py, the RPT float rule of spec node RPT.MODEL.FMT
(REQ-RPT.MODEL.FMT-002, AC-7), and of its wiring into CI.

The expected findings are the `// expect:` annotations in the fixture crates under
fixtures/rpt/model/fmt/lint/, hand-labelled line by line from the rules. The clippy layer
(`disallowed_types`) is proven by `python tools/rpt_float_lint.py --clippy-selftest`, a line
of ci/fixture-checks.txt; this file checks that the line is there.
Run from the repository root:  python -m unittest discover -s tools/tests -v
"""

import io
import re
import sys
import tempfile
import tomllib
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


def by_line(findings, name):
    got = {}
    for f in findings:
        if f.path.endswith(name):
            got.setdefault(f.line, []).append(f.rule)
    return {k: sorted(v) for k, v in got.items()}


class BadCrate(unittest.TestCase):
    def test_every_annotated_rule_fires_and_nothing_else(self):
        findings = lint.run(ROOT, [FIX / "bad"])
        for name in ["lib.rs", "scan_only.rs"]:
            src = (FIX / "bad" / "src" / name).read_text(encoding="utf-8")
            self.assertEqual(by_line(findings, "/" + name), lint.annotated(src, "expect"), name)

    def test_the_eight_review_bypasses_are_each_caught_by_both_layers(self):
        src = (FIX / "bad" / "src" / "lib.rs").read_text(encoding="utf-8")
        expect = lint.annotated(src, "expect")
        clippy = lint.annotated(src, "clippy")
        lines = src.splitlines()
        for n in range(1, 9):
            start = next(i for i, l in enumerate(lines, 1) if l.startswith(f"// Review bypass {n}:"))
            end = next((i for i, l in enumerate(lines, 1) if i > start and l.startswith("// ")), len(lines) + 1)
            block = range(start, end)
            self.assertTrue(any(i in expect for i in block), f"bypass {n}: no scan finding")
            self.assertTrue(any(i in clippy for i in block), f"bypass {n}: no clippy annotation")

    def test_cli_fails_on_bad_crate(self):
        code, out = quiet_main(["--crate", str(FIX / "bad")])
        self.assertEqual(code, 1)
        self.assertIn("RPT-FLOAT-ARG", out)

    def test_ac7_alone(self):
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

    def test_unlexable_file_fails_closed(self):
        found = [(f.line, f.rule) for f in lint.run(ROOT, [FIX / "unlexable"])]
        self.assertEqual(found, [(4, "RPT-LEX")])

    def test_not_utf8_fails_closed(self):
        with tempfile.TemporaryDirectory() as d:
            src = Path(d) / "src"
            src.mkdir()
            (src / "lib.rs").write_bytes(b"pub fn f() {}\xff\n")
            self.assertEqual([f.rule for f in lint.run(Path(d), [Path(d)])], ["RPT-LEX"])

    def test_floats_are_allowed_only_in_the_listed_files(self):
        code = "#![allow(clippy::disallowed_types)]\npub fn f(x: f64) -> f64 { x * 2.0 }\n"
        toks = lint.lex(code)
        self.assertEqual(lint.scan_tokens("crates/fid-report/src/fmt/model.rs", toks), [])
        self.assertEqual(lint.scan_tokens("crates/fid-report/src/json_num.rs", toks), [])
        rules = sorted(f.rule for f in lint.scan_tokens("crates/fid-report/src/doc/defs.rs", toks))
        self.assertEqual(rules, ["RPT-FLOAT-LITERAL", "RPT-FLOAT-TYPE", "RPT-FLOAT-TYPE", "RPT-LINT-ALLOW"])
        # Format rules apply in fmt too.
        spec = lint.lex('fn g(x: f64) -> String { format!("{:.2}", x) }\n')
        self.assertEqual([f.rule for f in lint.scan_tokens("crates/fid-report/src/fmt/mod.rs", spec)], ["RPT-FLOAT-SPEC"])

    def test_lint_switches(self):
        for attr in ["#[allow(warnings)]", "#![allow(clippy::all)]", "#[expect(clippy::style)]",
                     "#[cfg_attr(test, allow(clippy::disallowed_methods))]"]:
            rules = [f.rule for f in lint.scan_tokens("crates/fid-report/src/x.rs", lint.lex(attr + "\nfn a() {}\n"))]
            self.assertEqual(rules, ["RPT-LINT-ALLOW"], attr)
        ok = "#[allow(clippy::unwrap_used, dead_code)]\nfn a() {}\n"
        self.assertEqual(lint.scan_tokens("crates/fid-report/src/x.rs", lint.lex(ok)), [])


class Wiring(unittest.TestCase):
    """Fails if the rule is unhooked from CI or stops covering crates/fid-report*."""

    def test_ci_runs_the_scan_and_the_clippy_selftest(self):
        lines = (ROOT / "ci" / "fixture-checks.txt").read_text(encoding="utf-8").splitlines()
        self.assertIn("python tools/rpt_float_lint.py", lines)
        self.assertIn("python tools/rpt_float_lint.py --clippy-selftest", lines)
        wf = (ROOT / ".github" / "workflows" / "rust.yml").read_text(encoding="utf-8")
        self.assertIn("cargo clippy --workspace --all-targets -- -D warnings", wf)
        self.assertIn("list=ci/fixture-checks.txt", wf)

    def test_every_report_crate_is_covered(self):
        on_disk = sorted(p.name for p in (ROOT / "crates").iterdir() if p.name.startswith("fid-report"))
        self.assertGreaterEqual(len(on_disk), 2)
        self.assertEqual([c.name for c in lint.crate_dirs(ROOT)], on_disk)
        members = tomllib.loads((ROOT / "Cargo.toml").read_text(encoding="utf-8"))["workspace"]["members"]
        self.assertIn("crates/*", members)
        for name in on_disk:
            manifest = tomllib.loads((ROOT / "crates" / name / "Cargo.toml").read_text(encoding="utf-8"))
            self.assertEqual(manifest.get("lints"), {"workspace": True}, name)
        self.assertEqual(lint.check_config(ROOT), [])

    def test_allowed_files_are_only_fmt_and_the_json_boundary(self):
        self.assertEqual(lint.ALLOWED_FLOAT_FILES, ("crates/fid-report/src/fmt/", "crates/fid-report/src/json_num.rs"))
        allowing = []
        for c in lint.crate_dirs(ROOT):
            for f in (c / "src").rglob("*.rs"):
                if re.search(r"allow\(clippy::disallowed_types\)", f.read_text(encoding="utf-8")):
                    allowing.append(f.relative_to(ROOT).as_posix())
        self.assertEqual(sorted(allowing), ["crates/fid-report/src/fmt/mod.rs", "crates/fid-report/src/json_num.rs"])

    def test_missing_clippy_config_is_a_finding(self):
        with tempfile.TemporaryDirectory() as d:
            c = Path(d) / "crates" / "fid-report-x"
            (c / "src").mkdir(parents=True)
            (c / "Cargo.toml").write_text("[package]\n", encoding="utf-8")
            self.assertEqual([f.rule for f in lint.check_config(Path(d))], ["RPT-CONFIG"])
            (c / "clippy.toml").write_text('disallowed-types = ["f64"]\n', encoding="utf-8")
            self.assertEqual([f.message for f in lint.check_config(Path(d))], ["disallowed-types lacks f32"])

    def test_repository_report_crates_pass(self):
        self.assertEqual([str(f) for f in lint.run(ROOT)], [])


class Specs(unittest.TestCase):
    def test_bad_spec(self):
        for fmt in ["{:.3}", "{x:>8.2}", "{:.*}", "{:e}", "{0:E}", "a {:+.1} b"]:
            self.assertIsNotNone(lint.bad_spec(fmt), fmt)
        for fmt in ["{}", "{:?}", "{:#?}", "{:>8}", "{:08}", "{:#x}", "{{:.3}}", "{name}"]:
            self.assertIsNone(lint.bad_spec(fmt), fmt)


if __name__ == "__main__":
    unittest.main()
