//! Throwaway UI toolkit spike for spec node `VIZ.SHELL.SPKTREE`: an element tree of 10 000
//! items beside a wgpu viewport, measured against fixed pass criteria. Its report is input
//! for the maintainers' confirmation of D-005 (egui + winit + wgpu); the spike adopts
//! nothing, and this crate must never be a dependency of a product crate.
//!
//! The library is the toolkit-independent part, tested in CI without a GPU:
//!
//! - [`tree`]: synthetic items `{ id, parent, name, kind, status }` loaded from
//!   `fixtures/viz/tree-10k/tree.tsv`, rename and drag-to-reorder;
//! - [`view`]: the flattened visible-rows array (recomputed only on expand, collapse,
//!   filter or reorder), keyboard and mouse rules, multi-select, rename in place and the
//!   accessibility facts (role, name, level, expanded state) of each row;
//! - [`script`]: the scripted keyboard sequence (AC-3) and the benchmark schedule;
//! - [`stats`]: nearest-rank p95 and the 33 ms criterion;
//! - [`report`]: the deterministic `REPORT.md` renderer;
//! - [`lock`]: exact dependency versions from `Cargo.lock`.
//!
//! The example `tree10k` is the egui + winit + wgpu prototype over this library:
//!
//! ```text
//! cargo run --release -p fid-spike-ui --example tree10k                    # interactive
//! cargo run --release -p fid-spike-ui --example tree10k -- --bench <dir> \
//!     --hardware "<CPU, RAM, OS>" [--h1] [--font <ttf>]                       # scripted
//! ```
//!
//! The scripted run writes `<dir>/REPORT.md` and `<dir>/timings.csv`.
//!
//! Item names are file data (CONSTITUTION C6): they are held as
//! [`fid_hal_idl::UntrustedText`] and shown only in escaped form.

pub mod lock;
pub mod report;
pub mod script;
pub mod stats;
pub mod tree;
pub mod view;

/// The fixture tree (10 000 items, 6 levels, seed 5), embedded for the example and tests.
pub const FIXTURE_TREE: &str = include_str!("../../../fixtures/viz/tree-10k/tree.tsv");
/// The fixture keyboard sequence with expected focus and selection (AC-3).
pub const FIXTURE_KEYBOARD: &str = include_str!("../../../fixtures/viz/tree-10k/keyboard.tsv");

/// Runs a keyboard script against a fresh view of `tree` and returns, per step, whether
/// focus, selection and focused name matched.
pub fn run_keyboard(tree: &mut tree::Tree, steps: &[script::Step]) -> Vec<bool> {
    let mut v = view::TreeView::new(tree);
    let mut out = Vec::with_capacity(steps.len());
    for s in steps {
        v.apply(tree, &s.action);
        let sel: Vec<u32> = v.selection().iter().copied().collect();
        let name = v.focus().and_then(|f| tree.get(f)).map(|i| i.name.clone());
        let want_name = s.name.as_deref().map(fid_hal_idl::UntrustedText::new);
        out.push(v.focus() == s.focus && sel == s.selection && name == want_name);
    }
    out
}
