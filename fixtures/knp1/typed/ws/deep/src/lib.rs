//! Review: a 70-deep recursive macro whose innermost expansion fuses.

macro_rules! deep {
    ($x:expr;) => {
        $x.mul_add(1.0, 2.0)
    };
    ($x:expr; $h:tt $($t:tt)*) => {
        deep!($x; $($t)*)
    };
}

pub fn f(x: f64) -> f64 {
    deep!(x; a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a a)
}
