//! Bounded parsing of serialised ViewStates.
//!
//! Order of checks, so the first fault found is deterministic:
//! 1. input size ([`MAX_INPUT_BYTES`]);
//! 2. a lexical pre-scan that tracks the field path and refuses nesting over [`MAX_DEPTH`], more
//!    than [`MAX_SEPARATORS`] commas (which bounds the number of JSON values before any tree is
//!    built), and non-finite numbers (`NaN`, `Infinity`, or a literal that overflows), naming the
//!    field;
//! 3. JSON syntax and duplicate keys (`fid_canon::parse_with_limits`);
//! 4. `schema` (before the key set, so a future version is reported as such);
//! 5. structure (key sets, types, collection sizes before their entries are decoded);
//! 6. [`ViewState::validate`];
//! 7. for [`parse_canonical`] only: the input bytes equal the canonical bytes.

use super::{
    Camera, ClipPlane, EntityId, LegendLayoutRef, Projection, TransparencyEntry, ViewState, ViewStateError, Viewport,
    VisibilityRule, MAX_CLIP_PLANES, MAX_DEPTH, MAX_ENTRIES, MAX_INPUT_BYTES, SCHEMA_VERSION,
};
use fid_canon::{CValue, CanonError};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Most commas outside strings: two per visibility or transparency entry at [`MAX_ENTRIES`]
/// each, plus room for the camera, viewport and [`MAX_CLIP_PLANES`] clip planes.
pub const MAX_SEPARATORS: usize = 4 * MAX_ENTRIES + 1_024;

fn malformed(field: &str, reason: &str) -> ViewStateError {
    ViewStateError::Malformed { field: field.to_string(), reason: reason.to_string() }
}

/// Parses a serialised ViewState in any valid JSON layout (whitespace, key order, number
/// spelling and collection order are free) and validates it. The result has its collections
/// in canonical order. `-0` parses as `+0.0`.
pub fn parse(input: &[u8]) -> Result<ViewState, ViewStateError> {
    if input.len() > MAX_INPUT_BYTES {
        return Err(ViewStateError::LimitExceeded { field: "$".to_string(), limit: MAX_INPUT_BYTES });
    }
    prescan(input)?;
    let tree = fid_canon::parse_with_limits(input, MAX_INPUT_BYTES, MAX_DEPTH).map_err(|e| canon_error(&e))?;
    let mut vs = decode(&tree)?;
    vs.validate()?;
    vs.sort_collections();
    Ok(vs)
}

/// [`parse`], then requires the input to be exactly the canonical bytes
/// ([`ViewStateError::NotCanonical`] otherwise). Use where bytes are compared or hashed.
pub fn parse_canonical(input: &[u8]) -> Result<ViewState, ViewStateError> {
    let vs = parse(input)?;
    let canon = vs.to_canonical_bytes()?;
    if canon != input {
        let at = canon.iter().zip(input.iter()).position(|(a, b)| a != b).unwrap_or(canon.len().min(input.len()));
        return Err(ViewStateError::NotCanonical { at });
    }
    Ok(vs)
}

fn canon_error(e: &CanonError) -> ViewStateError {
    match e {
        // The pre-scan reports these with a field; kept as a fallback.
        CanonError::NonFiniteNumber => ViewStateError::NonFiniteNumber { field: "$".to_string() },
        CanonError::InputLimit => ViewStateError::LimitExceeded { field: "$".to_string(), limit: MAX_DEPTH },
        CanonError::DuplicateKey(k) => {
            ViewStateError::Malformed { field: "$".to_string(), reason: format!("duplicate key {k:?}") }
        }
        CanonError::InvalidUtf8 => malformed("$", "invalid UTF-8"),
        CanonError::Syntax(at) => {
            ViewStateError::Malformed { field: "$".to_string(), reason: format!("invalid JSON at byte {at}") }
        }
    }
}

// ------------------------------------------------------------------ pre-scan

enum Frame {
    /// Object; the most recent key, if one has been read since the last comma.
    Obj(Option<String>),
    /// Array; index of the current element.
    Arr(usize),
}

fn path(stack: &[Frame]) -> String {
    let mut s = String::new();
    for f in stack {
        match f {
            Frame::Obj(Some(k)) => {
                if !s.is_empty() {
                    s.push('.');
                }
                s.push_str(k);
            }
            Frame::Obj(None) => {}
            Frame::Arr(i) => s.push_str(&format!("[{i}]")),
        }
    }
    if s.is_empty() {
        s.push('$');
    }
    s
}

