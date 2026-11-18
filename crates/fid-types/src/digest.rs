//! `sha256:<64 lowercase hex>` digests.

use crate::error::{kept, TypesError};
use std::fmt;
use std::str::FromStr;

const PREFIX: &[u8; 7] = b"sha256:";
const LEN: usize = 7 + 64;
const HEX: &[u8; 16] = b"0123456789abcdef";

/// A SHA-256 digest in its wire form, `sha256:` followed by 64 lowercase hex digits.
///
/// Only valid text can be held, so [`Digest::as_str`] always returns the wire form.
/// Ordering is that of the wire strings.
#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Digest([u8; LEN]);

const fn is_lower_hex(b: u8) -> bool {
    matches!(b, b'0'..=b'9' | b'a'..=b'f')
}

const fn nibble(c: u8) -> u8 {
    if c.is_ascii_digit() {
        c - b'0'
    } else {
        c - b'a' + 10
    }
}

impl Digest {
    /// JSON Schema pattern of the wire form.
    pub const PATTERN: &'static str = "^sha256:[0-9a-f]{64}$";

    /// The tombstone digest, `sha256:` followed by 64 zeros.
    pub const TOMBSTONE: Digest = Digest::from_bytes([0; 32]);

    /// The wire form of 32 raw digest bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Digest {
        let mut out = [0u8; LEN];
        let mut i = 0;
        while i < 7 {
            out[i] = PREFIX[i];
            i += 1;
        }
        let mut j = 0;
        while j < 32 {
            out[7 + 2 * j] = HEX[(bytes[j] >> 4) as usize];
            out[8 + 2 * j] = HEX[(bytes[j] & 0x0f) as usize];
            j += 1;
        }
        Digest(out)
    }

    /// Parses the wire form. Upper-case hex, other algorithms and wrong lengths are refused.
    pub fn parse(s: &str) -> Result<Digest, TypesError> {
        let b = s.as_bytes();
        let ok = b.len() == LEN && b.starts_with(PREFIX) && b[7..].iter().all(|&c| is_lower_hex(c));
        if !ok {
            return Err(TypesError::BadDigest(kept(s)));
        }
        let mut out = [0u8; LEN];
        out.copy_from_slice(b);
        Ok(Digest(out))
    }

    /// Whether `s` is a valid wire form.
    pub fn is_valid(s: &str) -> bool {
        Digest::parse(s).is_ok()
    }

    /// The wire form.
    pub fn as_str(&self) -> &str {
        // Always ASCII by construction.
        std::str::from_utf8(&self.0).unwrap_or_default()
    }

    /// The 64 hex digits without the `sha256:` prefix.
    pub fn hex(&self) -> &str {
        self.as_str().get(7..).unwrap_or_default()
    }

    /// The 32 raw digest bytes.
    pub fn to_bytes(self) -> [u8; 32] {
        let mut out = [0u8; 32];
        for (j, o) in out.iter_mut().enumerate() {
            *o = (nibble(self.0[7 + 2 * j]) << 4) | nibble(self.0[8 + 2 * j]);
        }
        out
    }

    /// Whether this is [`Digest::TOMBSTONE`].
    pub fn is_tombstone(&self) -> bool {
        *self == Digest::TOMBSTONE
    }
}

impl fmt::Display for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

impl fmt::Debug for Digest {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Digest({:?})", self.as_str())
    }
}

impl FromStr for Digest {
    type Err = TypesError;
    fn from_str(s: &str) -> Result<Self, TypesError> {
        Digest::parse(s)
    }
}

impl From<[u8; 32]> for Digest {
    fn from(bytes: [u8; 32]) -> Self {
        Digest::from_bytes(bytes)
    }
}

impl AsRef<str> for Digest {
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tombstone_is_64_zeros() {
        assert_eq!(Digest::TOMBSTONE.as_str(), format!("sha256:{}", "0".repeat(64)));
        assert!(Digest::TOMBSTONE.is_tombstone());
        assert_eq!(Digest::TOMBSTONE.to_bytes(), [0; 32]);
    }

    #[test]
    fn bytes_round_trip() {
        let mut b = [0u8; 32];
        for (i, x) in b.iter_mut().enumerate() {
            *x = (i as u8).wrapping_mul(37).wrapping_add(11);
        }
        let d = Digest::from_bytes(b);
        assert_eq!(d.to_bytes(), b);
        assert_eq!(Digest::parse(d.as_str()), Ok(d));
        assert_eq!(d.hex().len(), 64);
        assert!(!d.is_tombstone());
        let all_ff = Digest::from_bytes([0xff; 32]);
        assert_eq!(all_ff.hex(), "f".repeat(64));
    }

    #[test]
    fn bad_forms_are_refused() {
        let hex = "a".repeat(64);
        for bad in [
            String::new(),
            "sha256:".to_string(),
            hex.clone(),
            format!("sha256:{}", "A".repeat(64)),
            format!("sha256:{}", "a".repeat(63)),
            format!("sha256:{}", "a".repeat(65)),
            format!("sha256:{}g", "a".repeat(63)),
            format!("SHA256:{hex}"),
            format!("sha512:{hex}"),
            format!(" sha256:{hex}"),
            format!("sha256:{}é", "a".repeat(62)),
        ] {
            assert_eq!(Digest::parse(&bad).map_err(|e| e.code()), Err("bad_digest"), "{bad:?}");
            assert!(!Digest::is_valid(&bad));
        }
        assert!(Digest::is_valid(&format!("sha256:{hex}")));
    }
}
