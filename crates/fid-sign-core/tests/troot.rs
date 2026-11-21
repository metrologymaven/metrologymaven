//! TRUST.SIGN.TROOT acceptance criteria AC-1 to AC-6 and the rules of REQ-001 to REQ-006,
//! against `fixtures/troot/`. Every expected report, code and reason comes from
//! `fixtures/troot/make_fixtures.py` (an independent stdlib Python reference that signs with
//! the published RFC test keys and classifies by the spec's rules), never from this crate.
//!
//! AC-5 (offline): these tests also run in CI's wasm32 job, under wasmtime with only the
//! workspace folder mapped, a WASI preview 1 sandbox that has no network API at all; the
//! AC-1 report must be the same there byte for byte (`ac1_..` and `every_case_...`).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::PathBuf;

use fid_canon::CValue;
use fid_pkg::reader::{FileOpener, HandleTable};
use fid_registry::decl::CommandDecl;
use fid_sign_core::troot::{
    check_envelope, check_signature, declarations, invoke_cli, invoke_mcp, CheckParams, Keyring, Revocations,
    SignatureStatus, TrustInputs, TrustRoot, TrustedKey, UtcTime, DETAILS, MAX_FILE_BYTES, MAX_KEYID_TRIALS,
    MAX_SIGNATURES_PER_PACKAGE, MAX_SIGNATURE_STATEMENT_BYTES, MAX_VERIFIED_BYTES, MCP_CHECK, NOTES,
    SIGNATURE_PREDICATE_TYPE, TROOT_REASONS,
};
use fid_sign_core::{Envelope, Sig};

fn path(rel: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/troot").join(rel)
}

fn p(rel: &str) -> String {
    path(rel).to_str().unwrap().to_owned()
}

fn bytes(rel: &str) -> Vec<u8> {
    std::fs::read(path(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn obj(v: &CValue) -> &std::collections::BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        other => panic!("not an object: {other:?}"),
    }
}

fn s<'a>(v: &'a CValue, k: &str) -> Option<&'a str> {
    match obj(v).get(k) {
        Some(CValue::Str(s)) => Some(s),
        None => None,
        other => panic!("{k}: not a string: {other:?}"),
    }
}

struct Case {
    name: String,
    package: String,
    trust_root: String,
    revoked: Option<String>,
    keys: Option<String>,
    as_of: Option<String>,
    expect: Option<String>,
    error: Option<(String, Option<String>)>,
}

fn cases() -> Vec<Case> {
    let v = fid_canon::parse(&bytes("cases.json")).unwrap();
    let CValue::Arr(items) = v else { panic!("cases.json is not an array") };
    items
        .iter()
        .map(|c| Case {
            name: s(c, "name").unwrap().to_owned(),
            package: s(c, "package").unwrap().to_owned(),
            trust_root: s(c, "trust_root").unwrap().to_owned(),
            revoked: s(c, "revoked").map(str::to_owned),
            keys: s(c, "keys").map(str::to_owned),
            as_of: s(c, "as_of").map(str::to_owned),
            expect: s(c, "expect").map(str::to_owned),
            error: obj(c).get("error").map(|e| (s(e, "code").unwrap().to_owned(), s(e, "reason").map(str::to_owned))),
        })
        .collect()
}

fn cli_args(c: &Case) -> Vec<String> {
    let mut a = vec!["check-signature".to_owned(), p(&c.package), "--trust-root".to_owned(), p(&c.trust_root)];
    if let Some(r) = &c.revoked {
        a.extend(["--revoked".to_owned(), p(r)]);
    }
    if let Some(t) = &c.as_of {
        a.extend(["--as-of".to_owned(), t.clone()]);
    }
    if let Some(k) = &c.keys {
        a.extend(["--keys".to_owned(), p(k)]);
    }
    a.push("--json".to_owned());
    a
}

fn run_cli(args: &[String]) -> fid_sign_core::troot::CliOutput {
    let refs: Vec<&str> = args.iter().map(String::as_str).collect();
    invoke_cli(&FileOpener, &refs)
}

