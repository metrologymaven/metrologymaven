//! Status markers, the locked status-palette guard, provenance badges and the out-of-tolerance
//! pattern definitions (spec node `VIZ.OVERLAY.STATUS`). One pure mapping from what the kernel
//! supplies (a display status, a provenance, whether a GD&T result is provisional) and a theme to
//! what the user sees. Every overlay, the shell and the kiosk use it, so "never colour alone"
//! lives in one place (REQ-VIZ.OVERLAY-005).
//!
//! - [`status_glyph`]: each of the six display states has a distinct glyph, a distinct pattern,
//!   a message key and an English text, the same in every theme; only the colour comes from the
//!   theme (REQ-001, REQ-005). Glyph and text IDs are those of `tokens/status.json` (`state.<s>`,
//!   `icon.state.<s>`, UX.TOKENS.STATUS).
//! - [`provenance_badge`]: one badge per provenance (REQ-004).
//! - [`Palette`]: the locked default colours of `tokens/status.json` (UX.TOKENS.STATUS) and the
//!   out-of-range colours `under` and `over` of the palette catalogue (UX.TOKENS.DATAPAL), and
//!   [`Palette::apply`], the guard that refuses a skin which would draw a guarded state within
//!   [`PaletteGuardConfig::min_delta_e2000`] (CIEDE2000, under normal vision and the three
//!   Machado 2009 dichromacies) of the conforming colour, and keeps the palette unchanged
//!   (REQ-002, REQ-003).
//! - [`PATTERNS`]: the pattern definitions, among them `pattern.out-of-tolerance` (the generic
//!   cue of UX.TOKENS.STATUS) refined by `pattern.under-tolerance` and `pattern.over-tolerance`.
//!   VIZ.OVERLAY.CMAP draws them on out-of-range regions ([`out_of_tolerance_cue`]).
//! - [`OverlayItem`] (a status marker) and [`GdtResultItem`] (a GD&T result): a number cannot
//!   be built without a provenance (`MissingProvenance`), and a value that is claimed, rests on
//!   declared or uncharacterized input, or has a claim provenance never gets the measured
//!   styling. **A caller showing a GD&T result must use [`GdtResultItem`]**, whose constructor
//!   takes the [`Interpretation`] computed by `fid_gdt::requires_provisional_label_bytes` on the
//!   stored record bytes (D-045); [`OverlayItem`] cannot carry an interpretation, so a GD&T
//!   result shown through it would show no provisional label. VIZ.SHELL, the consumer, must test
//!   that every GD&T result it shows goes through [`GdtResultItem`].
//! - [`RenderSpec`]: what an item looks like in a theme, with canonical JSON bytes and a digest.
//!
//! Determinism D0 for the mapping: tables, `BTreeMap`, no clock, no randomness, no toolkit state.
//! The guard's colour differences use the standard library's `powf`, `cbrt`, `atan2`, `sin`,
//! `cos` and `exp`, whose last bits may differ between platforms, so a skin colour within about
//! 1e-12 of a threshold could get a different verdict on another platform; every shipped and
//! fixture colour is far from that.

pub mod colour;

use colour::{min_delta_e2000, parse_hex, to_hex, Rgb, Vision};
use fid_canon::CValue;
use fid_kdec::Outcome;
use fid_render::scene::proj::DisplayStatus;
use std::collections::BTreeMap;
use std::fmt;

// ------------------------------------------------------------------ states, themes, provenances

/// The six display states (wire names as the keys of `tokens/status.json`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum DisplayState {
    Conforms,
    Nonconforms,
    Indeterminate,
    NotDecidable,
    NotEvaluated,
    Stale,
}

impl DisplayState {
    pub const ALL: [DisplayState; 6] = [
        DisplayState::Conforms,
        DisplayState::Nonconforms,
        DisplayState::Indeterminate,
        DisplayState::NotDecidable,
        DisplayState::NotEvaluated,
        DisplayState::Stale,
    ];

    /// The state name of `tokens/status.json` and of the glyph IDs.
    pub fn name(self) -> &'static str {
        match self {
            DisplayState::Conforms => "conforms",
            DisplayState::Nonconforms => "nonconforms",
            DisplayState::Indeterminate => "indeterminate",
            DisplayState::NotDecidable => "not-decidable",
            DisplayState::NotEvaluated => "not-evaluated",
            DisplayState::Stale => "stale",
        }
    }

    /// The upper-case display name (`NONCONFORMS`, `NOT_EVALUATED`, ...).
    pub fn upper(self) -> &'static str {
        match self {
            DisplayState::Conforms => "CONFORMS",
            DisplayState::Nonconforms => "NONCONFORMS",
            DisplayState::Indeterminate => "INDETERMINATE",
            DisplayState::NotDecidable => "NOT_DECIDABLE",
            DisplayState::NotEvaluated => "NOT_EVALUATED",
            DisplayState::Stale => "STALE",
        }
    }

    pub fn parse(s: &str) -> Option<DisplayState> {
        DisplayState::ALL.into_iter().find(|x| x.name() == s)
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The four themes of UX.TOKENS.COLOR.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Theme {
    Light,
    Dark,
    Hc,
    Floor,
}

impl Theme {
    pub const ALL: [Theme; 4] = [Theme::Light, Theme::Dark, Theme::Hc, Theme::Floor];

    pub fn name(self) -> &'static str {
        match self {
            Theme::Light => "light",
            Theme::Dark => "dark",
            Theme::Hc => "hc",
            Theme::Floor => "floor",
        }
    }

    /// The theme with this name, or [`StatusError::UnknownTheme`].
    pub fn parse(s: &str) -> Result<Theme, StatusError> {
        Theme::ALL.into_iter().find(|t| t.name() == s).ok_or_else(|| StatusError::UnknownTheme(s.to_string()))
    }

    fn index(self) -> usize {
        self as usize
    }
}

