use std::collections::HashMap;

pub fn count(keys: &[u32]) -> HashMap<u32, usize> {
    let mut m = HashMap::new();
    for k in keys {
        *m.entry(*k).or_insert(0) += 1;
    }
    m
}
