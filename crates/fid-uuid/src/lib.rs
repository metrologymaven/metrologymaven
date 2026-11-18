//! A 128-bit UUID value and RFC 9562 UUIDv5 derivation, shared by the project model
//! (`fid-project`, CORE.PRJ.CONT.ID) and the device interface types (`fid-hal-idl`,
//! HAL.IDL), which may not depend on CORE (CONSTITUTION §2.1, D-035).
//!
//! Only the value type and the deterministic UUIDv5 live here; UUIDv7 generation and its
//! randomness stay in `fid-project`.

use sha1::{Digest as _, Sha1};
use std::fmt;
use std::str::FromStr;

/// Text that is not a 36-character hyphenated UUID. Stable code `bad_id`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UuidError;

impl UuidError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        "bad_id"
    }
}

impl fmt::Display for UuidError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code())
    }
}

impl std::error::Error for UuidError {}

/// A 128-bit UUID. Immutable: there is no way to change one in place.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid([u8; 16]);

impl Uuid {
    /// The nil UUID (all zero).
    pub const NIL: Uuid = Uuid([0; 16]);
    /// The max UUID (all one).
    pub const MAX: Uuid = Uuid([0xff; 16]);

    pub const fn from_bytes(bytes: [u8; 16]) -> Uuid {
        Uuid(bytes)
    }

    pub const fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    pub const fn from_u128(v: u128) -> Uuid {
        Uuid(v.to_be_bytes())
    }

    pub const fn as_u128(&self) -> u128 {
        u128::from_be_bytes(self.0)
    }

    /// The version field (top nibble of octet 6).
    pub const fn version(&self) -> u8 {
        self.0[6] >> 4
    }

    /// The top two bits of octet 8; `0b10` for RFC 9562 UUIDs.
    pub const fn variant_bits(&self) -> u8 {
        self.0[8] >> 6
    }

    /// Parses the 36-character hyphenated form (`8-4-4-4-12` hex digits, either case).
    pub fn parse(text: &str) -> Result<Uuid, UuidError> {
        let s = text.as_bytes();
        if s.len() != 36 {
            return Err(UuidError);
        }
        let mut out = [0u8; 16];
        let mut nibble = 0usize;
        for (i, &c) in s.iter().enumerate() {
            if matches!(i, 8 | 13 | 18 | 23) {
                if c != b'-' {
                    return Err(UuidError);
                }
                continue;
            }
            let v = hex_val(c).ok_or(UuidError)?;
            let slot = out.get_mut(nibble / 2).ok_or(UuidError)?;
            *slot |= if nibble % 2 == 0 { v << 4 } else { v };
            nibble += 1;
        }
        Ok(Uuid(out))
    }
}

fn hex_val(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => Some(c - b'0'),
        b'a'..=b'f' => Some(c - b'a' + 10),
        b'A'..=b'F' => Some(c - b'A' + 10),
        _ => None,
    }
}

impl fmt::Display for Uuid {
    /// Lowercase hyphenated form.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for (i, b) in self.0.iter().enumerate() {
            if matches!(i, 4 | 6 | 8 | 10) {
                f.write_str("-")?;
            }
            write!(f, "{b:02x}")?;
        }
        Ok(())
    }
}

impl FromStr for Uuid {
    type Err = UuidError;
    fn from_str(s: &str) -> Result<Uuid, UuidError> {
        Uuid::parse(s)
    }
}

/// UUIDv5 (RFC 9562 §5.5): SHA-1 of namespace bytes and name, with version and variant set.
/// SHA-1 serves identity only, not security.
pub fn uuid_v5(namespace: &Uuid, name: &[u8]) -> Uuid {
    let mut h = Sha1::new();
    h.update(namespace.as_bytes());
    h.update(name);
    let digest = h.finalize();
    let mut b = [0u8; 16];
    for (o, d) in b.iter_mut().zip(digest.iter()) {
        *o = *d;
    }
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    Uuid(b)
}
