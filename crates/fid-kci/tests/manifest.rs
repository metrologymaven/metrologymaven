//! Acceptance criteria of spec node KRN.MAN.MANI.SCHEMA (AC-1, AC-2; AC-3 is
//! `fixtures/man/check_manifest_schema.py` in CI). Fixtures and expected digests come from
//! `fixtures/man/make_manifest_fixtures.py` (Python json and hashlib); error outcomes are
//! hand-labelled in `fixtures/man/manifest/labels.txt`, which the Python schema check reads too.
//! An independent SHA-256 (`sha2` via fid-testkit) cross-checks the digests.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::manifest::{
    Artifact, KernelManifest, ManifestCore, ManifestError, MAX_ALGORITHMS, MAX_ARTIFACTS, MAX_ID_LEN, MAX_INPUT_BYTES,
    MAX_TOKEN_LEN,
};
use fid_kci::AlgorithmId;
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man")
}

fn fixture(name: &str) -> Vec<u8> {
    let path = if name == "manifest-v1.json" { fixtures().join(name) } else { fixtures().join("manifest").join(name) };
    std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn base_bytes() -> Vec<u8> {
    fixture("manifest-v1.json")
}

fn base() -> KernelManifest {
    KernelManifest::parse(&base_bytes()).unwrap()
}

fn expected_digest(name: &str) -> String {
    let text = std::fs::read_to_string(fixtures().join("manifest-v1-digests.txt")).unwrap();
    text.lines()
        .filter(|l| !l.starts_with('#'))
        .find_map(|l| l.strip_prefix(name).and_then(|r| r.strip_prefix('\t')))
        .unwrap()
        .to_string()
}

// ---------------------------------------------------------------- AC-1

#[test]
fn ac1_round_trip_is_byte_identical_and_digests_match() {
    let bytes = base_bytes();
    assert!(!bytes.ends_with(b"\n"), "fixture must be the bare canonical bytes (check .gitattributes -text)");
    let m = KernelManifest::parse(&bytes).unwrap();
    assert_eq!(m.canonical_json(), &bytes[..], "REQ-004: parse then write reproduces the input");
    assert_eq!(m.core_digest().as_str(), expected_digest("core_digest"));
    assert_eq!(m.manifest_digest().as_str(), expected_digest("manifest_digest"));
    // Independent SHA-256 over the same bytes.
    assert_eq!(m.manifest_digest().hex(), fid_testkit::sha256_hex(&bytes));
    assert_eq!(m.core().core_digest().unwrap(), *m.core_digest());
    assert_eq!(m.core_digest().hex(), fid_testkit::sha256_hex(&m.core().canonical_json().unwrap()));
}

#[test]
fn ac1_encoding_twice_and_rebuilding_give_identical_bytes() {
    let m = base();
    let rebuilt = KernelManifest::new(m.core().clone(), m.artifacts().to_vec()).unwrap();
    assert_eq!(rebuilt, m);
    assert_eq!(rebuilt.canonical_json(), m.canonical_json());
    assert_eq!(m.core().canonical_json().unwrap(), m.core().canonical_json().unwrap());
    let again = KernelManifest::parse(rebuilt.canonical_json()).unwrap();
    assert_eq!(again.canonical_json(), &base_bytes()[..]);
}

#[test]
fn ac1_sort_rules_use_the_id_total_order() {
    let m = base();
    let ids: Vec<String> = m.core().algorithms.iter().map(|a| a.id.to_string()).collect();
    assert_eq!(
        ids,
        [
            "fit.cylinder.ls@1.0.0",
            "fit.cylinder.ls@2.0.0",
            "fit.cylinder.ls@10.0.0",
            "fit.plane.ls@1.0.0",
            "fit.plane.ls2@1.0.0",
            "gdt.flatness.mz@0.1.0",
            "unc.prop.gum@1.2.3",
        ]
    );
    let arts: Vec<(&str, &str)> = m.artifacts().iter().map(|a| (a.role.as_str(), a.target.as_str())).collect();
    assert_eq!(
        arts,
        [
            ("native", "aarch64-apple-darwin"),
            ("native", "x86_64-unknown-linux-gnu"),
            ("wasm", "wasm32-unknown-unknown")
        ]
    );
}

#[test]
fn non_canonical_input_re_encodes_to_the_canonical_bytes() {
    for name in ["ok-whitespace.json", "ok-escaped.json"] {
        let m = KernelManifest::parse(&fixture(name)).unwrap();
        assert_eq!(m.canonical_json(), &base_bytes()[..], "{name}");
    }
}

// ---------------------------------------------------------------- AC-2 and the labelled set

fn detail(e: &ManifestError) -> String {
    match e {
        ManifestError::UnknownField(k) | ManifestError::DuplicateKey(k) => k.clone(),
        ManifestError::MissingField(p) | ManifestError::BadValue(p) | ManifestError::BadHex(p) => p.to_string(),
        ManifestError::Unsorted { list, index } => format!("{list}[{index}]"),
        ManifestError::TooLarge { what, .. } => what.to_string(),
        ManifestError::Syntax { .. } | ManifestError::CoreDigestMismatch { .. } => "-".to_string(),
    }
}

#[test]
fn ac2_labelled_fixtures_give_their_labelled_outcome() {
    let labels = std::fs::read_to_string(fixtures().join("manifest/labels.txt")).unwrap();
    let mut n = 0;
    let mut codes = Vec::new();
    for line in labels.lines().filter(|l| !l.is_empty() && !l.starts_with('#')) {
        let cols: Vec<&str> = line.split('\t').collect();
        let [name, rust, want_detail, _schema] = cols[..] else { panic!("bad label line {line:?}") };
        let got = KernelManifest::parse(&fixture(name));
        match (&got, rust) {
            (Ok(m), "ok") => {
                // Every accepted file is canonical after one write, and parsing that again is stable.
                let again = KernelManifest::parse(m.canonical_json()).unwrap();
                assert_eq!(again.canonical_json(), m.canonical_json(), "{name}");
            }
            (Err(e), code) if e.code() == code => {
                assert_eq!(detail(e), want_detail, "{name}: {e}");
                codes.push(code);
            }
            _ => panic!("{name}: got {got:?}, label says {rust} {want_detail}"),
        }
        n += 1;
    }
    assert_eq!(n, 45, "every labelled file was checked");
    // The four AC-2 error paths are among them.
    for code in ["unknown_field", "duplicate_key", "unsorted", "core_digest_mismatch"] {
        assert!(codes.contains(&code), "{code}");
    }
}

#[test]
fn input_over_4_mib_is_too_large() {
    let mut exact = base_bytes();
    exact.resize(MAX_INPUT_BYTES, b' ');
    assert_eq!(KernelManifest::parse(&exact).unwrap().canonical_json(), &base_bytes()[..]);
    exact.push(b' ');
    assert_eq!(KernelManifest::parse(&exact), Err(ManifestError::TooLarge { what: "input", limit: MAX_INPUT_BYTES }));
}

// ---------------------------------------------------------------- bounds (same limits as the schema check)

fn alg_json(i: usize) -> String {
    format!(
        "{{\"card_digest\":\"sha256:{}\",\"id\":\"a.b.c@{i}.0.0\",\"impl_digest\":\"sha256:{}\"}}",
        "0".repeat(64),
        "1".repeat(64)
    )
}

/// The base manifest with its algorithms array replaced by `n` generated entries.
fn with_algorithms(n: usize) -> Vec<u8> {
    let text = String::from_utf8(base_bytes()).unwrap();
    let start = text.find("\"algorithms\":[").unwrap() + "\"algorithms\":".len();
    let end = start + text[start..].find("}],").unwrap() + 2;
    let list: Vec<String> = (0..n).map(alg_json).collect();
    format!("{}[{}]{}", &text[..start], list.join(","), &text[end..]).into_bytes()
}

#[test]
fn list_bounds_match_the_schema() {
    // Within the bound the list is read (the core digest then no longer matches: proof the
    // reader got past the list); one more entry is refused while reading.
    let at = KernelManifest::parse(&with_algorithms(MAX_ALGORITHMS)).unwrap_err();
    assert_eq!(at.code(), "core_digest_mismatch");
    let over = KernelManifest::parse(&with_algorithms(MAX_ALGORITHMS + 1)).unwrap_err();
    assert_eq!(over, ManifestError::TooLarge { what: "core.algorithms", limit: MAX_ALGORITHMS });

    let m = base();
    let art =
        |i: usize| Artifact { role: "native".into(), target: format!("t{i:04}"), sha256: m.artifacts()[0].sha256 };
    let ok = KernelManifest::new(m.core().clone(), (0..MAX_ARTIFACTS).map(art).collect()).unwrap();
    assert_eq!(KernelManifest::parse(ok.canonical_json()).unwrap(), ok);
    let too_many = KernelManifest::new(m.core().clone(), (0..=MAX_ARTIFACTS).map(art).collect());
    assert_eq!(too_many, Err(ManifestError::TooLarge { what: "artifacts", limit: MAX_ARTIFACTS }));
}

#[test]
fn length_bounds_match_the_schema() {
    let m = base();
    let with = |f: &dyn Fn(&mut ManifestCore)| {
        let mut core = m.core().clone();
        f(&mut core);
        KernelManifest::new(core, m.artifacts().to_vec()).map(|x| KernelManifest::parse(x.canonical_json()))
    };
    assert!(with(&|c| c.name = "n".repeat(MAX_TOKEN_LEN)).unwrap().is_ok());
    assert_eq!(with(&|c| c.name = "n".repeat(MAX_TOKEN_LEN + 1)), Err(ManifestError::BadValue("core.name")));
    assert!(with(&|c| c.semver = format!("1.0.0-{}", "r".repeat(MAX_TOKEN_LEN - 6))).unwrap().is_ok());
    assert_eq!(
        with(&|c| c.semver = format!("1.0.0-{}", "r".repeat(MAX_TOKEN_LEN - 5))),
        Err(ManifestError::BadValue("core.semver"))
    );
    let long_id = format!("{}.{}.{}@4294967295.0.0", "a".repeat(41), "b".repeat(41), "c".repeat(29));
    assert_eq!(long_id.len(), MAX_ID_LEN);
    let set_id = |id: String| {
        move |c: &mut ManifestCore| {
            c.algorithms.truncate(1);
            c.algorithms[0].id = AlgorithmId::parse(&id).unwrap();
        }
    };
    assert!(with(&set_id(long_id.clone())).unwrap().is_ok());
    assert_eq!(with(&set_id(format!("a{long_id}"))), Err(ManifestError::BadValue("core.algorithms[].id")));

    let target = |t: String| {
        let mut arts = m.artifacts().to_vec();
        arts.truncate(1);
        arts[0].target = t;
        KernelManifest::new(m.core().clone(), arts)
    };
    assert!(target("t".repeat(MAX_TOKEN_LEN)).is_ok());
    assert_eq!(target("t".repeat(MAX_TOKEN_LEN + 1)), Err(ManifestError::BadValue("artifacts[].target")));
}

#[test]
fn writer_refuses_what_the_reader_refuses() {
    let m = base();
    let mut core = m.core().clone();
    core.algorithms.swap(0, 1);
    assert_eq!(core.core_digest(), Err(ManifestError::Unsorted { list: "core.algorithms", index: 1 }));
    let mut arts = m.artifacts().to_vec();
    arts.push(arts[0].clone());
    assert_eq!(
        KernelManifest::new(m.core().clone(), arts),
        Err(ManifestError::Unsorted { list: "artifacts", index: 3 })
    );
}

#[test]
fn a_thousand_algorithms_parse_and_digest() {
    // Performance budget (< 5 ms) is unverified until measured; this only exercises the size.
    let err = KernelManifest::parse(&with_algorithms(1000)).unwrap_err();
    assert_eq!(err.code(), "core_digest_mismatch");
}

#[test]
fn hostile_inputs_fail_closed() {
    let bad: [&[u8]; 8] =
        [b"", b"[]", b"{", b"{\"core\":", b"\"x\"", b"{\"artifacts\":[,]}", b"\xff", b"{\"a\\ud800\":1}"];
    for bad in bad {
        let e = KernelManifest::parse(bad).unwrap_err();
        assert!(matches!(e.code(), "syntax" | "bad_value" | "unknown_field"), "{bad:?}: {e}");
    }
    // Deep nesting is refused at the first wrong type, without recursion.
    let deep = format!("{{\"artifacts\":{}", "[".repeat(100_000));
    assert_eq!(KernelManifest::parse(deep.as_bytes()).unwrap_err().code(), "bad_value");
}
