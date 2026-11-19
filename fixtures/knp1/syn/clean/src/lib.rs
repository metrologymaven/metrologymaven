//! AC-5: KNP-1-clean code: facade transcendentals and an ordered map. No findings.

use fid_num::f::sqrt;
use std::collections::BTreeMap;

pub fn table(xs: &[f64]) -> BTreeMap<u64, f64> {
    let mut m = BTreeMap::new();
    for (i, x) in xs.iter().enumerate() {
        let s = fid_num::f::sin(*x).unwrap_or(0.0) + sqrt(x.abs()).unwrap_or(0.0);
        let k = i.max(3) as u64;
        m.insert(k, s);
    }
    m
}
