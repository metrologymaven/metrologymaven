//! The few strict DER structures this crate reads and writes: SubjectPublicKeyInfo
//! (RFC 5280 section 4.1, RFC 5480, RFC 8410) and the ECDSA `Ecdsa-Sig-Value`
//! SEQUENCE { r INTEGER, s INTEGER }. Lengths must be definite and minimal; nothing
//! is accepted that BER would allow but DER forbids.

use crate::SigError;

pub(crate) const TAG_INTEGER: u8 = 0x02;
pub(crate) const TAG_BIT_STRING: u8 = 0x03;
pub(crate) const TAG_OID: u8 = 0x06;
pub(crate) const TAG_SEQUENCE: u8 = 0x30;

/// 1.3.101.112 (id-Ed25519, RFC 8410).
pub(crate) const OID_ED25519: &[u8] = b"\x2b\x65\x70";
/// 1.2.840.10045.2.1 (id-ecPublicKey, RFC 5480).
pub(crate) const OID_EC_PUBLIC_KEY: &[u8] = b"\x2a\x86\x48\xce\x3d\x02\x01";
/// 1.2.840.10045.3.1.7 (secp256r1 / prime256v1, RFC 5480).
pub(crate) const OID_P256: &[u8] = b"\x2a\x86\x48\xce\x3d\x03\x01\x07";

/// The P-256 group order n, big endian.
pub(crate) const P256_N: [u8; 32] = [
    0xff, 0xff, 0xff, 0xff, 0x00, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xbc, 0xe6, 0xfa,
    0xad, 0xa7, 0x17, 0x9e, 0x84, 0xf3, 0xb9, 0xca, 0xc2, 0xfc, 0x63, 0x25, 0x51,
];
/// floor(n / 2): an S above this is "high".
pub(crate) const P256_HALF_N: [u8; 32] = [
    0x7f, 0xff, 0xff, 0xff, 0x80, 0x00, 0x00, 0x00, 0x7f, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff, 0xde, 0x73, 0x7d,
    0x56, 0xd3, 0x8b, 0xcf, 0x42, 0x79, 0xdc, 0xe5, 0x61, 0x7e, 0x31, 0x92, 0xa8,
];

/// Reads one TLV: `(tag, content, rest)`. Single-byte tags; lengths in short form or
/// minimal long form up to two length bytes.
pub(crate) fn tlv(input: &[u8]) -> Option<(u8, &[u8], &[u8])> {
    let (&tag, rest) = input.split_first()?;
    let (&first, rest) = rest.split_first()?;
    let (len, rest) = match first {
        0x00..=0x7f => (usize::from(first), rest),
        0x81 => {
            let (&l, rest) = rest.split_first()?;
            if l < 0x80 {
                return None;
            }
            (usize::from(l), rest)
        }
        0x82 => {
            let (&hi, rest) = rest.split_first()?;
            let (&lo, rest) = rest.split_first()?;
            let l = (usize::from(hi) << 8) | usize::from(lo);
            if l < 0x100 {
                return None;
            }
            (l, rest)
        }
        _ => return None,
    };
    if rest.len() < len {
        return None;
    }
    let (content, rest) = rest.split_at(len);
    Some((tag, content, rest))
}

/// `input` must be exactly one TLV with `tag`; returns its content.
pub(crate) fn exactly(input: &[u8], tag: u8) -> Option<&[u8]> {
    match tlv(input)? {
        (t, content, []) if t == tag => Some(content),
        _ => None,
    }
}

/// Appends a TLV with a minimal DER length.
pub(crate) fn push_tlv(out: &mut Vec<u8>, tag: u8, content: &[u8]) {
    out.push(tag);
    let len = content.len();
    if len < 0x80 {
        out.push(len as u8);
    } else if len < 0x100 {
        out.push(0x81);
        out.push(len as u8);
    } else {
        out.push(0x82);
        out.push((len >> 8) as u8);
        out.push(len as u8);
    }
    out.extend_from_slice(content);
}

