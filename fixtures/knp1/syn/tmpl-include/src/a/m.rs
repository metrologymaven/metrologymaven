// Relative to this file, `bin/sub/x.rs` is `src/a/bin/sub/x.rs`, which the scan lists.
macro_rules! m {
    () => {
        include!("bin/sub/x.rs")
    };
}
