//! Hand-written stand-in for a vendored solver (KRN.SOLV.CONIC.VEND fixture).
pub mod x;

pub fn norm2(v: &[f64]) -> f64 {
    let mut s = 0.0;
    for x in v {
        s += x.powi(2);
    }
    s
}