/// The five provenances of a displayed number.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Provenance {
    Computed,
    Measured,
    Entered,
    ReportedBySource,
    Suggested,
}

impl Provenance {
    pub const ALL: [Provenance; 5] = [
        Provenance::Computed,
        Provenance::Measured,
        Provenance::Entered,
        Provenance::ReportedBySource,
        Provenance::Suggested,
    ];

    pub fn name(self) -> &'static str {
        match self {
            Provenance::Computed => "computed",
            Provenance::Measured => "measured",
            Provenance::Entered => "entered",
            Provenance::ReportedBySource => "reported-by-source",
            Provenance::Suggested => "suggested",
        }
    }

    /// Whether a number of this provenance is a claim rather than a value this system measured
    /// or computed: entered by a person, reported by a source, or suggested.
    pub fn is_claim(self) -> bool {
        matches!(self, Provenance::Entered | Provenance::ReportedBySource | Provenance::Suggested)
    }
}

// ------------------------------------------------------------------ errors

/// A palette slot the guard checks: a display state or an out-of-tolerance region colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Slot {
    State(DisplayState),
    UnderTolerance,
    OverTolerance,
}

impl Slot {
    pub const ALL: [Slot; 8] = [
        Slot::State(DisplayState::Conforms),
        Slot::State(DisplayState::Nonconforms),
        Slot::State(DisplayState::Indeterminate),
        Slot::State(DisplayState::NotDecidable),
        Slot::State(DisplayState::NotEvaluated),
        Slot::State(DisplayState::Stale),
        Slot::UnderTolerance,
        Slot::OverTolerance,
    ];

    /// The skin key.
    pub fn name(self) -> &'static str {
        match self {
            Slot::State(s) => s.name(),
            Slot::UnderTolerance => "under-tolerance",
            Slot::OverTolerance => "over-tolerance",
        }
    }

    pub fn parse(s: &str) -> Option<Slot> {
        Slot::ALL.into_iter().find(|x| x.name() == s)
    }
}

impl fmt::Display for Slot {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Slot::State(s) => f.write_str(s.upper()),
            Slot::UnderTolerance => f.write_str("UNDER_TOLERANCE"),
            Slot::OverTolerance => f.write_str("OVER_TOLERANCE"),
        }
    }
}

/// Every error of this module is a value (CONSTITUTION §8).
#[derive(Debug, Clone, PartialEq)]
pub enum StatusError {
    /// A skin would draw `state` within the guard distance of the conforming colour, or break a
    /// rule of REQ-003, in `theme`. The palette is left unchanged.
    LockedPaletteViolation { state: Slot, theme: Theme },
    /// An overlay item shows a number without a provenance badge.
    MissingProvenance,
    /// An overlay item has a provenance but no number to badge.
    ProvenanceWithoutNumber,
    /// A claimed value badged `measured`.
    ClaimedAsMeasured,
    /// A number that is not finite.
    NonFiniteNumber,
    /// A theme name that is not light, dark, hc or floor.
    UnknownTheme(String),
    /// The kernel reported an evaluation error; it has no display state and is drawn as an error.
    KernelError(String),
    /// A skin file that is not `status-skin@1`.
    SkinSchema(String),
}

impl fmt::Display for StatusError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            StatusError::LockedPaletteViolation { state, theme } => {
                write!(f, "LockedPaletteViolation {{ state: {state}, theme: {} }}", theme.name())
            }
            StatusError::MissingProvenance => f.write_str("MissingProvenance"),
            StatusError::ProvenanceWithoutNumber => f.write_str("ProvenanceWithoutNumber"),
            StatusError::ClaimedAsMeasured => f.write_str("ClaimedAsMeasured"),
            StatusError::NonFiniteNumber => f.write_str("NonFiniteNumber"),
            StatusError::UnknownTheme(t) => write!(f, "UnknownTheme {t:?}"),
            StatusError::KernelError(c) => write!(f, "KernelError {c}"),
            StatusError::SkinSchema(m) => write!(f, "SkinSchema: {m}"),
        }
    }
}

// ------------------------------------------------------------------ patterns

/// How a pattern is drawn, in logical pixels (original definitions of this project).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum PatternKind {
    /// No texture: the colour fill alone, always with the glyph and text.
    Solid,
    /// Parallel lines at `angle` degrees (0 horizontal, counter-clockwise), one every `period`,
    /// `width` wide; `dash` > 0 breaks each line into dashes and gaps of that length.
    Hatch { angle: u16, period: u8, width: u8, dash: u8 },
    /// Two hatches at 45 and 135 degrees.
    Cross { period: u8, width: u8 },
    /// A square grid of dots.
    Dots { period: u8, radius: u8 },
}

