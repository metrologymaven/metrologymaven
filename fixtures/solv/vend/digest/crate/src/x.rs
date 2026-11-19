use std::collections::BTreeMap;

pub fn count(keys: &[u32]) -> BTreeMap<u32, usize> {
    let mut m = BTreeMap::new();
    for k in keys {
        *m.entry(*k).or_insert(0) += 1;
    }
    m
}
