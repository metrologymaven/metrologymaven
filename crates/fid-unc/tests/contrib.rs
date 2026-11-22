//! Integration tests for spec node `UNC.MODEL.CONTRIB.CONTRACT` (AC-1 .. AC-3,
//! REQ-001 .. REQ-005).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::principal::{
    Channel, ChannelKind, Identity, OsUser, Principal, PrincipalId, PrincipalKind, SessionId, SessionKey, SessionTable,
    UpgradeGrant,
};
use fid_canon::{parse, to_canonical_bytes, CValue};
use fid_kci::kci::sha256_digest;
use fid_kci::manifest::KernelManifest;
use fid_kunc::model::{InputSourceKind, ModelError, SourceRef};
use fid_types::{Determinism, SideEffect};
use fid_unc::contrib::{
    declarations, expand, invoke_cli, invoke_mcp, ContribCtx, ContribError, ContributorLibrary, ContributorModel,
    ParamInput, LIST_CONTRIBUTOR, SHOW_CONTRIBUTOR,
};
use std::collections::BTreeMap;
use std::fs;
use std::path::PathBuf;

const FIXTURE_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/model/contrib/");
const TOY_MANIFEST: &str =
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/model/graph/store/toy_release_manifest.json");

struct TestChannel {
    kind: ChannelKind,
    identity: Identity,
    session: SessionKey,
}

impl Channel for TestChannel {
    fn kind(&self) -> ChannelKind {
        self.kind
    }
    fn authenticated_identity(&self) -> Option<Identity> {
        Some(self.identity.clone())
    }
    fn session(&self) -> &SessionKey {
        &self.session
    }
    fn upgrade_grant(&self) -> Option<&UpgradeGrant> {
        None
    }
}

fn make_principal(table: &mut SessionTable, kind: PrincipalKind, subject: &str) -> Principal {
    let ch = match kind {
        PrincipalKind::Human => ChannelKind::Gui,
        PrincipalKind::Agent => ChannelKind::Mcp,
        PrincipalKind::Script => ChannelKind::Cli,
        PrincipalKind::Service => ChannelKind::Service,
        PrincipalKind::Device => ChannelKind::Device,
        PrincipalKind::System => ChannelKind::Internal,
    };
    let mut identity = Identity::new(PrincipalId::new(subject).unwrap());
    identity.os_user = Some(OsUser::new(subject).unwrap());
    let session = table.open_session(SessionId::new(&format!("{}-{subject}", ch.as_str())).unwrap()).unwrap();
    table.derive_principal(&TestChannel { kind: ch, identity, session }).unwrap()
}

fn toy_manifest() -> KernelManifest {
    let bytes = fs::read(TOY_MANIFEST).unwrap();
    KernelManifest::parse_canonical(&bytes).unwrap()
}

fn load_contrib(name: &str) -> ContributorModel {
    let bytes = fs::read(format!("{FIXTURE_DIR}{name}")).unwrap();
    ContributorModel::parse(&bytes).unwrap()
}

fn valid_example_inputs() -> (BTreeMap<String, ParamInput>, BTreeMap<String, SourceRef>) {
    let mut params = BTreeMap::new();
    params.insert("gain".to_owned(), ParamInput::value(1.25));
    params.insert("length_in".to_owned(), ParamInput::value(0.25));

    let mut sources = BTreeMap::new();
    sources.insert(
        "gain".to_owned(),
        SourceRef {
            kind: InputSourceKind::Dcc,
            digest: sha256_digest(b"fixture gain dcc"),
            author: "DCC Lab Fixture".to_owned(),
        },
    );
    sources.insert(
        "length_in".to_owned(),
        SourceRef {
            kind: InputSourceKind::Certificate,
            digest: sha256_digest(b"fixture length cert"),
            author: "Cal Lab Fixture".to_owned(),
        },
    );
    (params, sources)
}

// ---------------------------------------------------------------------------
// AC-1: Expand `uc_example.json` -> validates, node IDs carry instance prefix,
//       digest equals `expected_expand.sha256`

