//! The characteristic table spike (spec node `VIZ.SHELL.SPKTABLE`), toolkit-independent part,
//! tested in CI without a GPU:
//!
//! - [`dec`]: exact decimal strings (grammar, comparison, subtraction) with no binary round
//!   trip;
//! - [`data`]: the synthetic rows `{ id, balloon, characteristic, nominal, upper, lower,
//!   actual, deviation, status, uec, unit, method, feature, note }`, generated from
//!   SplitMix64 (seed 9) as in `fixtures/viz/char-table-100k/make_table.py`;
//! - [`grid`]: column order and widths with two frozen columns, sort and filter (a pure
//!   function the example runs on a background thread), cell focus and selection, editing,
//!   TSV copy, the fly-to stub and the accessibility facts of each cell;
//! - [`script`]: the scripted keyboard sequence (AC-4), the exact-decimal check (AC-3) and
//!   the benchmark schedule (AC-1, AC-2);
//! - [`report`]: the deterministic `REPORT.md` renderer (REQ-VIZ.SHELL.SPKTABLE-005).
//!
//! The example `chartable` is the egui + winit + wgpu prototype over this module:
//!
//! ```text
//! cargo run --release -p fid-spike-ui --example chartable                    # interactive
//! cargo run --release -p fid-spike-ui --example chartable -- --bench <dir> \
//!     --hardware "<CPU, RAM, OS>" [--h1] [--font <ttf>] [--screen-reader pass|fail]  # scripted
//! ```

pub mod data;
pub mod dec;
pub mod grid;
pub mod report;
pub mod script;

/// The fixture keyboard sequence with the state expected after each step (AC-4).
pub const FIXTURE_KEYBOARD: &str = include_str!("../../../../fixtures/viz/char-table-100k/keyboard.tsv");
