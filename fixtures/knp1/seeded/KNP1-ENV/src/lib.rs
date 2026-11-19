//! Seeded KNP1-ENV violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn home_set() -> bool {
    std::env::var_os("HOME").is_some()
}
