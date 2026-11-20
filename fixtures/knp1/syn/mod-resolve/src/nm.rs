// Non-mod-rs: `mod child;` loads src/nm/child.rs; an out-of-line `#[path]` outside inline
// modules is relative to this file's directory (src/side.rs); inside the inline module `inl` it
// is relative to src/nm/inl/.
mod child;
#[path = "side.rs"]
mod side;
mod inl {
    #[path = "deep.rs"]
    mod deep;
}
