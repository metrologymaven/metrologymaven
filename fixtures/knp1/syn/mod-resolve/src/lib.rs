//! Module resolution fixture; `src/bin/**` is excluded from the scan.

// (a) `#[path]` inside an inline module is relative to src/bin/, so rustc loads src/bin/x.rs
// (not the listed src/x.rs); (b) a plain `mod w;` there loads src/bin/w.rs.
mod bin {
    #[path = "x.rs"]
    mod x;
    mod w;
}

// Nested inline modules: src/n1/n2/leaf.rs (listed).
mod n1 {
    mod n2 {
        mod leaf;
    }
}

// An inline module with `#[path]`: its directory is src/alt/, so src/alt/inner.rs (listed).
#[path = "alt"]
mod pa {
    mod inner;
}

// A non-mod-rs file: its modules live in src/nm/.
pub mod nm;

// `cfg_attr(…, path = …)`: the scan cannot tell which file rustc loads.
#[cfg_attr(unix, path = "n1/n2/leaf.rs")]
mod ca;
