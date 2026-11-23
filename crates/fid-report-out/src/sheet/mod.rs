//! Tabular report export pipeline (`RPT.OUTPUT.SHEET`).
//!
//! - [`cell`]: format-neutral table-to-cell model, unit headers, multi-unit column splitting
//!   and formula-injection neutralisation (`RPT.OUTPUT.SHEET.CELL`).

pub mod cell;

pub use cell::{convert_report, neutralise_text, Cell, CellText, HeaderCell, SheetError, SheetTable};