fn report(case: &str) -> CValue {
    let c = cases().into_iter().find(|c| c.name == case).unwrap();
    let out = run_cli(&cli_args(&c));
    assert_eq!(out.code, 0, "{case}: {}", String::from_utf8_lossy(&out.stdout));
    fid_canon::parse(out.stdout.trim_ascii_end()).unwrap()
}

fn sigs(r: &CValue) -> Vec<CValue> {
    match obj(r).get("signatures") {
        Some(CValue::Arr(a)) => a.clone(),
        other => panic!("signatures: {other:?}"),
    }
}

fn key_fingerprint(name: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/dsse/keys").join(name);
    std::fs::read_to_string(p).unwrap().trim().to_owned()
}

// ------------------------------------------------------------------ every case

#[test]
fn every_case_matches_the_python_reference() {
    let all = cases();
    assert!(all.len() >= 25, "{}", all.len());
    for c in &all {
        let out = run_cli(&cli_args(c));
        match (&c.expect, &c.error) {
            (Some(expect), None) => {
                assert_eq!(out.code, 0, "{}: {}", c.name, String::from_utf8_lossy(&out.stdout));
                let mut want = bytes(expect);
                want.push(b'\n');
                assert_eq!(
                    String::from_utf8_lossy(&out.stdout),
                    String::from_utf8_lossy(&want),
                    "{}: report differs from {expect}",
                    c.name
                );
            }
            (None, Some((code, reason))) => {
                assert_eq!(out.code, 1, "{}", c.name);
                let v = fid_canon::parse(out.stdout.trim_ascii_end()).unwrap();
                let e = obj(&v).get("error").unwrap();
                assert_eq!(s(e, "code"), Some(code.as_str()), "{}", c.name);
                assert_eq!(s(e, "reason"), reason.as_deref(), "{}", c.name);
                assert!(!String::from_utf8_lossy(&out.stdout).contains("VALID_TRUSTED"), "{}", c.name);
            }
            _ => panic!("{}: a case has an expected report or an error", c.name),
        }
    }
}

// ------------------------------------------------------------------ AC-1 .. AC-4

#[test]
fn ac1_both_signatures_valid_trusted_with_the_published_fingerprints() {
    let r = report("ac1-both-trusted");
    assert_eq!(s(&r, "package_status"), Some("SIGNED"));
    let list = sigs(&r);
    assert_eq!(list.len(), 2);
    let want = [key_fingerprint("p256-test.fingerprint"), key_fingerprint("ed25519-test.fingerprint")];
    for (sig, fp) in list.iter().zip(&want) {
        assert_eq!(s(sig, "status"), Some("VALID_TRUSTED"));
        assert_eq!(s(sig, "fingerprint"), Some(fp.as_str()));
        assert_eq!(s(sig, "detail"), None);
        // D-045: the signing time is reported only as a claim.
        assert!(s(sig, "claimed_signed_at").is_some());
        assert!(!obj(sig).contains_key("signed_at"));
    }
}

#[test]
fn ac2_one_trusted_one_untrusted() {
    let list = sigs(&report("ac2-one-trusted"));
    let st: Vec<_> = list.iter().map(|x| s(x, "status").unwrap()).collect();
    assert_eq!(st, ["VALID_TRUSTED", "VALID_UNTRUSTED"]);
    assert_eq!(s(&list[1], "fingerprint"), Some(key_fingerprint("ed25519-test.fingerprint").as_str()));
    // Without a keyring the second key is not listed anywhere, so it cannot be checked.
    let list = sigs(&report("ac2-one-no-keyring"));
    let st: Vec<_> = list.iter().map(|x| s(x, "status").unwrap()).collect();
    assert_eq!(st, ["VALID_TRUSTED", "UNKNOWN_KEY"]);
    assert_eq!(s(&list[1], "fingerprint"), None);
}

