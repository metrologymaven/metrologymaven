//! RFC 9562 §5.5 UUIDv5 over a private FIPS 180-4 SHA-1, for the registration group ID
//! (REQ-SCAN.CLOUD.REG.RREP-005).
//!
//! SHA-1 serves identity only (RFC 9562 requires it for version 5), never security. It is a
//! private copy here because `fid-uuid` is a model-layer crate over the RustCrypto `sha1` crate
//! and `fid-geo-cad` keeps its own copy crate-private; a kernel crate may reach neither the
//! model layer nor a third-party crate (layers.toml, kernel-allowlist.toml). The output matches
//! Python's `uuid.uuid5` byte for byte: the unit tests check the FIPS 180-4 vectors and an RFC
//! 9562 example, and the fixture group IDs come from Python's stdlib.
//!
//! Every array index below is a constant or masked with `& 15`, so nothing here can panic.

const H0: [u32; 5] = [0x6745_2301, 0xEFCD_AB89, 0x98BA_DCFE, 0x1032_5476, 0xC3D2_E1F0];

/// One SHA-1 compression of a 64-byte block, with the 16-word rolling message schedule.
fn compress(state: &mut [u32; 5], block: &[u8]) {
    let mut w = [0u32; 16];
    for (slot, c) in w.iter_mut().zip(block.chunks_exact(4)) {
        let mut b = [0u8; 4];
        b.copy_from_slice(c);
        *slot = u32::from_be_bytes(b);
    }
    let [mut a, mut b, mut c, mut d, mut e] = *state;
    for t in 0..80usize {
        let wt = if t < 16 {
            w[t & 15]
        } else {
            let v = (w[(t + 13) & 15] ^ w[(t + 8) & 15] ^ w[(t + 2) & 15] ^ w[t & 15]).rotate_left(1);
            w[t & 15] = v;
            v
        };
        let (f, k) = match t {
            0..=19 => ((b & c) | (!b & d), 0x5A82_7999),
            20..=39 => (b ^ c ^ d, 0x6ED9_EBA1),
            40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1B_BCDC),
            _ => (b ^ c ^ d, 0xCA62_C1D6),
        };
        let tmp = a.rotate_left(5).wrapping_add(f).wrapping_add(e).wrapping_add(k).wrapping_add(wt);
        e = d;
        d = c;
        c = b.rotate_left(30);
        b = a;
        a = tmp;
    }
    for (s, v) in state.iter_mut().zip([a, b, c, d, e]) {
        *s = s.wrapping_add(v);
    }
}

/// SHA-1 of the concatenation of `parts` (FIPS 180-4 §6.1; padding §5.1.1).
pub(crate) fn sha1(parts: &[&[u8]]) -> [u8; 20] {
    let len: usize = parts.iter().map(|p| p.len()).sum();
    let mut msg = Vec::with_capacity(len + 72);
    for p in parts {
        msg.extend_from_slice(p);
    }
    let bit_len = u64::try_from(len).unwrap_or(u64::MAX).wrapping_mul(8);
    msg.push(0x80);
    while msg.len() % 64 != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());
    let mut state = H0;
    for block in msg.chunks_exact(64) {
        compress(&mut state, block);
    }
    let mut out = [0u8; 20];
    for (o, s) in out.chunks_exact_mut(4).zip(state) {
        o.copy_from_slice(&s.to_be_bytes());
    }
    out
}

/// UUIDv5 of `name` in `namespace`: the first 16 bytes of SHA-1(namespace | name) with the
/// version nibble set to 5 and the RFC 9562 variant bits `10`.
pub(crate) fn uuid_v5(namespace: &[u8; 16], name: &[u8]) -> [u8; 16] {
    let h = sha1(&[namespace, name]);
    let mut b = [0u8; 16];
    for (o, x) in b.iter_mut().zip(h) {
        *o = x;
    }
    b[6] = (b[6] & 0x0f) | 0x50;
    b[8] = (b[8] & 0x3f) | 0x80;
    b
}

/// Hyphenated lowercase form `xxxxxxxx-xxxx-xxxx-xxxx-xxxxxxxxxxxx`.
pub(crate) fn format_uuid(b: &[u8; 16]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(36);
    for (i, &x) in b.iter().enumerate() {
        if matches!(i, 4 | 6 | 8 | 10) {
            s.push('-');
        }
        s.push(char::from(HEX[usize::from(x >> 4)]));
        s.push(char::from(HEX[usize::from(x & 15)]));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(b: &[u8]) -> String {
        b.iter().map(|x| format!("{x:02x}")).collect()
    }

    #[test]
    fn sha1_fips_vectors() {
        // FIPS 180-4 examples (also RFC 3174 §7.3), plus the empty message and the
        // two-block padding boundary (55 and 56 bytes).
        assert_eq!(hex(&sha1(&[b"abc"])), "a9993e364706816aba3e25717850c26c9cd0d89d");
        assert_eq!(
            hex(&sha1(&[b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq"])),
            "84983e441c3bd26ebaae4aa1f95129e5e54670f1"
        );
        assert_eq!(hex(&sha1(&[b""])), "da39a3ee5e6b4b0d3255bfef95601890afd80709");
        assert_eq!(hex(&sha1(&[b"ab", b"c"])), "a9993e364706816aba3e25717850c26c9cd0d89d");
        let million = vec![b'a'; 1_000_000];
        assert_eq!(hex(&sha1(&[&million])), "34aa973cd4c4daa4f61eeb2bdbad27316534016f");
    }

    #[test]
    fn uuid_v5_matches_python() {
        // RFC 9562 Appendix A.4: UUIDv5 of "www.example.com" in the DNS namespace.
        let dns = [0x6b, 0xa7, 0xb8, 0x10, 0x9d, 0xad, 0x11, 0xd1, 0x80, 0xb4, 0x00, 0xc0, 0x4f, 0xd4, 0x30, 0xc8];
        assert_eq!(format_uuid(&uuid_v5(&dns, b"www.example.com")), "2ed6657d-e927-568b-95e1-2665a8aea6a2");
    }
}
