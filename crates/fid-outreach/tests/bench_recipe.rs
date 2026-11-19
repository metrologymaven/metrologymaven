//! Acceptance criteria of spec node OUTR.BENCH.RUN.RECIPE (AC-1 .. AC-7) and further rules.
//! Fixtures under `fixtures/outr/bench/recipe/` are written by `make_fixtures.py` (digests and the
//! expected `ok` report computed in Python); every expected finding below is hand-labelled from the
//! spec's rules, not produced by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_outreach::bench::recipe::{
    check, default_registry, parse_catalogue, register, Code, Finding, KindValidator, RecipeContext, Registry,
};
use std::path::PathBuf;
use std::process::Command;

fn fixture(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/outr/bench/recipe").join(rel)
}

/// Runs `fid-outreach bench-recipe check --catalogue catalogue.toml <args> --json` inside the
/// case folder; returns (exit code, stdout, stderr).
fn cli(case: &str, args: &[&str], json: bool) -> (i32, String, String) {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-outreach"));
    cmd.current_dir(fixture(case)).args(["bench-recipe", "check", "--catalogue", "catalogue.toml"]).args(args);
    if json {
        cmd.arg("--json");
    }
    let out = cmd.output().unwrap();
    (
        out.status.code().unwrap(),
        String::from_utf8(out.stdout).unwrap(),
        String::from_utf8(out.stderr).unwrap(),
    )
}

#[test]
fn ac1_ok_folder_lists_four_valid_recipes() {
    let (code, out, err) = cli("ok", &["recipes"], true);
    assert_eq!(code, 0, "{out}{err}");
    let want = std::fs::read_to_string(fixture("expected/ok.json")).unwrap();
    assert_eq!(out, want);
    let (code, text, _) = cli("ok", &["recipes"], false);
    assert_eq!(code, 0);
    assert_eq!(text.lines().filter(|l| l.starts_with("recipe ")).count(), 4, "{text}");
    for kind in ["accuracy", "fidelity", "performance", "comparison"] {
        assert!(text.contains(&format!(" {kind} recipes/{kind}.toml")), "{kind}: {text}");
    }
}

#[test]
fn ac2_missing_tolerance_is_field_malformed_naming_recipe_and_field() {
    let (code, out, _) = cli("missing-tolerance", &["recipes"], true);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\"code\":\"field_malformed\""), "{out}");
    assert!(out.contains("\"field\":\"fields.max_abs_error.tolerance\""), "{out}");
    assert!(out.contains("\"id\":\"fixture.missing-tolerance\""), "{out}");
    assert!(out.contains("\"recipes\":[]"), "{out}");
    let (_, text, _) = cli("missing-tolerance", &["recipes"], false);
    assert!(
        text.contains("field_malformed recipes/recipe.toml fixture.missing-tolerance fields.max_abs_error.tolerance"),
        "{text}"
    );
}

#[test]
fn ac3_bad_digest_is_dataset_digest_mismatch() {
    let (code, out, _) = cli("bad-digest", &["recipes"], true);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\"code\":\"dataset_digest_mismatch\""), "{out}");
    assert!(out.contains("\"field\":\"dataset.sha256\""), "{out}");
    assert_eq!(out.matches("\"code\":").count(), 1, "only the digest is wrong: {out}");
}

#[test]
fn ac4_other_program_is_source_invalid() {
    let (code, out, _) = cli("other-program", &["recipes"], true);
    assert_eq!(code, 1, "{out}");
    assert!(out.contains("\"code\":\"source_invalid\""), "{out}");
    assert!(out.contains("\"field\":\"source.argv\""), "{out}");
    assert!(out.contains("python"), "{out}");
    assert_eq!(out.matches("\"code\":").count(), 1, "{out}");
}

#[test]
fn ac5_duplicate_id_names_both_files() {
    let (code, out, _) = cli("dup-id", &["recipes"], false);
    assert_eq!(code, 1, "{out}");
    let dup: Vec<&str> = out.lines().filter(|l| l.starts_with("duplicate_id ")).collect();
    assert_eq!(dup.len(), 2, "{out}");
    assert!(dup[0].starts_with("duplicate_id recipes/a.toml fixture.same-id"), "{out}");
    assert!(dup[1].starts_with("duplicate_id recipes/b.toml fixture.same-id"), "{out}");
    for line in dup {
        assert!(line.contains("recipes/a.toml, recipes/b.toml"), "{line}");
    }
    assert!(!out.lines().any(|l| l.starts_with("recipe ")), "no recipe is valid: {out}");
}

