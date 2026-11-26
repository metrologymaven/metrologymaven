//! Acceptance criteria of spec node AUTO.PROG.CHECK. Programs and expected results come from
//! `fixtures/program/check/make_fixtures.py` (the other modules' diagnostics hand-labelled from
//! their spec rules, the checker's own part from an independent Python model of its rules);
//! nothing here derives an expected value from this crate. The extra cases below are
//! hand-labelled from the rules of spec/auto/prog/check/NODE.md.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::BusError;
use fid_bus::unit::{normalise_value, Normalised};
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{digest_bytes, parse, to_canonical_bytes, CValue};
use fid_program::check::{command_def, BAD_ARGUMENTS, CHECK_COMMAND, DEPRECATED_COMMAND, UNKNOWN_COMMAND};
use fid_program::{
    check_in_force, check_program, class_rank, CheckDiag, CheckResult, Module, Program, Severity, StepKind, Uuid7,
    CLASS_ORDER,
};
use fid_registry::query::Audience;
use fid_registry::{validate, Catalogue, CommandDecl, CommandRef, Manifest, ManifestInForce};
use fid_types::{Digest, SideEffect};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn dir() -> PathBuf {
    root().join("fixtures/program/check")
}

fn expected_top() -> BTreeMap<String, CValue> {
    let CValue::Obj(m) = parse(&std::fs::read(dir().join("expected.json")).unwrap()).unwrap() else {
        panic!("expected.json root")
    };
    m
}

fn path_of(key: &str) -> PathBuf {
    let CValue::Str(rel) = expected_top().get(key).cloned().unwrap() else { panic!("{key}") };
    root().join(rel)
}

fn cases() -> BTreeMap<String, CValue> {
    let Some(CValue::Obj(c)) = expected_top().get("cases").cloned() else { panic!("cases") };
    c
}

fn catalogue() -> Catalogue {
    Catalogue::from_json(&std::fs::read(path_of("catalogue")).unwrap()).unwrap()
}

fn manifest_bytes() -> Vec<u8> {
    std::fs::read(path_of("manifest")).unwrap()
}

fn manifest() -> Manifest {
    Manifest::from_json(&manifest_bytes()).unwrap()
}

/// SHA-256 of the manifest file, which is canonical (an input, not an expected value).
fn manifest_digest() -> Digest {
    Digest::from_bytes(digest_bytes(&manifest_bytes()).0)
}

fn program_bytes(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(format!("{name}.json"))).unwrap()
}

fn run_with(name: &str, m: &Manifest) -> CheckResult {
    check_program(&program_bytes(name), m, manifest_digest(), &catalogue())
}

fn run(name: &str) -> CheckResult {
    run_with(name, &manifest())
}

fn text(v: &CValue) -> String {
    String::from_utf8(to_canonical_bytes(v).unwrap()).unwrap()
}

fn assert_case(name: &str) {
    let want = cases().get(name).cloned().unwrap_or_else(|| panic!("no expected case {name}"));
    assert_eq!(text(&run(name).to_value()), text(&want), "{name}");
}

/// The step IDs of `main`, in order.
fn main_ids(name: &str) -> Vec<Uuid7> {
    Program::parse(&program_bytes(name)).unwrap().main.iter().map(|s| s.step_id).collect()
}

fn check_diags(r: &CheckResult) -> Vec<&CheckDiag> {
    r.diagnostics.iter().filter(|d| d.module == Module::Check).collect()
}

#[test]
fn fixture_inputs_agree() {
    assert_eq!(manifest().quantity_catalogue_digest, fid_registry::QuantityCatalogue::digest(&catalogue()));
    assert_eq!(cases().len(), 11);
}

/// AC-1: the clean program passes with no errors, class W2, per-step classes as in
/// `clean.expected.json`.
#[test]
fn ac1_clean_program_passes() {
    let r = run("clean");
    assert!(r.passed);
    assert!(r.diagnostics.iter().all(|d| d.severity != Severity::Error));
    assert_eq!(r.program_class, Some(SideEffect::W2));
    let want = parse(&std::fs::read(dir().join("clean.expected.json")).unwrap()).unwrap();
    assert_eq!(text(&r.to_value()), text(&want));
    assert_case("clean");
}

/// AC-2: two `unknown_command` diagnostics at the right steps, `passed: false`.
#[test]
fn ac2_unknown_commands() {
    let r = run("unknown-cmd");
    let ids = main_ids("unknown-cmd");
    let unknown: Vec<_> = r.diagnostics.iter().filter(|d| d.code == UNKNOWN_COMMAND).collect();
    assert_eq!(unknown.len(), 2);
    assert_eq!(unknown[0].step_id, Some(ids[0]));
    assert_eq!(unknown[0].names, ["demo.nope@1"]);
    assert_eq!(unknown[1].step_id, Some(ids[1]));
    assert_eq!(unknown[1].names, ["demo.set_offset@3"]);
    assert!(!r.passed);
    assert_case("unknown-cmd");
}