/// One pattern definition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PatternDef {
    pub id: &'static str,
    pub kind: PatternKind,
    /// The more general pattern this one refines, if any.
    pub refines: Option<&'static str>,
}

/// The generic out-of-tolerance cue of UX.TOKENS.STATUS.
pub const OOT_PATTERN: &str = "pattern.out-of-tolerance";
pub const UNDER_PATTERN: &str = "pattern.under-tolerance";
pub const OVER_PATTERN: &str = "pattern.over-tolerance";

/// Every pattern this leaf defines: one per display state and the three out-of-tolerance ones.
pub const PATTERNS: [PatternDef; 9] = [
    PatternDef { id: "pattern.conforms", kind: PatternKind::Solid, refines: None },
    PatternDef { id: "pattern.nonconforms", kind: PatternKind::Cross { period: 6, width: 1 }, refines: None },
    PatternDef { id: "pattern.indeterminate", kind: PatternKind::Dots { period: 6, radius: 1 }, refines: None },
    PatternDef {
        id: "pattern.not-decidable",
        kind: PatternKind::Hatch { angle: 90, period: 6, width: 1, dash: 0 },
        refines: None,
    },
    PatternDef {
        id: "pattern.not-evaluated",
        kind: PatternKind::Hatch { angle: 0, period: 8, width: 1, dash: 0 },
        refines: None,
    },
    PatternDef {
        id: "pattern.stale",
        kind: PatternKind::Hatch { angle: 45, period: 8, width: 1, dash: 3 },
        refines: None,
    },
    PatternDef { id: OOT_PATTERN, kind: PatternKind::Cross { period: 4, width: 2 }, refines: None },
    PatternDef {
        id: UNDER_PATTERN,
        kind: PatternKind::Hatch { angle: 135, period: 4, width: 2, dash: 0 },
        refines: Some(OOT_PATTERN),
    },
    PatternDef {
        id: OVER_PATTERN,
        kind: PatternKind::Hatch { angle: 45, period: 4, width: 2, dash: 0 },
        refines: Some(OOT_PATTERN),
    },
];

/// The definition of a pattern ID.
pub fn pattern_def(id: &str) -> Option<PatternDef> {
    PATTERNS.iter().copied().find(|p| p.id == id)
}

// ------------------------------------------------------------------ glyphs and badges

/// What a display state looks like in a theme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StatusGlyph {
    pub state: DisplayState,
    /// Glyph ID of UX.ICONS.CLASS: `state.<state>`.
    pub glyph: &'static str,
    /// Pattern ID ([`PATTERNS`]).
    pub pattern: &'static str,
    /// Accessible-name message key: `icon.state.<state>`.
    pub text_key: &'static str,
    /// The English source text of `text_key`.
    pub text: &'static str,
    /// The colour, the only theme-dependent field.
    pub color: Rgb,
}

/// `(glyph, pattern, text key, English text)` per state, in [`DisplayState::ALL`] order.
const STATE_TABLE: [(&str, &str, &str, &str); 6] = [
    ("state.conforms", "pattern.conforms", "icon.state.conforms", "Conforms"),
    ("state.nonconforms", "pattern.nonconforms", "icon.state.nonconforms", "Does not conform"),
    ("state.indeterminate", "pattern.indeterminate", "icon.state.indeterminate", "Indeterminate"),
    ("state.not-decidable", "pattern.not-decidable", "icon.state.not-decidable", "Not decidable"),
    ("state.not-evaluated", "pattern.not-evaluated", "icon.state.not-evaluated", "Not evaluated"),
    ("state.stale", "pattern.stale", "icon.state.stale", "Stale"),
];

/// A provenance badge.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Badge {
    pub provenance: Provenance,
    /// Glyph ID of UX.ICONS.CLASS: `badge.<provenance>`.
    pub glyph: &'static str,
    /// Message key: `icon.badge.<provenance>`.
    pub text_key: &'static str,
    pub text: &'static str,
}

const BADGE_TABLE: [(&str, &str, &str); 5] = [
    ("badge.computed", "icon.badge.computed", "Computed"),
    ("badge.measured", "icon.badge.measured", "Measured"),
    ("badge.entered", "icon.badge.entered", "Entered"),
    ("badge.reported-by-source", "icon.badge.reported-by-source", "Reported by source"),
    ("badge.suggested", "icon.badge.suggested", "Suggested"),
];

/// The badge of a provenance (pure; REQ-004).
pub fn provenance_badge(p: Provenance) -> Badge {
    let (glyph, text_key, text) = BADGE_TABLE[p as usize];
    Badge { provenance: p, glyph, text_key, text }
}

/// The glyph of a state in a theme with the locked palette (pure; REQ-001, REQ-005).
pub fn status_glyph(state: DisplayState, theme: Theme) -> StatusGlyph {
    Palette::locked().glyph(state, theme)
}

/// Under-range or over-range side of a tolerance.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Side {
    Under,
    Over,
}

/// The cue VIZ.OVERLAY.CMAP draws on an out-of-range region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RegionCue {
    pub side: Side,
    pub pattern: &'static str,
    pub text_key: &'static str,
    pub text: &'static str,
    pub color: Rgb,
}

