//! AC-2: the user of the macro has no finding of its own.

pub fn use_it(x: f64) -> f64 {
    knp1_macro_dep::m!(x)
}