/// AC-3: one `bad_arguments` naming all three parameters.
#[test]
fn ac3_bad_arguments_name_every_parameter() {
    let r = run("bad-args");
    let bad: Vec<_> = r.diagnostics.iter().filter(|d| d.code == BAD_ARGUMENTS).collect();
    assert_eq!(bad.len(), 1);
    assert_eq!(bad[0].names, ["axis", "colour", "offset"]);
    assert!(!r.passed);
    assert_case("bad-args");
}

/// AC-4: diagnostics from the type, flow, operator and family checks, in document order.
#[test]
fn ac4_diagnostics_of_all_modules_in_order() {
    let r = run("mixed");
    let modules: Vec<Module> = r.diagnostics.iter().map(|d| d.module).collect();
    assert_eq!(modules, [Module::Family, Module::Oper, Module::Expr, Module::Flow]);
    assert_case("mixed");
}

/// AC-5: an M1 step makes the program M1 and is listed with class M1.
#[test]
fn ac5_motion_program_class() {
    let r = run("motion");
    assert_eq!(r.program_class, Some(SideEffect::M1));
    let ids = main_ids("motion");
    let step = r.steps.iter().find(|s| s.step_id == ids[0]).unwrap();
    assert_eq!(step.class, Some(SideEffect::M1));
    assert_eq!(r.steps[1].human_only, Some(true));
    assert_case("motion");
}

/// AC-6 (in part): the same inputs give byte-identical output, equal to the committed expected
/// bytes on every platform CI runs. The Record Journal half needs the command handler.
#[test]
fn ac6_output_is_byte_identical() {
    let a = to_canonical_bytes(&run("mixed").to_value()).unwrap();
    let b = to_canonical_bytes(&run("mixed").to_value()).unwrap();
    assert_eq!(a, b);
    assert_eq!(a, to_canonical_bytes(cases().get("mixed").unwrap()).unwrap());
}

/// The remaining fixture cases: deprecated majors, unreachable steps, literal schemas,
/// iteration limits once per loop, and a document that does not parse.
#[test]
fn other_fixture_cases() {
    for name in ["deprecated", "unreachable", "literals", "limits", "zero-limit", "unchecked"] {
        assert_case(name);
    }
}

/// REQ-001, D-035: a removed or hidden major, or one whose lifecycle state cannot be read, is
/// exactly an absent command: the FULL result (every module's diagnostics, the step classes, the
/// program class) is byte-identical to that of a manifest without the major, and its
/// `unknown_command` has the same message as a real miss.
#[test]
fn removed_hidden_and_unreadable_majors_are_absent() {
    let key = CommandRef { id: "demo.read_status".into(), major: 1 };
    let target = main_ids("clean")[2];
    let mut absent_manifest = manifest();
    absent_manifest.commands.remove(&key).unwrap();
    let absent = run_with("clean", &absent_manifest);
    for state in [CValue::Str("removed".into()), CValue::Str("hidden".into()), CValue::Str("gone".into()), CValue::Null]
    {
        let mut m = manifest();
        m.commands.get_mut(&key).unwrap().lifecycle.insert("state".into(), state.clone());
        let r = run_with("clean", &m);
        assert_eq!(text(&r.to_value()), text(&absent.to_value()), "{state:?}");
        let modules: Vec<Module> =
            r.diagnostics.iter().filter(|d| d.step_id == Some(target)).map(|d| d.module).collect();
        assert_eq!(modules, [Module::Expr, Module::Check], "{state:?}");
        let d = check_diags(&r);
        assert_eq!((d[0].code, d[0].step_id), (UNKNOWN_COMMAND, Some(target)));
        assert_eq!(d[0].message, "unknown_command: demo.read_status@1");
        assert_eq!(r.steps[2].class, None);
        assert_eq!(r.program_class, None);
        assert!(!r.passed);
    }
}

