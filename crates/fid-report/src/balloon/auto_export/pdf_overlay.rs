//! Pure-integer deterministic vector PDF balloon overlay writer (`RPT.BALLOON.AUTO.EXPORT`,
//! `REQ-001`, `REQ-002`, `REQ-005`).
//!
//! Renders vector drawing content plus crisp vector balloons styled per `UX.REPORT.BALLOON`
//! (leader lines, arrowheads, 4-arc cubic Bézier circles, centered numbers, and non-colour
//! outcome cues when `outcome_marking` is enabled) into a deterministic PDF 1.7 document
//! (fixed `/Info`, zero timestamps, deterministic `/ID`, zero floats).

use crate::balloon::auto_text::SheetSegment;
use crate::balloon::register::Balloon;
use crate::balloon::sheet::{digest_bytes, DrawingSheet};
use std::collections::BTreeMap;
use std::fmt::Write as _;

/// Evaluation outcome state used for optional non-colour cues (`UX.REPORT.BALLOON`, `REQ-002`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BalloonOutcome {
    Conforms,
    NonConforms,
    Indeterminate,
    NotEvaluated,
}

impl BalloonOutcome {
    pub const fn as_str(self) -> &'static str {
        match self {
            BalloonOutcome::Conforms => "conforms",
            BalloonOutcome::NonConforms => "non_conforms",
            BalloonOutcome::Indeterminate => "indeterminate",
            BalloonOutcome::NotEvaluated => "not_evaluated",
        }
    }
}

/// UX.REPORT.BALLOON nominal balloon radius (`3.50 mm` = `350` units of `0.01 mm`).
const BALLOON_RADIUS_CMM: i64 = 350;

/// Renders one deterministic vector-overlaid ballooned PDF across all `sheets` in order (`D0`).
pub fn render_ballooned_pdf(
    sheets: &[DrawingSheet],
    balloons: &[Balloon],
    sheet_segments: &BTreeMap<crate::balloon::sheet::SheetId, Vec<SheetSegment>>,
    outcomes: &BTreeMap<fid_gdt::chars::dprof::CharId, BalloonOutcome>,
    outcome_marking: bool,
) -> Vec<u8> {
    // Object layout:
    //   1: Catalog
    //   2: Pages
    //   3: Font /F1 (Helvetica-Bold)
    //   4: Info (fixed producer, zero timestamps)
    //   For each sheet i (0..N):
    //     obj (5 + 2*i): Page
    //     obj (6 + 2*i): Content stream
    let mut page_objs = Vec::with_capacity(sheets.len());
    let mut stream_objs = Vec::with_capacity(sheets.len());
    let mut kid_refs = Vec::with_capacity(sheets.len());

    for (idx, sheet) in sheets.iter().enumerate() {
        let page_num = 5 + idx * 2;
        let stream_num = page_num + 1;
        kid_refs.push(format!("{page_num} 0 R"));

        let w_cmm = sheet.width_cmm.max(1000);
        let h_cmm = sheet.height_cmm.max(1000);
        let w_pt = fmt_cmm_pt(w_cmm);
        let h_pt = fmt_cmm_pt(h_cmm);

        let page_dict = format!(
            "<< /Type /Page /Parent 2 0 R /MediaBox [0 0 {w_pt} {h_pt}] /Resources << /Font << /F1 3 0 R >> >> /Contents {stream_num} 0 R >>"
        );
        page_objs.push(page_dict.into_bytes());

        let mut cs = String::with_capacity(4096);
        // 1. Drawing sheet border & source vector segments (subtle slate stroke).
        cs.push_str("q\n0.5 w 0.35 0.40 0.48 RG\n");
        if let Some(segs) = sheet_segments.get(&sheet.id) {
            for seg in segs {
                let x0 = fmt_cmm_pt(seg.x0);
                let y0 = fmt_cmm_pt(h_cmm - seg.y0);
                let x1 = fmt_cmm_pt(seg.x1);
                let y1 = fmt_cmm_pt(h_cmm - seg.y1);
                let _ = writeln!(cs, "{x0} {y0} m {x1} {y1} l S");
            }
        }
        cs.push_str("Q\n");

        // 2. Vector balloon overlays in ascending balloon-number order (`REQ-002`).
        let mut sheet_balloons: Vec<&Balloon> = balloons.iter().filter(|b| b.sheet == sheet.id).collect();
        sheet_balloons.sort_by_key(|b| b.number);

        for b in sheet_balloons {
            let outcome = outcomes.get(&b.characteristic).copied().unwrap_or(BalloonOutcome::NotEvaluated);
            emit_vector_balloon(&mut cs, b, h_cmm, outcome, outcome_marking);
        }

        let stream_bytes = cs.into_bytes();
        let mut stm = format!("<< /Length {} >>\nstream\n", stream_bytes.len()).into_bytes();
        stm.extend_from_slice(&stream_bytes);
        stm.extend_from_slice(b"\nendstream");
        stream_objs.push(stm);
    }

    let mut objs: Vec<Vec<u8>> = Vec::with_capacity(4 + sheets.len() * 2);
    objs.push(b"<< /Type /Catalog /Pages 2 0 R >>".to_vec());
    objs.push(format!("<< /Type /Pages /Kids [{}] /Count {} >>", kid_refs.join(" "), sheets.len()).into_bytes());
    objs.push(b"<< /Type /Font /Subtype /Type1 /BaseFont /Helvetica-Bold >>".to_vec());
    objs.push(b"<< /Producer (Metrology Maven Balloon Pack 1.0) /Creator (fid-report) >>".to_vec());
    for (p, s) in page_objs.into_iter().zip(stream_objs) {
        objs.push(p);
        objs.push(s);
    }

    // Compute deterministic 16-byte PDF /ID from SHA-256 of all object payloads (D0, AC-2).
    let mut id_input = Vec::new();
    for o in &objs {
        id_input.extend_from_slice(o);
    }
    let id_hex: String = digest_bytes(&id_input).as_str().trim_start_matches("sha256:").chars().take(32).collect();

    let mut out = b"%PDF-1.7\n%\xE2\xE3\xCF\xD3\n".to_vec();
    let mut offsets = Vec::with_capacity(objs.len() + 1);
    offsets.push(0usize);
    for (idx, body) in objs.iter().enumerate() {
        offsets.push(out.len());
        out.extend_from_slice(format!("{} 0 obj\n", idx + 1).as_bytes());
        out.extend_from_slice(body);
        out.extend_from_slice(b"\nendobj\n");
    }
    let xref_pos = out.len();
    out.extend_from_slice(format!("xref\n0 {}\n0000000000 65535 f \n", objs.len() + 1).as_bytes());
    for &off in &offsets[1..] {
        out.extend_from_slice(format!("{off:010} 00000 n \n").as_bytes());
    }
    out.extend_from_slice(
        format!(
            "trailer\n<< /Size {} /Root 1 0 R /Info 4 0 R /ID [<{id_hex}> <{id_hex}>] >>\nstartxref\n{xref_pos}\n%%EOF\n",
            objs.len() + 1
        )
        .as_bytes(),
    );
    out
}

