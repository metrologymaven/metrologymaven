//! Acceptance criteria of spec node CORE.REG.DEF.FIELD (AC-1 .. AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_registry::{command_def_schema, CommandDef, Network, Projection, SideEffect};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(rel: &str) -> CValue {
    parse(&std::fs::read(root().join("fixtures/core/reg/def/field").join(rel)).unwrap()).unwrap()
}

/// AC-1: every valid definition parses and round-trips equal; one per side-effect class.
#[test]
fn ac1_valid_round_trip() {
    let CValue::Arr(defs) = fixture("defs_valid.json") else { panic!("array expected") };
    let mut classes = Vec::new();
    for (i, v) in defs.into_iter().enumerate() {
        let def = CommandDef::from_value(v).unwrap_or_else(|e| panic!("definition {i}: {e}"));
        let again = CommandDef::from_value(def.to_value()).unwrap();
        assert_eq!(again, def, "definition {i} must round-trip");
        classes.push(def.side_effect);
    }
    for c in SideEffect::ALL {
        assert!(classes.contains(c), "no fixture for side effect {c}");
    }
}

/// AC-2: each invalid file fails with the code in its `expect` field.
#[test]
fn ac2_invalid_cases() {
    let dir = root().join("fixtures/core/reg/def/field/defs_invalid");
    let mut names: Vec<_> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    names.sort();
    assert_eq!(names.len(), 8);
    for p in names {
        let CValue::Obj(mut o) = parse(&std::fs::read(&p).unwrap()).unwrap() else { panic!() };
        let Some(CValue::Str(expect)) = o.remove("expect") else { panic!("{p:?}: no expect") };
        let def = o.remove("def").unwrap();
        let err = CommandDef::from_value(def).unwrap_err();
        assert_eq!(err.code(), expect, "{p:?}: {err}");
    }
}

fn base() -> CommandDef {
    let CValue::Arr(defs) = fixture("defs_valid.json") else { panic!() };
    defs.into_iter().map(|v| CommandDef::from_value(v).unwrap()).find(|d| d.side_effect == SideEffect::W2).unwrap()
}

/// AC-3: human_only caps the MCP projection at request_only.
#[test]
fn ac3_effective_projection() {
    let mut d = base();
    d.human_only = true;
    d.mcp_projection = Projection::Exposed;
    assert_eq!(d.effective_projection(), Projection::RequestOnly);
    d.mcp_projection = Projection::Never;
    assert_eq!(d.effective_projection(), Projection::Never);
    d.human_only = false;
    d.mcp_projection = Projection::Exposed;
    assert_eq!(d.effective_projection(), Projection::Exposed);
}

/// AC-4: absent network, human_only and cli_bare take their defaults.
#[test]
fn ac4_defaults() {
    let CValue::Obj(mut o) = base().to_value() else { panic!() };
    o.remove("network");
    o.remove("human_only");
    o.remove("cli_bare");
    let d = CommandDef::from_value(CValue::Obj(o)).unwrap();
    assert_eq!((d.network, d.human_only, d.cli_bare), (Network::None, false, false));
}

/// AC-5 / REQ-010: the checked-in schema is generated from the Rust types.
#[test]
fn ac5_schema_is_generated() {
    let mut want = to_canonical_bytes(&command_def_schema()).unwrap();
    want.push(b'\n');
    let have = std::fs::read(root().join("schema/command-def.schema.json")).unwrap_or_default();
    assert!(
        have == want,
        "schema/command-def.schema.json is out of date. Replace its contents with:\n{}",
        String::from_utf8_lossy(&want)
    );
}

#[test]
fn bad_types_and_ids() {
    let mut o = match base().to_value() {
        CValue::Obj(o) => o,
        _ => panic!(),
    };
    o.insert("id".into(), CValue::Str("Align.Run".into()));
    assert_eq!(CommandDef::from_value(CValue::Obj(o.clone())).unwrap_err().code(), "bad_id");
    o.insert("id".into(), CValue::Str("align.run".into()));
    o.insert("algorithms".into(), CValue::Arr(vec![CValue::Str("fit.cylinder@1".into())]));
    assert_eq!(CommandDef::from_value(CValue::Obj(o.clone())).unwrap_err().code(), "bad_algorithm");
    o.insert("algorithms".into(), CValue::Arr(vec![]));
    o.insert("major".into(), CValue::Num(1.5));
    assert_eq!(CommandDef::from_value(CValue::Obj(o)).unwrap_err().code(), "bad_type");
}