/// The out-of-range cue of a side in a theme with the locked palette.
pub fn out_of_tolerance_cue(side: Side, theme: Theme) -> RegionCue {
    Palette::locked().region_cue(side, theme)
}

// ------------------------------------------------------------------ palette and guard

/// The status colours per theme, plus the out-of-range colours.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    /// `[theme][slot]` in [`Theme::ALL`] and [`Slot::ALL`] order.
    colours: [[Rgb; 8]; 4],
}

/// The locked status palette of `design/tokens/status.json` (UX.TOKENS.STATUS), in
/// [`DisplayState::ALL`] order per theme in [`Theme::ALL`] order, and the out-of-range colours
/// `under` and `over` shared by every palette of the catalogue (UX.TOKENS.DATAPAL). A test keeps
/// these equal to the shipped files.
pub const LOCKED: [[&str; 8]; 4] = [
    ["#00637e", "#c72d2e", "#392e00", "#1c1f0c", "#1c1d1f", "#8c4200", "#7c6d45", "#8b5f57"],
    ["#00b5e4", "#ff7269", "#eadba3", "#f6fae1", "#f5f7f9", "#f58028", "#7c6d45", "#8b5f57"],
    ["#004b61", "#a90617", "#221d06", "#1c1f0c", "#000000", "#6b3100", "#7c6d45", "#8b5f57"],
    ["#00637e", "#c72d2e", "#392e00", "#1c1f0c", "#1c1d1f", "#8c4200", "#7c6d45", "#8b5f57"],
];

/// The guard's threshold (`min_delta_e2000` of `tokens/status.json`) [unverified threshold].
pub const LOCKED_MIN_DELTA_E2000: f64 = 20.0;
/// Least CIEDE2000 between a neutral state and nonconforms or stale (UX.TOKENS.STATUS
/// `STATUS-DISTANCE`).
pub const LOCKED_MIN_NEUTRAL_DELTA_E2000: f64 = 10.0;

/// Least CIEDE2000, under normal vision, between stale and nonconforms (maintainer decision on
/// PR 355; the locked palette gives at least 18).
pub const LOCKED_MIN_STALE_DELTA_E2000_NORMAL: f64 = 10.0;

/// The guard's configuration.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PaletteGuardConfig {
    /// Every slot other than CONFORMS keeps at least this from the conforming colour.
    pub min_delta_e2000: f64,
    /// Indeterminate, not-decidable and not-evaluated keep at least this from nonconforms and
    /// stale (REQ-003).
    pub min_neutral_delta_e2000: f64,
    /// Stale keeps at least this from nonconforms under normal vision (REQ-003). Under simulated
    /// dichromacy the two only have to differ: the glyph and the word tell them apart.
    pub min_stale_delta_e2000_normal: f64,
}

impl Default for PaletteGuardConfig {
    fn default() -> Self {
        PaletteGuardConfig {
            min_delta_e2000: LOCKED_MIN_DELTA_E2000,
            min_neutral_delta_e2000: LOCKED_MIN_NEUTRAL_DELTA_E2000,
            min_stale_delta_e2000_normal: LOCKED_MIN_STALE_DELTA_E2000_NORMAL,
        }
    }
}

/// A skin: replacement colours per theme and slot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Skin {
    pub colours: BTreeMap<(Theme, Slot), Rgb>,
}

/// Skin file format ID.
pub const SKIN_FORMAT: &str = "status-skin@1";
const MAX_SKIN_BYTES: usize = 64 * 1024;

impl Skin {
    /// Reads `{ "format": "status-skin@1", "themes": { "<theme>": { "<slot>": "#rrggbb" } } }`
    /// (strict JSON: duplicate keys refused).
    pub fn parse(bytes: &[u8]) -> Result<Skin, StatusError> {
        let bad = |m: &str| StatusError::SkinSchema(m.to_string());
        let v =
            fid_canon::parse_with_limits(bytes, MAX_SKIN_BYTES, 8).map_err(|e| bad(&format!("invalid JSON: {e}")))?;
        let CValue::Obj(m) = v else {
            return Err(bad("not an object"));
        };
        if m.len() != 2 || m.get("format") != Some(&CValue::Str(SKIN_FORMAT.to_string())) {
            return Err(bad("keys must be exactly format (status-skin@1) and themes"));
        }
        let Some(CValue::Obj(themes)) = m.get("themes") else {
            return Err(bad("themes must be an object"));
        };
        let mut out = Skin::default();
        for (t, row) in themes {
            let theme = Theme::parse(t)?;
            let CValue::Obj(row) = row else {
                return Err(bad("each theme must be an object of slot colours"));
            };
            for (k, c) in row {
                let slot = Slot::parse(k).ok_or_else(|| bad(&format!("unknown slot {k:?}")))?;
                if slot == CONFORMS_SLOT {
                    return Err(bad(&format!("{t}: the conforms colour is locked; a skin may not set it")));
                }
                let rgb = match c {
                    CValue::Str(s) => parse_hex(s),
                    _ => None,
                }
                .ok_or_else(|| bad(&format!("{t} {k}: colour must be lowercase #rrggbb")))?;
                out.colours.insert((theme, slot), rgb);
            }
        }
        Ok(out)
    }
}

