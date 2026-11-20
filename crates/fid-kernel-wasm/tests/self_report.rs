//! KRN.MAN.MANI.GEN REQ-002 through the shim: the self-report query answers from the generated
//! constant (`src/manifest_core_digest.rs`). The committed constant is `None`, so the
//! reference table reports `manifest_core_digest_unset`; with a digest attached the answer is the
//! envelope `fixtures/man/gen-toy-v1/make_fixtures.py` (Python) computes for that digest.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use fid_kci::{AlgorithmTable, CardRegistry};
use fid_kernel_wasm::manifest_core_digest::MANIFEST_CORE_DIGEST;
use fid_kernel_wasm::shim::{reference_table, respond, with_embedded_digest, EXIT_KERNEL_ERROR, EXIT_OK};

const QUERY: &[u8] = br#"{"algorithm":"kernel.manifest.core_digest@1.0.0","inputs":[],"params":""}"#;

fn toy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/gen-toy-v1")
}

fn empty() -> AlgorithmTable {
    AlgorithmTable::new(Vec::new(), CardRegistry::default()).unwrap()
}

#[test]
fn committed_constant_is_unset_and_reported_as_such() {
    assert_eq!(MANIFEST_CORE_DIGEST, None);
    let table = reference_table().unwrap();
    assert_eq!(table.manifest_core_digest(), None);
    assert_eq!(
        respond(Some(&table), QUERY),
        (EXIT_KERNEL_ERROR, br#"{"error":"manifest_core_digest_unset"}"#.to_vec())
    );
}

#[test]
fn embedded_digest_is_reported_byte_for_byte() {
    let digest = std::fs::read_to_string(toy().join("expected-core-digest.txt")).unwrap();
    let table = with_embedded_digest(empty(), Some(digest.trim_end())).unwrap();
    let expected = std::fs::read(toy().join("expected-self-report.json")).unwrap();
    assert_eq!(respond(Some(&table), QUERY), (EXIT_OK, expected));
    // Parameters, a seed or an input are refused.
    let with_params = br#"{"algorithm":"kernel.manifest.core_digest@1.0.0","inputs":[],"params":"00"}"#;
    assert_eq!(respond(Some(&table), with_params), (EXIT_KERNEL_ERROR, br#"{"error":"invalid_params"}"#.to_vec()));
    let with_seed = br#"{"algorithm":"kernel.manifest.core_digest@1.0.0","inputs":[],"params":"","seed":"1"}"#;
    assert_eq!(respond(Some(&table), with_seed), (EXIT_KERNEL_ERROR, br#"{"error":"invalid_params"}"#.to_vec()));
}

#[test]
fn malformed_constant_gives_no_table() {
    for bad in ["", "sha256:00", "SHA256:514cdb2e3144eac24d2fab3599c5139d0689e17b3972d13181e5401b09ec5621"] {
        assert!(with_embedded_digest(empty(), Some(bad)).is_none(), "{bad:?}");
    }
    assert!(with_embedded_digest(empty(), None).unwrap().manifest_core_digest().is_none());
}
