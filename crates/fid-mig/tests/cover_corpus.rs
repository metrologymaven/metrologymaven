//! MIG.COVER.CORPUS: AC-1..AC-6 at the library, plus the other finding codes. Fixtures and
//! expected digests come from `fixtures/mig/cover/corpus/make_fixtures.py`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_mig::cover::corpus::{self, Code, CorpusReport, RegistryCheck, Visibility};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/mig/cover/corpus")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

fn expected(name: &str) -> String {
    String::from_utf8(read(name)).unwrap().trim_end().to_string()
}

fn run_with(manifest: &str, registry: Option<&str>) -> CorpusReport {
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    let reg = registry.map(read);
    corpus::check(&read(manifest), &catalogue, &read("sources.json"), reg.as_deref())
}

fn run(manifest: &str) -> CorpusReport {
    run_with(manifest, None)
}

fn lines(r: &CorpusReport) -> Vec<String> {
    r.findings.iter().map(ToString::to_string).collect()
}

#[test]
fn ac1_ok_corpus_passes_with_expected_digest() {
    let r = run("ok/corpus.json");
    assert!(r.is_ok(), "{:?}", lines(&r));
    assert_eq!(r.digest().unwrap().as_str(), expected("ok/expected-digest.txt"));
    let m = r.manifest.as_ref().unwrap();
    assert_eq!(m.entries.len(), 3);
    assert_eq!(m.format_id.as_str(), "qif");
    assert!(!m.private_run());
}

#[test]
fn ac2_uncatalogued_entry() {
    let r = run("missing.json");
    assert!(!r.is_ok());
    assert_eq!(lines(&r), vec!["CORPUS-UNCATALOGUED mig-qif-demo-z".to_string()]);
    assert!(r.digest().is_none());
}

#[test]
fn ac3_public_fetch_only_entry() {
    let r = run("public-fetchonly.json");
    assert_eq!(r.findings.len(), 1, "{:?}", lines(&r));
    assert_eq!(r.findings[0].code, Code::Visibility);
    assert!(lines(&r)[0].starts_with("CORPUS-VISIBILITY mig-qif-fetchonly"));
}

#[test]
fn ac4_pending_review_source() {
    let r = run("pending-source.json");
    assert_eq!(r.findings.len(), 1, "{:?}", lines(&r));
    assert_eq!(r.findings[0].code, Code::Source);
    assert!(lines(&r)[0].starts_with("CORPUS-SOURCE points-csv "), "{:?}", lines(&r));
    assert!(lines(&r)[0].contains("PENDING_REVIEW"));
}

#[test]
fn ac5_private_entry_marks_private_run() {
    let r = run("mixed.json");
    assert!(r.is_ok(), "{:?}", lines(&r));
    let m = r.manifest.as_ref().unwrap();
    assert!(m.private_run());
    assert!(m.entries.iter().any(|e| e.visibility == Visibility::Private));
    assert_eq!(r.digest().unwrap().as_str(), expected("expected-digest-mixed.txt"));
}

#[test]
fn ac6_entry_order_does_not_change_the_digest() {
    let a = run("ok/corpus.json");
    let b = run("ok/corpus-shuffled.json");
    assert!(b.is_ok(), "{:?}", lines(&b));
    assert_eq!(a.digest(), b.digest());
    // The file order is kept in the loaded manifest; only the digest sorts.
    let ids = |r: &CorpusReport| -> Vec<String> {
        r.manifest.as_ref().unwrap().entries.iter().map(|e| e.catalogue_id.clone()).collect()
    };
    assert_ne!(ids(&a), ids(&b));
}

#[test]
fn digest_mismatch() {
    let r = run("digest-mismatch.json");
    assert_eq!(r.findings.len(), 1, "{:?}", lines(&r));
    assert_eq!(r.findings[0].code, Code::Digest);
    assert_eq!(r.findings[0].subject, "mig-qif-demo-b");
}

#[test]
fn public_customer_data() {
    let r = run("public-customer.json");
    assert_eq!(r.findings.len(), 1, "{:?}", lines(&r));
    assert_eq!(r.findings[0].code, Code::Visibility);
    assert!(lines(&r)[0].contains("customer data"));
}

#[test]
fn duplicate_entry() {
    let r = run("dup-entry.json");
    assert_eq!(lines(&r), vec!["CORPUS-DUP mig-qif-demo-a catalogue ID listed twice".to_string()]);
}

#[test]
fn schema_errors_stop_the_check() {
    for (name, subject) in [("format-mismatch.json", "mig-csv-demo"), ("unknown-key.json", "mig-qif-demo-b")] {
        let r = run(name);
        assert!(r.manifest.is_none(), "{name}");
        assert_eq!(r.findings.len(), 1, "{name}: {:?}", lines(&r));
        assert_eq!(r.findings[0].code, Code::Schema, "{name}");
        assert_eq!(r.findings[0].subject, subject, "{name}");
    }
}