const CONFORMS_SLOT: Slot = Slot::State(DisplayState::Conforms);

const NEUTRAL: [DisplayState; 3] =
    [DisplayState::Indeterminate, DisplayState::NotDecidable, DisplayState::NotEvaluated];

impl Palette {
    /// The locked default.
    pub fn locked() -> Palette {
        let mut colours = [[[0u8; 3]; 8]; 4];
        for (t, row) in LOCKED.iter().enumerate() {
            for (s, hex) in row.iter().enumerate() {
                colours[t][s] = parse_hex(hex).unwrap_or([0, 0, 0]);
            }
        }
        Palette { colours }
    }

    fn slot_index(slot: Slot) -> usize {
        match slot {
            Slot::State(s) => s.index(),
            Slot::UnderTolerance => 6,
            Slot::OverTolerance => 7,
        }
    }

    /// The colour of a slot in a theme.
    pub fn colour(&self, slot: Slot, theme: Theme) -> Rgb {
        self.colours[theme.index()][Palette::slot_index(slot)]
    }

    /// The glyph of a state in a theme.
    pub fn glyph(&self, state: DisplayState, theme: Theme) -> StatusGlyph {
        let (glyph, pattern, text_key, text) = STATE_TABLE[state.index()];
        StatusGlyph { state, glyph, pattern, text_key, text, color: self.colour(Slot::State(state), theme) }
    }

    /// The out-of-range cue of a side in a theme.
    pub fn region_cue(&self, side: Side, theme: Theme) -> RegionCue {
        match side {
            Side::Under => RegionCue {
                side,
                pattern: UNDER_PATTERN,
                text_key: "icon.pattern.under-tolerance",
                text: "Below tolerance",
                color: self.colour(Slot::UnderTolerance, theme),
            },
            Side::Over => RegionCue {
                side,
                pattern: OVER_PATTERN,
                text_key: "icon.pattern.over-tolerance",
                text: "Above tolerance",
                color: self.colour(Slot::OverTolerance, theme),
            },
        }
    }

    /// The first rule a palette breaks, in [`Theme::ALL`] then [`Slot::ALL`] order. Distances
    /// to CONFORMS are measured against the **locked** CONFORMS colour, whatever the palette
    /// holds:
    /// - CONFORMS is the locked colour;
    /// - every other slot at least `min_delta_e2000` from the locked CONFORMS (REQ-002);
    /// - indeterminate, not-decidable and not-evaluated at least `min_neutral_delta_e2000` from
    ///   nonconforms and stale; stale at least `min_stale_delta_e2000_normal` from nonconforms
    ///   under normal vision and not the same colour (REQ-003).
    pub fn violation(&self, config: &PaletteGuardConfig) -> Option<(Slot, Theme)> {
        let locked = Palette::locked();
        for theme in Theme::ALL {
            let c = |s: Slot| self.colour(s, theme);
            let conforms = locked.colour(CONFORMS_SLOT, theme);
            if c(CONFORMS_SLOT) != conforms {
                return Some((CONFORMS_SLOT, theme));
            }
            for slot in Slot::ALL.into_iter().skip(1) {
                let x = c(slot);
                if min_delta_e2000(x, conforms).0 < config.min_delta_e2000 {
                    return Some((slot, theme));
                }
                if let Slot::State(s) = slot {
                    let nc = [c(Slot::State(DisplayState::Nonconforms)), c(Slot::State(DisplayState::Stale))];
                    if NEUTRAL.contains(&s)
                        && nc.iter().any(|y| min_delta_e2000(x, *y).0 < config.min_neutral_delta_e2000)
                    {
                        return Some((slot, theme));
                    }
                    if s == DisplayState::Stale
                        && (x == nc[0]
                            || colour::delta_e2000_under(Vision::Normal, x, nc[0])
                                < config.min_stale_delta_e2000_normal)
                    {
                        return Some((slot, theme));
                    }
                }
            }
        }
        None
    }

    /// Applies a skin through the guard: either every colour of the skin is taken, or none is
    /// and the palette keeps its colours ([`StatusError::LockedPaletteViolation`]). A skin that
    /// sets CONFORMS is refused as [`StatusError::SkinSchema`].
    pub fn apply(&mut self, skin: &Skin, config: &PaletteGuardConfig) -> Result<(), StatusError> {
        if let Some((t, _)) = skin.colours.keys().find(|(_, slot)| *slot == CONFORMS_SLOT) {
            return Err(StatusError::SkinSchema(format!(
                "{}: the conforms colour is locked; a skin may not set it",
                t.name()
            )));
        }
        let mut next = self.clone();
        for ((theme, slot), rgb) in &skin.colours {
            next.colours[theme.index()][Palette::slot_index(*slot)] = *rgb;
        }
        match next.violation(config) {
            Some((state, theme)) => Err(StatusError::LockedPaletteViolation { state, theme }),
            None => {
                *self = next;
                Ok(())
            }
        }
    }
}

// ------------------------------------------------------------------ kernel status

/// The evidence facts of a kernel decision that a display must keep (`fid_kdec::EvidenceBasis`,
/// carried by VIZ.RENDER.SCENE.PROJ): any of them means the value may not look measured
/// (RPT.MODEL.BIND's `looks_measured()` is false).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash)]
pub struct Evidence {
    /// The value was declared or derived from a specification, not evaluated.
    pub claimed: bool,
    /// The value rests on at least one declared input.
    pub declared_input: bool,
    /// The value rests on at least one uncharacterized input.
    pub uncharacterized_input: bool,
}