#[test]
fn ac3_revoked_before_signing_time() {
    let list = sigs(&report("ac3-revoked-before"));
    assert_eq!(s(&list[0], "status"), Some("REVOKED"));
    assert_eq!(s(&list[1], "status"), Some("VALID_TRUSTED"));
    let list = sigs(&report("revoked-after"));
    assert!(list.iter().all(|x| s(x, "status") == Some("VALID_TRUSTED")));
}

#[test]
fn ac4_truncated_trust_root_is_malformed() {
    let out = run_cli(&[
        "check-signature".to_owned(),
        p("packages/two-signatures.zip"),
        "--trust-root".to_owned(),
        p("malformed/truncated.json"),
        "--json".to_owned(),
    ]);
    assert_eq!(out.code, 1);
    let v = fid_canon::parse(out.stdout.trim_ascii_end()).unwrap();
    assert_eq!(s(obj(&v).get("error").unwrap(), "code"), Some("troot.malformed"));
}

#[test]
fn req005_oversize_files_are_malformed_before_parsing() {
    let mut big = b"{\"keys\":[]}".to_vec();
    big.resize(MAX_FILE_BYTES + 1, b' ');
    assert_eq!(TrustRoot::parse(&big).unwrap_err().reason, "too_large");
    assert_eq!(Revocations::parse(&big).unwrap_err().reason, "too_large");
    assert_eq!(Keyring::parse(&big).unwrap_err().reason, "too_large");
    let mut ok = b"{\"keys\":[]}".to_vec();
    ok.resize(MAX_FILE_BYTES, b' ');
    assert!(TrustRoot::parse(&ok).unwrap().keys.is_empty());
    for e in [TrustRoot::parse(b"{").unwrap_err(), TrustRoot::parse(b"{\"keys\":{}}").unwrap_err()] {
        assert!(TROOT_REASONS.contains(&e.reason), "{}", e.reason);
        assert_eq!(e.code(), "troot.malformed");
    }
}

#[test]
fn req006_unsigned_package() {
    let r = report("unsigned");
    assert_eq!(s(&r, "package_status"), Some("UNSIGNED"));
    assert!(sigs(&r).is_empty());
}

#[test]
fn detail_codes_are_the_documented_ones() {
    let mut seen = std::collections::BTreeSet::new();
    let mut notes = std::collections::BTreeSet::new();
    for c in cases().iter().filter(|c| c.expect.is_some()) {
        for sig in sigs(&report(&c.name)) {
            if let Some(d) = s(&sig, "detail") {
                assert!(DETAILS.contains(&d), "{}: {d}", c.name);
                seen.insert(d.to_owned());
            }
            let Some(CValue::Arr(ns)) = obj(&sig).get("notes") else { panic!("{}: notes missing", c.name) };
            for n in ns {
                let CValue::Str(n) = n else { panic!("note not a string") };
                assert!(NOTES.contains(&n.as_str()), "{}: {n}", c.name);
                notes.insert(n.clone());
            }
        }
    }
    // Every documented code is exercised by the fixtures, except the two bounds the
    // fixtures cannot carry cheaply; `bounds_*` below covers those.
    let missing: Vec<_> = DETAILS.iter().filter(|d| !seen.contains(**d)).copied().collect();
    assert_eq!(missing, ["keyid_missing", "payload_too_large"]);
    assert_eq!(notes.len(), NOTES.len());
}

// ------------------------------------------------------------------ AC-6 and surfaces