#[test]
fn registry_manifest_format_catalogue() {
    assert!(run_with("ok/corpus.json", Some("registry-qif.json")).is_ok());
    let r = run_with("ok/corpus.json", Some("registry-empty.json"));
    assert_eq!(lines(&r), vec!["CORPUS-SCHEMA qif not a registered format".to_string()]);
}

#[test]
fn malformed_inputs_fail_closed() {
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    let sources = read("sources.json");
    let ok = read("ok/corpus.json");
    let code0 = |r: &CorpusReport| (r.findings[0].code, r.findings[0].subject.clone());

    for bad in [
        &b"[]"[..],
        b"{}",
        b"not json",
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","entries":[],"extra":"y"}"#,
        br#"{"schema":"mig-corpus@2","format_id":"qif","name":"x","entries":[]}"#,
        br#"{"schema":"mig-corpus@1","format_id":"native:demo","name":"x","entries":[]}"#,
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"[link](x)","entries":[]}"#,
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","entries":{}}"#,
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","name":"y","entries":[]}"#,
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","entries":[{"catalogue_id":"mig-qif-demo-a","sha256":"sha256:00","format_id":"qif","visibility":"public"}]}"#,
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","entries":[{"catalogue_id":"Bad ID","sha256":"sha256:0000000000000000000000000000000000000000000000000000000000000000","format_id":"qif","visibility":"public"}]}"#,
        br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","entries":[{"catalogue_id":"mig-qif-demo-a","sha256":"sha256:0000000000000000000000000000000000000000000000000000000000000000","format_id":"qif","visibility":"internal"}]}"#,
    ] {
        let r = corpus::check(bad, &catalogue, &sources, None);
        assert!(r.manifest.is_none() && !r.findings.is_empty(), "{}", String::from_utf8_lossy(bad));
        assert!(r.findings.iter().all(|f| f.code == Code::Schema), "{:?}", lines(&r));
    }

    // An empty corpus is schema-valid and has a digest.
    let empty = br#"{"schema":"mig-corpus@1","format_id":"qif","name":"x","entries":[]}"#;
    assert!(corpus::check(empty, &catalogue, &sources, None).is_ok());

    for bad_cat in [
        "not = [toml",
        "[[dataset]]\nid = \"mig-qif-demo-a\"\nsha256 = \"XYZ\"\nredistribution = \"mirror\"\n",
        "[[dataset]]\nid = \"mig-qif-demo-a\"\nsha256 = \"00\"\nredistribution = \"maybe\"\n",
        "dataset = 3\n",
    ] {
        let r = corpus::check(&ok, bad_cat, &sources, None);
        assert_eq!(code0(&r), (Code::Schema, corpus::CATALOGUE_SUBJECT.to_string()), "{bad_cat}");
    }
    // A repeated catalogue ID fails the whole catalogue.
    let twice = format!(
        "{catalogue}\n[[dataset]]\nid = \"mig-qif-demo-a\"\nsha256 = \"{}\"\nredistribution = \"mirror\"\n",
        "0".repeat(64)
    );
    assert_eq!(code0(&corpus::check(&ok, &twice, &sources, None)).1, corpus::CATALOGUE_SUBJECT);
    // A non-boolean customer-data flag fails closed.
    let flag = catalogue.replace("contains_customer_data = true", "contains_customer_data = \"yes\"");
    assert_eq!(code0(&corpus::check(&ok, &flag, &sources, None)).1, corpus::CATALOGUE_SUBJECT);

    let r = corpus::check(&ok, &catalogue, b"{}", None);
    assert_eq!(code0(&r), (Code::Schema, corpus::SOURCES_SUBJECT.to_string()));
}

#[test]
fn unreviewed_admission_does_not_count() {
    // The qif entry stays OPEN_EXPORT_ONLY but loses its attestation: the register's own rules
    // refuse it, so the corpus format is not admitted.
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    let sources = String::from_utf8(read("sources.json")).unwrap();
    let unattested = sources.replace("\"no_copied_text_attestation\": true", "\"no_copied_text_attestation\": false");
    assert_ne!(sources, unattested);
    let r = corpus::check(&read("ok/corpus.json"), &catalogue, unattested.as_bytes(), None);
    assert_eq!(r.findings.len(), 1, "{:?}", lines(&r));
    assert_eq!(r.findings[0].code, Code::Source);
    assert!(lines(&r)[0].contains("fails review, attestation or policy"));
}

#[test]
fn no_register_entry_for_the_format() {
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    let sources = String::from_utf8(read("sources.json"))
        .unwrap()
        .replace("\"format_id\": \"qif\"", "\"format_id\": \"qif-other\"");
    let r = corpus::check(&read("ok/corpus.json"), &catalogue, sources.as_bytes(), None);
    assert_eq!(lines(&r), vec!["CORPUS-SOURCE qif no source register entry".to_string()]);
}

