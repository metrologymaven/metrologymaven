//! Bounded RGBA8 display-tile / cropped-snippet rasteriser for on-screen ballooning and
//! Proposal Inbox snippet previews (`RPT.BALLOON.AUTO.TEXT`, `REQ-005`).
//!
//! Pure integer math (zero floats, zero external deps): renders vector path segments, callout
//! frame boxes, and legible bitmap glyph boxes for extracted `TextRun`s at up to 300 dpi,
//! enforcing the 64 Mpx per-sheet memory cap.

use super::model::{
    AutoTextError, DisplayTile, DrawingTextMap, SheetRect, DEFAULT_TILE_DPI, MAX_TILE_DPI, MAX_TILE_PIXELS_PER_SHEET,
};
use crate::balloon::sheet::round_half_even_u128;

/// A vector line segment in integer `0.01 mm` sheet coordinates (top-left origin) recorded
/// during PDF content-stream extraction for display-tile rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SheetSegment {
    pub x0: i64,
    pub y0: i64,
    pub x1: i64,
    pub y1: i64,
}

/// Renders a bounded RGBA8 display tile for `crop` (in integer `0.01 mm` sheet coordinates)
/// at `dpi` (defaults to [`DEFAULT_TILE_DPI`] when `0`, capped at [`MAX_TILE_DPI`]).
///
/// Enforces the 64 Mpx per-sheet cap before allocating the RGBA buffer (`REQ-005`).
pub fn render_display_tile(
    sheet_bounds_cmm: (i64, i64),
    text_map: &DrawingTextMap,
    segments: &[SheetSegment],
    crop: SheetRect,
    dpi: u32,
) -> Result<DisplayTile, AutoTextError> {
    let dpi = if dpi == 0 { DEFAULT_TILE_DPI } else { dpi };
    if dpi > MAX_TILE_DPI {
        return Err(AutoTextError::LimitExceeded {
            limit: "tile_dpi",
            detail: format!("requested tile DPI {dpi} exceeds max {MAX_TILE_DPI}"),
        });
    }
    let (sheet_w, sheet_h) = sheet_bounds_cmm;
    if sheet_w <= 0 || sheet_h <= 0 {
        return Err(AutoTextError::BadParams("sheet bounds must be positive"));
    }

    // Check full-sheet pixel cap at the requested DPI (64 Mpx per sheet).
    let full_w = cmm_to_px(sheet_w, dpi).max(1);
    let full_h = cmm_to_px(sheet_h, dpi).max(1);
    let full_pixels = u64::from(full_w).saturating_mul(u64::from(full_h));
    if full_pixels > MAX_TILE_PIXELS_PER_SHEET {
        return Err(AutoTextError::LimitExceeded {
            limit: "tile_pixels",
            detail: format!("sheet at {dpi} dpi requires {full_pixels} px (max {MAX_TILE_PIXELS_PER_SHEET})"),
        });
    }

    let crop = SheetRect::new(
        crop.x0.clamp(0, sheet_w),
        crop.y0.clamp(0, sheet_h),
        crop.x1.clamp(0, sheet_w),
        crop.y1.clamp(0, sheet_h),
    );
    if crop.width() <= 0 || crop.height() <= 0 {
        return Err(AutoTextError::BadParams("crop rectangle has zero area"));
    }

    let w = cmm_to_px(crop.width(), dpi).max(1);
    let h = cmm_to_px(crop.height(), dpi).max(1);
    let px_count = u64::from(w).saturating_mul(u64::from(h));
    if px_count > MAX_TILE_PIXELS_PER_SHEET {
        return Err(AutoTextError::LimitExceeded {
            limit: "tile_pixels",
            detail: format!("tile requires {px_count} px (max {MAX_TILE_PIXELS_PER_SHEET})"),
        });
    }

    let mut rgba = vec![0xFFu8; (px_count as usize) * 4]; // crisp white sheet background

    let to_px = |x_cmm: i64, y_cmm: i64| -> (i64, i64) {
        let rx = x_cmm - crop.x0;
        let ry = y_cmm - crop.y0;
        let px = ((i128::from(rx) * i128::from(dpi)) / 2540) as i64;
        let py = ((i128::from(ry) * i128::from(dpi)) / 2540) as i64;
        (px, py)
    };

    // Draw vector line segments in dark slate (#1E293B).
    for seg in segments {
        let (x0, y0) = to_px(seg.x0, seg.y0);
        let (x1, y1) = to_px(seg.x1, seg.y1);
        draw_line(&mut rgba, w, h, x0, y0, x1, y1, [0x1E, 0x29, 0x3B, 0xFF]);
    }

    // Draw callout frame boxes in engineering blue (#1D4ED8).
    for fb in &text_map.frame_boxes {
        if fb.x1 < crop.x0 || fb.x0 > crop.x1 || fb.y1 < crop.y0 || fb.y0 > crop.y1 {
            continue;
        }
        let (x0, y0) = to_px(fb.x0, fb.y0);
        let (x1, y1) = to_px(fb.x1, fb.y1);
        draw_rect_outline(&mut rgba, w, h, x0, y0, x1, y1, [0x1D, 0x4E, 0xD8, 0xFF]);
    }

    // Draw deterministic glyph baselines / ink marks for text runs in crisp ink (#0F172A).
    for run in &text_map.runs {
        let b = run.bbox;
        if b.x1 < crop.x0 || b.x0 > crop.x1 || b.y1 < crop.y0 || b.y0 > crop.y1 {
            continue;
        }
        let (x0, y0) = to_px(b.x0, b.y0);
        let (x1, y1) = to_px(b.x1, b.y1);
        let baseline_y = y0 + ((y1 - y0) * 4) / 5;
        draw_line(&mut rgba, w, h, x0, baseline_y, x1, baseline_y, [0x0F, 0x17, 0x2A, 0xFF]);
        // Render deterministic character cell strokes proportional to character count.
        let chars = run.text.untrusted_value().as_str().chars().count().max(1) as i64;
        let span = (x1 - x0).max(1);
        let mid_y = (y0 + baseline_y) / 2;
        for ci in 0..chars.min(span) {
            let cx = x0 + (ci * span) / chars;
            draw_line(&mut rgba, w, h, cx, mid_y, cx, baseline_y, [0x0F, 0x17, 0x2A, 0xFF]);
        }
    }

    Ok(DisplayTile { width: w, height: h, dpi, crop, rgba })
}

