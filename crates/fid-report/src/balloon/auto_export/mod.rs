//! Quick ballooned-PDF and AS9102 Form 3 starter pack export (`RPT.BALLOON.AUTO.EXPORT`).
//!
//! - `report.export_balloon_pack@1` (`W1`, `write_root`, `D0`): writes deterministic
//!   vector-overlaid `ballooned.pdf` and formula-injection-safe `form3-starter.csv` into an
//!   approved `IO.REG.ROOTS` export root, returning SHA-256 digests and journaling the export.

pub mod command;
pub mod csv_form3;
pub mod pdf_overlay;

pub use command::{
    command_def, export_balloon_pack, BalloonPackResult, ExportBalloonPackEvent, ExportBalloonPackParams,
    ExportPackError, EXPORT_BALLOON_PACK,
};
pub use csv_form3::{csv_cell_untrusted, render_form3_starter_csv, EvaluatedRowValue};
pub use pdf_overlay::{render_ballooned_pdf, BalloonOutcome};
