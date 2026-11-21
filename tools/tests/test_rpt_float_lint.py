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
        # The JSON number: only the wildcard pattern outside the allowed files.
        ok = lint.lex("fn k(v: &CValue) -> bool { matches!(v, CValue::Num(_)) }\n")
        self.assertEqual(lint.scan_tokens("crates/fid-report/src/doc/x.rs", ok), [])
        bad = lint.lex("use fid_canon::{CValue, CValue::Num};\nfn k(v: &CValue) { if let CValue::Num(x) = v {} }\n")
        self.assertEqual([f.rule for f in lint.scan_tokens("crates/fid-report/src/doc/x.rs", bad)], ["RPT-NUM", "RPT-NUM"])
        self.assertEqual(lint.scan_tokens("crates/fid-report/src/json_num.rs", bad), [])
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
        self.assertIn("python tools/rpt_float_lint.py --mir", lines)
        self.assertIn("python tools/rpt_float_lint.py --mir-selftest", lines)
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

    @staticmethod
    def tree(d, clippy, dep_src="pub fn ok() -> u32 { 1 }\n", workspace=""):
        root = Path(d)
        (root / "Cargo.toml").write_text('[workspace]\nmembers = ["crates/*"]\n' + workspace, encoding="utf-8")
        c = root / "crates" / "fid-report-x"
        (c / "src").mkdir(parents=True)
        (c / "Cargo.toml").write_text('[package]\n[dependencies]\ndep = { path = "../dep" }\n', encoding="utf-8")
        if clippy is not None:
            (c / "clippy.toml").write_text(clippy, encoding="utf-8")
        dep = root / "crates" / "dep" / "src"
        dep.mkdir(parents=True)
        (dep / "lib.rs").write_text(dep_src, encoding="utf-8")
        return root

    def test_missing_clippy_config_is_a_finding(self):
        with tempfile.TemporaryDirectory() as d:
            root = self.tree(d, None)
            self.assertEqual([f.rule for f in lint.check_config(root)], ["RPT-CONFIG"])
        with tempfile.TemporaryDirectory() as d:
            root = self.tree(d, 'disallowed-types = ["f64"]\n')
            self.assertEqual([f.message for f in lint.check_config(root)], ["disallowed-types lacks f32"])

    def test_a_new_float_returning_dependency_function_is_a_finding(self):
        types = 'disallowed-types = ["f32", "f64"]\n'
        src = "pub mod m;\npub use m::Thing;\n"
        with tempfile.TemporaryDirectory() as d:
            root = self.tree(d, types, src)
            (root / "crates" / "dep" / "src" / "m.rs").write_text(
                "pub struct Thing;\nimpl Thing {\n    pub fn v(&self) -> Option<f64> { None }\n"
                "    pub fn n(&self) -> u32 { 0 }\n}\npub fn r() -> Result<f32, ()> { Ok(0.0) }\n"
                "fn private() -> f64 { 0.0 }\n",
                encoding="utf-8",
            )
            msgs = sorted(f.message for f in lint.check_config(root))
            self.assertEqual(msgs, ["disallowed-methods lacks dep::m::Thing::v", "disallowed-methods lacks dep::m::r"])
            listed = types + 'disallowed-methods = ["dep::Thing::v", { path = "dep::m::r" }]\n'
            (root / "crates" / "fid-report-x" / "clippy.toml").write_text(listed, encoding="utf-8")
            self.assertEqual(lint.check_config(root), [])

    def test_weakened_configuration_is_a_finding(self):
        types = 'disallowed-types = ["f32", "f64"]\n'
        for ws in [
            '[workspace.lints.clippy]\ndisallowed_types = "allow"\n',
            '[workspace.lints.clippy]\nstyle = { level = "allow", priority = -1 }\n',
            '[workspace.lints.clippy]\nall = "allow"\n',
            '[workspace.lints.clippy]\ndisallowed_methods = "allow"\n',
        ]:
            with tempfile.TemporaryDirectory() as d:
                root = self.tree(d, types, workspace=ws)
                self.assertEqual([f.rule for f in lint.check_config(root)], ["RPT-CONFIG"], ws)
        for where in [".clippy.toml", "crates/fid-report-x/.clippy.toml"]:
            with tempfile.TemporaryDirectory() as d:
                root = self.tree(d, types)
                (root / where).write_text("", encoding="utf-8")
                self.assertEqual([f.rule for f in lint.check_config(root)], ["RPT-CONFIG"], where)
        with tempfile.TemporaryDirectory() as d:
            root = self.tree(d, types)
            (root / ".github" / "workflows").mkdir(parents=True)
            (root / ".github" / "workflows" / "x.yml").write_text("env:\n  CLIPPY_CONF_DIR: /tmp\n", encoding="utf-8")
            self.assertEqual([f.rule for f in lint.check_config(root)], ["RPT-CONFIG"])

    def test_the_review_listed_methods_are_disallowed_in_fid_report(self):
        cfg = tomllib.loads((ROOT / "crates" / "fid-report" / "clippy.toml").read_text(encoding="utf-8"))
        paths = {e["path"] for e in cfg["disallowed-methods"]}
        for p in ["fid_gdt::chars::record::Quantity::value", "fid_kunc::UncertaintyStatement::u_std",
                  "fid_num::rng::Cursor::uniform_f64", "fid_kunc::dist::open_unit"]:
            self.assertIn(p, paths)

    def test_repository_report_crates_pass(self):
        self.assertEqual([str(f) for f in lint.run(ROOT)], [])


