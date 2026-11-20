//! `m!()` is called here, so rustc resolves its `include!("bin/sub/x.rs")` against `src/`:
//! it includes `src/bin/sub/x.rs`, which the scan excludes and never reads.

#[macro_use]
mod a;

pub fn f() -> u8 {
    m!()
}

// Outside a template, a plain `include!` of a listed `.rs` file is scanned like a module.
pub fn g() -> u8 {
    include!("a/bin/sub/x.rs")
}
