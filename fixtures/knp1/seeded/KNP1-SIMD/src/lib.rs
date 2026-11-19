//! Seeded KNP1-SIMD violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn has_avx2() -> bool {
    is_x86_feature_detected!("avx2")
}