# A hand-written MIR file in the stable `--emit=mir` text form (rustc 1.81+): trimmed and
# full paths, an impl span, a closure, a constant, a promoted body, drop glue, a shim and
# trait impls printed as `<T as Trait>::m` (a foreign self type, a dependency's trait).
MIR = """// WARNING: This output format is intended for human consumers only
fn val(_1: u64) -> f64 {
    let mut _0: f64;
}

fn fmt::<impl at crates/fid-report/src/fmt/model.rs:3:1: 3:7>::g(_1: &S) -> f64 {
    let mut _0: f64;
}

fn <impl at crates/fid-report/src/bind/model.rs:4:1: 4:7>::half(_1: &W) -> u64 {
    let _2: f64;
}

fn infer::{closure#0}(_1: &mut {closure@crates/fid-report/src/bind/expand.rs:8:55: 8:58}, _2: &u8) -> bool {
    _0 = Lt(copy _3, const 2f64);
}

fn clean(_1: u32) -> u32 {
    let mut _0: u32;
}

const LIMIT: u64 = const 3_u64;

promoted[0] in bind::journal::read: &[u8; 2] = {
    let mut _0: &std::vec::Vec<f64>;
}

fn std::ptr::drop_in_place::<Vec<f64>>(_1: *mut Vec<f64>) -> () {
}

fn nowhere() -> f64 {
}

const prom_in_fmt::promoted[0]: &f64 = {
    let mut _0: &f64;
}

const prom_elsewhere::promoted[0]: &f64 = {
    let mut _0: &f64;
}

fn <std::vec::Vec<u8> as bind::Tr>::m(_1: &Vec<u8>) -> u64 {
    let _2: f64;
}

fn <Count as dep::Measure>::measure(_1: &Count) -> f64 {
}

fn <Vec<f64> as std::clone::Clone>::clone(_1: &Vec<f64>) -> Vec<f64> - shim(Vec<f64>) {
}

fn std::fmt::float::write(_1: f64) -> () {
}
"""