fn emit_vector_balloon(cs: &mut String, b: &Balloon, sheet_h_cmm: i64, outcome: BalloonOutcome, outcome_marking: bool) {
    // Convert top-left sheet coordinates (0.01 mm) to bottom-left PDF coordinates (0.01 mm).
    let ax = b.anchor.x;
    let ay = sheet_h_cmm - b.anchor.y;
    let lx = b.label.x;
    let ly = sheet_h_cmm - b.label.y;
    let r = BALLOON_RADIUS_CMM;

    // Color palette (high-contrast clean-room stroke) + mandatory non-colour shape cues when enabled.
    let stroke_rgb = if !outcome_marking {
        "0.75 0.12 0.16"
    } else {
        match outcome {
            BalloonOutcome::Conforms => "0.08 0.45 0.24",
            BalloonOutcome::NonConforms => "0.75 0.10 0.14",
            BalloonOutcome::Indeterminate => "0.70 0.42 0.05",
            BalloonOutcome::NotEvaluated => "0.18 0.26 0.40",
        }
    };

    let _ = writeln!(cs, "q\n1.2 w {stroke_rgb} RG 1 1 1 rg");

    // 1. Leader line from anchor to balloon center, plus vector arrowhead dot at anchor.
    let _ = writeln!(cs, "{} {} m {} {} l S", fmt_cmm_pt(ax), fmt_cmm_pt(ay), fmt_cmm_pt(lx), fmt_cmm_pt(ly));
    // Small filled anchor marker diamond (non-lossy vector).
    let d = 70i64;
    let _ = writeln!(
        cs,
        "{stroke_rgb} rg {} {} m {} {} l {} {} l {} {} l h f 1 1 1 rg",
        fmt_cmm_pt(ax),
        fmt_cmm_pt(ay + d),
        fmt_cmm_pt(ax + d),
        fmt_cmm_pt(ay),
        fmt_cmm_pt(ax),
        fmt_cmm_pt(ay - d),
        fmt_cmm_pt(ax - d),
        fmt_cmm_pt(ay)
    );

    // 2. Non-colour outer cue ring when outcome_marking is enabled (`REQ-002`):
    //    - NonConforms: double outer ring (`r + 90 cmm`)
    //    - Indeterminate: dashed outer ring (`[3 2] 0 d`)
    //    - Conforms: subtle solid double-check tick on top-right perimeter
    if outcome_marking {
        match outcome {
            BalloonOutcome::NonConforms => {
                emit_bezier_circle(cs, lx, ly, r + 90);
                cs.push_str("S\n");
            }
            BalloonOutcome::Indeterminate => {
                cs.push_str("q [3 2] 0 d ");
                emit_bezier_circle(cs, lx, ly, r + 80);
                cs.push_str("S Q\n");
            }
            BalloonOutcome::Conforms => {
                let cx = lx + r;
                let cy = ly + r;
                let _ = writeln!(
                    cs,
                    "{} {} m {} {} l {} {} l S",
                    fmt_cmm_pt(cx - 90),
                    fmt_cmm_pt(cy),
                    fmt_cmm_pt(cx - 30),
                    fmt_cmm_pt(cy - 60),
                    fmt_cmm_pt(cx + 80),
                    fmt_cmm_pt(cy + 70)
                );
            }
            BalloonOutcome::NotEvaluated => {}
        }
    }

    // 3. White-filled stroked 4-arc cubic Bézier balloon circle (`B` = fill + stroke).
    emit_bezier_circle(cs, lx, ly, r);
    cs.push_str("B\n");

    // 4. Centered vector balloon number text.
    let num_str = b.number.to_string();
    let font_pt = if num_str.len() <= 2 { "9" } else { "7.5" };
    let est_w_cmm = (num_str.len() as i64) * 190;
    let tx = lx - est_w_cmm / 2;
    let ty = ly - 115;
    let _ = writeln!(
        cs,
        "BT {stroke_rgb} rg /F1 {font_pt} Tf 1 0 0 1 {} {} Tm ({num_str}) Tj ET\nQ",
        fmt_cmm_pt(tx),
        fmt_cmm_pt(ty)
    );
}

