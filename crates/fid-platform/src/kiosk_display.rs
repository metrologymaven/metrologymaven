//! Metrology Maven reference kiosk display (spec node `PLAT.DEPLOY.KIOSK.DISPLAY`): the typed
//! schema of `platform/kiosk-display.toml`, its strict, bounded validator and the derived
//! physical pixel density that `VIZ.KIOSK`'s size rules cite.
//!
//! Determinism D0: pure functions over bytes; diagnostics have a fixed order (see
//! [`validate`]).

use std::fmt;

/// The only `schema_version` this reader supports.
pub const SUPPORTED_SCHEMA: u32 = 1;
/// Largest accepted file, in bytes (16 KiB).
pub const MAX_BYTES: usize = 16 * 1024;
/// Path of the shipped file, relative to the repository root.
pub const DEFAULT_PATH: &str = "platform/kiosk-display.toml";
/// Accepted diagonal, in inches, inclusive (REQ-PLAT.DEPLOY.KIOSK.DISPLAY-003).
pub const DIAGONAL_MIN_IN: f64 = 10.0;
pub const DIAGONAL_MAX_IN: f64 = 43.0;
/// Accepted resolution side, in pixels, inclusive (REQ-PLAT.DEPLOY.KIOSK.DISPLAY-003).
pub const SIDE_MIN_PX: u32 = 800;
pub const SIDE_MAX_PX: u32 = 7680;
/// Millimetres per inch (exact by definition).
pub const MM_PER_IN: f64 = 25.4;

const KEYS: [&str; 6] = ["description", "diagonal_in", "glove_touch", "resolution", "schema_version", "touch"];

/// Touch technology of the panel (REQ-PLAT.DEPLOY.KIOSK.DISPLAY-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Touch {
    ProjectedCapacitive,
    Resistive,
    None,
}

impl Touch {
    /// Every value, in the order the file format lists them.
    pub const ALL: &'static [Touch] = &[Touch::ProjectedCapacitive, Touch::Resistive, Touch::None];

    /// The name used in the file.
    pub fn as_str(self) -> &'static str {
        match self {
            Touch::ProjectedCapacitive => "projected_capacitive",
            Touch::Resistive => "resistive",
            Touch::None => "none",
        }
    }

    /// Parses the name used in the file.
    pub fn from_name(s: &str) -> Option<Touch> {
        Touch::ALL.iter().copied().find(|t| t.as_str() == s)
    }
}

impl fmt::Display for Touch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A validated reference kiosk display.
#[derive(Debug, Clone, PartialEq)]
pub struct KioskDisplay {
    pub schema_version: u32,
    /// Visible diagonal in inches, 10 to 43.
    pub diagonal_in: f64,
    /// Native resolution (width, height) in pixels, each 800 to 7680.
    pub resolution: (u32, u32),
    pub touch: Touch,
    pub glove_touch: bool,
    /// Generic description of the panel (no brand needed).
    pub description: String,
}

impl KioskDisplay {
    /// Physical pixels per millimetre before rounding, assuming square pixels:
    /// `hypot(w, h) / (diagonal_in * 25.4)`.
    pub fn px_per_mm_raw(&self) -> f64 {
        let (w, h) = self.resolution;
        f64::from(w).hypot(f64::from(h)) / (self.diagonal_in * MM_PER_IN)
    }

    /// Physical pixels per millimetre rounded to 3 decimals (REQ-PLAT.DEPLOY.KIOSK.DISPLAY-002),
    /// as the decimal text the CLI prints (e.g. `4.034`).
    pub fn px_per_mm_text(&self) -> String {
        format!("{:.3}", self.px_per_mm_raw())
    }

    /// Physical pixels per millimetre rounded to 3 decimals: the nearest `f64` to
    /// [`KioskDisplay::px_per_mm_text`].
    pub fn px_per_mm(&self) -> f64 {
        let raw = self.px_per_mm_raw();
        format!("{raw:.3}").parse::<f64>().unwrap_or(raw)
    }
}

/// A typed display-file error.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DisplayError {
    /// Fatal: the file exceeds [`MAX_BYTES`].
    TooLarge { found: usize, max: usize },
    /// Not UTF-8 or not TOML (fatal; `line` is 1-based when the parser reports one), or a
    /// known field that is missing or has the wrong type or shape (`line` is `None`).
    Parse { line: Option<usize>, msg: String },
    /// A key the schema does not define.
    UnknownField { field: String },
    /// A field whose value is outside its allowed range (`schema_version` other than 1 is
    /// reported here too).
    OutOfRange { field: String, msg: String },
}

impl fmt::Display for DisplayError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            DisplayError::TooLarge { found, max } => write!(f, "TooLarge {{ found: {found}, max: {max} }}"),
            DisplayError::Parse { line: Some(l), msg } => write!(f, "Parse {{ line: {l} }}: {msg}"),
            DisplayError::Parse { line: None, msg } => write!(f, "Parse: {msg}"),
            DisplayError::UnknownField { field } => write!(f, "UnknownField {{ field: {field} }}"),
            DisplayError::OutOfRange { field, msg } => write!(f, "OutOfRange {{ field: {field} }}: {msg}"),
        }
    }
}

impl std::error::Error for DisplayError {}

fn line_of(text: &str, offset: usize) -> usize {
    text.as_bytes().iter().take(offset).filter(|&&b| b == b'\n').count() + 1
}

fn missing(field: &str) -> DisplayError {
    DisplayError::Parse { line: None, msg: format!("{field} is missing") }
}

fn wrong(field: &str, what: &str) -> DisplayError {
    DisplayError::Parse { line: None, msg: format!("{field} is not {what}") }
}

fn out_of_range(field: &str, msg: String) -> DisplayError {
    DisplayError::OutOfRange { field: field.to_string(), msg }
}