impl Evidence {
    /// Whether any fact is set.
    pub fn any(self) -> bool {
        self.claimed || self.declared_input || self.uncharacterized_input
    }
}

/// The display state of a scene node's status (VIZ.RENDER.SCENE.PROJ) and its evidence facts,
/// all kept. A kernel evaluation error has no display state ([`StatusError::KernelError`]); the
/// stale state comes from VIZ.RENDER.SCENE.STALE, which adds it to `DisplayStatus`.
pub fn from_display_status(s: &DisplayStatus) -> Result<(DisplayState, Evidence), StatusError> {
    match s {
        DisplayStatus::Kernel(k) => {
            let state = match k.outcome {
                Outcome::Conforms => DisplayState::Conforms,
                Outcome::Nonconforms => DisplayState::Nonconforms,
                Outcome::Indeterminate => DisplayState::Indeterminate,
                Outcome::NotDecidable(_) => DisplayState::NotDecidable,
            };
            let evidence = Evidence {
                claimed: k.claimed,
                declared_input: k.declared_input,
                uncharacterized_input: k.uncharacterized_input,
            };
            Ok((state, evidence))
        }
        DisplayStatus::NotEvaluated => Ok((DisplayState::NotEvaluated, Evidence::default())),
        DisplayStatus::Error(code) => Err(StatusError::KernelError(code.as_str().to_string())),
    }
}

// ------------------------------------------------------------------ overlay items

/// The interpretation of a GD&T result (D-045): provisional unless its stored record is signed off.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Interpretation {
    Provisional,
    SignedOff,
}

impl Interpretation {
    /// From the result of `fid_gdt::requires_provisional_label_bytes(record)`, which the caller
    /// must compute on the stored record bytes of the result it shows.
    pub fn from_requires_label(requires_provisional_label: bool) -> Interpretation {
        if requires_provisional_label {
            Interpretation::Provisional
        } else {
            Interpretation::SignedOff
        }
    }
}

/// Catalog key and English text of the provisional label (as `fid_gdt::chars::interp`).
pub const PROVISIONAL_LABEL_KEY: &str = "gdt.interpretation.provisional";
pub const PROVISIONAL_LABEL_TEXT: &str = "interpretation: provisional";
/// The line a signed-off result shows instead.
pub const SIGNED_OFF_LINE: &str = "interpretation: signed_off";
/// The glyph cue of a provisional result (UX.ICONS.CLASS ID).
pub const PROVISIONAL_CUE: &str = "badge.provisional";
/// The word every value with the claimed styling carries, and the colour roles (UX.TOKENS.COLOR).
pub const CLAIMED_LABEL_KEY: &str = "value.claimed";
pub const CLAIMED_LABEL_TEXT: &str = "claimed";
pub const CLAIMED_ROLE: &str = "sem.text.value.claimed";
pub const MEASURED_ROLE: &str = "sem.text.value.measured";
/// The evidence labels of a value resting on declared or uncharacterized input.
pub const DECLARED_LABEL_KEY: &str = "label.declared-input";
pub const DECLARED_LABEL_TEXT: &str = "declared input";
pub const UNCHARACTERIZED_LABEL_KEY: &str = "label.uncharacterized-input";
pub const UNCHARACTERIZED_LABEL_TEXT: &str = "uncharacterized input";

/// A displayed number and its provenance. Built only through an item builder:
///
/// ```compile_fail
/// let v = fid_overlay::status::Value { number: 1.0, provenance: fid_overlay::status::Provenance::Measured };
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Value {
    number: f64,
    provenance: Provenance,
}

impl Value {
    pub fn number(&self) -> f64 {
        self.number
    }

    pub fn provenance(&self) -> Provenance {
        self.provenance
    }
}

#[derive(Debug, Clone, PartialEq)]
struct Parts {
    state: DisplayState,
    evidence: Evidence,
    value: Option<Value>,
}

/// A status marker, with an optional badged number. It cannot carry a GD&T interpretation: a
/// GD&T result is a [`GdtResultItem`]. Built only through [`OverlayItem::status`]:
///
/// ```compile_fail
/// use fid_overlay::status::{DisplayState, OverlayItem};
/// let i = OverlayItem { state: DisplayState::Conforms };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayItem {
    parts: Parts,
}

/// A GD&T result: a status marker that always carries its [`Interpretation`] (D-045). A caller
/// showing a GD&T result **must** build it here, with
/// `Interpretation::from_requires_label(fid_gdt::requires_provisional_label_bytes(record))`
/// computed on the result's stored record bytes.
#[derive(Debug, Clone, PartialEq)]
pub struct GdtResultItem {
    parts: Parts,
    interpretation: Interpretation,
}

/// Builds an [`OverlayItem`]; [`ItemBuilder::build`] enforces the item rules.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemBuilder {
    state: DisplayState,
    evidence: Evidence,
    number: Option<f64>,
    provenance: Option<Provenance>,
}