/// The key inside a SubjectPublicKeyInfo.
pub(crate) enum SpkiKey<'a> {
    Ed25519(&'a [u8]),
    P256(&'a [u8]),
}

fn bad_spki() -> SigError {
    SigError::Malformed("SubjectPublicKeyInfo is not valid DER")
}

/// Parses a SubjectPublicKeyInfo. Unknown algorithms (RSA, other curves, ...) are
/// `UnsupportedAlgorithm`; structural problems are `Malformed`.
pub(crate) fn parse_spki(der: &[u8]) -> Result<SpkiKey<'_>, SigError> {
    let body = exactly(der, TAG_SEQUENCE).ok_or_else(bad_spki)?;
    let (tag, alg, rest) = tlv(body).ok_or_else(bad_spki)?;
    if tag != TAG_SEQUENCE {
        return Err(bad_spki());
    }
    let bits = exactly(rest, TAG_BIT_STRING).ok_or_else(bad_spki)?;
    let (tag, oid, params) = tlv(alg).ok_or_else(bad_spki)?;
    if tag != TAG_OID {
        return Err(bad_spki());
    }
    let key = match bits.split_first() {
        Some((0, key)) => key,
        _ => return Err(bad_spki()),
    };
    if oid == OID_ED25519 {
        // RFC 8410: the parameters are absent.
        if !params.is_empty() {
            return Err(bad_spki());
        }
        Ok(SpkiKey::Ed25519(key))
    } else if oid == OID_EC_PUBLIC_KEY {
        let curve = exactly(params, TAG_OID).ok_or_else(bad_spki)?;
        if curve == OID_P256 {
            Ok(SpkiKey::P256(key))
        } else {
            Err(SigError::UnsupportedAlgorithm)
        }
    } else {
        Err(SigError::UnsupportedAlgorithm)
    }
}

/// SubjectPublicKeyInfo DER for an Ed25519 key.
pub(crate) fn spki_ed25519(key: &[u8; 32]) -> Vec<u8> {
    let mut alg = Vec::new();
    push_tlv(&mut alg, TAG_OID, OID_ED25519);
    spki(&alg, key)
}

/// SubjectPublicKeyInfo DER for an uncompressed P-256 point.
pub(crate) fn spki_p256(point: &[u8; 65]) -> Vec<u8> {
    let mut alg = Vec::new();
    push_tlv(&mut alg, TAG_OID, OID_EC_PUBLIC_KEY);
    push_tlv(&mut alg, TAG_OID, OID_P256);
    spki(&alg, point)
}

fn spki(alg_content: &[u8], key: &[u8]) -> Vec<u8> {
    let mut body = Vec::new();
    push_tlv(&mut body, TAG_SEQUENCE, alg_content);
    let mut bits = Vec::with_capacity(key.len() + 1);
    bits.push(0);
    bits.extend_from_slice(key);
    push_tlv(&mut body, TAG_BIT_STRING, &bits);
    let mut out = Vec::new();
    push_tlv(&mut out, TAG_SEQUENCE, &body);
    out
}

/// Copies a strictly DER-encoded positive INTEGER into `out`, right aligned.
fn read_uint(int: &[u8], out: &mut [u8]) -> Option<()> {
    let (&first, tail) = int.split_first()?;
    if first & 0x80 != 0 {
        return None; // negative
    }
    let magnitude = if first == 0 {
        match tail.first() {
            Some(&next) if next & 0x80 != 0 => tail,
            _ => return None, // zero, or a superfluous leading zero
        }
    } else {
        int
    };
    let start = out.len().checked_sub(magnitude.len())?;
    out.get_mut(start..)?.copy_from_slice(magnitude);
    Some(())
}

