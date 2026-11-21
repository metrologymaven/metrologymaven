//! Challenge-response proofs (REQ-AUTO.CTRL.AUTH-003, -004, -006). Spec node `AUTO.CTRL.AUTH`.
//!
//! The server's `hello` carries a fresh 32-byte nonce and its `instance_id`. The client checks
//! that `instance_id` against the discovery file it connected from, then answers `auth` with
//! its own 32-byte nonce and
//! `HMAC-SHA256(token, "fid-ctrl-client-v1" ‖ server_nonce ‖ client_nonce ‖ instance_id)`.
//! The server verifies that in constant time against its own instance ID and answers with
//! `HMAC-SHA256(token, "fid-ctrl-server-v1" ‖ client_nonce ‖ server_nonce ‖ instance_id)`,
//! which the client verifies the same way. Binding the instance ID means a proof made for one
//! app instance is useless on another that shares the token file (a relay). The labels and
//! nonces have fixed lengths and the instance ID (UTF-8) comes last, so the concatenation is
//! unambiguous. Nonces and proofs travel as 64 lowercase hex digits.
//!
//! [`Token`] has no `Debug`, `Display` or byte accessor and is zeroised on drop, so the secret
//! cannot reach a message, log line or error text through this crate.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use std::fmt;
use zeroize::Zeroize;

/// Label of the client proof (REQ-AUTO.CTRL.AUTH-003).
pub const CLIENT_LABEL: &[u8] = b"fid-ctrl-client-v1";
/// Label of the server proof (REQ-AUTO.CTRL.AUTH-004).
pub const SERVER_LABEL: &[u8] = b"fid-ctrl-server-v1";
/// Token, nonce and proof length in bytes.
pub const LEN: usize = 32;
/// The authentication operation.
pub const AUTH_OP: &str = "auth";

/// Authentication error codes (spec node AUTO.CTRL.AUTH, Design: Errors).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AuthError {
    /// The token file grants access to some other principal (REQ-AUTO.CTRL.AUTH-002).
    TokenPermissions,
    /// An operation before authentication, a wrong proof, or a malformed `auth` (REQ-005).
    AuthFailed,
    /// The token file could not be created or read, or does not hold exactly 32 bytes.
    TokenIo,
}

impl AuthError {
    /// Stable machine-readable code.
    pub fn code(&self) -> &'static str {
        match self {
            AuthError::TokenPermissions => "token_permissions",
            AuthError::AuthFailed => "auth_failed",
            AuthError::TokenIo => "token_io",
        }
    }
}

impl fmt::Display for AuthError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for AuthError {}

/// The 32-byte shared secret. No `Debug`, no `Display`, no accessor; zeroised on drop.
pub struct Token([u8; LEN]);

impl Token {
    /// Wraps token bytes (read from the token file, or a test vector).
    pub fn from_bytes(bytes: [u8; LEN]) -> Token {
        Token(bytes)
    }

    fn mac(&self, label: &[u8], first: &Nonce, second: &Nonce, instance_id: &str) -> Hmac<Sha256> {
        // HMAC accepts keys of any length, so this cannot fail; the fallback keeps the
        // no-panic rule and is never taken.
        let mut mac = match <Hmac<Sha256> as KeyInit>::new_from_slice(&self.0) {
            Ok(m) => m,
            Err(_) => <Hmac<Sha256> as KeyInit>::new(&Default::default()),
        };
        mac.update(label);
        mac.update(&first.0);
        mac.update(&second.0);
        mac.update(instance_id.as_bytes());
        mac
    }
}

impl Drop for Token {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// A 32-byte nonce. Not secret.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nonce(pub [u8; LEN]);

impl Nonce {
    /// A fresh nonce from the OS CSPRNG.
    pub fn random() -> Result<Nonce, AuthError> {
        let mut b = [0u8; LEN];
        getrandom::getrandom(&mut b).map_err(|_| AuthError::AuthFailed)?;
        Ok(Nonce(b))
    }

    /// Lowercase hex.
    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    /// Parses exactly 64 lowercase hex digits.
    pub fn from_hex(s: &str) -> Option<Nonce> {
        from_hex(s).map(Nonce)
    }
}

/// A 32-byte HMAC-SHA256 proof. Not secret (it is sent on the wire).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Proof(pub [u8; LEN]);

impl Proof {
    /// Lowercase hex.
    pub fn to_hex(&self) -> String {
        to_hex(&self.0)
    }

    /// Parses exactly 64 lowercase hex digits.
    pub fn from_hex(s: &str) -> Option<Proof> {
        from_hex(s).map(Proof)
    }
}

fn finish(mac: Hmac<Sha256>) -> Proof {
    let out = mac.finalize().into_bytes();
    let mut p = [0u8; LEN];
    p.copy_from_slice(&out[..LEN]);
    Proof(p)
}

/// `HMAC-SHA256(token, CLIENT_LABEL ‖ server_nonce ‖ client_nonce ‖ instance_id)`.
pub fn client_proof(token: &Token, instance_id: &str, server_nonce: &Nonce, client_nonce: &Nonce) -> Proof {
    finish(token.mac(CLIENT_LABEL, server_nonce, client_nonce, instance_id))
}

/// `HMAC-SHA256(token, SERVER_LABEL ‖ client_nonce ‖ server_nonce ‖ instance_id)`.
pub fn server_proof(token: &Token, instance_id: &str, client_nonce: &Nonce, server_nonce: &Nonce) -> Proof {
    finish(token.mac(SERVER_LABEL, client_nonce, server_nonce, instance_id))
}

/// Constant-time check of a client proof (the MAC crate's `verify_slice`).
pub fn verify_client_proof(
    token: &Token,
    instance_id: &str,
    server_nonce: &Nonce,
    client_nonce: &Nonce,
    proof: &Proof,
) -> bool {
    token.mac(CLIENT_LABEL, server_nonce, client_nonce, instance_id).verify_slice(&proof.0).is_ok()
}

/// Constant-time check of a server proof, for the client side (REQ-AUTO.CTRL.AUTH-004).
pub fn verify_server_proof(
    token: &Token,
    instance_id: &str,
    client_nonce: &Nonce,
    server_nonce: &Nonce,
    proof: &Proof,
) -> bool {
    token.mac(SERVER_LABEL, client_nonce, server_nonce, instance_id).verify_slice(&proof.0).is_ok()
}

fn to_hex(b: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(b.len() * 2);
    for &x in b {
        s.push(char::from(DIGITS[usize::from(x >> 4)]));
        s.push(char::from(DIGITS[usize::from(x & 0x0f)]));
    }
    s
}

fn from_hex(s: &str) -> Option<[u8; LEN]> {
    let b = s.as_bytes();
    if b.len() != 2 * LEN {
        return None;
    }
    let digit = |c: u8| match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        _ => None,
    };
    let mut out = [0u8; LEN];
    for (i, o) in out.iter_mut().enumerate() {
        *o = (digit(b[2 * i])? << 4) | digit(b[2 * i + 1])?;
    }
    Some(out)
}
