//! A dependency that hands out floats without its callers naming a float type: a public
//! `f64` field, a `Vec<f64>`, a trait method and a float constant.

pub struct Expanded {
    pub u: f64,
}

pub fn make() -> Expanded {
    Expanded { u: 0.5 }
}

pub fn many() -> Vec<f64> {
    vec![0.25, 0.75]
}

pub trait Measure {
    fn measure(&self) -> f64;
}

pub struct Probe;

impl Measure for Probe {
    fn measure(&self) -> f64 {
        0.125
    }
}

pub const LIMIT: f64 = 0.5;

pub struct Wrapper<T>(pub T);

pub fn wrapped() -> Wrapper<f64> {
    Wrapper(0.5)
}
