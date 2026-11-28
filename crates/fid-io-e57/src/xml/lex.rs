//! Lexical forms of values and the per-element rules shared by the reader and the writer.

use super::tree::{E57Node, Precision};

/// True for an XML white-space character.
pub(crate) fn is_ws(c: char) -> bool {
    matches!(c, ' ' | '\t' | '\n' | '\r')
}

/// The text without leading and trailing XML white space (XML Schema `collapse` for a single
/// token).
pub(crate) fn trim_ws(s: &str) -> &str {
    s.trim_matches(is_ws)
}

/// A strict `xs:long`: an optional sign and one or more ASCII digits.
pub(crate) fn parse_i64(s: &str) -> Result<i64, &'static str> {
    let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return Err("integer_syntax");
    }
    s.parse::<i64>().map_err(|_| "integer_out_of_range")
}

/// A strict non-negative integer (ASCII digits only) that fits `u64`.
pub(crate) fn parse_u64(s: &str) -> Result<u64, &'static str> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return Err("integer_syntax");
    }
    s.parse::<u64>().map_err(|_| "integer_out_of_range")
}

/// A strict, finite `xs:double`: `[+-]? (digits [. digits?] | . digits) ([eE] [+-]? digits)?`.
/// `INF`, `NaN` and every other form are refused (`float_syntax`); a value that overflows
/// binary64 is `float_not_finite`. The conversion is Rust's correctly rounded `str::parse`.
pub(crate) fn parse_f64(s: &str) -> Result<f64, &'static str> {
    let b = s.as_bytes();
    let mut i = usize::from(matches!(b.first(), Some(b'+') | Some(b'-')));
    let digits = |i: &mut usize| {
        let start = *i;
        while b.get(*i).is_some_and(u8::is_ascii_digit) {
            *i += 1;
        }
        *i - start
    };
    let int = digits(&mut i);
    let mut frac = 0;
    if b.get(i) == Some(&b'.') {
        i += 1;
        frac = digits(&mut i);
    }
    if int + frac == 0 {
        return Err("float_syntax");
    }
    if matches!(b.get(i), Some(b'e') | Some(b'E')) {
        i += 1;
        if matches!(b.get(i), Some(b'+') | Some(b'-')) {
            i += 1;
        }
        if digits(&mut i) == 0 {
            return Err("float_syntax");
        }
    }
    if i != b.len() {
        return Err("float_syntax");
    }
    let v = s.parse::<f64>().map_err(|_| "float_syntax")?;
    if v.is_finite() {
        Ok(v)
    } else {
        Err("float_not_finite")
    }
}

/// `0` or `1`.
pub(crate) fn parse_bool01(s: &str) -> Result<bool, &'static str> {
    match s {
        "0" => Ok(false),
        "1" => Ok(true),
        _ => Err("boolean_syntax"),
    }
}

/// `single` or `double`.
pub(crate) fn parse_precision(s: &str) -> Result<Precision, &'static str> {
    match s {
        "single" => Ok(Precision::Single),
        "double" => Ok(Precision::Double),
        _ => Err("precision_unknown"),
    }
}

/// True for a character that may appear in a string value: an XML 1.0 `Char` that is not a
/// control character of U+007F to U+009F. (U+0000 to U+001F other than tab, LF and CR are not
/// XML characters at all.)
pub(crate) fn is_string_char(c: char) -> bool {
    match c {
        '\t' | '\n' | '\r' => true,
        '\u{0}'..='\u{1F}' | '\u{7F}'..='\u{9F}' | '\u{FFFE}' | '\u{FFFF}' => false,
        _ => true,
    }
}

/// XML 1.0 (fifth edition) `NameStartChar` without `:`.
fn is_name_start(c: char) -> bool {
    matches!(c,
        'A'..='Z' | '_' | 'a'..='z' | '\u{C0}'..='\u{D6}' | '\u{D8}'..='\u{F6}' | '\u{F8}'..='\u{2FF}'
        | '\u{370}'..='\u{37D}' | '\u{37F}'..='\u{1FFF}' | '\u{200C}'..='\u{200D}' | '\u{2070}'..='\u{218F}'
        | '\u{2C00}'..='\u{2FEF}' | '\u{3001}'..='\u{D7FF}' | '\u{F900}'..='\u{FDCF}' | '\u{FDF0}'..='\u{FFFD}'
        | '\u{10000}'..='\u{EFFFF}')
}

/// True for an XML NCName (a name without `:`).
pub(crate) fn is_ncname(s: &str) -> bool {
    let mut chars = s.chars();
    match chars.next() {
        Some(c) if is_name_start(c) => {}
        _ => return false,
    }
    chars.all(|c| {
        is_name_start(c)
            || matches!(c, '-' | '.' | '0'..='9' | '\u{B7}' | '\u{300}'..='\u{36F}' | '\u{203F}'..='\u{2040}')
    })
}

fn in_range<T: PartialOrd + Copy>(v: Option<T>, min: Option<T>, max: Option<T>) -> Result<(), &'static str> {
    if let (Some(lo), Some(hi)) = (min, max) {
        if lo > hi {
            return Err("minimum_above_maximum");
        }
    }
    if let Some(v) = v {
        if min.is_some_and(|lo| v < lo) || max.is_some_and(|hi| v > hi) {
            return Err("value_out_of_range");
        }
    }
    Ok(())
}

