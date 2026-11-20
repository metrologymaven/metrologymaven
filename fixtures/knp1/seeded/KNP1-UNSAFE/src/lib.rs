//! Seeded KNP1-UNSAFE violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn first(v: &[u8]) -> u8 {
    unsafe { *v.get_unchecked(0) }
}
