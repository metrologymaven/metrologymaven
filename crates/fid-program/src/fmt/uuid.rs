//! UUIDv7 identifiers for programs and steps (RFC 9562 §5.7).
//!
//! Only one text form is accepted: lowercase, hyphenated 8-4-4-4-12, version 7,
//! variant `10`. One form per ID keeps the canonical bytes of a program unique.

use std::fmt;

/// A version-7 UUID. The text form is the only stored form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Uuid7([u8; 16]);

impl Uuid7 {
    /// Builds a UUIDv7 from a Unix timestamp in milliseconds (low 48 bits) and random bytes.
    /// The version and variant bits of `rand` are overwritten. Editors that add a step
    /// mint its ID with this; the format never changes an existing ID.
    pub fn from_parts(unix_ms: u64, rand: [u8; 10]) -> Self {
        let mut b = [0u8; 16];
        let ts = unix_ms.to_be_bytes();
        b[..6].copy_from_slice(&ts[2..]);
        b[6..].copy_from_slice(&rand);
        b[6] = 0x70 | (b[6] & 0x0f);
        b[8] = 0x80 | (b[8] & 0x3f);
        Uuid7(b)
    }

    /// The 16 bytes, big-endian.
    pub fn as_bytes(&self) -> &[u8; 16] {
        &self.0
    }

    /// The embedded Unix timestamp in milliseconds.
    pub fn unix_ms(&self) -> u64 {
        let mut ts = [0u8; 8];
        ts[2..].copy_from_slice(&self.0[..6]);
        u64::from_be_bytes(ts)
    }

    /// Parses the canonical text form; anything else (uppercase, braces, other
    /// versions or variants) is `None`.
    pub fn parse(s: &str) -> Option<Self> {
        let s = s.as_bytes();
        if s.len() != 36 {
            return None;
        }
        let mut out = [0u8; 16];
        let mut nibble = 0usize;
        for (i, &c) in s.iter().enumerate() {
            if matches!(i, 8 | 13 | 18 | 23) {
                if c != b'-' {
                    return None;
                }
                continue;
            }
            let v = match c {
                b'0'..=b'9' => c - b'0',
                b'a'..=b'f' => c - b'a' + 10,
                _ => return None,
            };
            let byte = out.get_mut(nibble / 2)?;
            *byte |= if nibble % 2 == 0 { v << 4 } else { v };
            nibble += 1;
        }
        if out[6] >> 4 != 7 || out[8] >> 6 != 0b10 {
            return None;
        }
        Some(Uuid7(out))
    }
}

impl fmt::Display for Uuid7 {
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
