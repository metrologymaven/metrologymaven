//! Seeded KNP1-CLOCK violation: exactly one planted finding (KRN.NUM.LINT.SYN AC-1).

pub fn now_ns() -> u128 {
    let t = std::time::Instant::now();
    t.elapsed().as_nanos()
}
