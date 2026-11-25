//! Page setup of reports (`RPT.DESIGN.PAGE`).
//!
//! - `report.set_page_layout@1` (`W2`, one journal event, a failed command journals
//!   nothing) sets the page size (ISO A4, ISO A3, US Letter or custom), orientation, four
//!   margins and the header and footer area heights of the report default or, with a page
//!   ID, of that page only; pages without an override use the default (REQ-006).
//! - `report.show_page_layout@1` (`R0`) returns every page's effective layout and body
//!   rectangle (REQ-008).
//! - Every length is an `i64` count of `0.01 mm`, parsed from `mm` or `in` text with
//!   integer arithmetic only ([`length`]); no float anywhere (D0, REQ-003, REQ-004).
//! - Landscape swaps width and height; the body is the page minus the margins, minus the
//!   header area at the top and the footer area at the bottom, and must stay at least
//!   20 mm wide and high (REQ-005).
//! - The page setup is stored beside the report document ([`LayoutBackend`]), never in it,
//!   so a layout command changes no block, binding, value, decision or formatter choice
//!   (REQ-RPT.DESIGN-007). Replay ([`replay_exact`]) reads one journal of report and
//!   page-setup events in order (REQ-007).

pub mod backend;
pub mod command;
pub mod defs;
pub mod error;
pub mod event;
pub mod length;
pub mod model;
pub mod replay;

pub use backend::{LayoutBackend, MemoryProject};
pub use command::{
    execute, set_page_layout, show_page_layout, show_value, EffectivePageLayout, SetOutcome, SetPageLayout,
    ShowPageLayout, SizeArg,
};
pub use defs::command_defs;
pub use error::LayoutError;
pub use event::{is_layout_event, LayoutEvent, SET_PAGE_LAYOUT, SHOW_PAGE_LAYOUT};
pub use length::{parse_length, Length, LengthFault, MAX_LENGTH, UNITS_PER_INCH, UNITS_PER_MM};
pub use model::{
    LayoutSource, Margins, Orientation, PageLayout, PageSize, Rect, ReportLayout, DEFAULT_LAYOUT, MAX_CUSTOM_SIDE,
    MIN_BODY, MIN_CUSTOM_SIDE,
};
pub use replay::{replay_exact, LayoutReplayRun};
