//! Seeded KNP1-IO-FS violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn exists(p: &str) -> bool {
    std::fs::metadata(p).is_ok()
}
