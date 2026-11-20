//! Descriptor registry rules and the statement-class list of spec leaf IO.DMIS.PARSE.TREE
//! (AC-4, REQ-005). The expected class list of the test registry comes from
//! `fixtures/io/dmis/parse/tree/make_expected.py`, an independent Python reference.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_dmis::registry::{
    LabelKind, Registry, RegistryError, Shape, StatementDescriptor, CLASSES_FILE, MAX_PROGRAM_LEN, MAX_SHAPE_DEPTH,
};
use std::path::{Path, PathBuf};

fn repo() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn d(class: &str, major: &str, label: LabelKind, shape: Shape) -> StatementDescriptor {
    StatementDescriptor::new(class, major, label, shape, "TEST", "P1a")
}

/// The same eight classes as `REGISTRY` in make_expected.py (shapes do not appear in the list).
fn test_registry() -> Registry {
    let any = || Shape::AnyField;
    let req = |p: &str| LabelKind::Required(p.into());
    Registry::from_descriptors([
        d("feat_point", "FEAT", req("F"), Shape::kw("POINT")),
        d("feat_circle", "FEAT", req("F"), Shape::kw("CIRCLE")),
        d("tol_diam", "TOL", req("T"), any()),
        d("units", "UNITS", LabelKind::None, any()),
        d("text", "TEXT", LabelKind::None, any()),
        d("output", "OUTPUT", LabelKind::None, any()),
        d("nest", "NESTT", LabelKind::Optional("V".into()), any()),
        d("endfil", "ENDFIL", LabelKind::None, Shape::seq(vec![])),
    ])
    .unwrap()
}

#[test]
fn ac4_class_list_is_generated_byte_identically() {
    let r = test_registry();
    let a = r.classes_json().unwrap();
    let b = test_registry().classes_json().unwrap();
    assert_eq!(a, b);
    let want = std::fs::read(repo().join("fixtures/io/dmis/parse/tree/classes.json")).unwrap();
    assert_eq!(String::from_utf8(a).unwrap(), String::from_utf8(want).unwrap());
}

#[test]
fn class_list_does_not_depend_on_registration_order() {
    let mut list: Vec<StatementDescriptor> = test_registry().descriptors().into_iter().cloned().collect();
    list.reverse();
    let reversed = Registry::from_descriptors(list).unwrap();
    assert_eq!(reversed.classes_json().unwrap(), test_registry().classes_json().unwrap());
}

/// REQ-005: CI fails if the committed list differs from the built-in registry's.
/// Regenerate with `cargo run -p fid-io-dmis --bin dmis-classes -- --out docs/dmis/dmis-statement-classes.json`.
#[test]
fn committed_file_is_fresh() {
    let want = Registry::builtin().unwrap().classes_json().unwrap();
    let got = std::fs::read(repo().join("docs/dmis").join(CLASSES_FILE)).unwrap();
    assert_eq!(String::from_utf8(got).unwrap(), String::from_utf8(want).unwrap());
}

#[test]
fn duplicate_class_is_rejected() {
    let mut r = test_registry();
    let err = r.add(d("units", "UNITSX", LabelKind::None, Shape::AnyField)).unwrap_err();
    assert_eq!(err, RegistryError::DuplicateClass { class: "units".into() });
    assert_eq!(err.code(), "duplicate_class");
}

#[test]
fn duplicate_descriptor_is_rejected() {
    let mut r = test_registry();
    let err = r.add(d("feat_point2", "feat", LabelKind::Required("f".into()), Shape::kw("point"))).unwrap_err();
    assert_eq!(err, RegistryError::DuplicateDescriptor { class: "feat_point2".into(), existing: "feat_point".into() });
    // The same shape with another label rule is a different descriptor.
    r.add(d("feat_point3", "FEAT", LabelKind::None, Shape::kw("POINT"))).unwrap();
}

fn invalid(desc: StatementDescriptor) -> &'static str {
    match Registry::new().add(desc) {
        Err(RegistryError::InvalidDescriptor { reason, .. }) => reason,
        other => panic!("{other:?}"),
    }
}

#[test]
fn invalid_descriptors_are_rejected() {
    let none = || LabelKind::None;
    assert_eq!(invalid(d("Feat", "FEAT", none(), Shape::AnyField)), "bad_class_id");
    assert_eq!(invalid(d("a", "1A", none(), Shape::AnyField)), "bad_major_word");
    assert_eq!(invalid(d("a", "A", LabelKind::Required("F1".into()), Shape::AnyField)), "bad_label_prefix");
    assert_eq!(invalid(StatementDescriptor::new("a", "A", none(), Shape::AnyField, "", "P1a")), "bad_family_or_phase");
    assert_eq!(invalid(d("a", "A", none(), Shape::Keyword("a b".into()))), "bad_keyword");
    assert_eq!(invalid(d("a", "A", none(), Shape::alt(vec![]))), "empty_alternative");
    assert_eq!(invalid(d("a", "A", none(), Shape::repeat(Shape::Number, 3, Some(2)))), "bad_repeat_bounds");
    assert_eq!(invalid(d("a", "A", none(), Shape::repeat(Shape::opt(Shape::Number), 0, None))), "nullable_repeat");
    assert_eq!(invalid(d("a", "A", none(), Shape::repeat(Shape::seq(vec![]), 1, Some(2)))), "nullable_repeat");
    let big = u32::try_from(MAX_PROGRAM_LEN).unwrap() + 1;
    assert_eq!(invalid(d("a", "A", none(), Shape::repeat(Shape::Number, big, Some(big)))), "program_too_long");
    let mut deep = Shape::Number;
    for _ in 0..=MAX_SHAPE_DEPTH {
        deep = Shape::opt(deep);
    }
    assert_eq!(invalid(d("a", "A", none(), deep)), "shape_too_deep");
}

#[test]
fn registry_knows_its_majors() {
    let r = test_registry();
    assert!(r.knows("FEAT"));
    assert!(!r.knows("FOO"));
    assert_eq!(r.len(), 8);
    assert_eq!(Registry::builtin().unwrap().len(), fid_io_dmis::gdef::descriptors().len());
}
