//! Seeded KNP1-HASH violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn distinct(v: &[u32]) -> usize {
    v.iter().collect::<std::collections::HashSet<_>>().len()
}