#[test]
fn ac1_expand_example_validates_prefixes_and_matches_expected_digest() {
    let contrib = load_contrib("uc_example.json");
    assert_eq!(contrib.id(), "uc:example-gain@1.0.0");
    assert_eq!(contrib.semver(), "1.0.0");
    assert_eq!(contrib.param_names(), vec!["gain", "length_in"]);

    // Canonical roundtrip preserves digest byte-for-byte.
    let reparsed = ContributorModel::parse(contrib.canonical_json()).unwrap();
    assert_eq!(reparsed.digest(), contrib.digest());
    assert_eq!(reparsed, contrib);

    let (params, sources) = valid_example_inputs();
    let manifest = toy_manifest();
    let sub = expand(&contrib, "inst1", &params, &sources, &manifest).unwrap();

    assert_eq!(sub.contributor_id(), "uc:example-gain@1.0.0");
    assert_eq!(sub.contributor_digest(), contrib.digest());
    assert_eq!(sub.instance_id(), "inst1");

    // Every node ID and edge endpoint carries the `inst1.` prefix.
    let m = sub.model();
    assert!(!m.inputs().is_empty());
    assert!(!m.operators().is_empty());
    assert!(!m.outputs().is_empty());
    for n in m.inputs() {
        assert!(n.id.starts_with("inst1."), "input not prefixed: {}", n.id);
        assert!(n.source.is_some(), "source not attached to {}", n.id);
    }
    for op in m.operators() {
        assert!(op.id.starts_with("inst1."), "operator not prefixed: {}", op.id);
    }
    for out in m.outputs() {
        assert!(out.id.starts_with("inst1."), "output not prefixed: {}", out.id);
    }
    for edge in m.edges() {
        assert!(edge.from.node.starts_with("inst1."), "edge.from not prefixed: {}", edge.from.node);
        assert!(edge.to.node.starts_with("inst1."), "edge.to not prefixed: {}", edge.to.node);
    }

    // Independent Python reference SHA-256 matches bit-for-bit.
    let want_sha = fs::read_to_string(format!("{FIXTURE_DIR}expected_expand.sha256")).unwrap();
    assert_eq!(sub.digest().as_str(), want_sha.trim());
}

// ---------------------------------------------------------------------------
// AC-2: Error paths (out_of_range, source_kind_not_accepted, contributor_invalid
//       with unknown_operator)

#[test]
fn ac2_error_paths() {
    let contrib = load_contrib("uc_example.json");
    let manifest = toy_manifest();

    // 1. Parameter above its valid range -> `out_of_range` naming param and bound, no extrapolation.
    let (mut params_hi, sources) = valid_example_inputs();
    params_hi.insert("gain".to_owned(), ParamInput::value(2.5));
    let err_hi = expand(&contrib, "inst1", &params_hi, &sources, &manifest).unwrap_err();
    assert_eq!(err_hi.code(), "out_of_range");
    let msg_hi = err_hi.to_string();
    assert!(msg_hi.contains("gain"), "expected param name in message: {msg_hi}");
    assert!(msg_hi.contains("2"), "expected upper bound in message: {msg_hi}");
    assert!(matches!(err_hi, ContribError::OutOfRange { ref param, .. } if param == "gain"));

    // Also below lower bound -> `out_of_range`.
    let (mut params_lo, sources) = valid_example_inputs();
    params_lo.insert("length_in".to_owned(), ParamInput::value(0.0005));
    let err_lo = expand(&contrib, "inst1", &params_lo, &sources, &manifest).unwrap_err();
    assert_eq!(err_lo.code(), "out_of_range");
    assert!(matches!(err_lo, ContribError::OutOfRange { ref param, .. } if param == "length_in"));

    // 2. `declared` source where only `certificate`/`dcc` are accepted -> `source_kind_not_accepted`.
    let (params, mut bad_sources) = valid_example_inputs();
    bad_sources.insert(
        "gain".to_owned(),
        SourceRef {
            kind: InputSourceKind::Declared,
            digest: sha256_digest(b"declared gain"),
            author: "Operator".to_owned(),
        },
    );
    let err_src = expand(&contrib, "inst1", &params, &bad_sources, &manifest).unwrap_err();
    assert_eq!(err_src.code(), "source_kind_not_accepted");
    assert!(matches!(
        err_src,
        ContribError::SourceKindNotAccepted { ref param, got: Some(InputSourceKind::Declared), .. } if param == "gain"
    ));

    // Missing source -> `source_kind_not_accepted`.
    let (params, mut missing_sources) = valid_example_inputs();
    missing_sources.remove("gain");
    let err_missing = expand(&contrib, "inst1", &params, &missing_sources, &manifest).unwrap_err();
    assert_eq!(err_missing.code(), "source_kind_not_accepted");
    assert!(matches!(
        err_missing,
        ContribError::SourceKindNotAccepted { ref param, got: None, .. } if param == "gain"
    ));

    // 3. Template with an unknown operator -> `contributor_invalid` carrying `unknown_operator`.
    let bad_op_contrib = load_contrib("uc_unknown_op.json");
    let (params, sources) = valid_example_inputs();
    let err_op = expand(&bad_op_contrib, "inst1", &params, &sources, &manifest).unwrap_err();
    assert_eq!(err_op.code(), "contributor_invalid");
    match err_op {
        ContribError::ContributorInvalid { findings } => {
            assert!(
                findings
                    .iter()
                    .any(|f| matches!(f, ModelError::UnknownOperator { .. }) && f.code() == "unknown_operator"),
                "expected unknown_operator finding, got: {findings:?}"
            );
        }
        other => panic!("expected ContributorInvalid, got {other:?}"),
    }
}

