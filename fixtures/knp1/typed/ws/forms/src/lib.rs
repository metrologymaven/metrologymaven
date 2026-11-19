//! Review: function pointer, closure, type alias, associated type, generic trait impl.

pub fn fold(v: &[f64]) -> f64 {
    v.iter().copied().fold(0.0, f64::max)
}

pub fn closure(v: &[f64]) -> f64 {
    let m = |a: f64, b: f64| a.min(b);
    v.iter().copied().fold(1.0, m)
}

type F = f64;

pub fn alias(a: F, b: F) -> F {
    F::max(a, b)
}

pub trait Num {
    type T;
    fn s(x: Self::T) -> Self::T;
}

pub struct K;

impl Num for K {
    type T = f64;
    fn s(x: Self::T) -> Self::T {
        x.sqrt()
    }
}

pub trait Root {
    fn root(self) -> f64;
}

impl<T: Into<f64>> Root for T {
    fn root(self) -> f64 {
        let x: f64 = self.into();
        x.cbrt()
    }
}