/// Longest key kept for field paths; longer keys are unknown anyway and fail later.
const MAX_KEY_KEPT: usize = 64;

/// A lexical pass that never builds values: it tracks the field path and enforces depth,
/// separator and finiteness limits. It does not judge syntax; [`fid_canon`] does that next.
fn prescan(input: &[u8]) -> Result<(), ViewStateError> {
    let mut stack: Vec<Frame> = Vec::new();
    let mut commas = 0usize;
    let mut i = 0usize;
    while let Some(&b) = input.get(i) {
        match b {
            b'"' => {
                // String: find its closing quote; keep it as the key if an object expects one.
                let start = i + 1;
                i = start;
                while let Some(&c) = input.get(i) {
                    if c == b'\\' {
                        i += 2;
                        continue;
                    }
                    if c == b'"' {
                        break;
                    }
                    i += 1;
                }
                let end = i.min(input.len());
                if let Some(Frame::Obj(key @ None)) = stack.last_mut() {
                    let raw = input.get(start..end).unwrap_or_default();
                    let kept = raw.get(..MAX_KEY_KEPT).unwrap_or(raw);
                    *key = Some(String::from_utf8_lossy(kept).into_owned());
                }
                i += 1;
            }
            b'{' | b'[' => {
                if stack.len() >= MAX_DEPTH {
                    return Err(ViewStateError::LimitExceeded { field: path(&stack), limit: MAX_DEPTH });
                }
                stack.push(if b == b'{' { Frame::Obj(None) } else { Frame::Arr(0) });
                i += 1;
            }
            b'}' | b']' => {
                stack.pop();
                i += 1;
            }
            b',' => {
                commas += 1;
                if commas > MAX_SEPARATORS {
                    return Err(ViewStateError::LimitExceeded { field: path(&stack), limit: MAX_ENTRIES });
                }
                match stack.last_mut() {
                    Some(Frame::Arr(n)) => *n += 1,
                    Some(Frame::Obj(k)) => *k = None,
                    None => {}
                }
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while matches!(input.get(i), Some(b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')) {
                    i += 1;
                }
                if b == b'-' && i == start + 1 && word_at(input, i) == b"Infinity" {
                    return Err(ViewStateError::NonFiniteNumber { field: path(&stack) });
                }
                let text = std::str::from_utf8(input.get(start..i).unwrap_or_default()).unwrap_or_default();
                if let Ok(v) = text.parse::<f64>() {
                    if !v.is_finite() {
                        return Err(ViewStateError::NonFiniteNumber { field: path(&stack) });
                    }
                }
            }
            b'A'..=b'Z' | b'a'..=b'z' => {
                let w = word_at(input, i);
                if w == b"NaN" || w == b"Infinity" {
                    return Err(ViewStateError::NonFiniteNumber { field: path(&stack) });
                }
                i += w.len();
            }
            _ => i += 1,
        }
    }
    Ok(())
}

/// The run of ASCII letters starting at `i`.
fn word_at(input: &[u8], i: usize) -> &[u8] {
    let rest = input.get(i..).unwrap_or_default();
    let n = rest.iter().take_while(|c| c.is_ascii_alphabetic()).count();
    rest.get(..n).unwrap_or_default()
}

// ------------------------------------------------------------------ decoding

type Map = BTreeMap<String, CValue>;

fn join(field: &str, key: &str) -> String {
    if field.is_empty() {
        key.to_string()
    } else {
        format!("{field}.{key}")
    }
}

/// The object at `field` with exactly the keys `keys`.
fn object<'a>(v: &'a CValue, field: &str, keys: &[&str]) -> Result<&'a Map, ViewStateError> {
    let shown = if field.is_empty() { "$" } else { field };
    let CValue::Obj(map) = v else {
        return Err(malformed(shown, "expected an object"));
    };
    for k in keys {
        if !map.contains_key(*k) {
            return Err(malformed(&join(field, k), "missing field"));
        }
    }
    if let Some(extra) = map.keys().find(|k| !keys.contains(&k.as_str())) {
        return Err(malformed(&join(field, extra), "unknown field"));
    }
    Ok(map)
}

fn get<'a>(map: &'a Map, field: &str, key: &str) -> Result<&'a CValue, ViewStateError> {
    map.get(key).ok_or_else(|| malformed(&join(field, key), "missing field"))
}

