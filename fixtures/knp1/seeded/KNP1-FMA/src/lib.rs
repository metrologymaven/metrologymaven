//! Seeded KNP1-FMA violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn fused(a: f64, b: f64, c: f64) -> f64 {
    a.mul_add(b, c)
}
