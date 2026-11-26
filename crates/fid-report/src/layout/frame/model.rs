//! Data model of block frames (`RPT.DESIGN.PLACE.FRAME`, `Design: Data structures`).
//!
//! Integer geometry only: every coordinate is a [`Length`] in `0.01 mm`. Wire and stored
//! forms write integers as decimal strings, as the page setup does.

use crate::doc::model::obj;
use crate::doc::{BlockId, Report};
use crate::layout::page::{LayoutError, Length, MAX_LENGTH};
use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_types::Digest;
use std::collections::BTreeMap;

/// Smallest frame width and height: `1 mm` (REQ-004).
pub const MIN_SIDE: Length = 100;
/// Largest frame width and height: `2000 mm`, the largest page side of `RPT.DESIGN.PAGE`.
pub const MAX_SIDE: Length = MAX_LENGTH;
/// Largest `|x|` and `|y|` of a frame: `2000 mm` from the body origin. A frame may lie
/// outside the body (REQ-006), but not further out than the largest page.
pub const MAX_COORD: Length = MAX_LENGTH;

/// One block's frame: left `x` and top `y` relative to the page body origin, width `w` and
/// height `h`, all in `0.01 mm` (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BlockFrame {
    pub x: Length,
    pub y: Length,
    pub w: Length,
    pub h: Length,
}

fn invalid(field: &'static str, reason: &'static str) -> LayoutError {
    LayoutError::LayoutInvalid { field, reason }
}

fn int(n: Length) -> CValue {
    CValue::Str(n.to_string())
}

impl BlockFrame {
    /// Checks the frame bounds, in the order `x`, `y`, `w`, `h`: `|x|` and `|y|` at most
    /// [`MAX_COORD`] (`too_large`); `w` and `h` at least [`MIN_SIDE`] (`too_small`, REQ-004)
    /// and at most [`MAX_SIDE`] (`too_large`). No body check: a frame may lie outside the
    /// body (REQ-006).
    pub fn check(&self) -> Result<(), LayoutError> {
        for (field, v) in [("x", self.x), ("y", self.y)] {
            if !(-MAX_COORD..=MAX_COORD).contains(&v) {
                return Err(invalid(field, "too_large"));
            }
        }
        for (field, v) in [("w", self.w), ("h", self.h)] {
            if v < MIN_SIDE {
                return Err(invalid(field, "too_small"));
            }
            if v > MAX_SIDE {
                return Err(invalid(field, "too_large"));
            }
        }
        Ok(())
    }

    /// The frame with each edge snapped to `pitch` ([`snap_edge`]), then `w` and `h`
    /// recomputed from the snapped edges (REQ-002). `None` on arithmetic overflow, which a
    /// frame built from bounded lengths cannot reach.
    pub fn snapped(&self, pitch: Length) -> Option<BlockFrame> {
        let left = snap_edge(self.x, pitch)?;
        let top = snap_edge(self.y, pitch)?;
        let right = snap_edge(self.x.checked_add(self.w)?, pitch)?;
        let bottom = snap_edge(self.y.checked_add(self.h)?, pitch)?;
        Some(BlockFrame { x: left, y: top, w: right.checked_sub(left)?, h: bottom.checked_sub(top)? })
    }

    /// Stored and journaled form: `{h, w, x, y}`, integers as decimal strings.
    pub fn to_value(&self) -> CValue {
        obj([("h", int(self.h)), ("w", int(self.w)), ("x", int(self.x)), ("y", int(self.y))])
    }

    /// Reads the stored form back: exactly its keys, canonical decimal integers (an optional
    /// `-` on `x` and `y` only, no `-0`, no leading zero). It does not run [`Self::check`].
    pub fn from_value(v: &CValue) -> Result<BlockFrame, LayoutError> {
        let CValue::Obj(m) = v else { return Err(store("frame")) };
        let keys = ["h", "w", "x", "y"];
        if m.len() != keys.len() || keys.iter().any(|k| !m.contains_key(*k)) {
            return Err(store("frame keys"));
        }
        let get = |k: &str, signed: bool| -> Result<Length, LayoutError> {
            match m.get(k) {
                Some(CValue::Str(s)) => canonical_int(s, signed).ok_or(store("frame integer")),
                _ => Err(store("frame integer")),
            }
        };
        Ok(BlockFrame { x: get("x", true)?, y: get("y", true)?, w: get("w", false)?, h: get("h", false)? })
    }
}

fn store(what: &'static str) -> LayoutError {
    LayoutError::StoreInvalid(what)
}