#[test]
fn ac6_mcp_with_handles_equals_the_cli_json() {
    let mut table = HandleTable::new();
    table.insert("pkg-1", path("packages/two-signatures.zip"));
    table.insert("root-1", path("roots/both.json"));
    table.insert("rev-1", path("revoked/revoked-before.json"));
    table.insert("keys-1", path("keyring/keyring.json"));
    let mcp = invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1","trust_root":"root-1"}"#).unwrap();
    assert_eq!(mcp, bytes("expected/ac1-both-trusted.json"));
    let cli = report("ac1-both-trusted");
    assert_eq!(fid_canon::parse(&mcp).unwrap(), cli);
    let mcp = invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1","trust_root":"root-1","revoked":"rev-1"}"#).unwrap();
    assert_eq!(mcp, bytes("expected/ac3-revoked-before.json"));

    let code = |r: Result<Vec<u8>, Vec<u8>>| {
        let v = fid_canon::parse(&r.unwrap_err()).unwrap();
        s(obj(&v).get("error").unwrap(), "code").unwrap().to_owned()
    };
    assert_eq!(code(invoke_mcp(&table, "verify_other", b"{}")), "mcp.unknown_tool");
    let bad_time = br#"{"package":"pkg-1","trust_root":"root-1","as_of":"2026-06-01T00:00:00+02:00"}"#;
    assert_eq!(code(invoke_mcp(&table, MCP_CHECK, bad_time)), "params.invalid");
    let as_of = br#"{"package":"pkg-1","trust_root":"root-1","revoked":"rev-1","as_of":"2026-06-01T00:00:00Z"}"#;
    assert!(invoke_mcp(&table, MCP_CHECK, as_of).is_ok());
    assert_eq!(code(invoke_mcp(&table, MCP_CHECK, b"[]")), "params.invalid");
    assert_eq!(code(invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1"}"#)), "params.invalid");
    assert_eq!(
        code(invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1","trust_root":"root-1","x":"y"}"#)),
        "params.invalid"
    );
    assert_eq!(code(invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1","trust_root":""}"#)), "params.invalid");
    assert_eq!(
        code(invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1","trust_root":"root-1","keys":1}"#)),
        "params.invalid"
    );
    assert_eq!(
        code(invoke_mcp(&table, MCP_CHECK, br#"{"package":"pkg-1","trust_root":"root-1","trust_root":"root-1"}"#)),
        "params.invalid"
    );
    // An unregistered handle (an agent passing a raw path) never opens a file.
    let raw = format!(r#"{{"package":"pkg-1","trust_root":{:?}}}"#, p("roots/both.json"));
    assert_eq!(code(invoke_mcp(&table, MCP_CHECK, raw.as_bytes())), "troot.malformed");
    let raw = format!(r#"{{"package":{:?},"trust_root":"root-1"}}"#, p("packages/two-signatures.zip"));
    assert_eq!(code(invoke_mcp(&table, MCP_CHECK, raw.as_bytes())), "pkg.io");
}

#[test]
fn cli_usage_errors_and_text_output() {
    let pkg = p("packages/two-signatures.zip");
    let root = p("roots/both.json");
    for args in [
        vec![],
        vec!["inspect-package".to_owned()],
        vec!["check-signature".to_owned()],
        vec!["check-signature".to_owned(), pkg.clone()],
        vec!["check-signature".to_owned(), pkg.clone(), "--trust-root".to_owned()],
        vec!["check-signature".to_owned(), pkg.clone(), "--trust-root".to_owned(), "--json".to_owned()],
        vec!["check-signature".to_owned(), pkg.clone(), "--trust-root".to_owned(), root.clone(), "--x".to_owned()],
        vec!["check-signature".to_owned(), pkg.clone(), pkg.clone(), "--trust-root".to_owned(), root.clone()],
        vec![
            "check-signature".to_owned(),
            pkg.clone(),
            "--trust-root".to_owned(),
            root.clone(),
            "--trust-root".to_owned(),
            root.clone(),
        ],
    ] {
        assert_eq!(run_cli(&args).code, 2, "{args:?}");
    }
    let out = run_cli(&["check-signature".to_owned(), pkg, "--trust-root".to_owned(), root]);
    assert_eq!(out.code, 0);
    let text = String::from_utf8(out.stdout).unwrap();
    assert!(text.starts_with("package SIGNED\n0 VALID_TRUSTED "), "{text}");
    assert!(text.contains("signing time claimed by the signer: 2026-03-01T10:00:00Z"), "{text}");
    let missing = run_cli(&[
        "check-signature".to_owned(),
        "no/such/package.zip".to_owned(),
        "--trust-root".to_owned(),
        p("roots/both.json"),
        "--json".to_owned(),
    ]);
    assert_eq!(missing.code, 1);
}

#[test]
fn trust_files_are_read_before_the_package() {
    // A malformed trust root fails even when the package does not exist.
    let r = check_signature(
        &FileOpener,
        CheckParams {
            package: "no/such/package.zip".to_owned(),
            trust_root: p("malformed/truncated.json"),
            revoked: None,
            keys: None,
            as_of: None,
        },
    );
    assert_eq!(r.unwrap_err().code(), "troot.malformed");
}

#[test]
fn status_strings_and_times() {
    assert_eq!(SignatureStatus::UnknownKey.as_str(), "UNKNOWN_KEY");
    assert!(UtcTime::parse("2026-03-02T09:30:00.25Z").unwrap() < UtcTime::parse("2026-03-02T09:30:00.5Z").unwrap());
}

// ------------------------------------------------------------------ declaration

#[test]
fn declaration_validates() {
    let decls = declarations();
    assert_eq!(decls.len(), 1);
    let d = &decls[0].def;
    assert_eq!((d.id.as_str(), d.major), ("verify.check_signature", 1));
    assert_eq!(fid_registry::tool_name(&d.id, d.major), MCP_CHECK);
    let raw: Vec<_> = decls.iter().map(CommandDecl::to_validation).collect();
    assert_eq!(fid_registry::validate(&raw), Vec::new());
    let catalogue = fid_registry::Catalogue::geo_frame().unwrap();
    for schema in [&d.params_schema, &d.result_schema] {
        assert_eq!(fid_registry::check_schema(schema, &catalogue), Vec::new());
    }
}

// ------------------------------------------------------------------ bounded work (review of #408)

fn ed_key() -> (fid_sign_core::PublicKey, fid_sign_core::Fingerprint) {
    let pem = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/dsse/keys/ed25519-test.pub.pem"),
    )
    .unwrap();
    let key = fid_sign_core::PublicKey::from_pem(&pem).unwrap();
    let fp = key.fingerprint();
    (key, fp)
}

fn root_of(n: usize) -> TrustRoot {
    let (key, fingerprint) = ed_key();
    let t = UtcTime::parse("2026-01-01T00:00:00Z").unwrap();
    let k = TrustedKey { label: "k".to_owned(), key, fingerprint, not_before: t, not_after: None };
    TrustRoot { keys: vec![k; n] }
}

fn env(payload: Vec<u8>, keyid: &str) -> Envelope {
    Envelope {
        payload,
        payload_type: fid_sign_core::IN_TOTO_PAYLOAD_TYPE.to_owned(),
        signatures: vec![Sig { keyid: keyid.to_owned(), sig: vec![0; 64] }],
    }
}

fn one(e: &Envelope, root: &TrustRoot) -> (SignatureStatus, Option<String>) {
    let (rev, ring) = (Revocations::default(), Keyring::default());
    let inputs = TrustInputs { trust_root: root, revocations: &rev, keyring: &ring, as_of: None };
    let mut budget = MAX_VERIFIED_BYTES;
    let r = check_envelope(e, "signatures/x", Some(&"a".repeat(64)), &inputs, 0, &mut budget).unwrap();
    assert_eq!(r.len(), 1);
    (r[0].status, r[0].detail.clone())
}

#[test]
fn bounds_payload_over_one_mib_is_refused_before_parsing() {
    let root = root_of(1);
    let fp = root.keys[0].fingerprint.as_str().to_owned();
    let big = vec![b' '; MAX_SIGNATURE_STATEMENT_BYTES + 1];
    assert_eq!(one(&env(big, &fp), &root), (SignatureStatus::Invalid, Some("payload_too_large".to_owned())));
    let at = vec![b' '; MAX_SIGNATURE_STATEMENT_BYTES];
    assert_eq!(one(&env(at, &fp), &root), (SignatureStatus::Invalid, Some("payload_not_bound".to_owned())));
}

#[test]
fn bounds_empty_keyid_is_tried_only_against_a_few_keys() {
    // A bound payload (a valid signature statement), a garbage signature, an empty keyid.
    let stmt = format!(
        r#"{{"_type":"https://in-toto.io/Statement/v1","predicate":{{"signed_at":"2026-03-01T10:00:00Z"}},"predicateType":"{SIGNATURE_PREDICATE_TYPE}","subject":[{{"digest":{{"sha256":"{}"}},"name":"statement.json"}}]}}"#,
        "a".repeat(64)
    );
    let e = env(stmt.into_bytes(), "");
    let at_cap = root_of(MAX_KEYID_TRIALS);
    assert_eq!(one(&e, &at_cap), (SignatureStatus::UnknownKey, Some("no_listed_key_verifies".to_owned())));
    let over = root_of(MAX_KEYID_TRIALS + 1);
    assert_eq!(one(&e, &over), (SignatureStatus::UnknownKey, Some("keyid_missing".to_owned())));
}

#[test]
fn bounds_more_than_64_signatures_in_a_package_is_malformed() {
    assert_eq!(MAX_SIGNATURES_PER_PACKAGE, 64);
    let c = cases().into_iter().find(|c| c.name == "too-many-signatures").unwrap();
    let out = run_cli(&cli_args(&c));
    assert_eq!(out.code, 1);
    let v = fid_canon::parse(out.stdout.trim_ascii_end()).unwrap();
    assert_eq!(s(obj(&v).get("error").unwrap(), "code"), Some("sig.malformed"));
}

#[test]
fn bounds_64_junk_envelopes_finish_quickly_and_verify_nothing() {
    let c = cases().into_iter().find(|c| c.name == "junk-64").unwrap();
    let start = std::time::Instant::now();
    let r = report("junk-64");
    let took = start.elapsed();
    let list = sigs(&r);
    assert_eq!(list.len(), 64);
    assert!(list.iter().all(|x| s(x, "status") == Some("INVALID") && s(x, "detail") == Some("payload_not_bound")));
    // Generous for debug builds and wasm: no signature is verified, so this is reading and
    // hashing about 200 KiB.
    assert!(took < std::time::Duration::from_secs(20), "{took:?} for {}", c.name);
}

#[test]
fn bounds_verification_budget_edge() {
    use sha2::Digest as _;
    assert_eq!(MAX_VERIFIED_BYTES, 256 * 1024 * 1024);
    // A payload that is the statement itself (bound, so it would be verified).
    let payload = br#"{"statement":"stand-in"}"#.to_vec();
    let digest: String = sha2::Sha256::digest(&payload).iter().map(|b| format!("{b:02x}")).collect();
    let root = root_of(3);
    let fp = root.keys[0].fingerprint.as_str().to_owned();
    let (rev, ring) = (Revocations::default(), Keyring::default());
    let inputs = TrustInputs { trust_root: &root, revocations: &rev, keyring: &ring, as_of: None };
    let e = env(payload.clone(), &fp);
    let len = u64::try_from(e.pae().len()).unwrap();
    // One keyid signature costs one PAE.
    let mut exact = len;
    let r = check_envelope(&e, "s", Some(&digest), &inputs, 0, &mut exact).unwrap();
    assert_eq!((r.len(), exact), (1, 0));
    let mut short = len - 1;
    let err = check_envelope(&e, "s", Some(&digest), &inputs, 0, &mut short).unwrap_err();
    assert_eq!((err.needed, err.left, short), (len, len - 1, len - 1));
    // An empty keyid costs one PAE per listed key (three here).
    let e = env(payload, "");
    let mut exact = 3 * len;
    assert!(check_envelope(&e, "s", Some(&digest), &inputs, 0, &mut exact).is_ok());
    assert_eq!(exact, 0);
    let mut short = 3 * len - 1;
    assert!(check_envelope(&e, "s", Some(&digest), &inputs, 0, &mut short).is_err());
    // An unbound envelope costs nothing, even with no budget left.
    let mut none = 0;
    let unbound = env(b"not the statement".to_vec(), &fp);
    let r = check_envelope(&unbound, "s", Some(&digest), &inputs, 0, &mut none).unwrap();
    assert_eq!(r[0].detail.as_deref(), Some("payload_not_bound"));
}
