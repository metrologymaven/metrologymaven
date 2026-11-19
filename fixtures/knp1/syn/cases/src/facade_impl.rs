pub fn sqrt(x: f64) -> f64 { //~ note:KNP1-LIBM=sqrt
    x.sqrt() //~ note:KNP1-LIBM=sqrt
}

pub fn bad(v: &[f64]) -> std::collections::HashSet<u64> { //~ KNP1-HASH=HashSet
    v.iter().map(|x| x.to_bits()).collect()
}
