//! Integration tests for spec node `IO.DMIS.DIAL` (AC-1..AC-5, REQ-IO.DMIS.DIAL-001..005,
//! `import.dmis_list_dialects@1` registry command validation, and 100-profile perf budget).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{digest_bytes, parse as parse_json, CValue};
use fid_io_dmis::dial::{
    list_dialects_command_def, parse_profile, DialectError, DialectOrigin, ProfileSet, SemVer, BASE_DMIS_53,
    LIST_DIALECTS_COMMAND, LIST_DIALECTS_MAJOR,
};
use fid_io_dmis::tree::{parse, Node};
use fid_registry::{Catalogue, CommandDecl};
use fid_types::{Determinism, Digest, SideEffect};
use std::path::PathBuf;
use std::time::Instant;

fn fix_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/dmis/dial")
}

#[test]
fn ac1_list_dialects_reports_test_vendor_bundled_sorted() {
    let mut set = ProfileSet::new();
    set.load_file(&fix_dir().join("test-vendor.toml"), DialectOrigin::Bundled).expect("load test-vendor");

    let list = set.list(None);
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].id, "test-vendor");
    assert_eq!(list[0].version, SemVer::parse("1.0.0").unwrap());
    assert_eq!(list[0].base, BASE_DMIS_53);
    assert_eq!(list[0].origin, DialectOrigin::Bundled);
    assert_eq!(list[0].statements, 1);

    // Canonical JSON matches the expected fields
    let json_bytes = set.list_json(Some(DialectOrigin::Bundled)).expect("list_json");
    let parsed = parse_json(&json_bytes).expect("parse json");
    let CValue::Arr(arr) = parsed else {
        panic!("expected JSON array");
    };
    assert_eq!(arr.len(), 1);
    assert!(set.list(Some(DialectOrigin::Pack)).is_empty());

    // Registry command definition validates and exports cleanly through CORE.REG.VAL
    let def = list_dialects_command_def().expect("command_def");
    assert_eq!(def.id, LIST_DIALECTS_COMMAND);
    assert_eq!(def.major, LIST_DIALECTS_MAJOR);
    assert_eq!(def.side_effect, SideEffect::R0);
    assert_eq!(def.determinism, Determinism::D0);
    assert!(!def.human_only);
    let cat = Catalogue::geo_frame().expect("catalogue");
    let decls = vec![CommandDecl::from_def(&def)];
    let export = fid_registry::manifest::export_with(&decls, &cat).expect("export command");
    assert_eq!(export.definitions.len(), 1);
}

#[test]
fn ac2_uses_vndx_unknown_without_dialect_and_statement_with_test_vendor() {
    let dmi = std::fs::read(fix_dir().join("uses_vndx.dmi")).expect("read uses_vndx.dmi");

    // Without dialect: VNDX/ON parses as Unknown
    let tree_base = parse(&dmi, None).expect("parse without dialect");
    let vndx_base =
        tree_base.statements.iter().find(|n| matches!(n, Node::Unknown(u) if u.keyword.as_deref() == Some("VNDX")));
    assert!(vndx_base.is_some(), "VNDX/ON must be Unknown without dialect: {:?}", tree_base.statements);

    // With test-vendor: VNDX/ON parses as Statement of class `test-vendor.vndx_on`
    let mut set = ProfileSet::new();
    set.load_file(&fix_dir().join("test-vendor.toml"), DialectOrigin::Bundled).expect("load");
    let (tree_dial, used) =
        set.parse_with_profile(&dmi, "test-vendor", Some(SemVer::parse("1.0.0").unwrap())).expect("parse with dialect");

    let vndx_stmt = tree_dial.statements.iter().find_map(|n| match n {
        Node::Statement(s) if s.class.as_ref() == "test-vendor.vndx_on" => Some(s),
        _ => None,
    });
    let stmt = vndx_stmt.expect("VNDX/ON must be a recognized Statement under test-vendor");
    assert_eq!(stmt.class.as_ref(), "test-vendor.vndx_on");

    // AC-4: DialectUse matches file SHA-256
    let file_bytes = std::fs::read(fix_dir().join("test-vendor.toml")).expect("read toml");
    let file_sha = Digest::from_bytes(digest_bytes(&file_bytes).0);
    assert_eq!(used.id, "test-vendor");
    assert_eq!(used.version, SemVer::parse("1.0.0").unwrap());
    assert_eq!(used.digest, file_sha);
}

#[test]
fn ac3_redefines_feat_fails_base_redefinition_and_is_not_listed() {
    let bytes = std::fs::read(fix_dir().join("redefines_feat.toml")).expect("read redefines_feat.toml");
    let err = parse_profile("redefines_feat.toml", &bytes, DialectOrigin::Bundled).unwrap_err();
    assert_eq!(err.code(), "dialect_invalid");
    assert_eq!(err.reason(), Some("base_redefinition"));

    // Directory loader skips invalid profiles (redefines_feat.toml, bad_shape.toml), records errors,
    // and lists only valid profiles (test-vendor.toml).
    let (set, errs) = ProfileSet::load_dirs(Some(&fix_dir()), &[]);
    assert_eq!(set.len(), 1);
    assert_eq!(set.list(None)[0].id, "test-vendor");
    assert!(
        errs.iter().any(|e| e.code() == "dialect_invalid" && e.reason() == Some("base_redefinition")),
        "expected base_redefinition error, got {errs:?}"
    );
    assert!(
        errs.iter().any(|e| e.code() == "dialect_invalid" && e.reason() == Some("shape_compile_error")),
        "expected shape_compile_error, got {errs:?}"
    );
}

#[test]
fn ac5_unknown_version_or_id_fails_with_dialect_not_found() {
    let mut set = ProfileSet::new();
    set.load_file(&fix_dir().join("test-vendor.toml"), DialectOrigin::Bundled).expect("load");

    let err = set.select("test-vendor", Some(SemVer::parse("9.9.9").unwrap())).unwrap_err();
    assert_eq!(err.code(), "dialect_not_found");
    assert_eq!(
        err,
        DialectError::DialectNotFound { id: "test-vendor".to_string(), version: Some("9.9.9".to_string()) }
    );

    let err2 = set.select("no-such-vendor", None).unwrap_err();
    assert_eq!(err2.code(), "dialect_not_found");
}

#[test]
fn sorting_and_100_profile_performance_budget() {
    let mut set = ProfileSet::new();
    let start = Instant::now();
    for i in (0..100u32).rev() {
        let id = format!("vendor-{i:03}");
        let major = format!("VND{i:03}");
        let toml = format!(
            "schema = \"fid.dmis-dialect\"\n\
             schema_version = 1\n\
             id = \"{id}\"\n\
             version = \"1.{}.0\"\n\
             base = \"dmis-5.3\"\n\
             title = \"Synthetic Vendor {i}\"\n\
             source_note = \"Synthetic perf test\"\n\
             [[descriptors]]\n\
             class = \"{id}.cmd\"\n\
             major = \"{major}\"\n\
             label_kind = \"none\"\n\
             shape = {{ kind = \"keyword\", value = \"ON\" }}\n",
            i % 3
        );
        set.load_bytes(&format!("{id}.toml"), toml.as_bytes(), DialectOrigin::Pack).expect("load perf profile");
    }
    let elapsed = start.elapsed();
    assert_eq!(set.len(), 100);
    // Verify strictly sorted by (id, version)
    let listed = set.list(None);
    for w in listed.windows(2) {
        assert!((&w[0].id, w[0].version) < (&w[1].id, w[1].version));
    }
    assert!(elapsed.as_millis() < 2_000, "loading 100 profiles took {elapsed:?}");
}
