//! Pure integer resolver for `RPT.BALLOON.BLOCK` (`REQ-001`..`REQ-005`, `REQ-007`).
//!
//! Zero floating-point arithmetic: aspect-preserving rational scaling, exact centering, and
//! ONE half-even rounding per mapped coordinate in `0.01 mm` units.

use super::model::{
    Affine2Int, BalloonGlyph, BalloonStyle, BlockFrame, BlockPoint, DrawingBlockContent, DrawingBlockError,
    DrawingPayload, FitMode, OutcomeMark, SheetRect,
};
use crate::balloon::register::BalloonRegister;
use crate::balloon::sheet::SheetInfo;
use crate::fmt::Catalog;
use fid_gdt::chars::dprof::CharId;
use fid_kdec::Outcome;
use std::collections::BTreeMap;

/// Exact greatest common divisor for positive `i128` integers.
fn gcd_i128(mut a: i128, mut b: i128) -> i128 {
    a = a.abs();
    b = b.abs();
    while b != 0 {
        let r = a % b;
        a = b;
        b = r;
    }
    if a == 0 {
        1
    } else {
        a
    }
}

/// Exact integer division `num / den` (`den > 0`) with IEEE round-half-to-even (`REQ-001`).
pub fn round_half_even_i128(num: i128, den: i128) -> Result<i64, DrawingBlockError> {
    if den <= 0 {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "fit", reason: "non_positive_denominator" });
    }
    let neg = num < 0;
    let abs_num = num.unsigned_abs();
    let abs_den = den.unsigned_abs();
    let q = abs_num / abs_den;
    let r = abs_num % abs_den;
    let twice_r = r.checked_mul(2).ok_or_else(|| DrawingBlockError::Store("half-even overflow".into()))?;
    let rounded_abs = if twice_r > abs_den || (twice_r == abs_den && (q & 1) == 1) {
        q.checked_add(1).ok_or_else(|| DrawingBlockError::Store("half-even overflow".into()))?
    } else {
        q
    };
    let signed = if neg {
        let s = i128::try_from(rounded_abs).map_err(|_| DrawingBlockError::Store("half-even overflow".into()))?;
        -s
    } else {
        i128::try_from(rounded_abs).map_err(|_| DrawingBlockError::Store("half-even overflow".into()))?
    };
    i64::try_from(signed).map_err(|_| DrawingBlockError::Store("mapped coordinate overflows i64".into()))
}

/// Computes the semantic colour role, non-colour cue glyph (`design/tokens/status.json`), and
/// catalog text (`RPT.MODEL.FMT`) for a characteristic's optional decision outcome (`REQ-003`).
pub fn outcome_mark(outcome: Option<Outcome>, catalog: &Catalog) -> OutcomeMark {
    match outcome {
        Some(Outcome::Conforms) => OutcomeMark {
            colour_role: "status.conforms".into(),
            symbol: "state.conforms".into(),
            label: catalog.text("report.outcome.conforms").into(),
        },
        Some(Outcome::Nonconforms) => OutcomeMark {
            colour_role: "status.nonconforms".into(),
            symbol: "state.nonconforms".into(),
            label: catalog.text("report.outcome.nonconforms").into(),
        },
        Some(Outcome::Indeterminate) => OutcomeMark {
            colour_role: "status.indeterminate".into(),
            symbol: "state.indeterminate".into(),
            label: catalog.text("report.outcome.indeterminate").into(),
        },
        Some(Outcome::NotDecidable(_)) => OutcomeMark {
            colour_role: "status.not-decidable".into(),
            symbol: "state.not-decidable".into(),
            label: catalog.text("report.outcome.not_decidable").into(),
        },
        None => OutcomeMark {
            colour_role: "status.not-evaluated".into(),
            symbol: "state.not-evaluated".into(),
            label: catalog.text("report.uec.not_evaluated").into(),
        },
    }
}