/// Strict DER `Ecdsa-Sig-Value` to fixed `r || s` (32 + 32 bytes). `None` for any
/// non-canonical encoding.
pub(crate) fn ecdsa_from_der(der: &[u8]) -> Option<[u8; 64]> {
    let body = exactly(der, TAG_SEQUENCE)?;
    let (tag, r, rest) = tlv(body)?;
    if tag != TAG_INTEGER {
        return None;
    }
    let s = exactly(rest, TAG_INTEGER)?;
    let mut out = [0u8; 64];
    let (r_out, s_out) = out.split_at_mut(32);
    read_uint(r, r_out)?;
    read_uint(s, s_out)?;
    Some(out)
}

fn push_uint(out: &mut Vec<u8>, value: &[u8]) {
    let skip = value.iter().take_while(|&&b| b == 0).count();
    let magnitude = value.get(skip..).unwrap_or(&[]);
    let mut content = Vec::with_capacity(magnitude.len() + 1);
    if magnitude.first().map_or(true, |&b| b & 0x80 != 0) {
        content.push(0);
    }
    content.extend_from_slice(magnitude);
    push_tlv(out, TAG_INTEGER, &content);
}

/// Fixed `r || s` to DER `Ecdsa-Sig-Value`.
pub(crate) fn ecdsa_to_der(rs: &[u8; 64]) -> Vec<u8> {
    let (r, s) = rs.split_at(32);
    let mut body = Vec::with_capacity(70);
    push_uint(&mut body, r);
    push_uint(&mut body, s);
    let mut out = Vec::with_capacity(72);
    push_tlv(&mut out, TAG_SEQUENCE, &body);
    out
}

/// `s > floor(n / 2)` for a 32-byte big-endian S.
pub(crate) fn is_high_s(s: &[u8]) -> bool {
    s > &P256_HALF_N[..]
}

/// Replaces a high S by n - S.
pub(crate) fn normalize_low_s(rs: &mut [u8; 64]) {
    let s = &mut rs[32..];
    if !is_high_s(s) {
        return;
    }
    let mut borrow = 0i16;
    for (byte, &n) in s.iter_mut().rev().zip(P256_N.iter().rev()) {
        let d = i16::from(n) - i16::from(*byte) - borrow;
        borrow = i16::from(d < 0);
        *byte = (d + 256 * borrow) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::{ecdsa_from_der, ecdsa_to_der, tlv};

    #[test]
    fn lengths_must_be_minimal() {
        assert!(tlv(b"\x04\x01\xaa").is_some());
        assert!(tlv(b"\x04\x81\x01\xaa").is_none());
        assert!(tlv(b"\x04\x80").is_none());
        assert!(tlv(b"\x04\x02\xaa").is_none());
    }

    #[test]
    fn ecdsa_round_trip_and_strictness() {
        let mut rs = [0u8; 64];
        rs[31] = 1;
        rs[32] = 0x80;
        let der = ecdsa_to_der(&rs);
        let mut expected = vec![0x30, 0x26, 0x02, 0x01, 0x01, 0x02, 0x21, 0x00, 0x80];
        expected.extend_from_slice(&[0u8; 31]);
        assert_eq!(der, expected);
        assert_eq!(ecdsa_from_der(&der), Some(rs));
        // Superfluous leading zero, negative r, zero r, trailing byte.
        assert_eq!(ecdsa_from_der(b"\x30\x07\x02\x02\x00\x01\x02\x01\x01"), None);
        assert_eq!(ecdsa_from_der(b"\x30\x06\x02\x01\x81\x02\x01\x01"), None);
        assert_eq!(ecdsa_from_der(b"\x30\x06\x02\x01\x00\x02\x01\x01"), None);
        assert_eq!(ecdsa_from_der(b"\x30\x06\x02\x01\x01\x02\x01\x01\x00"), None);
        assert!(ecdsa_from_der(b"\x30\x06\x02\x01\x01\x02\x01\x01").is_some());
    }
}