/// Builds a [`GdtResultItem`] with the same rules.
#[derive(Debug, Clone, PartialEq)]
pub struct GdtResultBuilder {
    inner: ItemBuilder,
    interpretation: Interpretation,
}

impl OverlayItem {
    /// A status marker.
    pub fn status(state: DisplayState) -> ItemBuilder {
        ItemBuilder { state, evidence: Evidence::default(), number: None, provenance: None }
    }

    pub fn state(&self) -> DisplayState {
        self.parts.state
    }

    pub fn evidence(&self) -> Evidence {
        self.parts.evidence
    }

    pub fn value(&self) -> Option<Value> {
        self.parts.value
    }

    /// The item's look in a theme with a palette (pure; REQ-005).
    pub fn render(&self, palette: &Palette, theme: Theme) -> RenderSpec {
        render_parts(&self.parts, None, palette, theme)
    }
}

impl GdtResultItem {
    /// A GD&T result with its interpretation (required).
    pub fn new(state: DisplayState, interpretation: Interpretation) -> GdtResultBuilder {
        GdtResultBuilder { inner: OverlayItem::status(state), interpretation }
    }

    pub fn state(&self) -> DisplayState {
        self.parts.state
    }

    pub fn evidence(&self) -> Evidence {
        self.parts.evidence
    }

    pub fn value(&self) -> Option<Value> {
        self.parts.value
    }

    pub fn interpretation(&self) -> Interpretation {
        self.interpretation
    }

    /// The result's look in a theme with a palette (pure; REQ-005, REQ-006).
    pub fn render(&self, palette: &Palette, theme: Theme) -> RenderSpec {
        render_parts(&self.parts, Some(self.interpretation), palette, theme)
    }
}

impl ItemBuilder {
    /// The evidence facts of the kernel decision ([`from_display_status`]).
    pub fn evidence(mut self, e: Evidence) -> ItemBuilder {
        self.evidence = e;
        self
    }

    /// The value is claimed (the kernel's `claimed` flag).
    pub fn claimed(mut self, claimed: bool) -> ItemBuilder {
        self.evidence.claimed = claimed;
        self
    }

    pub fn number(mut self, n: f64) -> ItemBuilder {
        self.number = Some(n);
        self
    }

    pub fn provenance(mut self, p: Provenance) -> ItemBuilder {
        self.provenance = Some(p);
        self
    }

    fn parts(&self) -> Result<Parts, StatusError> {
        let value = match (self.number, self.provenance) {
            (Some(_), None) => return Err(StatusError::MissingProvenance),
            (None, Some(_)) => return Err(StatusError::ProvenanceWithoutNumber),
            (Some(n), Some(_)) if !n.is_finite() => return Err(StatusError::NonFiniteNumber),
            (Some(number), Some(provenance)) => Some(Value { number, provenance }),
            (None, None) => None,
        };
        if self.evidence.any() && value.is_some_and(|v| v.provenance == Provenance::Measured) {
            return Err(StatusError::ClaimedAsMeasured);
        }
        Ok(Parts { state: self.state, evidence: self.evidence, value })
    }

    /// `MissingProvenance` for a number without a provenance (REQ-004), `ProvenanceWithoutNumber`
    /// for the other way round, `NonFiniteNumber`, and `ClaimedAsMeasured` for a value that is
    /// claimed or rests on declared or uncharacterized input but is badged `measured`.
    pub fn build(self) -> Result<OverlayItem, StatusError> {
        self.parts().map(|parts| OverlayItem { parts })
    }
}

impl GdtResultBuilder {
    pub fn evidence(mut self, e: Evidence) -> GdtResultBuilder {
        self.inner = self.inner.evidence(e);
        self
    }

    pub fn claimed(mut self, claimed: bool) -> GdtResultBuilder {
        self.inner = self.inner.claimed(claimed);
        self
    }

    pub fn number(mut self, n: f64) -> GdtResultBuilder {
        self.inner = self.inner.number(n);
        self
    }

    pub fn provenance(mut self, p: Provenance) -> GdtResultBuilder {
        self.inner = self.inner.provenance(p);
        self
    }

    /// The rules of [`ItemBuilder::build`].
    pub fn build(self) -> Result<GdtResultItem, StatusError> {
        self.inner.parts().map(|parts| GdtResultItem { parts, interpretation: self.interpretation })
    }
}

fn render_parts(parts: &Parts, interpretation: Option<Interpretation>, palette: &Palette, theme: Theme) -> RenderSpec {
    RenderSpec {
        theme,
        glyph: palette.glyph(parts.state, theme),
        value: parts.value.map(|v| (v, provenance_badge(v.provenance))),
        evidence: parts.evidence,
        interpretation,
    }
}

/// What an item looks like in a theme. Only [`OverlayItem::render`] and
/// [`GdtResultItem::render`] make one:
///
/// ```compile_fail
/// let r = fid_overlay::status::RenderSpec { theme: fid_overlay::status::Theme::Light };
/// ```
#[derive(Debug, Clone, PartialEq)]
pub struct RenderSpec {
    theme: Theme,
    glyph: StatusGlyph,
    value: Option<(Value, Badge)>,
    evidence: Evidence,
    interpretation: Option<Interpretation>,
}

fn s(x: &str) -> CValue {
    CValue::Str(x.to_string())
}