fn f64_at(v: &CValue, field: &str) -> Result<f64, ViewStateError> {
    match v {
        CValue::Num(f) if f.is_finite() => Ok(*f),
        CValue::Num(_) => Err(ViewStateError::NonFiniteNumber { field: field.to_string() }),
        _ => Err(malformed(field, "expected a number")),
    }
}

/// A JSON number that is an integer in `0..=max`.
fn uint_at(v: &CValue, field: &str, max: u32) -> Result<u32, ViewStateError> {
    let f = f64_at(v, field)?;
    if f.fract() != 0.0 || f < 0.0 || f > f64::from(max) {
        return Err(malformed(field, "expected an unsigned integer"));
    }
    // Integral and within 0..=u32::MAX, so the conversion is exact.
    Ok(f as u32)
}

fn vec3_at(v: &CValue, field: &str) -> Result<[f64; 3], ViewStateError> {
    match v {
        CValue::Arr(items) if items.len() == 3 => {
            let mut out = [0.0; 3];
            for (i, (o, item)) in out.iter_mut().zip(items).enumerate() {
                *o = f64_at(item, &format!("{field}[{i}]"))?;
            }
            Ok(out)
        }
        _ => Err(malformed(field, "expected an array of 3 numbers")),
    }
}

fn str_at<'a>(v: &'a CValue, field: &str) -> Result<&'a str, ViewStateError> {
    match v {
        CValue::Str(s) => Ok(s),
        _ => Err(malformed(field, "expected a string")),
    }
}

fn uuid_at(v: &CValue, field: &str) -> Result<EntityId, ViewStateError> {
    EntityId::parse(str_at(v, field)?).map_err(|_| malformed(field, "expected a UUID"))
}

fn digest_at(v: &CValue, field: &str) -> Result<Digest, ViewStateError> {
    Digest::parse(str_at(v, field)?).map_err(|_| malformed(field, "expected a sha256: digest"))
}

fn optional<T>(
    v: &CValue,
    field: &str,
    f: impl FnOnce(&CValue, &str) -> Result<T, ViewStateError>,
) -> Result<Option<T>, ViewStateError> {
    match v {
        CValue::Null => Ok(None),
        other => f(other, field).map(Some),
    }
}

fn array_at<'a>(v: &'a CValue, field: &str, limit: usize) -> Result<&'a [CValue], ViewStateError> {
    match v {
        CValue::Arr(items) if items.len() > limit => {
            Err(ViewStateError::LimitExceeded { field: field.to_string(), limit })
        }
        CValue::Arr(items) => Ok(items),
        _ => Err(malformed(field, "expected an array")),
    }
}

const ROOT_KEYS: [&str; 9] =
    ["camera", "clips", "colormap", "labels", "legend", "schema", "transparency", "viewport", "visibility"];

fn decode(tree: &CValue) -> Result<ViewState, ViewStateError> {
    let CValue::Obj(root) = tree else {
        return Err(malformed("$", "expected an object"));
    };
    // Schema first: a future version may have other fields.
    let schema_v = get(root, "", "schema")?;
    let schema = match schema_v {
        CValue::Num(f) if f.fract() == 0.0 && *f >= 0.0 && *f <= 9_007_199_254_740_992.0 => *f as u64,
        _ => return Err(malformed("schema", "expected an unsigned integer")),
    };
    if schema != u64::from(SCHEMA_VERSION) {
        return Err(ViewStateError::UnsupportedSchema { found: schema });
    }
    let root = object(tree, "", &ROOT_KEYS)?;

    let camera = decode_camera(get(root, "", "camera")?)?;

    let vp = object(get(root, "", "viewport")?, "viewport", &["height_px", "width_px"])?;
    let viewport = Viewport {
        height_px: uint_at(get(vp, "viewport", "height_px")?, "viewport.height_px", u32::MAX)?,
        width_px: uint_at(get(vp, "viewport", "width_px")?, "viewport.width_px", u32::MAX)?,
    };

    let clip_items = array_at(get(root, "", "clips")?, "clips", MAX_CLIP_PLANES)?;
    let vis_items = array_at(get(root, "", "visibility")?, "visibility", MAX_ENTRIES)?;
    let tr_items = array_at(get(root, "", "transparency")?, "transparency", MAX_ENTRIES)?;

    let mut clips = Vec::with_capacity(clip_items.len());
    for (i, item) in clip_items.iter().enumerate() {
        let f = format!("clips[{i}]");
        let m = object(item, &f, &["id", "normal", "offset_m"])?;
        clips.push(ClipPlane {
            id: uint_at(get(m, &f, "id")?, &join(&f, "id"), u32::MAX)?,
            normal: vec3_at(get(m, &f, "normal")?, &join(&f, "normal"))?,
            offset_m: f64_at(get(m, &f, "offset_m")?, &join(&f, "offset_m"))?,
        });
    }

    let mut visibility = Vec::with_capacity(vis_items.len());
    for (i, item) in vis_items.iter().enumerate() {
        let f = format!("visibility[{i}]");
        let m = object(item, &f, &["entity", "visible"])?;
        let visible = match get(m, &f, "visible")? {
            CValue::Bool(b) => *b,
            _ => return Err(malformed(&join(&f, "visible"), "expected a boolean")),
        };
        visibility.push(VisibilityRule { entity: uuid_at(get(m, &f, "entity")?, &join(&f, "entity"))?, visible });
    }

    let mut transparency = Vec::with_capacity(tr_items.len());
    for (i, item) in tr_items.iter().enumerate() {
        let f = format!("transparency[{i}]");
        let m = object(item, &f, &["alpha", "entity"])?;
        transparency.push(TransparencyEntry {
            alpha: f64_at(get(m, &f, "alpha")?, &join(&f, "alpha"))?,
            entity: uuid_at(get(m, &f, "entity")?, &join(&f, "entity"))?,
        });
    }

    Ok(ViewState {
        schema: SCHEMA_VERSION,
        camera,
        viewport,
        clips,
        visibility,
        transparency,
        colormap: optional(get(root, "", "colormap")?, "colormap", digest_at)?,
        legend: optional(get(root, "", "legend")?, "legend", |v, f| digest_at(v, f).map(LegendLayoutRef))?,
        labels: optional(get(root, "", "labels")?, "labels", uuid_at)?,
    })
}

