//! Strict standard base64 (RFC 4648 section 4) with padding.
//!
//! Decoding accepts only the canonical form: the standard alphabet, a length that is a
//! multiple of four, at most two `=` at the very end and zero unused bits. Whitespace and
//! the URL-safe alphabet are rejected.

const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

fn sym(n: u32) -> char {
    char::from(ALPHABET.get((n & 63) as usize).copied().unwrap_or(b'A'))
}

/// Encodes `data` as padded standard base64.
pub fn encode(data: &[u8]) -> String {
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(sym(n >> 18));
        out.push(sym(n >> 12));
        out.push(if chunk.len() > 1 { sym(n >> 6) } else { '=' });
        out.push(if chunk.len() > 2 { sym(n) } else { '=' });
    }
    out
}

fn value(c: u8) -> Option<u32> {
    match c {
        b'A'..=b'Z' => Some(u32::from(c - b'A')),
        b'a'..=b'z' => Some(u32::from(c - b'a') + 26),
        b'0'..=b'9' => Some(u32::from(c - b'0') + 52),
        b'+' => Some(62),
        b'/' => Some(63),
        _ => None,
    }
}

/// Decodes canonical padded standard base64; `None` for anything else.
pub fn decode(text: &[u8]) -> Option<Vec<u8>> {
    if text.len() % 4 != 0 {
        return None;
    }
    let quads = text.len() / 4;
    let mut out = Vec::with_capacity(quads * 3);
    for (i, quad) in text.chunks(4).enumerate() {
        let pad = if i + 1 == quads { quad.iter().rev().take_while(|&&c| c == b'=').count() } else { 0 };
        if pad > 2 {
            return None;
        }
        let mut n: u32 = 0;
        for (j, &c) in quad.iter().enumerate() {
            let v = if j >= 4 - pad { 0 } else { value(c)? };
            n = (n << 6) | v;
        }
        // Unused bits of the last symbol before the padding must be zero.
        if (pad == 1 && n & 0xff != 0) || (pad == 2 && n & 0xffff != 0) {
            return None;
        }
        out.push((n >> 16) as u8);
        if pad < 2 {
            out.push((n >> 8) as u8);
        }
        if pad < 1 {
            out.push(n as u8);
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{decode, encode};

    #[test]
    fn rfc4648_vectors() {
        let cases: [(&[u8], &str); 7] = [
            (b"", ""),
            (b"f", "Zg=="),
            (b"fo", "Zm8="),
            (b"foo", "Zm9v"),
            (b"foob", "Zm9vYg=="),
            (b"fooba", "Zm9vYmE="),
            (b"foobar", "Zm9vYmFy"),
        ];
        for (raw, text) in cases {
            assert_eq!(encode(raw), text);
            assert_eq!(decode(text.as_bytes()).as_deref(), Some(raw));
        }
    }

    #[test]
    fn rejects_non_canonical() {
        for bad in ["Zg", "Zh==", "Zm9=", "Zg=a", "Z===", "Zg==Zg==", "Zm9v\n", "Zm-v", "Zm_v", "====", "Zm 9"] {
            assert_eq!(decode(bad.as_bytes()), None, "{bad:?}");
        }
    }
}