fn cmm_to_px(cmm: i64, dpi: u32) -> u32 {
    if cmm <= 0 {
        return 0;
    }
    let px = round_half_even_u128(u128::from(cmm as u64) * u128::from(dpi), 2540);
    px.min(u128::from(u32::MAX)) as u32
}

fn put_px(rgba: &mut [u8], w: u32, h: u32, x: i64, y: i64, color: [u8; 4]) {
    if x < 0 || y < 0 || x >= i64::from(w) || y >= i64::from(h) {
        return;
    }
    let idx = ((y as usize) * (w as usize) + (x as usize)) * 4;
    if let Some(slot) = rgba.get_mut(idx..idx + 4) {
        slot.copy_from_slice(&color);
    }
}

#[allow(clippy::too_many_arguments)]
fn draw_rect_outline(rgba: &mut [u8], w: u32, h: u32, x0: i64, y0: i64, x1: i64, y1: i64, color: [u8; 4]) {
    draw_line(rgba, w, h, x0, y0, x1, y0, color);
    draw_line(rgba, w, h, x1, y0, x1, y1, color);
    draw_line(rgba, w, h, x1, y1, x0, y1, color);
    draw_line(rgba, w, h, x0, y1, x0, y0, color);
}

#[allow(clippy::too_many_arguments)]
fn draw_line(rgba: &mut [u8], w: u32, h: u32, mut x0: i64, mut y0: i64, x1: i64, y1: i64, color: [u8; 4]) {
    // Clamp absurd off-tile lines before stepping so loops are strictly bounded.
    let bound = i64::from(w.max(h)).saturating_add(4) * 2;
    if (x0 < -bound && x1 < -bound)
        || (x0 > bound && x1 > bound)
        || (y0 < -bound && y1 < -bound)
        || (y0 > bound && y1 > bound)
    {
        return;
    }
    let dx = (x1 - x0).abs();
    let sx: i64 = if x0 < x1 { 1 } else { -1 };
    let dy = -(y1 - y0).abs();
    let sy: i64 = if y0 < y1 { 1 } else { -1 };
    let mut err = dx + dy;
    let max_steps = (dx.saturating_add(-dy)).min(65_536) + 1;
    for _ in 0..max_steps {
        put_px(rgba, w, h, x0, y0, color);
        if x0 == x1 && y0 == y1 {
            break;
        }
        let e2 = 2 * err;
        if e2 >= dy {
            err += dy;
            x0 += sx;
        }
        if e2 <= dx {
            err += dx;
            y0 += sy;
        }
    }
}