/// Pure resolver turning a stored [`DrawingPayload`] plus the live [`BalloonRegister`],
/// [`SheetInfo`], and characteristic outcomes into [`DrawingBlockContent`]
/// (`REQ-001`..`REQ-005`, `REQ-007`, `AC-1`..`AC-4`).
pub fn resolve(
    payload: &DrawingPayload,
    frame: BlockFrame,
    register: &BalloonRegister,
    sheets: &dyn SheetInfo,
    outcomes: &BTreeMap<CharId, Outcome>,
    catalog: &Catalog,
    style: &BalloonStyle,
) -> Result<DrawingBlockContent, DrawingBlockError> {
    if frame.width_cmm <= 0 || frame.height_cmm <= 0 {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "frame", reason: "non_positive_frame" });
    }
    let Some((sheet_w, sheet_h)) = sheets.bounds(payload.sheet) else {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "sheet", reason: "sheet_missing_or_not_ready" });
    };
    let Some(drawing_rev) = sheets.revision(payload.sheet) else {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "sheet", reason: "sheet_missing" });
    };
    if sheet_w <= 0 || sheet_h <= 0 {
        return Err(DrawingBlockError::InvalidBlockPayload { field: "sheet", reason: "invalid_sheet_bounds" });
    }

    let src = match payload.crop {
        Some(c) => {
            if !c.fits_in_sheet(sheet_w, sheet_h) {
                return Err(DrawingBlockError::InvalidBlockPayload { field: "crop", reason: "crop_out_of_bounds" });
            }
            c
        }
        None => SheetRect::new(0, 0, sheet_w, sheet_h),
    };

    // Exact rational isotropic scale preserving aspect ratio (REQ-007).
    let fw = i128::from(frame.width_cmm);
    let fh = i128::from(frame.height_cmm);
    let sw = i128::from(src.width);
    let sh = i128::from(src.height);

    let (raw_num, raw_den) = match payload.fit {
        FitMode::ScaleToFit => {
            if fw * sh <= fh * sw {
                (fw, sw)
            } else {
                (fh, sh)
            }
        }
        FitMode::FixedScale { num, den } => {
            if num == 0 || den == 0 {
                return Err(DrawingBlockError::InvalidBlockPayload { field: "fit", reason: "zero_scale" });
            }
            (i128::from(num), i128::from(den))
        }
    };
    let g = gcd_i128(raw_num, raw_den);
    let scale_num_128 = raw_num / g;
    let scale_den_128 = raw_den / g;
    let scale_num = i64::try_from(scale_num_128).map_err(|_| DrawingBlockError::Store("scale_num overflow".into()))?;
    let scale_den = i64::try_from(scale_den_128).map_err(|_| DrawingBlockError::Store("scale_den overflow".into()))?;

    // Centre the scaled source rect inside the block frame (REQ-007).
    let offset_x = round_half_even_i128(fw * scale_den_128 - sw * scale_num_128, 2 * scale_den_128)?;
    let offset_y = round_half_even_i128(fh * scale_den_128 - sh * scale_num_128, 2 * scale_den_128)?;

    let transform = Affine2Int { scale_num, scale_den, offset_x, offset_y };

    let map_pt = |px: i64, py: i64| -> Result<BlockPoint, DrawingBlockError> {
        // ONE half-even rounding per coordinate (REQ-001).
        let dx = i128::from(px) - i128::from(src.x);
        let dy = i128::from(py) - i128::from(src.y);
        let rx = round_half_even_i128(dx * scale_num_128, scale_den_128)?;
        let ry = round_half_even_i128(dy * scale_num_128, scale_den_128)?;
        let bx = rx.checked_add(offset_x).ok_or_else(|| DrawingBlockError::Store("x offset overflow".into()))?;
        let by = ry.checked_add(offset_y).ok_or_else(|| DrawingBlockError::Store("y offset overflow".into()))?;
        Ok(BlockPoint::new(bx, by))
    };

    // Read balloons live from register at the generation head (already sorted by ascending number, REQ-002).
    let balloons = register.list(drawing_rev);
    let mut glyphs = Vec::new();
    let mut excluded = Vec::new();

    for b in balloons {
        if b.sheet != payload.sheet {
            continue;
        }
        if !src.contains(b.anchor.x, b.anchor.y) {
            excluded.push(b.number);
            continue;
        }
        let anchor = map_pt(b.anchor.x, b.anchor.y)?;
        let label = map_pt(b.label.x, b.label.y)?;
        let leader = payload.show_leaders.then_some((anchor, label));
        let outcome = payload.outcome_marking.then(|| outcome_mark(outcomes.get(&b.characteristic).copied(), catalog));
        glyphs.push(BalloonGlyph { number: b.number, anchor, label, leader, style: *style, outcome });
    }

    Ok(DrawingBlockContent {
        sheet_id: payload.sheet,
        sheet_image: sheets.image(payload.sheet),
        transform,
        glyphs,
        excluded,
    })
}