/// D-035: a hidden major is absent to an agent too (the agent view keeps hidden majors, the
/// checker does not): the agent's result equals the agent's result without the major.
#[test]
fn hidden_major_is_absent_in_the_agent_view() {
    let hide = |decls: Vec<CommandDecl>| -> Vec<CommandDecl> {
        decls
            .into_iter()
            .map(|d| {
                let CValue::Obj(mut m) = d.0 else { panic!("decl") };
                if m.get("id") == Some(&CValue::Str("demo.approve_plan".into())) {
                    m.insert(
                        "lifecycle".into(),
                        obj(&[
                            ("remove_after", CValue::Str("2027-12-01".into())),
                            ("since", CValue::Str("2026-02-01".into())),
                            ("state", CValue::Str("hidden".into())),
                        ]),
                    );
                }
                CommandDecl::from_value(CValue::Obj(m))
            })
            .collect()
    };
    let hidden = ManifestInForce::new(&hide(demo_decls()), catalogue()).unwrap();
    let without: Vec<CommandDecl> = demo_decls()
        .into_iter()
        .filter(|d| !matches!(&d.0, CValue::Obj(m) if m.get("id") == Some(&CValue::Str("demo.approve_plan".into()))))
        .collect();
    let gone = ManifestInForce::new(&without, catalogue()).unwrap();
    let bytes = program_bytes("motion");
    for audience in [Audience::Person, Audience::Agent] {
        let mut a = check_in_force(&bytes, &hidden, audience).unwrap();
        let b = check_in_force(&bytes, &gone, audience).unwrap();
        a.manifest_digest = b.manifest_digest;
        assert_eq!(text(&a.to_value()), text(&b.to_value()), "{audience:?}");
        assert_eq!(a.steps[1].class, None);
    }
}

/// REQ-002: a property schema that cannot be compiled on its own is `unchecked`, an error; a
/// literal is never passed without a check.
#[test]
fn uncompilable_property_schema_is_unchecked() {
    let mut m = manifest();
    let def = m.commands.get_mut(&CommandRef { id: "demo.measure".into(), major: 1 }).unwrap();
    let CValue::Obj(schema) = &mut def.params_schema else { panic!("schema") };
    let Some(CValue::Obj(props)) = schema.get_mut("properties") else { panic!("properties") };
    props.insert(
        "feature".into(),
        CValue::Obj(BTreeMap::from([("$ref".into(), CValue::Str("#/properties/samples".into()))])),
    );
    let r = run_with("motion", &m);
    let d = check_diags(&r);
    assert_eq!(d.len(), 1);
    assert_eq!(d[0].names, ["feature"]);
    assert_eq!(d[0].message, "bad_arguments: feature (unchecked)");
    assert!(!r.passed);
}

/// Entity references are checked for shape only: the `x-ref` literal of the type checker's
/// bore program (`points: "pc-17"`) is no `bad_arguments`.
#[test]
fn entity_reference_literals_are_shape_checked() {
    let bytes = std::fs::read(root().join("fixtures/program/types/valid/bore.json")).unwrap();
    let r = check_program(&bytes, &manifest(), manifest_digest(), &catalogue());
    let p = Program::parse(&bytes).unwrap();
    let fit = p
        .main
        .iter()
        .find(|s| matches!(&s.kind, StepKind::Invoke { command, .. } if command == "feature.fit_cylinder"))
        .unwrap();
    assert!(r.diagnostics.iter().all(|d| d.step_id != Some(fit.step_id) || d.module != Module::Check));
    assert!(r.steps.iter().any(|s| s.step_id == fit.step_id && s.class == Some(SideEffect::R1)));
}

/// REQ-004: the class order of the spec.
#[test]
fn class_order_is_the_spec_order() {
    let names: Vec<&str> = CLASS_ORDER.iter().map(|c| c.as_str()).collect();
    assert_eq!(names, ["R0", "R1", "I1", "W1", "W2", "X1", "D1", "W3", "M1", "S1", "A0"]);
    assert!(class_rank(SideEffect::M1) > class_rank(SideEffect::W3));
    assert!(class_rank(SideEffect::X1) < class_rank(SideEffect::D1));
    assert_eq!(SideEffect::ALL.len(), CLASS_ORDER.len());
}

/// REQ-005: the deprecation warning names the replacement and does not fail the check.
#[test]
fn deprecated_major_warns_with_replacement() {
    let r = run("deprecated");
    let d = check_diags(&r);
    assert_eq!(d.len(), 1);
    assert_eq!((d[0].code, d[0].severity), (DEPRECATED_COMMAND, Severity::Warning));
    assert_eq!(d[0].names, ["demo.set_offset@1", "demo.set_offset@2"]);
    assert!(r.passed);
}

// ------------------------------------------------------------------ D-035

/// The `demo.*` definitions of the fixture manifest as declarations.
fn demo_decls() -> Vec<CommandDecl> {
    let CValue::Obj(m) = parse(&manifest_bytes()).unwrap() else { panic!("manifest") };
    let Some(CValue::Arr(cmds)) = m.get("commands") else { panic!("commands") };
    cmds.iter()
        .filter_map(|c| {
            let CValue::Obj(mut d) = c.clone() else { return None };
            d.remove("definition_digest");
            matches!(d.get("id"), Some(CValue::Str(id)) if id.starts_with("demo."))
                .then(|| CommandDecl::from_value(CValue::Obj(d)))
        })
        .collect()
}

