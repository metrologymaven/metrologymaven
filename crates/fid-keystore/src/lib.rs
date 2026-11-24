//! OS keystore contract, sidecar metadata, test backend and conformance suite (spec node
//! `TRUST.SIGN.KEYSTORE.TRAIT`).
//!
//! - [`api`]: [`api::KeyStore`] trait, [`api::KeyInfo`], opaque [`api::KeyHandle`],
//!   single-use [`api::UvRelease`] / [`api::ReleasedSigner`], [`api::KeyStoreError`], and
//!   RFC 8785 canonical JSON sidecar metadata at `<config>/fid/keys/<fingerprint>.json`.
//! - [`mem`] (feature `test-backend` only): in-memory [`mem::MemKeyStore`] backed by
//!   `fid-sign-core`'s `test-signer`, never compiled into release builds.
//! - [`conformance`]: reusable conformance test suite every OS backend must pass in CI.
//!
//! Security boundary (REQ-TRUST.SIGN-004, REQ-TRUST.SIGN.KEYSTORE.TRAIT-001): no method or type
//! of this crate returns or exposes private key bytes, and no private-key type implements
//! `Serialize`, `Display` or content-bearing `Debug`.

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

#[cfg(all(feature = "test-backend", not(debug_assertions)))]
compile_error!(
    "fid-keystore feature `test-backend` holds private keys in memory and must never be enabled in release builds (REQ-TRUST.SIGN.KEYSTORE.TRAIT-005)"
);

pub mod api;
pub mod conformance;
#[cfg(feature = "test-backend")]
pub mod mem;

pub use api::{
    decode_sidecar, delete_sidecar, encode_sidecar, is_valid_uv_method, list_sidecars, read_sidecar, sidecar_dir,
    sidecar_path, validate_label, validate_rfc3339_utc, write_sidecar, KeyHandle, KeyHandleOps, KeyInfo, KeyStore,
    KeyStoreError, ReleasedSigner, UvRelease, MAX_LABEL_BYTES, MAX_SIDECAR_BYTES, UV_METHODS,
};
pub use fid_sign_core::{Alg, Fingerprint, PublicKey, SignError, Signer};
