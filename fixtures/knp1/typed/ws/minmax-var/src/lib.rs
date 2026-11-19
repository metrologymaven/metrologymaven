//! AC-1: `.max(` and `.min(` on float variables.

pub fn f() -> f64 {
    let a: f64 = 1.0;
    let b = a.max(2.0);
    b
}

pub fn g(a: f64, b: f64) -> f64 { a.min(b) }