/// Rounds (to nearest, ties to even) to a finite `f32` (for `precision="single"`): every
/// value up to just below `f32::MAX` plus half an ulp, so `3.4028235e38` passes.
fn single_ok(v: Option<f64>) -> bool {
    v.is_none_or(|x| (x as f32).is_finite())
}

fn finite(v: Option<f64>) -> bool {
    v.is_none_or(f64::is_finite)
}

/// The rules on one element's own attributes and value (not its children), shared by the
/// reader (after building the element) and the writer (before writing it). Containers pass.
pub(crate) fn check_leaf(node: &E57Node) -> Result<(), &'static str> {
    match node {
        E57Node::Integer { value, minimum, maximum } => in_range(*value, *minimum, *maximum),
        E57Node::ScaledInteger { raw, minimum, maximum, scale, offset } => {
            if !finite(*scale) || !finite(*offset) {
                return Err("float_not_finite");
            }
            if *scale == Some(0.0) {
                return Err("scale_zero");
            }
            in_range(*raw, *minimum, *maximum)
        }
        E57Node::Float { value, precision, minimum, maximum } => {
            if !finite(*value) || !finite(*minimum) || !finite(*maximum) {
                return Err("float_not_finite");
            }
            if *precision == Some(Precision::Single)
                && !(single_ok(*value) && single_ok(*minimum) && single_ok(*maximum))
            {
                return Err("float_out_of_range");
            }
            in_range(*value, *minimum, *maximum)
        }
        E57Node::String(t) => {
            if t.raw().chars().all(is_string_char) {
                Ok(())
            } else {
                Err("control_character")
            }
        }
        E57Node::Structure(_) | E57Node::Vector { .. } | E57Node::CompressedVector { .. } | E57Node::Blob { .. } => {
            Ok(())
        }
    }
}

/// The element names of the E57 namespace this reader knows (root, `data3D`, `images2D`,
/// point fields and their parts), sorted for binary search \[unverified against ASTM
/// E2807: taken from public descriptions and libE57Format output\]. An unprefixed element
/// with any other name is kept and counted ([`super::E57Xml::unknown_elements`]).
pub const E57_NAMES: &[&str] = &[
    "acquisitionEnd",
    "acquisitionStart",
    "associatedData3DGuid",
    "atmosphericPressure",
    "azimuthEnd",
    "azimuthStart",
    "cartesianBounds",
    "cartesianInvalidState",
    "cartesianX",
    "cartesianY",
    "cartesianZ",
    "codecs",
    "colorBlue",
    "colorBlueMaximum",
    "colorBlueMinimum",
    "colorGreen",
    "colorGreenMaximum",
    "colorGreenMinimum",
    "colorLimits",
    "colorRed",
    "colorRedMaximum",
    "colorRedMinimum",
    "columnIndex",
    "columnMaximum",
    "columnMinimum",
    "coordinateMetadata",
    "creationDateTime",
    "cylindricalRepresentation",
    "data3D",
    "dateTimeValue",
    "description",
    "e57LibraryVersion",
    "elevationMaximum",
    "elevationMinimum",
    "focalLength",
    "formatName",
    "groupingByLine",
    "groups",
    "guid",
    "idElementName",
    "idElementValue",
    "imageHeight",
    "imageMask",
    "imageWidth",
    "images2D",
    "indexBounds",
    "intensity",
    "intensityLimits",
    "intensityMaximum",
    "intensityMinimum",
    "isAtomicClockReferenced",
    "isColorInvalid",
    "isIntensityInvalid",
    "isTimeStampInvalid",
    "jpegImage",
    "name",
    "originalGuids",
    "pinholeRepresentation",
    "pixelHeight",
    "pixelWidth",
    "pngImage",
    "point",
    "pointCount",
    "pointCountPrototype",
    "pointGroupingSchemes",
    "points",
    "pose",
    "principalPointX",
    "principalPointY",
    "prototype",
    "radius",
    "rangeMaximum",
    "rangeMinimum",
    "relativeHumidity",
    "returnCount",
    "returnIndex",
    "returnMaximum",
    "returnMinimum",
    "rotation",
    "rowIndex",
    "rowMaximum",
    "rowMinimum",
    "sensorFirmwareVersion",
    "sensorHardwareVersion",
    "sensorModel",
    "sensorSerialNumber",
    "sensorSoftwareVersion",
    "sensorVendor",
    "sphericalAzimuth",
    "sphericalBounds",
    "sphericalElevation",
    "sphericalInvalidState",
    "sphericalRange",
    "sphericalRepresentation",
    "startPointIndex",
    "temperature",
    "timeStamp",
    "translation",
    "vectorChild",
    "versionMajor",
    "versionMinor",
    "visualReferenceRepresentation",
    "w",
    "x",
    "xMaximum",
    "xMinimum",
    "y",
    "yMaximum",
    "yMinimum",
    "z",
    "zMaximum",
    "zMinimum",
];

/// True for a name in [`E57_NAMES`].
pub(crate) fn is_e57_name(s: &str) -> bool {
    E57_NAMES.binary_search(&s).is_ok()
}