#[test]
fn ac6_unparsable_exits_2_naming_the_file() {
    let (code, out, err) = cli("unparsable", &["recipes"], false);
    assert_eq!(code, 2, "{out}{err}");
    assert!(err.contains("recipes/broken.toml"), "{err}");
    let (code, out, _) = cli("unparsable", &["recipes"], true);
    assert_eq!(code, 2);
    assert!(out.contains("\"status\":\"error\"") && out.contains("\"file\":\"recipes/broken.toml\""), "{out}");
}

#[test]
fn ac7_report_is_byte_identical_whatever_the_argument_order() {
    let a = cli("dup-id", &["recipes/a.toml", "recipes/b.toml"], true);
    let b = cli("dup-id", &["recipes/b.toml", "recipes/a.toml"], true);
    let c = cli("dup-id", &["recipes"], true);
    assert_eq!(a, b);
    assert_eq!(a, c);
    let files = ["recipes/performance.toml", "recipes/fidelity.toml", "recipes/comparison.toml", "recipes/accuracy.toml"];
    let mut rev = files;
    rev.reverse();
    let x = cli("ok", &files, true);
    let y = cli("ok", &rev, true);
    assert_eq!(x, y);
    assert_eq!(x.1, std::fs::read_to_string(fixture("expected/ok.json")).unwrap());
}

