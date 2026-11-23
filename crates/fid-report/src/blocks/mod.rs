//! Shipped report block kinds, payload schemas, resolvers and commands
//! (spec node `RPT.MODEL.BLOCKS`).
//!
//! Implemented leaves:
//! - [`table`]: characteristic table block (`RPT.MODEL.BLOCKS.TABLE`, `table` v1,
//!   `report.set_table_block@1`).
//! - [`text`]: text, image, header and footer blocks (`RPT.MODEL.BLOCKS.TEXT`,
//!   `text`/`image`/`header`/`footer` v1, `report.set_text_block@1`,
//!   `report.set_image_block@1`).

pub mod table;
pub mod text;
