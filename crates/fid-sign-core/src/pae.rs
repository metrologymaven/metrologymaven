//! DSSE v1.0 pre-authentication encoding.

/// `PAE(type, body) = "DSSEv1" SP LEN(type) SP type SP LEN(body) SP body`, where `LEN` is
/// the byte length as ASCII decimal without leading zeros.
pub fn pae(payload_type: &[u8], payload: &[u8]) -> Vec<u8> {
    let type_len = payload_type.len().to_string();
    let body_len = payload.len().to_string();
    let mut out = Vec::with_capacity(10 + type_len.len() + payload_type.len() + body_len.len() + payload.len());
    out.extend_from_slice(b"DSSEv1 ");
    out.extend_from_slice(type_len.as_bytes());
    out.push(b' ');
    out.extend_from_slice(payload_type);
    out.push(b' ');
    out.extend_from_slice(body_len.as_bytes());
    out.push(b' ');
    out.extend_from_slice(payload);
    out
}

#[cfg(test)]
mod tests {
    use super::pae;

    #[test]
    fn hand_labelled() {
        assert_eq!(pae(b"", b""), b"DSSEv1 0  0 ".to_vec());
        assert_eq!(pae(b"ab", b"xyz"), b"DSSEv1 2 ab 3 xyz".to_vec());
    }
}