// ---------------------------------------------------------------------------
// AC-3: Shipped library stub + `fid unc list-contributor --json` /
//       `fid unc show-contributor --id ... --json` + MCP parity + declarations

#[test]
fn ac3_list_and_show_contributor_cli_mcp_and_declarations() {
    let lib = ContributorLibrary::load_dir(&PathBuf::from(format!("{FIXTURE_DIR}library"))).unwrap();
    assert_eq!(lib.len(), 3);

    let mut table = SessionTable::new();
    let human = make_principal(&mut table, PrincipalKind::Human, "alice");
    let agent = make_principal(&mut table, PrincipalKind::Agent, "agent-1");
    let ctx_human = ContribCtx { principal: &human, library: &lib };
    let ctx_agent = ContribCtx { principal: &agent, library: &lib };

    let expected_raw = fs::read(format!("{FIXTURE_DIR}expected_list.json")).unwrap();
    let CValue::Obj(expected_root) = parse(&expected_raw).unwrap() else {
        panic!("expected object");
    };
    let want_all = to_canonical_bytes(expected_root.get("all").unwrap()).unwrap();
    let want_cmm = to_canonical_bytes(expected_root.get("cmm").unwrap()).unwrap();

    // `fid unc list-contributor --json` (unfiltered) -> sorted by ID, matches independent Python fixture.
    let cli_all = invoke_cli(&ctx_human, &["unc", "list-contributor", "--json"]).unwrap();
    assert_eq!(cli_all, want_all);

    // MCP `unc_list_contributor` returns identical canonical bytes.
    let mcp_all = invoke_mcp(&ctx_agent, "unc_list_contributor", b"{}").unwrap();
    assert_eq!(mcp_all, want_all);

    // Filtered by `--applicability cmm`.
    let cli_cmm = invoke_cli(&ctx_human, &["unc", "list-contributor", "--applicability", "cmm", "--json"]).unwrap();
    assert_eq!(cli_cmm, want_cmm);
    let mcp_cmm = invoke_mcp(&ctx_agent, "unc_list_contributor", br#"{"applicability":"cmm"}"#).unwrap();
    assert_eq!(mcp_cmm, want_cmm);

    // `fid unc show-contributor --id uc:example-gain@1.0.0 --json` returns full contributor.
    let cli_show =
        invoke_cli(&ctx_human, &["unc", "show-contributor", "--id", "uc:example-gain@1.0.0", "--json"]).unwrap();
    let mcp_show = invoke_mcp(&ctx_agent, "unc_show_contributor", br#"{"id":"uc:example-gain@1.0.0"}"#).unwrap();
    assert_eq!(cli_show, mcp_show);

    let CValue::Obj(show_obj) = parse(&cli_show).unwrap() else {
        panic!("expected object");
    };
    assert_eq!(show_obj.get("id"), Some(&CValue::Str("uc:example-gain@1.0.0".into())));
    assert_eq!(show_obj.get("semver"), Some(&CValue::Str("1.0.0".into())));
    let CValue::Str(canon_text) = show_obj.get("canonical").unwrap() else {
        panic!("expected canonical string");
    };
    let parsed_back = ContributorModel::parse(canon_text.as_bytes()).unwrap();
    assert_eq!(show_obj.get("digest"), Some(&CValue::Str(parsed_back.digest().as_str().to_owned())));

    // Missing contributor -> `contributor_not_found`.
    let err_nf =
        invoke_cli(&ctx_human, &["unc", "show-contributor", "--id", "uc:does-not-exist@1.0.0", "--json"]).unwrap_err();
    assert_eq!(err_nf.code(), "contributor_not_found");

    // Verify tamper detection on contributor digest.
    let raw_example = fs::read_to_string(format!("{FIXTURE_DIR}uc_example.json")).unwrap();
    let tampered = raw_example.replace("Clause 4.1", "Clause 9.9");
    let err_tamper = ContributorModel::parse(tampered.as_bytes()).unwrap_err();
    assert_eq!(err_tamper.code(), "schema_invalid");

    // Verify registry declarations (R0, D0, Activity Log only -> effects empty).
    let decls = declarations();
    let ids: Vec<String> = decls.iter().map(|d| format!("{}@{}", d.def.id, d.def.major)).collect();
    assert_eq!(ids, vec![LIST_CONTRIBUTOR, SHOW_CONTRIBUTOR]);
    for d in &decls {
        assert_eq!(d.def.side_effect, SideEffect::R0);
        assert_eq!(d.def.determinism, Determinism::D0);
        assert!(d.def.effects.is_empty(), "R0 commands must not emit Record Journal effects");
    }
}
