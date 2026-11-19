//! AC-2: a local macro expanding to `x.mul_add(y, z)`, used at line 20.

macro_rules! fused {
    ($x:expr, $y:expr, $z:expr) => {
        $x.mul_add($y, $z)
    };
}

// Lines 9 to 18 are padding so that the call site is line 20.
//
//
//
//
//
//
//
//
//
pub fn f(x: f64, y: f64, z: f64) -> f64 {
    fused!(x, y, z)
}
