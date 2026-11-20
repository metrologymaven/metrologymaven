//! Follow-up to #235: an ordinary compile error (E0277) whose wording, "expected an `FnMut(f64)`
//! closure, found `f64`", resembles a driver configuration warning. It is KNP1-TYPED-BUILD for
//! this crate, not a fatal KNP1-TYPED-DRIVER for the whole check.

pub fn total(v: &[f64]) -> f64 {
    v.iter().copied().map(1.0f64).sum()
}
