//! The lints are force-warned: `allow` attributes cannot silence them; `g` exists only on aarch64.
#![allow(clippy::all, clippy::disallowed_methods)]

#[allow(clippy::disallowed_methods)]
pub fn f(x: f64) -> f64 {
    x.powi(2)
}

#[cfg(target_arch = "aarch64")]
pub fn g(x: f64) -> f64 {
    x.mul_add(1.0, 2.0)
}
