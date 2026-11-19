// `use` renames are merged for the whole file (fail closed): this inner alias makes every
// `fid_num::…` path in the file resolve to `std::primitive::f64::…`, which is not the facade.
mod inner {
    use std::primitive::f64 as fid_num;
}

pub fn c(x: f64) -> f64 {
    fid_num::f::sqrt(x) //~ KNP1-LIBM=sqrt
}
