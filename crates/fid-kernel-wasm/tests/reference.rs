//! KRN.MAN.WASM: the reference build (no `test-algorithms` feature) registers no algorithm
//! until the `fid-kernel` assembly crate exists, so every call is `unknown_algorithm`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kernel_wasm::shim::{reference_table, respond, EXIT_BAD_INPUT, EXIT_KERNEL_ERROR};

const CALL: &[u8] = br#"{"algorithm":"test.echo.identity@1.0.0","inputs":[],"params":""}"#;

#[test]
fn reference_table_without_the_feature_is_empty() {
    let reference = reference_table().unwrap();
    if cfg!(feature = "test-algorithms") {
        assert!(!reference.ids().is_empty());
    } else {
        assert!(reference.ids().is_empty());
        assert_eq!(respond(Some(&reference), CALL), (EXIT_KERNEL_ERROR, br#"{"error":"unknown_algorithm"}"#.to_vec()));
    }
    assert_eq!(respond(None, b"{}"), (EXIT_BAD_INPUT, br#"{"error":"malformed_call"}"#.to_vec()));
    assert_eq!(respond(None, CALL), (EXIT_KERNEL_ERROR, br#"{"error":"table_unavailable"}"#.to_vec()));
}