/// Emits a 4-arc cubic Bézier circle centered at `(cx, cy)` with radius `r` (all in `0.01 mm`)
/// using pure integer `kappa = 552285 / 1_000_000`.
fn emit_bezier_circle(cs: &mut String, cx: i64, cy: i64, r: i64) {
    let k = ((i128::from(r) * 552_285) / 1_000_000) as i64;
    let _ = writeln!(cs, "{} {} m", fmt_cmm_pt(cx + r), fmt_cmm_pt(cy));
    let _ = writeln!(
        cs,
        "{} {} {} {} {} {} c",
        fmt_cmm_pt(cx + r),
        fmt_cmm_pt(cy + k),
        fmt_cmm_pt(cx + k),
        fmt_cmm_pt(cy + r),
        fmt_cmm_pt(cx),
        fmt_cmm_pt(cy + r)
    );
    let _ = writeln!(
        cs,
        "{} {} {} {} {} {} c",
        fmt_cmm_pt(cx - k),
        fmt_cmm_pt(cy + r),
        fmt_cmm_pt(cx - r),
        fmt_cmm_pt(cy + k),
        fmt_cmm_pt(cx - r),
        fmt_cmm_pt(cy)
    );
    let _ = writeln!(
        cs,
        "{} {} {} {} {} {} c",
        fmt_cmm_pt(cx - r),
        fmt_cmm_pt(cy - k),
        fmt_cmm_pt(cx - k),
        fmt_cmm_pt(cy - r),
        fmt_cmm_pt(cx),
        fmt_cmm_pt(cy - r)
    );
    let _ = writeln!(
        cs,
        "{} {} {} {} {} {} c h",
        fmt_cmm_pt(cx + k),
        fmt_cmm_pt(cy - r),
        fmt_cmm_pt(cx + r),
        fmt_cmm_pt(cy - k),
        fmt_cmm_pt(cx + r),
        fmt_cmm_pt(cy)
    );
}

/// Converts integer `0.01 mm` (`cmm`) to PDF points formatted with 3 decimal places using pure
/// 128-bit integer arithmetic (`pt * 1000 = round_half_even(cmm * 72_000, 2540)`).
pub(crate) fn fmt_cmm_pt(cmm: i64) -> String {
    let neg = cmm < 0;
    let mag = cmm.unsigned_abs() as u128;
    let num = mag.saturating_mul(72_000);
    let den = 2540u128;
    let q = num / den;
    let r = num % den;
    let milli_pt = match (2 * r).cmp(&den) {
        std::cmp::Ordering::Greater => q + 1,
        std::cmp::Ordering::Less => q,
        std::cmp::Ordering::Equal => q + (q & 1),
    };
    let whole = milli_pt / 1000;
    let frac = milli_pt % 1000;
    let sign = if neg && milli_pt > 0 { "-" } else { "" };
    if frac == 0 {
        format!("{sign}{whole}")
    } else {
        format!("{sign}{whole}.{frac:03}")
    }
}
