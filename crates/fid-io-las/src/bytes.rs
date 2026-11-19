//! Little-endian field access that never panics. Readers check lengths before decoding;
//! an out-of-range access here yields zeros instead of a panic.

/// `N` bytes of `b` starting at `at`, or zeros if the range is outside `b`.
pub(crate) fn arr<const N: usize>(b: &[u8], at: usize) -> [u8; N] {
    let mut out = [0u8; N];
    if let Some(s) = at.checked_add(N).and_then(|end| b.get(at..end)) {
        out.copy_from_slice(s);
    }
    out
}

pub(crate) fn u8_at(b: &[u8], at: usize) -> u8 {
    b.get(at).copied().unwrap_or(0)
}

pub(crate) fn u16_at(b: &[u8], at: usize) -> u16 {
    u16::from_le_bytes(arr(b, at))
}

pub(crate) fn u32_at(b: &[u8], at: usize) -> u32 {
    u32::from_le_bytes(arr(b, at))
}

pub(crate) fn u64_at(b: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(arr(b, at))
}

pub(crate) fn i32_at(b: &[u8], at: usize) -> i32 {
    i32::from_le_bytes(arr(b, at))
}

/// An IEEE binary64 from its little-endian bits (bit-exact, NaN payloads included).
pub(crate) fn f64_at(b: &[u8], at: usize) -> f64 {
    f64::from_bits(u64_at(b, at))
}

/// Bytes `b[..]` up to (not including) the first NUL.
pub(crate) fn until_nul(b: &[u8]) -> &[u8] {
    match b.iter().position(|&c| c == 0) {
        Some(n) => b.get(..n).unwrap_or(b),
        None => b,
    }
}
