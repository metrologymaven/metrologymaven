use std::collections::HashMap as M;

pub fn table() -> usize {
    M::<u8, u8>::new().len()
}
