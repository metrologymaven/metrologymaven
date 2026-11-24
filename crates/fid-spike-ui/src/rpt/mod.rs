//! The report designer canvas spike (spec node `VIZ.SHELL.SPKRPT`), toolkit-independent
//! part, tested in CI without a GPU:
//!
//! - [`canvas`]: pages (A4 or Letter) and blocks `{ kind, rect, content }` loaded from
//!   `fixtures/viz/report-canvas-200/canvas.tsv`, CPU hit testing and the accessibility
//!   facts (role; type, name and position) of each block;
//! - [`edit`]: focus, keyboard move and resize, text editing, pointer drag with snapping to
//!   the grid and to guides, zoom, undo and redo;
//! - [`layout`]: the zoom transform from millimetre space to logical pixels, multi-page
//!   layout and the pointer hit test;
//! - [`script`]: the scripted keyboard sequence (AC-3), the drag cases and the benchmark
//!   schedule (AC-1, AC-2);
//! - [`report`]: the deterministic `REPORT.md` renderer (AC-5).
//!
//! The example `rptcanvas` is the egui + winit + wgpu prototype over this module:
//!
//! ```text
//! cargo run --release -p fid-spike-ui --example rptcanvas                    # interactive
//! cargo run --release -p fid-spike-ui --example rptcanvas -- --bench <dir> \
//!     --hardware "<CPU, RAM, OS>" [--h1] [--font <ttf>] [--cjk-font <ttf or ttc>]  # scripted
//! ```

pub mod canvas;
pub mod edit;
pub mod layout;
pub mod report;
pub mod script;

/// The fixture canvas (5 pages, 200 blocks, seed 13), embedded for the example and tests.
pub const FIXTURE_CANVAS: &str = include_str!("../../../../fixtures/viz/report-canvas-200/canvas.tsv");
/// The fixture keyboard sequence with the expected state after each step (AC-3).
pub const FIXTURE_KEYBOARD: &str = include_str!("../../../../fixtures/viz/report-canvas-200/keyboard.tsv");
/// The fixture drag cases with expected snapped positions.
pub const FIXTURE_DRAG: &str = include_str!("../../../../fixtures/viz/report-canvas-200/drag.tsv");
/// The fixture accessibility labels of every block.
pub const FIXTURE_A11Y: &str = include_str!("../../../../fixtures/viz/report-canvas-200/a11y.tsv");
