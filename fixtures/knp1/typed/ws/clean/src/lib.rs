//! AC-3: `i32::max` and `BTreeMap` are fine.

use std::collections::BTreeMap;

pub fn f(a: i32, b: i32) -> i32 {
    i32::max(a, b) + a.max(b)
}

pub fn g() -> BTreeMap<u8, u8> {
    BTreeMap::new()
}
