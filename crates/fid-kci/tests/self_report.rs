//! The self-report query `kernel.manifest.core_digest@1.0.0` (spec node KRN.MAN.MANI.GEN,
//! REQ-002): answered by `evaluate` from the digest attached to the table, with no file. The
//! expected envelope comes from `fixtures/man/gen-toy-v1/make_fixtures.py` (Python), never from
//! this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

use fid_kci::kci::{sha256_digest, RESERVED_MODULE};
use fid_kci::{
    evaluate, AlgorithmId, AlgorithmTable, CardRegistry, Digest, InputArity, KernelAlgorithm, KernelCall, KernelError,
    MANIFEST_CORE_DIGEST_QUERY,
};

fn toy() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/man/gen-toy-v1")
}

fn core_digest() -> Digest {
    let text = std::fs::read_to_string(toy().join("expected-core-digest.txt")).unwrap();
    Digest::parse(text.trim_end()).unwrap()
}

fn query() -> KernelCall {
    KernelCall {
        algorithm: AlgorithmId::parse(MANIFEST_CORE_DIGEST_QUERY).unwrap(),
        params: vec![],
        seed: None,
        inputs: vec![],
    }
}

fn empty() -> AlgorithmTable {
    AlgorithmTable::new(Vec::new(), CardRegistry::default()).unwrap()
}

#[test]
fn query_reports_the_attached_core_digest() {
    let table = empty().with_manifest_core_digest(core_digest());
    assert_eq!(table.manifest_core_digest(), Some(&core_digest()));
    let result = evaluate(&table, &query()).unwrap();
    let expected = std::fs::read(toy().join("expected-self-report.json")).unwrap();
    assert_eq!(result.canonical_json(), expected);
    assert_eq!(result.output(), core_digest().as_str().as_bytes());
}

#[test]
fn query_without_a_digest_is_unset() {
    let table = empty();
    assert_eq!(table.manifest_core_digest(), None);
    let e = evaluate(&table, &query()).unwrap_err();
    assert_eq!(e, KernelError::ManifestCoreDigestUnset);
    assert_eq!(e.code(), "manifest_core_digest_unset");
}

#[test]
fn query_accepts_only_the_empty_call() {
    let table = empty().with_manifest_core_digest(core_digest());
    let mut c = query();
    c.inputs.push((b"x".to_vec(), sha256_digest(b"x")));
    assert_eq!(
        evaluate(&table, &c).unwrap_err(),
        KernelError::InputArityMismatch { expected: InputArity::exactly(0), got: 1 }
    );
    let mut c = query();
    c.params = b"{}".to_vec();
    assert_eq!(evaluate(&table, &c).unwrap_err(), KernelError::InvalidParams { reason: "params_not_empty" });
    let mut c = query();
    c.seed = Some(0);
    assert_eq!(evaluate(&table, &c).unwrap_err(), KernelError::InvalidParams { reason: "seed_not_accepted" });
    // Another version, or another query of the reserved module, is unknown.
    for id in ["kernel.manifest.core_digest@1.0.1", "kernel.manifest.digest@1.0.0", "kernel.other.core_digest@1.0.0"] {
        let c = KernelCall { algorithm: AlgorithmId::parse(id).unwrap(), ..query() };
        assert_eq!(evaluate(&table, &c).unwrap_err().code(), "unknown_algorithm", "{id}");
    }
}

struct Named(&'static str);

impl KernelAlgorithm for Named {
    fn id(&self) -> AlgorithmId {
        AlgorithmId::parse(self.0).unwrap()
    }
    fn input_arity(&self) -> InputArity {
        InputArity::exactly(0)
    }
    fn validate(&self, _: &[u8]) -> Result<(), KernelError> {
        Ok(())
    }
    fn run(&self, _: &[u8], _: Option<u64>, _: &[&[u8]]) -> Result<Vec<u8>, KernelError> {
        Ok(b"impostor".to_vec())
    }
}

#[test]
fn no_algorithm_may_register_under_the_reserved_module() {
    assert_eq!(RESERVED_MODULE, "kernel");
    for id in [MANIFEST_CORE_DIGEST_QUERY, "kernel.any.thing@2.0.0"] {
        let e = AlgorithmTable::new(vec![Box::new(Named(id))], CardRegistry::default()).unwrap_err();
        assert_eq!(e, KernelError::ReservedAlgorithm(AlgorithmId::parse(id).unwrap()));
        assert_eq!(e.code(), "reserved_algorithm");
    }
    // A neighbouring module is an ordinary algorithm.
    assert!(AlgorithmTable::new(vec![Box::new(Named("kernels.any.thing@1.0.0"))], CardRegistry::default()).is_ok());
}

/// The reserved module is refused in a manifest's algorithm list and in a spec card.
#[test]
fn reserved_module_is_refused_in_manifests_and_cards() {
    let m = fid_kci::KernelManifest::parse(&std::fs::read(toy().join("expected-core.json")).unwrap()).unwrap();
    let mut core = m.core().clone();
    core.algorithms[0].id = AlgorithmId::parse("kernel.plane.lsq@1.10.0").unwrap();
    assert_eq!(core.validate(), Err(fid_kci::ManifestError::BadValue("core.algorithms[].id")));
    let card = std::fs::read_to_string(toy().join("cards/fit-plane-lsq.toml")).unwrap();
    assert!(fid_kci::parse_card(&card).is_ok());
    let reserved = card.replace("id = \"fit.plane.lsq@1.10.0\"", "id = \"kernel.plane.lsq@1.10.0\"");
    assert_eq!(fid_kci::parse_card(&reserved).unwrap_err().code(), "bad_id");
}
