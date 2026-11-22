//! Dirty KNP-1 fixture crate for KRN.NUM.LINT.CLI AC-2: one powi and one HashMap.

pub fn bad_pow(x: f64) -> f64 {
    x.powi(3)
}

pub fn bad_map() -> std::collections::HashMap<u32, u32> {
    std::collections::HashMap::new()
}