fn decode_camera(v: &CValue) -> Result<Camera, ViewStateError> {
    let f = "camera";
    let m = object(v, f, &["eye_m", "far_m", "near_m", "projection", "target_m", "up"])?;
    let pv = get(m, f, "projection")?;
    let pf = "camera.projection";
    let kind = match pv {
        CValue::Obj(pm) => str_at(get(pm, pf, "kind")?, "camera.projection.kind")?,
        _ => return Err(malformed(pf, "expected an object")),
    };
    let projection = match kind {
        "perspective" => {
            let pm = object(pv, pf, &["fov_y_rad", "kind"])?;
            Projection::Perspective { fov_y_rad: f64_at(get(pm, pf, "fov_y_rad")?, "camera.projection.fov_y_rad")? }
        }
        "ortho" => {
            let pm = object(pv, pf, &["height_m", "kind"])?;
            Projection::Ortho { height_m: f64_at(get(pm, pf, "height_m")?, "camera.projection.height_m")? }
        }
        _ => return Err(malformed("camera.projection.kind", "expected \"perspective\" or \"ortho\"")),
    };
    Ok(Camera {
        eye_m: vec3_at(get(m, f, "eye_m")?, "camera.eye_m")?,
        target_m: vec3_at(get(m, f, "target_m")?, "camera.target_m")?,
        up: vec3_at(get(m, f, "up")?, "camera.up")?,
        projection,
        near_m: f64_at(get(m, f, "near_m")?, "camera.near_m")?,
        far_m: f64_at(get(m, f, "far_m")?, "camera.far_m")?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nonfinite(field: &str) -> Result<(), ViewStateError> {
        Err(ViewStateError::NonFiniteNumber { field: field.to_string() })
    }

    #[test]
    fn prescan_names_the_field_of_a_non_finite_literal() {
        assert_eq!(prescan(br#"{"camera":{"eye_m":[0,NaN,0]}}"#), nonfinite("camera.eye_m[1]"));
        assert_eq!(prescan(br#"{"a":1,"clips":[{"id":1},{"offset_m":-Infinity}]}"#), nonfinite("clips[1].offset_m"));
        assert_eq!(prescan(br#"{"x":"a,b","y":1e999}"#), nonfinite("y"));
        assert_eq!(prescan(br#"{"s":"NaN \"Infinity\" 1e999"}"#), Ok(()));
    }

    #[test]
    fn prescan_bounds_depth() {
        assert_eq!(prescan(b"[[[[]]]]"), Ok(()));
        assert_eq!(
            prescan(b"[[[[[]]]]]"),
            Err(ViewStateError::LimitExceeded { field: "[0][0][0][0]".to_string(), limit: MAX_DEPTH })
        );
    }
}
