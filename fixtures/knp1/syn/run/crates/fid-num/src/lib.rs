pub mod f;

pub fn norm(x: f64, y: f64) -> f64 {
    crate::f::sqrt(x * x + y * y)
}