fn obj(items: Vec<(&str, CValue)>) -> CValue {
    CValue::Obj(items.into_iter().map(|(k, v)| (k.to_string(), v)).collect())
}

fn label(key: &str, text: &str) -> CValue {
    obj(vec![("key", s(key)), ("text", s(text))])
}

impl RenderSpec {
    pub fn theme(&self) -> Theme {
        self.theme
    }

    pub fn glyph(&self) -> StatusGlyph {
        self.glyph
    }

    pub fn value(&self) -> Option<(Value, Badge)> {
        self.value
    }

    pub fn evidence(&self) -> Evidence {
        self.evidence
    }

    pub fn interpretation(&self) -> Option<Interpretation> {
        self.interpretation
    }

    /// Whether the claimed styling applies: an evidence fact is set, or the number's provenance
    /// is a claim (entered, reported by source, suggested). Then the value never gets the
    /// measured role (REQ-007).
    pub fn claimed_styling(&self) -> bool {
        self.evidence.any() || self.value.is_some_and(|(v, _)| v.provenance.is_claim())
    }

    pub fn to_cvalue(&self) -> CValue {
        let g = &self.glyph;
        let claimed = self.claimed_styling();
        let value = match &self.value {
            None => CValue::Null,
            Some((v, b)) => obj(vec![
                ("number", CValue::Num(v.number)),
                (
                    "badge",
                    obj(vec![
                        ("provenance", s(b.provenance.name())),
                        ("glyph", s(b.glyph)),
                        ("text_key", s(b.text_key)),
                        ("text", s(b.text)),
                    ]),
                ),
                ("role", s(if claimed || v.provenance.is_claim() { CLAIMED_ROLE } else { MEASURED_ROLE })),
            ]),
        };
        let mut labels = Vec::new();
        if claimed {
            labels.push(label(CLAIMED_LABEL_KEY, CLAIMED_LABEL_TEXT));
        }
        if self.evidence.declared_input {
            labels.push(label(DECLARED_LABEL_KEY, DECLARED_LABEL_TEXT));
        }
        if self.evidence.uncharacterized_input {
            labels.push(label(UNCHARACTERIZED_LABEL_KEY, UNCHARACTERIZED_LABEL_TEXT));
        }
        let interpretation = match self.interpretation {
            None => CValue::Null,
            Some(Interpretation::Provisional) => obj(vec![
                ("provisional", CValue::Bool(true)),
                ("label_key", s(PROVISIONAL_LABEL_KEY)),
                ("text", s(PROVISIONAL_LABEL_TEXT)),
                ("cue", s(PROVISIONAL_CUE)),
            ]),
            Some(Interpretation::SignedOff) => obj(vec![
                ("provisional", CValue::Bool(false)),
                ("label_key", CValue::Null),
                ("text", s(SIGNED_OFF_LINE)),
                ("cue", CValue::Null),
            ]),
        };
        obj(vec![
            ("format", s(RENDER_FORMAT)),
            ("theme", s(self.theme.name())),
            ("state", s(g.state.name())),
            ("glyph", s(g.glyph)),
            ("pattern", s(g.pattern)),
            ("text_key", s(g.text_key)),
            ("text", s(g.text)),
            ("color", s(&to_hex(g.color))),
            ("value", value),
            ("labels", CValue::Arr(labels)),
            ("interpretation", interpretation),
        ])
    }

    /// RFC 8785 canonical bytes.
    pub fn to_canonical_bytes(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    /// `sha256:` of [`RenderSpec::to_canonical_bytes`].
    pub fn digest(&self) -> Result<String, fid_canon::CanonError> {
        fid_canon::digest(&self.to_cvalue()).map(|d| d.to_string())
    }
}

/// Format ID of a render spec.
pub const RENDER_FORMAT: &str = "status-render@1";

/// The smallest CIEDE2000, over the four vision conditions, between a slot's colour and the
/// conforming colour of a theme (for reports and tests).
pub fn distance_to_conforms(palette: &Palette, slot: Slot, theme: Theme) -> (f64, Vision) {
    min_delta_e2000(palette.colour(slot, theme), palette.colour(Slot::State(DisplayState::Conforms), theme))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tables_are_distinct_and_complete() {
        let mut glyphs: Vec<&str> = STATE_TABLE.iter().map(|r| r.0).collect();
        let mut pats: Vec<PatternKind> = PATTERNS.iter().map(|p| p.kind).collect();
        glyphs.sort_unstable();
        glyphs.dedup();
        pats.sort_unstable();
        pats.dedup();
        assert_eq!(glyphs.len(), 6);
        assert_eq!(pats.len(), PATTERNS.len());
        for (i, st) in DisplayState::ALL.iter().enumerate() {
            assert_eq!(STATE_TABLE[i].0, format!("state.{}", st.name()));
            assert_eq!(STATE_TABLE[i].2, format!("icon.state.{}", st.name()));
            assert!(pattern_def(STATE_TABLE[i].1).is_some());
        }
        for (i, p) in Provenance::ALL.iter().enumerate() {
            assert_eq!(BADGE_TABLE[i].0, format!("badge.{}", p.name()));
        }
        assert!(Palette::locked().violation(&PaletteGuardConfig::default()).is_none());
    }
}