/// D-035: to an agent, a never-projected command (M1) does not exist. The agent gets the
/// agent view and its digest; the M1 step is `unknown_command` exactly as a real miss
/// (`type_unknown` then `unknown_command`, same message form), with no class and no schema
/// read. A person gets the full manifest and the M1 class.
#[test]
fn agent_audience_does_not_see_never_projected_commands() {
    let mif = ManifestInForce::new(&demo_decls(), catalogue()).unwrap();
    let bytes = program_bytes("motion");
    let move_step = main_ids("motion")[0];

    let person = check_in_force(&bytes, &mif, Audience::Person).unwrap();
    assert_eq!(person.manifest_digest, mif.digest());
    assert_eq!(person.program_class, Some(SideEffect::M1));
    assert!(check_diags(&person).is_empty());

    let agent = check_in_force(&bytes, &mif, Audience::Agent).unwrap();
    assert_eq!(agent.manifest_digest, mif.agent_view().digest);
    assert_ne!(agent.manifest_digest, mif.digest());
    assert_eq!(agent.program_class, None);
    assert_eq!(agent.steps[0].class, None);
    assert_eq!(agent.steps[0].human_only, None);
    let at: Vec<(Module, &str, String)> = agent
        .diagnostics
        .iter()
        .filter(|d| d.step_id == Some(move_step))
        .map(|d| (d.module, d.code, d.message.clone()))
        .collect();
    assert_eq!(
        at,
        [
            (Module::Expr, "type_unknown", "type_unknown at command: demo.move_axis@1".to_string()),
            (Module::Check, UNKNOWN_COMMAND, "unknown_command: demo.move_axis@1".to_string()),
        ]
    );
    // The same shape as a command that is absent for everyone (AC-2's demo.nope@1).
    let absent = run("unknown-cmd");
    let first = main_ids("unknown-cmd")[0];
    let shape: Vec<(Module, &str)> =
        absent.diagnostics.iter().filter(|d| d.step_id == Some(first)).map(|d| (d.module, d.code)).collect();
    assert_eq!(shape, at.iter().map(|(m, c, _)| (*m, *c)).collect::<Vec<_>>());
    // The other two steps are visible to the agent.
    assert_eq!(agent.steps[1].class, Some(SideEffect::W3));
    assert_eq!(agent.steps[2].class, Some(SideEffect::R1));
}

// ------------------------------------------------------------------ registry definition

struct FixtureHooks(Catalogue);

impl Hooks for FixtureHooks {
    fn literal(&self, kind: &str, value: &CValue) -> Result<Normalised, BusError> {
        normalise_value(value, kind, &self.0)
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn obj(pairs: &[(&str, CValue)]) -> CValue {
    CValue::Obj(pairs.iter().map(|(k, v)| (k.to_string(), v.clone())).collect())
}

/// The definition passes the registry's declaration validation, and its schemas compile in the
/// bus and accept and refuse what they should; every fixture result fits the result schema.
#[test]
fn command_definition_is_valid() {
    let def = command_def().unwrap();
    assert_eq!(format!("{}@{}", def.id, def.major), CHECK_COMMAND);
    assert_eq!(def.side_effect, SideEffect::R1);
    let errors = validate(&[CommandDecl::from_def(&def)]);
    assert!(errors.is_empty(), "{errors:?}");
    let hooks = FixtureHooks(catalogue());
    let params = Compiled::compile(&def.params_schema).unwrap();
    let s = |v: &str| CValue::Str(v.into());
    assert!(params.evaluate(&obj(&[("program", s("root:in/clean.json"))]), &hooks).is_ok());
    assert!(params.evaluate(&obj(&[("program_id", s("0190c000-0000-7000-8000-000000000101"))]), &hooks).is_ok());
    assert!(params.evaluate(&obj(&[]), &hooks).is_err());
    let both = obj(&[("program", s("root:in/clean.json")), ("program_id", s("0190c000-0000-7000-8000-000000000101"))]);
    assert!(params.evaluate(&both, &hooks).is_err());
    let result = Compiled::compile(&def.result_schema).unwrap();
    for (name, v) in cases() {
        if let Err(f) = result.evaluate(&v, &hooks) {
            panic!("{name}: {} at {} ({})", f.keyword, f.pointer, f.message);
        }
    }
    for name in ["mixed", "zero-limit", "unknown-cmd"] {
        assert!(result.evaluate(&run(name).to_value(), &hooks).is_ok(), "{name}");
    }
}