/// Validates a display file (REQ-PLAT.DEPLOY.KIOSK.DISPLAY-001 and -003) and returns every
/// diagnostic.
///
/// `TooLarge` and a non-UTF-8 or non-TOML file stop at once and are the only entry.
/// Otherwise the order is: unknown keys in key order, then the fields `schema_version`,
/// `diagonal_in`, `resolution`, `touch`, `glove_touch`, `description`.
pub fn validate(bytes: &[u8]) -> Result<KioskDisplay, Vec<DisplayError>> {
    if bytes.len() > MAX_BYTES {
        return Err(vec![DisplayError::TooLarge { found: bytes.len(), max: MAX_BYTES }]);
    }
    let Ok(text) = std::str::from_utf8(bytes) else {
        return Err(vec![DisplayError::Parse { line: None, msg: "file is not UTF-8".into() }]);
    };
    let table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            let line = e.span().map(|s| line_of(text, s.start));
            return Err(vec![DisplayError::Parse { line, msg: e.message().trim_end().to_string() }]);
        }
    };

    let mut errors = Vec::new();
    for k in table.keys() {
        if !KEYS.contains(&k.as_str()) {
            errors.push(DisplayError::UnknownField { field: k.clone() });
        }
    }

    let schema_version = match table.get("schema_version") {
        None => {
            errors.push(missing("schema_version"));
            None
        }
        Some(toml::Value::Integer(n)) if *n == i64::from(SUPPORTED_SCHEMA) => Some(SUPPORTED_SCHEMA),
        Some(toml::Value::Integer(n)) => {
            errors.push(out_of_range("schema_version", format!("{n} is not {SUPPORTED_SCHEMA}")));
            None
        }
        Some(_) => {
            errors.push(wrong("schema_version", "an integer"));
            None
        }
    };

    let diagonal_in = match table.get("diagonal_in") {
        None => {
            errors.push(missing("diagonal_in"));
            None
        }
        Some(v) => {
            // An integer such as `diagonal_in = 5` is accepted as a number (AC-2's fixture).
            let num = match v {
                toml::Value::Float(x) => Some(*x),
                toml::Value::Integer(n) => Some(i32::try_from(*n).map_or(f64::INFINITY, f64::from)),
                _ => None,
            };
            match num {
                None => {
                    errors.push(wrong("diagonal_in", "a number of inches"));
                    None
                }
                // NaN is in no range and is reported as out of range.
                Some(x) if (DIAGONAL_MIN_IN..=DIAGONAL_MAX_IN).contains(&x) => Some(x),
                Some(x) => {
                    errors.push(out_of_range(
                        "diagonal_in",
                        format!("{} is outside {DIAGONAL_MIN_IN}..={DIAGONAL_MAX_IN} inches", show(v, x)),
                    ));
                    None
                }
            }
        }
    };

    let resolution = match table.get("resolution") {
        None => {
            errors.push(missing("resolution"));
            None
        }
        Some(toml::Value::Array(a)) => match a.as_slice() {
            [toml::Value::Integer(w), toml::Value::Integer(h)] => {
                let side = |n: i64| u32::try_from(n).ok().filter(|v| (SIDE_MIN_PX..=SIDE_MAX_PX).contains(v));
                match (side(*w), side(*h)) {
                    (Some(w), Some(h)) => Some((w, h)),
                    _ => {
                        errors.push(out_of_range(
                            "resolution",
                            format!("[{w}, {h}] has a side outside {SIDE_MIN_PX}..={SIDE_MAX_PX} pixels"),
                        ));
                        None
                    }
                }
            }
            _ => {
                errors.push(wrong("resolution", "[width, height] in whole pixels"));
                None
            }
        },
        Some(_) => {
            errors.push(wrong("resolution", "[width, height] in whole pixels"));
            None
        }
    };

    let touch = match table.get("touch") {
        None => {
            errors.push(missing("touch"));
            None
        }
        Some(v) => {
            let t = v.as_str().and_then(Touch::from_name);
            if t.is_none() {
                errors.push(wrong("touch", "one of projected_capacitive, resistive, none"));
            }
            t
        }
    };

    let glove_touch = match table.get("glove_touch") {
        None => {
            errors.push(missing("glove_touch"));
            None
        }
        Some(toml::Value::Boolean(b)) => Some(*b),
        Some(_) => {
            errors.push(wrong("glove_touch", "a boolean"));
            None
        }
    };

    let description = match table.get("description") {
        None => {
            errors.push(missing("description"));
            None
        }
        Some(toml::Value::String(s)) if !s.trim().is_empty() => Some(s.clone()),
        Some(_) => {
            errors.push(wrong("description", "a non-empty string"));
            None
        }
    };

    if !errors.is_empty() {
        return Err(errors);
    }
    match (schema_version, diagonal_in, resolution, touch, glove_touch, description) {
        (Some(v), Some(d), Some(r), Some(t), Some(g), Some(s)) => Ok(KioskDisplay {
            schema_version: v,
            diagonal_in: d,
            resolution: r,
            touch: t,
            glove_touch: g,
            description: s,
        }),
        _ => Err(vec![DisplayError::Parse { line: None, msg: "invalid display file".into() }]),
    }
}

/// The value as written in the file, for diagnostics.
fn show(v: &toml::Value, x: f64) -> String {
    match v {
        toml::Value::Integer(n) => n.to_string(),
        _ => x.to_string(),
    }
}

/// Parses and validates a display file, returning the first error if there is any.
pub fn parse(bytes: &[u8]) -> Result<KioskDisplay, DisplayError> {
    validate(bytes).map_err(|errs| {
        errs.into_iter().next().unwrap_or(DisplayError::Parse { line: None, msg: "invalid display file".into() })
    })
}