class Mir(unittest.TestCase):
    """Layer 3: the MIR scan, on a hand-written MIR file (the compiler half is proven by
    `--mir-selftest` in CI, a line of ci/fixture-checks.txt)."""

    def setUp(self):
        self.dir = tempfile.TemporaryDirectory()
        c = Path(self.dir.name) / "crates" / "fid-report"
        (c / "src" / "fmt").mkdir(parents=True)
        (c / "src" / "bind").mkdir()
        (c / "src" / "fmt" / "mod.rs").write_text("pub fn val(x: u64) -> f64 { f64::from_bits(x) }\npub fn prom_in_fmt() {}\n")
        (c / "src" / "bind" / "expand.rs").write_text("pub fn infer() {}\npub fn clean() {}\npub fn prom_elsewhere() {}\n")
        self.crate = c

    def tearDown(self):
        self.dir.cleanup()

    def test_items_and_names(self):
        names = [i.name for i in lint.mir_items(MIR)]
        self.assertEqual(names[:5], ["val", "fmt::<impl at crates/fid-report/src/fmt/model.rs:3:1: 3:7>::g",
                                     "<impl at crates/fid-report/src/bind/model.rs:4:1: 4:7>::half",
                                     "infer::{closure#0}", "clean"])
        self.assertIn("LIMIT", names)
        self.assertIn("bind::journal::read", names)

    def test_findings_are_attributed_and_fail_closed(self):
        found = lint.mir_findings(MIR, self.crate, "crates/fid-report", lint.MIR_ALLOWED["fid-report"])
        got = sorted(f.message.split(" (")[0] for f in found)
        self.assertEqual(got, ["<Count as dep::Measure>::measure", "<impl at crates/fid-report/src/bind/model.rs:4:1: 4:7>::half",
                               "<std::vec::Vec<u8> as bind::Tr>::m", "bind::journal::read", "infer::{closure#0}",
                               "nowhere", "prom_elsewhere::promoted[0]", "std::fmt::float::write"])
        self.assertTrue(all(f.rule == "RPT-FLOAT-MIR" for f in found))

    def test_only_shims_and_drop_glue_are_skipped(self):
        files = {"src/fmt/mod.rs": "pub struct S;\n"}
        self.assertIsNone(lint.mir_attribution("std::ptr::drop_in_place::<Vec<f64>>", files))
        self.assertIsNone(lint.mir_attribution("<Vec<f64> as std::clone::Clone>::clone", files,
                                               "fn <Vec<f64> as std::clone::Clone>::clone(_1: &Vec<f64>) -> Vec<f64> - shim(Vec<f64>) {"))
        # A `std::`/`core::`/`alloc::` prefix alone is no longer a reason to skip.
        for name in ["<std::vec::Vec<u8> as bind::Tr>::m", "<core::option::Option<u8> as fmt::T>::m",
                     "<alloc::string::String as Tr>::m", "std::fmt::float::write"]:
            where = lint.mir_attribution(name, files, f"fn {name}() -> f64 {{")
            self.assertIsNotNone(where, name)
            self.assertFalse(where and all(lint._allowed(w, "", ("src/fmt/",)) for w in where), name)
        # A qualified path is allowed only when both its self type and its trait are.
        self.assertEqual(lint.mir_attribution("<fmt::S as fmt::T>::m", files), ["mod:fmt", "mod:fmt"])
        self.assertEqual(lint.mir_fn_name("<std::vec::Vec<u8> as bind::Tr>::m"), "m")
        self.assertEqual(lint.mir_fn_name("f::{closure#0}"), "f")
        self.assertEqual(lint.mir_fn_name("<impl at src/lib.rs:1:1: 1:9>::g"), "g")

    def test_a_qualified_path_with_an_unresolved_part_fails_closed(self):
        files = {"src/fmt/mod.rs": "pub trait Tr {}\n"}
        for name in ["<f64 as fmt::Tr>::m", "<u32 as Tr>::m"]:
            where = lint.mir_attribution(name, files, f"fn {name}(_1: &u32) -> f64 {{")
            self.assertEqual(where, [], name)
        mir = "fn <f64 as fmt::Tr>::m(_1: &f64) -> f64 {\n}\n\nfn <u32 as Tr>::m(_1: &u32) -> f64 {\n}\n"
        found = lint.mir_findings(mir, self.crate, "crates/fid-report", lint.MIR_ALLOWED["fid-report"])
        self.assertEqual(sorted(f.message.split(" (")[0] for f in found), ["<f64 as fmt::Tr>::m", "<u32 as Tr>::m"])

    def test_drop_glue_and_shims_match_exactly(self):
        self.assertTrue(lint.is_shim("fn std::ptr::drop_in_place::<Vec<f64>>(_1: *mut Vec<f64>) -> () {"))
        self.assertTrue(lint.is_shim("fn core::ptr::drop_in_place::<W>(_1: *mut W) -> () {"))
        self.assertTrue(lint.is_shim("fn <W as Clone>::clone(_1: &W) -> W - shim(W) {"))
        for h in ["fn drop_in_place_all() -> f64 {", "fn bind::drop_in_place::<W>(_1: *mut W) -> f64 {",
                  "fn shim_maker() -> f64 {", "fn f(_1: u8) -> f64 { // - shim(x) later"]:
            self.assertFalse(lint.is_shim(h), h)
        mir = "fn drop_in_place_all() -> f64 {\n}\n"
        (self.crate / "src" / "bind" / "drop.rs").write_text("pub fn drop_in_place_all() {}\n")
        found = lint.mir_findings(mir, self.crate, "crates/fid-report", lint.MIR_ALLOWED["fid-report"])
        self.assertEqual([f.message.split(" (")[0] for f in found], ["drop_in_place_all"])

    def test_no_shipped_manifest_enables_test_interpretation(self):
        # Only fid-report's own dev-dependency may turn on the test-only status hook.
        hits = []
        for m in sorted((ROOT / "crates").glob("*/Cargo.toml")):
            data = tomllib.loads(m.read_text(encoding="utf-8"))
            for section in ("dependencies", "build-dependencies"):
                for name, spec in data.get(section, {}).items():
                    if isinstance(spec, dict) and "test-interpretation" in spec.get("features", []):
                        hits.append(f"{m.parent.name} [{section}] {name}")
            for tgt in data.get("target", {}).values():
                for section in ("dependencies", "build-dependencies"):
                    for name, spec in tgt.get(section, {}).items():
                        if isinstance(spec, dict) and "test-interpretation" in spec.get("features", []):
                            hits.append(f"{m.parent.name} [target.{section}] {name}")
            for feat, deps in data.get("features", {}).items():
                if any("test-interpretation" in d for d in deps):
                    hits.append(f"{m.parent.name} [features] {feat}")
        self.assertEqual(hits, [])
        dev = tomllib.loads((ROOT / "crates" / "fid-report" / "Cargo.toml").read_text(encoding="utf-8"))
        self.assertIn("test-interpretation", dev["dev-dependencies"]["fid-report"]["features"])

    def test_selftest_fixture_has_trait_impls_outside_fmt(self):
        src = (ROOT / lint.MIR_FIXTURE / "src" / "lib.rs").read_text(encoding="utf-8")
        self.assertRegex(src, r"impl Measure for Count \{\n    fn measure\(&self\) -> f64 \{ // mir")
        self.assertRegex(src, r"impl LocalTrait for Vec<u8> \{\n    fn local_trait_on_foreign_type\([^\n]*// mir")

    def test_mir_emission_is_pinned_to_the_dev_profile(self):
        text = (ROOT / "tools" / "rpt_float_lint.py").read_text(encoding="utf-8")
        self.assertIn('"--lib", "--profile", "dev", "--all-features"', text)

    def test_a_crate_without_allowed_files_reports_every_float(self):
        found = lint.mir_findings(MIR, self.crate, "crates/fid-report", ())
        self.assertIn("val", [f.message.split(" (")[0] for f in found])


class Specs(unittest.TestCase):
    def test_bad_spec(self):
        for fmt in ["{:.3}", "{x:>8.2}", "{:.*}", "{:e}", "{0:E}", "a {:+.1} b"]:
            self.assertIsNotNone(lint.bad_spec(fmt), fmt)
        for fmt in ["{}", "{:?}", "{:#?}", "{:>8}", "{:08}", "{:#x}", "{{:.3}}", "{name}"]:
            self.assertIsNone(lint.bad_spec(fmt), fmt)


if __name__ == "__main__":
    unittest.main()
