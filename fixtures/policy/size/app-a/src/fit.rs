//! Fitting code in an app crate: belongs in a shared crate (POLICY-SIZE).
pub fn mean(xs: &[f64]) -> f64 {
    xs.iter().sum::<f64>() / xs.len() as f64
}