#[test]
fn oversized_inputs_are_refused() {
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    let sources = read("sources.json");
    let mut big = b"{\"schema\":\"mig-corpus@1\",\"format_id\":\"qif\",\"name\":\"".to_vec();
    big.resize(big.len() + corpus::MAX_MANIFEST_BYTES, b'x');
    big.extend(b"\",\"entries\":[]}");
    let r = corpus::check(&big, &catalogue, &sources, None);
    assert!(r.manifest.is_none() && r.findings[0].code == Code::Schema);
    let big_cat = format!("{catalogue}#{}\n", "x".repeat(corpus::MAX_CATALOGUE_BYTES));
    let r = corpus::check(&read("ok/corpus.json"), &big_cat, &sources, None);
    assert_eq!(r.findings[0].subject, corpus::CATALOGUE_SUBJECT);
}

#[test]
fn findings_are_sorted_by_subject_then_code() {
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    // Uncatalogued z, then fetch-only and customer-data files as public with a wrong digest,
    // appended in an order that is not the sorted one.
    let mut m = String::from_utf8(read("ok/corpus.json")).unwrap();
    for id in ["mig-qif-demo-z", "mig-qif-fetchonly", "mig-qif-customer"] {
        m = append_public(&m, id);
    }
    let r = corpus::check(m.as_bytes(), &catalogue, &read("sources.json"), None);
    let got: Vec<(&str, Code)> = r.findings.iter().map(|f| (f.subject.as_str(), f.code)).collect();
    assert_eq!(
        got,
        vec![
            ("mig-qif-customer", Code::Digest),
            ("mig-qif-customer", Code::Visibility),
            ("mig-qif-demo-z", Code::Uncatalogued),
            ("mig-qif-fetchonly", Code::Digest),
            ("mig-qif-fetchonly", Code::Visibility),
        ]
    );
}

/// Appends a public qif entry with an all-zero digest to a fixture manifest, before the
/// closing bracket of `entries` (the last `]` in the file).
fn append_public(text: &str, id: &str) -> String {
    let cut = text.rfind(']').unwrap();
    let (head, tail) = text.split_at(cut);
    let zeros = "0".repeat(64);
    format!(
        "{},{{\"catalogue_id\":\"{id}\",\"sha256\":\"sha256:{zeros}\",\"format_id\":\"qif\",\"visibility\":\"public\"}}\n{tail}",
        head.trim_end()
    )
}

#[test]
fn catalogue_unknown_keys_fail_closed() {
    let catalogue = String::from_utf8(read("datasets.toml")).unwrap();
    let sources = read("sources.json");
    let ok = read("ok/corpus.json");
    assert!(corpus::check(&ok, &catalogue, &sources, None).is_ok());
    // A misspelt customer-data flag on the public mig-qif-demo-a would otherwise read as false.
    let misspelt =
        catalogue.replacen("id = \"mig-qif-demo-a\"\n", "id = \"mig-qif-demo-a\"\ncontain_customer_data = true\n", 1);
    assert_ne!(misspelt, catalogue);
    let r = corpus::check(&ok, &misspelt, &sources, None);
    assert!(r.manifest.is_none());
    assert_eq!(r.findings.len(), 1, "{:?}", lines(&r));
    assert_eq!((r.findings[0].code, r.findings[0].subject.as_str()), (Code::Schema, corpus::CATALOGUE_SUBJECT));
    assert!(r.findings[0].detail.contains("unknown field contain_customer_data"), "{:?}", lines(&r));
    // An unknown top-level key fails the catalogue too.
    let top = format!("extra = 1\n{catalogue}");
    let r = corpus::check(&ok, &top, &sources, None);
    assert!(r.findings[0].detail.contains("unknown top-level key extra"), "{:?}", lines(&r));
    // Every ECO.DATA.CAT field name is accepted (the fixture uses all but notes and
    // contains_standards_text).
    let full = catalogue.replacen(
        "id = \"mig-qif-demo-a\"\n",
        "id = \"mig-qif-demo-a\"\nnotes = \"n\"\ncontains_standards_text = false\n",
        1,
    );
    assert!(corpus::check(&ok, &full, &sources, None).is_ok());
    assert_eq!(corpus::CATALOGUE_FIELDS.len(), 14);
}

#[test]
fn report_states_whether_the_registry_was_checked() {
    assert_eq!(run("ok/corpus.json").registry, RegistryCheck::NotChecked);
    assert_eq!(run_with("ok/corpus.json", Some("registry-qif.json")).registry, RegistryCheck::Checked);
    assert_eq!(run("missing.json").registry, RegistryCheck::NotChecked);
    assert_eq!(run_with("ok/corpus.json", Some("registry-empty.json")).registry, RegistryCheck::Checked);
}