#[test]
fn bad_usage_and_unreadable_inputs_exit_2() {
    assert_eq!(cli("ok", &[], false).0, 2, "no paths");
    assert_eq!(cli("ok", &["recipes", "--bogus"], false).0, 2);
    assert_eq!(cli("ok", &["no-such-folder"], false).0, 2);
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-outreach"));
    let out = cmd
        .current_dir(fixture("ok"))
        .args(["bench-recipe", "check", "--catalogue", "recipes/../no-such.toml", "recipes"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    // A recipe used as catalogue: its `dataset` is a table, not a [[dataset]] array.
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_fid-outreach"));
    let out = cmd
        .current_dir(fixture("ok"))
        .args(["bench-recipe", "check", "--catalogue", "recipes/accuracy.toml", "recipes", "--json"])
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
    assert!(String::from_utf8(out.stdout).unwrap().contains("\"file\":\"recipes/accuracy.toml\""));
}

// ---- In-memory cases: every expected finding hand-labelled from the REQs. ----

const DIGEST: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

const BASE: &str = r#"schema = "bench-recipe@1"
id = "demo.case"
kind = "accuracy"
title = "Demo"
since_release = "v0.2.0"
machine_profile = "minimum"
dataset = { id = "demo-data", sha256 = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa" }

[source]
type = "release_numbers"
selector = "/results/demo.case"
journal_selector = "/journals/demo.case"

[[fields]]
name = "err"
unit = "um"
better = "lower"
tolerance = "1.5"
tolerance_kind = "abs"

[accuracy]
"#;

fn run_with(text: &str, registry: &Registry) -> Vec<(Code, Option<String>)> {
    let cat = parse_catalogue(&format!("[[dataset]]\nid = \"demo-data\"\nsha256 = \"sha256:{DIGEST}\"\n"), "c").unwrap();
    let r = check(&cat, &[("r.toml".to_string(), text.to_string())], registry).unwrap();
    if r.findings.is_empty() {
        assert_eq!(r.recipes.len(), 1);
    } else {
        assert!(r.recipes.is_empty());
    }
    r.findings.into_iter().map(|f| (f.code, f.field)).collect()
}

fn run(text: &str) -> Vec<(Code, Option<String>)> {
    run_with(text, &default_registry())
}

fn one(code: Code, field: &str) -> Vec<(Code, Option<String>)> {
    vec![(code, Some(field.to_string()))]
}

#[test]
fn base_recipe_is_valid_with_bare_or_prefixed_digest() {
    assert_eq!(run(BASE), vec![]);
    let prefixed = BASE.replace("sha256 = \"aaaa", "sha256 = \"sha256:aaaa");
    assert_eq!(run(&prefixed), vec![]);
}

#[test]
fn req001_every_missing_common_field_is_named() {
    let mut got = run("[accuracy]\n");
    let mut want: Vec<(Code, Option<String>)> =
        ["dataset", "fields", "id", "kind", "machine_profile", "schema", "since_release", "source", "title"]
            .iter()
            .map(|f| (Code::FieldMissing, Some(f.to_string())))
            .collect();
    // [accuracy] without a kind: the section does not match kind "".
    want.push((Code::FieldMalformed, Some("accuracy".to_string())));
    got.sort();
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn req001_malformed_common_fields() {
    assert_eq!(run(&BASE.replace("bench-recipe@1", "bench-recipe@2")), one(Code::FieldMalformed, "schema"));
    assert_eq!(run(&BASE.replace("\"minimum\"", "\"fast\"")), one(Code::FieldMalformed, "machine_profile"));
    assert_eq!(run(&BASE.replace("\"v0.2.0\"", "\"\"")), one(Code::FieldMalformed, "since_release"));
    assert_eq!(run(&BASE.replace("title = \"Demo\"", "title = 3")), one(Code::FieldMalformed, "title"));
    assert_eq!(run(&BASE.replace("\"aaaaaaaa", "\"AAAAAAAA")), one(Code::FieldMalformed, "dataset.sha256"));
    // Keys inside the kind section belong to the kind validator (a stub here).
    assert_eq!(run(&format!("{BASE}extra = 1\n")), vec![]);
    assert_eq!(run(&format!("extra = 1\n{BASE}")), one(Code::FieldMalformed, "extra"));
}

#[test]
fn req002_unknown_dataset() {
    assert_eq!(run(&BASE.replace("\"demo-data\"", "\"other-data\"")), one(Code::DatasetUnknown, "dataset.id"));
    let other = BASE.replace(DIGEST, &"b".repeat(64));
    assert_eq!(run(&other), one(Code::DatasetDigestMismatch, "dataset.sha256"));
}

#[test]
fn req003_source_types_and_programs() {
    let release = "type = \"release_numbers\"\nselector = \"/results/demo.case\"\njournal_selector = \"/journals/demo.case\"\n";
    let command = |argv: &str, outputs: &str| {
        BASE.replace(release, &format!("type = \"command\"\nargv = {argv}\noutputs = {outputs}\n"))
    };
    assert_eq!(run(&command("[\"fid\", \"x\"]", "{ err = \"/err\" }")), vec![]);
    assert_eq!(run(&command("[\"fid-verify\"]", "{ err = \"/a~1b\" }")), vec![]);
    assert_eq!(run(&command("[\"/usr/bin/fid\"]", "{ err = \"/err\" }")), one(Code::SourceInvalid, "source.argv"));
    assert_eq!(run(&command("[\"fid.exe\"]", "{ err = \"/err\" }")), one(Code::SourceInvalid, "source.argv"));
    assert_eq!(run(&command("[]", "{ err = \"/err\" }")), one(Code::SourceInvalid, "source.argv"));
    assert_eq!(run(&command("[\"fid\"]", "{ err = \"err\" }")), one(Code::SourceInvalid, "source.outputs.err"));
    assert_eq!(run(&command("[\"fid\"]", "{ err = \"/a~2\" }")), one(Code::SourceInvalid, "source.outputs.err"));
    assert_eq!(run(&command("[\"fid\"]", "{}")), one(Code::SourceInvalid, "source.outputs.err"));
    assert_eq!(
        run(&command("[\"fid\"]", "{ err = \"/e\", other = \"/o\" }")),
        one(Code::SourceInvalid, "source.outputs.other")
    );
    assert_eq!(run(&BASE.replace("release_numbers", "shell")), one(Code::SourceInvalid, "source.type"));
    assert_eq!(
        run(&BASE.replace("selector = \"/results/demo.case\"\n", "")),
        one(Code::SourceInvalid, "source.selector")
    );
    assert_eq!(
        run(&BASE.replace("journal_selector = \"/journals/demo.case\"", "journal_selector = \"journals\"")),
        one(Code::SourceInvalid, "source.journal_selector")
    );
    assert_eq!(
        run(&BASE.replace("type = \"release_numbers\"\n", "type = \"release_numbers\"\nargv = [\"fid\"]\n")),
        one(Code::SourceInvalid, "source.argv")
    );
}

#[test]
fn req004_tolerance_and_direction() {
    for good in ["0", "12", "0.001", "10.50"] {
        assert_eq!(run(&BASE.replace("\"1.5\"", &format!("\"{good}\""))), vec![], "{good}");
    }
    for bad in ["-1", "1e-3", ".5", "5.", "1,5", " 1", ""] {
        assert_eq!(
            run(&BASE.replace("\"1.5\"", &format!("\"{bad}\""))),
            one(Code::FieldMalformed, "fields.err.tolerance"),
            "{bad}"
        );
    }
    assert_eq!(run(&BASE.replace("tolerance = \"1.5\"", "tolerance = 1.5")), one(Code::FieldMalformed, "fields.err.tolerance"));
    assert_eq!(run(&BASE.replace("\"abs\"", "\"pct\"")), one(Code::FieldMalformed, "fields.err.tolerance_kind"));
    for good in ["higher", "equal"] {
        assert_eq!(run(&BASE.replace("\"lower\"", &format!("\"{good}\""))), vec![]);
    }
    assert_eq!(run(&BASE.replace("\"lower\"", "\"smaller\"")), one(Code::FieldMalformed, "fields.err.better"));
    assert_eq!(run(&BASE.replace("\"um\"", "\"furlong\"")), one(Code::FieldMalformed, "fields.err.unit"));
    assert_eq!(run(&BASE.replace("name = \"err\"\n", "")).len(), 1);
    assert_eq!(run(&BASE.replace("name = \"err\"\n", "")), one(Code::FieldMalformed, "fields.#1.name"));
    let no_fields = BASE.replace("[[fields]]\nname = \"err\"\nunit = \"um\"\nbetter = \"lower\"\ntolerance = \"1.5\"\ntolerance_kind = \"abs\"\n", "");
    assert_eq!(run(&no_fields), one(Code::FieldMissing, "fields"));
    let twice = BASE.replace("[accuracy]", "[[fields]]\nname = \"err\"\nunit = \"1\"\nbetter = \"equal\"\ntolerance = \"0\"\ntolerance_kind = \"abs\"\n\n[accuracy]");
    assert_eq!(run(&twice), one(Code::FieldMalformed, "fields.err.name"));
}

#[test]
fn req005_kinds_and_sections() {
    assert_eq!(run(&BASE.replace("kind = \"accuracy\"", "kind = \"speed\"")), {
        let mut v = one(Code::FieldMalformed, "accuracy");
        v.push((Code::KindUnknown, Some("kind".to_string())));
        v.sort();
        v
    });
    assert_eq!(run(&BASE.replace("[accuracy]\n", "")), one(Code::KindSectionMissing, "accuracy"));
    assert_eq!(
        run(&format!("{BASE}\n[performance]\n")),
        one(Code::FieldMalformed, "performance"),
        "a second kind section"
    );
    let perf = BASE.replace("kind = \"accuracy\"", "kind = \"performance\"").replace("[accuracy]", "[performance]");
    assert_eq!(run(&perf), vec![]);
}

struct NeedsMethod;

impl KindValidator for NeedsMethod {
    fn kind(&self) -> &str {
        "accuracy"
    }

    fn validate(&self, section: &toml::Table, recipe: &RecipeContext<'_>) -> Vec<Finding> {
        if section.contains_key("method") {
            Vec::new()
        } else {
            vec![Finding::new(recipe, Code::FieldMissing, Some("accuracy.method"), "required by the accuracy kind")]
        }
    }
}

#[test]
fn req005_section_is_passed_to_the_registered_kind_validator() {
    let mut reg = default_registry();
    register(&mut reg, Box::new(NeedsMethod));
    assert_eq!(run_with(BASE, &reg), one(Code::FieldMissing, "accuracy.method"));
    assert_eq!(run_with(&format!("{BASE}method = \"plane\"\n"), &reg), vec![]);
    // A kind without a registered validator is unknown.
    let mut only_perf = Registry::new();
    register(&mut only_perf, Box::new(fid_outreach::bench::recipe::StubValidator("performance")));
    assert_eq!(run_with(BASE, &only_perf), one(Code::KindUnknown, "kind"));
}

#[test]
fn req006_id_pattern() {
    for good in ["a.b", "demo.case-1", "x1-y.z.w-2"] {
        assert_eq!(run(&BASE.replace("\"demo.case\"\nkind", &format!("\"{good}\"\nkind"))), vec![], "{good}");
    }
    for bad in ["demo", "Demo.case", "1demo.case", "demo..case", "demo.case.", "demo_x.case", "-a.b"] {
        assert_eq!(
            run(&BASE.replace("\"demo.case\"\nkind", &format!("\"{bad}\"\nkind"))),
            one(Code::IdMalformed, "id"),
            "{bad}"
        );
    }
}
