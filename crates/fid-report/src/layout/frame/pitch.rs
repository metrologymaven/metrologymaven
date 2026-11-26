//! The grid pitch (`RPT.DESIGN.PLACE.FRAME` REQ-002, `Design: Data structures`).
//!
//! `report.grid_pitch` is a user-scope setting declared here through `CORE.STO.SET`
//! ([`report_settings`]): an integer count of `0.01 mm`, at least 10 (`0.1 mm`), default 100
//! (`1 mm`). It is an integer, not a float quantity, because RPT geometry is integer only;
//! the settings schema's `minimum` makes `settings.set_value@1` refuse a smaller value
//! before anything is stored.
//!
//! Report code reads and checks it only through [`GridPitch`]:
//!
//! - [`GridPitch::parse`] turns typed text (`0.5mm`, `0.05in`) into a pitch, and
//!   [`GridPitch::new`] checks a count of units; below 10 units either fails with
//!   `report.layout_invalid { field: grid_pitch, reason: below_minimum }`.
//! - [`GridPitch::from_snapshot`] reads the resolved setting and fails closed: a stored value
//!   the resolver warned about (say a hand-edited `5` in the user file, which CORE.STO.SET
//!   would replace by the default) is `report.layout_invalid { grid_pitch,
//!   unusable_setting }`, never silently the default. The move command reads the pitch only
//!   when snapping is on.

use crate::json_num::as_u32;
use crate::layout::page::{parse_length, LayoutError, Length, MAX_LENGTH};
use fid_canon::CValue;
use fid_store::settings::{resolve, Resolved, Scope, SettingWarning, SettingsRegistry, Sources};

/// The setting key.
pub const GRID_PITCH_KEY: &str = "report.grid_pitch";
/// Smallest pitch: `0.1 mm` (REQ-002).
pub const MIN_PITCH: Length = 10;
/// Largest pitch: `2000 mm`, the largest page side.
pub const MAX_PITCH: Length = MAX_LENGTH;
/// The default pitch: `1 mm`.
pub const DEFAULT_PITCH: Length = 100;

fid_store::fid_settings! {
    pub fn report_settings;
    {
        key: "report.grid_pitch",
        owner: "report",
        schema: r#"{"type": "integer", "minimum": 10, "maximum": 200000, "description": "Report designer grid pitch, in 0.01 mm (10 = 0.1 mm)."}"#,
        default: "100",
        scopes: [User],
        admin: false,
    }
}

/// The pitch setting as the move command sees it: a usable pitch, or the error to return
/// when snapping is on.
pub type PitchSetting = Result<GridPitch, LayoutError>;

/// A checked grid pitch, in `0.01 mm`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridPitch(Length);

fn pitch_invalid(reason: &'static str) -> LayoutError {
    LayoutError::LayoutInvalid { field: "grid_pitch", reason }
}

impl Default for GridPitch {
    fn default() -> GridPitch {
        GridPitch(DEFAULT_PITCH)
    }
}

impl GridPitch {
    /// A pitch of `units`: below [`MIN_PITCH`] is `below_minimum`, above [`MAX_PITCH`]
    /// `too_large` (`report.layout_invalid`, field `grid_pitch`).
    pub fn new(units: Length) -> Result<GridPitch, LayoutError> {
        if units < MIN_PITCH {
            return Err(pitch_invalid("below_minimum"));
        }
        if units > MAX_PITCH {
            return Err(pitch_invalid("too_large"));
        }
        Ok(GridPitch(units))
    }

    /// A pitch typed as a length (`1mm`, `0.05in`), parsed exactly as page lengths are.
    pub fn parse(text: &str) -> Result<GridPitch, LayoutError> {
        GridPitch::new(parse_length(text).map_err(|f| pitch_invalid(f.reason()))?)
    }

    /// The pitch in `0.01 mm`.
    pub const fn units(self) -> Length {
        self.0
    }

    /// The setting value to hand to `settings.set_value@1` (a JSON integer).
    pub fn setting_value(self) -> CValue {
        let n = usize::try_from(self.0).unwrap_or(0);
        crate::json_num::num_usize(n)
    }

    /// The pitch of a resolved snapshot of `report.grid_pitch` and the warnings of the same
    /// read. Fails closed (`unusable_setting`) when a warning names the key or the whole
    /// user-scope file, when the snapshot holds the key not exactly once, or when the value
    /// is not an integer pitch.
    pub fn from_snapshot(values: &[Resolved], warnings: &[SettingWarning]) -> PitchSetting {
        if warnings.iter().any(concerns_pitch) {
            return Err(pitch_invalid("unusable_setting"));
        }
        let mut found = values.iter().filter(|r| r.key == GRID_PITCH_KEY);
        let (Some(r), None) = (found.next(), found.next()) else {
            return Err(pitch_invalid("unusable_setting"));
        };
        let units = as_u32(&r.value).ok_or(pitch_invalid("unusable_setting"))?;
        GridPitch::new(Length::from(units))
    }

    /// Reads `report.grid_pitch` through the settings resolver with this module's own
    /// declaration, returning the pitch setting and the resolver's warnings about the key.
    pub fn from_settings(sources: &Sources<'_>) -> (PitchSetting, Vec<SettingWarning>) {
        let Ok(registry) = SettingsRegistry::build(&report_settings()) else {
            return (Err(pitch_invalid("unusable_setting")), Vec::new());
        };
        let (values, warnings) = resolve(&registry, sources, |d| d.key() == GRID_PITCH_KEY);
        let pitch = GridPitch::from_snapshot(&values, &warnings);
        let warnings = warnings.into_iter().filter(concerns_pitch).collect();
        (pitch, warnings)
    }
}

/// A warning about `report.grid_pitch`, or about the user-scope settings file as a whole.
fn concerns_pitch(w: &SettingWarning) -> bool {
    match &w.key {
        Some(k) => k == GRID_PITCH_KEY,
        None => w.scope == Scope::User,
    }
}