/// A canonical decimal integer of at most 7 digits (every bounded frame value fits): no `+`,
/// no leading zero, no `-0`, and a `-` only when `signed`.
fn canonical_int(s: &str, signed: bool) -> Option<Length> {
    let (neg, digits) = match s.strip_prefix('-') {
        Some(d) if signed => (true, d),
        Some(_) => return None,
        None => (false, s),
    };
    let ok = !digits.is_empty()
        && digits.len() <= 7
        && digits.bytes().all(|b| b.is_ascii_digit())
        && (digits == "0" || !digits.starts_with('0'))
        && !(neg && digits == "0");
    if !ok {
        return None;
    }
    let n: Length = digits.parse().ok()?;
    Some(if neg { -n } else { n })
}

/// `v` rounded to the nearest multiple of `pitch`, ties away from zero (REQ-002); `None`
/// when `pitch` is not positive or on overflow. Integer arithmetic only.
pub fn snap_edge(v: Length, pitch: Length) -> Option<Length> {
    if pitch <= 0 {
        return None;
    }
    let a = v.checked_abs()?;
    let mut q = a / pitch;
    let r = a % pitch;
    // r < pitch <= i64::MAX / 2 for every pitch this crate accepts; checked anyway.
    if r.checked_mul(2)? >= pitch {
        q = q.checked_add(1)?;
    }
    let m = q.checked_mul(pitch)?;
    Some(if v < 0 { -m } else { m })
}

/// The frames of one report, keyed by block ID (`Design: Data structures`; `BTreeMap`, no
/// hash map). A block without an entry has no frame yet.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReportFrames {
    pub frames: BTreeMap<BlockId, BlockFrame>,
}

impl ReportFrames {
    /// The frame of `block`, if it has one.
    pub fn get(&self, block: &BlockId) -> Option<BlockFrame> {
        self.frames.get(block).copied()
    }

    /// Stored form: `{<block id>: {h, w, x, y}}`.
    pub fn to_value(&self) -> CValue {
        CValue::Obj(self.frames.iter().map(|(b, f)| (b.uuid().to_string(), f.to_value())).collect())
    }

    /// SHA-256 of the canonical (RFC 8785) bytes of [`Self::to_value`]; every
    /// `report.move_block_layout@1` event carries it, and exact replay compares it.
    pub fn digest(&self) -> Result<Digest, LayoutError> {
        let bytes = to_canonical_bytes(&self.to_value()).map_err(|_| store("canonical form"))?;
        Ok(Digest::from_bytes(digest_bytes(&bytes).0))
    }
}

/// SHA-256 of the canonical bytes of every live block's content, keyed by block ID:
/// `{<block id>: {kind, kind_version, payload}}`. Bindings, bound values, decisions and
/// formatter choices all live in the payload, so a placement command must leave this digest
/// unchanged (REQ-005, AC-4); page order and membership are not part of it.
pub fn payloads_digest(report: &Report) -> Result<Digest, LayoutError> {
    let mut m = BTreeMap::new();
    for page in report.pages() {
        for b in page.blocks() {
            let v = obj([
                ("kind", CValue::Str(b.kind().as_str().to_string())),
                ("kind_version", CValue::Str(b.kind_version().to_string())),
                ("payload", b.payload().value().clone()),
            ]);
            m.insert(b.id().uuid().to_string(), v);
        }
    }
    let bytes = to_canonical_bytes(&CValue::Obj(m)).map_err(|_| store("canonical form"))?;
    Ok(Digest::from_bytes(digest_bytes(&bytes).0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapping_ties_away_from_zero() {
        assert_eq!(snap_edge(50, 100), Some(100));
        assert_eq!(snap_edge(-50, 100), Some(-100));
        assert_eq!(snap_edge(49, 100), Some(0));
        assert_eq!(snap_edge(-49, 100), Some(0));
        assert_eq!(snap_edge(1004, 100), Some(1000));
        assert_eq!(snap_edge(0, 100), Some(0));
        assert_eq!(snap_edge(5, 0), None);
        assert_eq!(snap_edge(5, -10), None);
        assert_eq!(snap_edge(i64::MIN, 10), None);
    }

    #[test]
    fn canonical_integers() {
        assert_eq!(canonical_int("0", true), Some(0));
        assert_eq!(canonical_int("-5", true), Some(-5));
        assert_eq!(canonical_int("-5", false), None);
        for bad in ["", "-", "-0", "01", "+1", "1.0", "12345678", " 1"] {
            assert_eq!(canonical_int(bad, true), None, "{bad:?}");
        }
    }
}
