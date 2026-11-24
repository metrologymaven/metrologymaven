//! `fid-design page-check --root <design dir> [--json]` (or `--file <pages.json> [--json]`)
//! for spec node `UX.REPORT.PAGE` (REQ-UX.REPORT.PAGE-001 .. -003). Tooling, nearest class R0;
//! D0 (exact decimal mm strings parsed into integer micrometres, `BTreeMap`/`BTreeSet` only,
//! sorted findings, no floats in geometric checks, no network).
//!
//! # Format (`report/pages.json`, `page-grids@1`)
//!
//! Defines the four required grids `a4-portrait`, `a4-landscape`, `letter-portrait` and
//! `letter-landscape`, each with exact decimal millimetre strings (`width_mm`, `height_mm`,
//! `top_mm`, `right_mm`, `bottom_mm`, `left_mm`, `safe_margin_mm`, `column_width_mm`,
//! `gutter_mm`, `content_width_mm`, `header_band_mm`, `footer_band_mm`, `content_height_mm`)
//! and a 12-column grid (`columns: 12`).
//!
//! # Findings (exit 1)
//!
//! - `PAGE-SCHEMA`: malformed JSON, wrong keys, bad decimal mm string, `columns != 12`, wrong
//!   canonical A4/Letter dimensions, or non-positive band/column sizes.
//! - `PAGE-MISSING <grid>`: one of the four required grids is absent.
//! - `PAGE-MARGIN <grid>`: any margin is below 10.0 mm (10 000 µm).
//! - `PAGE-GRID-SUM <grid>`: `content_width != page_width - left - right` (tolerance 0.01 mm =
//!   10 µm), `columns * column_width + (columns - 1) * gutter != content_width` (tolerance
//!   10 µm), or `content_height != page_height - top - bottom - header - footer` (tolerance
//!   10 µm).
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::review::guard::value_defect;
use crate::review::{self, Kind};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report and file format IDs.
pub const REPORT_FORMAT: &str = "fid-design-page-check@1";
pub const PAGE_GRIDS_FORMAT: &str = "page-grids@1";
/// Relative path below `--root`.
pub const PAGES_FILE: &str = "report/pages.json";

/// Finding codes (UX.REPORT.PAGE Design).
pub const SCHEMA: &str = "PAGE-SCHEMA";
pub const MISSING: &str = "PAGE-MISSING";
pub const MARGIN: &str = "PAGE-MARGIN";
pub const GRID_SUM: &str = "PAGE-GRID-SUM";

/// Largest `pages.json` file read (1 MiB).
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 16;
/// Most grids accepted in one file.
pub const MAX_GRIDS: usize = 64;

/// Minimum allowed page margin in micrometres (10.0 mm, REQ-002).
pub const MIN_MARGIN_UM: i64 = 10_000;
/// Width/sum tolerance in micrometres (0.01 mm, REQ-002).
pub const TOLERANCE_UM: i64 = 10;
/// Required column count (REQ-001).
pub const REQUIRED_COLUMNS: u64 = 12;

/// The four required grids and their exact standard paper sizes in micrometres.
pub const REQUIRED_GRIDS: [(&str, i64, i64); 4] = [
    ("a4-landscape", 297_000, 210_000),
    ("a4-portrait", 210_000, 297_000),
    ("letter-landscape", 279_400, 215_900),
    ("letter-portrait", 215_900, 279_400),
];

const TOP_KEYS: [&str; 2] = ["format", "grids"];
const GRID_KEYS: [&str; 10] = [
    "column_width_mm",
    "columns",
    "content_height_mm",
    "content_width_mm",
    "footer_band_mm",
    "gutter_mm",
    "header_band_mm",
    "id",
    "margins",
    "page",
];
const GRID_KEYS_WITH_SAFE: [&str; 11] = [
    "column_width_mm",
    "columns",
    "content_height_mm",
    "content_width_mm",
    "footer_band_mm",
    "gutter_mm",
    "header_band_mm",
    "id",
    "margins",
    "page",
    "safe_margin_mm",
];
const PAGE_KEYS: [&str; 2] = ["height_mm", "width_mm"];
const MARGIN_KEYS: [&str; 4] = ["bottom_mm", "left_mm", "right_mm", "top_mm"];

/// Parses an exact non-negative decimal millimetre string (at most 3 fractional digits, no
/// exponent, no whitespace) into integer micrometres (`1 mm = 1_000 µm`).
pub fn parse_mm_to_um(s: &str) -> Option<i64> {
    if s.is_empty() || s.len() > 16 || s != s.trim() || s.starts_with('+') || s.starts_with('-') {
        return None;
    }
    let (whole_s, frac_s) = match s.split_once('.') {
        Some((w, f)) => {
            if f.is_empty() || f.len() > 3 || f.contains('.') {
                return None;
            }
            (w, f)
        }
        None => (s, ""),
    };
    if whole_s.is_empty() || !whole_s.bytes().all(|b| b.is_ascii_digit()) || !frac_s.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if whole_s.len() > 1 && whole_s.starts_with('0') {
        return None;
    }
    let whole: i64 = whole_s.parse().ok()?;
    let scale = match frac_s.len() {
        0 => 1_000,
        1 => 100,
        2 => 10,
        3 => 1,
        _ => return None,
    };
    let frac: i64 = if frac_s.is_empty() { 0 } else { frac_s.parse::<i64>().ok()?.checked_mul(scale)? };
    whole.checked_mul(1_000)?.checked_add(frac)
}

fn json_u64(v: Option<&CValue>) -> Option<u64> {
    match v {
        Some(CValue::Num(n)) if n.is_finite() && n.fract() == 0.0 && *n >= 1.0 && *n <= 1_000_000.0 => Some(*n as u64),
        _ => None,
    }
}

fn req_mm(obj: &BTreeMap<String, CValue>, key: &str, subj: &str, f: &mut Vec<Finding>) -> Option<(String, i64)> {
    match obj.get(key) {
        Some(CValue::Str(s)) => match parse_mm_to_um(s) {
            Some(um) => Some((s.clone(), um)),
            None => {
                f.push(Finding::new(
                    SCHEMA,
                    subj,
                    format!("`{key}` must be a non-negative decimal mm string with at most 3 fractional digits"),
                ));
                None
            }
        },
        _ => {
            f.push(Finding::new(SCHEMA, subj, format!("`{key}` must be a decimal mm string")));
            None
        }
    }
}

/// Summary of one parsed page grid reported in `--json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GridSummary {
    pub id: String,
    pub width_mm: String,
    pub height_mm: String,
    pub content_width_mm: String,
    pub content_height_mm: String,
    pub columns: u64,
}

/// Result of `fid-design page-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PageReport {
    pub grids: Vec<GridSummary>,
    pub findings: Vec<Finding>,
}

/// Runs `fid-design page-check --root <dir>` reading `<dir>/report/pages.json`.
pub fn check(root: &Path) -> Result<PageReport, String> {
    let md = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if md.file_type().is_symlink() || !md.is_dir() {
        return Err(format!("--root {} must be a plain directory", root.display()));
    }
    match review::kind(root, PAGES_FILE)? {
        Kind::File => check_file(&root.join(PAGES_FILE)),
        Kind::Missing => Err(format!("cannot read {}: file does not exist", root.join(PAGES_FILE).display())),
        Kind::Dir | Kind::Other => Err(format!("{} is not a regular file", root.join(PAGES_FILE).display())),
    }
}

/// Runs `fid-design page-check --file <pages.json>` directly on one file.
pub fn check_file(path: &Path) -> Result<PageReport, String> {
    let md = std::fs::symlink_metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if md.file_type().is_symlink() || !md.is_file() {
        return Err(format!("{} must be a regular file", path.display()));
    }
    let text = read_text(path, MAX_FILE_BYTES)?;
    Ok(check_text(&text))
}

/// Pure checker over the JSON text of a `page-grids@1` document.
pub fn check_text(text: &str) -> PageReport {
    let mut f = Vec::new();
    let mut summaries = Vec::new();

    let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_FILE_BYTES as usize, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            f.push(Finding::new(SCHEMA, PAGES_FILE, format!("not valid JSON: {}", e.code())));
            return PageReport { grids: summaries, findings: f };
        }
    };
    if let Some(d) = value_defect(&val) {
        f.push(Finding::new(SCHEMA, PAGES_FILE, format!("text refused at {d}")));
        return PageReport { grids: summaries, findings: f };
    }
    let CValue::Obj(top) = &val else {
        f.push(Finding::new(SCHEMA, PAGES_FILE, format!("{PAGE_GRIDS_FORMAT} must be a JSON object")));
        return PageReport { grids: summaries, findings: f };
    };
    let top_keys: Vec<&str> = top.keys().map(String::as_str).collect();
    if top_keys != TOP_KEYS {
        f.push(Finding::new(
            SCHEMA,
            PAGES_FILE,
            format!("{PAGE_GRIDS_FORMAT} must have exactly the keys {}", TOP_KEYS.join(", ")),
        ));
    }
    if top.get("format") != Some(&CValue::Str(PAGE_GRIDS_FORMAT.to_string())) {
        f.push(Finding::new(SCHEMA, PAGES_FILE, format!("`format` must be \"{PAGE_GRIDS_FORMAT}\"")));
    }
    let Some(CValue::Arr(grids)) = top.get("grids") else {
        f.push(Finding::new(SCHEMA, PAGES_FILE, "`grids` must be an array"));
        return PageReport { grids: summaries, findings: f };
    };
    if grids.len() > MAX_GRIDS {
        f.push(Finding::new(SCHEMA, PAGES_FILE, format!("`grids` exceeds {MAX_GRIDS} entries")));
        return PageReport { grids: summaries, findings: f };
    }

    let mut seen_ids = BTreeSet::new();
    for (i, item) in grids.iter().enumerate() {
        let fallback = format!("grids[{i}]");
        let CValue::Obj(g) = item else {
            f.push(Finding::new(SCHEMA, fallback, "grid entry must be a JSON object"));
            continue;
        };
        let id = match g.get("id") {
            Some(CValue::Str(s)) if !s.trim().is_empty() && s == s.trim() => s.clone(),
            _ => {
                f.push(Finding::new(SCHEMA, fallback, "grid entry missing non-blank string `id`"));
                continue;
            }
        };
        if !seen_ids.insert(id.clone()) {
            f.push(Finding::new(SCHEMA, &id, "grid `id` appears twice"));
        }
        let gkeys: Vec<&str> = g.keys().map(String::as_str).collect();
        if gkeys != GRID_KEYS && gkeys != GRID_KEYS_WITH_SAFE {
            f.push(Finding::new(
                SCHEMA,
                &id,
                format!("grid must have keys {} (optional `safe_margin_mm`)", GRID_KEYS.join(", ")),
            ));
        }

        let (page_w, page_h) = match g.get("page") {
            Some(CValue::Obj(pm)) => {
                let pkeys: Vec<&str> = pm.keys().map(String::as_str).collect();
                if pkeys != PAGE_KEYS {
                    f.push(Finding::new(SCHEMA, &id, "`page` must have keys `width_mm`, `height_mm`"));
                }
                (req_mm(pm, "width_mm", &id, &mut f), req_mm(pm, "height_mm", &id, &mut f))
            }
            _ => {
                f.push(Finding::new(SCHEMA, &id, "`page` must be an object"));
                (None, None)
            }
        };

        let margins = match g.get("margins") {
            Some(CValue::Obj(mm)) => {
                let mkeys: Vec<&str> = mm.keys().map(String::as_str).collect();
                if mkeys != MARGIN_KEYS {
                    f.push(Finding::new(
                        SCHEMA,
                        &id,
                        "`margins` must have keys `top_mm`, `right_mm`, `bottom_mm`, `left_mm`",
                    ));
                }
                let top = req_mm(mm, "top_mm", &id, &mut f);
                let right = req_mm(mm, "right_mm", &id, &mut f);
                let bottom = req_mm(mm, "bottom_mm", &id, &mut f);
                let left = req_mm(mm, "left_mm", &id, &mut f);
                for (side, val) in [("top_mm", &top), ("right_mm", &right), ("bottom_mm", &bottom), ("left_mm", &left)]
                {
                    if let Some((raw, um)) = val {
                        if *um < MIN_MARGIN_UM {
                            f.push(Finding::new(
                                MARGIN,
                                &id,
                                format!("margin `{side}` is {raw} mm, below minimum 10.0 mm"),
                            ));
                        }
                    }
                }
                Some((top, right, bottom, left))
            }
            _ => {
                f.push(Finding::new(SCHEMA, &id, "`margins` must be an object"));
                None
            }
        };

        if g.contains_key("safe_margin_mm") {
            if let Some((raw, um)) = req_mm(g, "safe_margin_mm", &id, &mut f) {
                if um < MIN_MARGIN_UM {
                    f.push(Finding::new(MARGIN, &id, format!("`safe_margin_mm` is {raw} mm, below minimum 10.0 mm")));
                }
            }
        }

        let columns = match json_u64(g.get("columns")) {
            Some(c) if c == REQUIRED_COLUMNS => Some(c),
            Some(c) => {
                f.push(Finding::new(SCHEMA, &id, format!("`columns` is {c}, must be {REQUIRED_COLUMNS}")));
                Some(c)
            }
            None => {
                f.push(Finding::new(SCHEMA, &id, format!("`columns` must be integer {REQUIRED_COLUMNS}")));
                None
            }
        };

        let col_w = req_mm(g, "column_width_mm", &id, &mut f);
        let gutter = req_mm(g, "gutter_mm", &id, &mut f);
        let content_w = req_mm(g, "content_width_mm", &id, &mut f);
        let header = req_mm(g, "header_band_mm", &id, &mut f);
        let footer = req_mm(g, "footer_band_mm", &id, &mut f);
        let content_h = req_mm(g, "content_height_mm", &id, &mut f);

        for (label, val) in [
            ("column_width_mm", &col_w),
            ("gutter_mm", &gutter),
            ("content_width_mm", &content_w),
            ("header_band_mm", &header),
            ("footer_band_mm", &footer),
            ("content_height_mm", &content_h),
        ] {
            if let Some((_, um)) = val {
                if *um <= 0 {
                    f.push(Finding::new(SCHEMA, &id, format!("`{label}` must be greater than 0 mm")));
                }
            }
        }

        // Standard paper sizes for the four required grids.
        if let Some(&(_, want_w, want_h)) = REQUIRED_GRIDS.iter().find(|&&(req_id, _, _)| req_id == id) {
            if let (Some((ws, w_um)), Some((hs, h_um))) = (&page_w, &page_h) {
                if *w_um != want_w || *h_um != want_h {
                    f.push(Finding::new(
                        SCHEMA,
                        &id,
                        format!(
                            "page size {ws} x {hs} mm does not match standard {} x {} mm",
                            fmt_um_as_mm(want_w),
                            fmt_um_as_mm(want_h)
                        ),
                    ));
                }
            }
        }

        // Integer-micrometre checks (REQ-002 & AC-3).
        if let (Some((_, w_um)), Some((_, Some((_, r_um)), _, Some((_, l_um)))), Some((_, cw_um))) =
            (&page_w, &margins, &content_w)
        {
            let expected_cw = w_um.saturating_sub(*l_um).saturating_sub(*r_um);
            if expected_cw <= 0 || cw_um.abs_diff(expected_cw) > TOLERANCE_UM as u64 {
                f.push(Finding::new(
                    GRID_SUM,
                    &id,
                    format!(
                        "`content_width_mm` ({} mm) does not equal page width minus left/right margins ({} mm) within 0.01 mm",
                        fmt_um_as_mm(*cw_um),
                        fmt_um_as_mm(expected_cw)
                    ),
                ));
            }
        }

        if let (Some(cols), Some((_, col_um)), Some((_, gut_um)), Some((_, cw_um))) =
            (columns, &col_w, &gutter, &content_w)
        {
            let cols_i = i64::try_from(cols).unwrap_or(0);
            let sum_um =
                cols_i.saturating_mul(*col_um).saturating_add(cols_i.saturating_sub(1).saturating_mul(*gut_um));
            if sum_um.abs_diff(*cw_um) > TOLERANCE_UM as u64 {
                f.push(Finding::new(
                    GRID_SUM,
                    &id,
                    format!(
                        "{cols} columns x {} mm + {} gutters x {} mm = {} mm, expected content width {} mm",
                        fmt_um_as_mm(*col_um),
                        cols.saturating_sub(1),
                        fmt_um_as_mm(*gut_um),
                        fmt_um_as_mm(sum_um),
                        fmt_um_as_mm(*cw_um)
                    ),
                ));
            }
        }

        if let (
            Some((_, h_um)),
            Some((Some((_, t_um)), _, Some((_, b_um)), _)),
            Some((_, hdr_um)),
            Some((_, ftr_um)),
            Some((_, ch_um)),
        ) = (&page_h, &margins, &header, &footer, &content_h)
        {
            let expected_ch =
                h_um.saturating_sub(*t_um).saturating_sub(*b_um).saturating_sub(*hdr_um).saturating_sub(*ftr_um);
            if expected_ch <= 0 || ch_um.abs_diff(expected_ch) > TOLERANCE_UM as u64 {
                f.push(Finding::new(
                    GRID_SUM,
                    &id,
                    format!(
                        "`content_height_mm` ({} mm) does not equal page height minus margins and header/footer bands ({} mm) within 0.01 mm",
                        fmt_um_as_mm(*ch_um),
                        fmt_um_as_mm(expected_ch)
                    ),
                ));
            }
        }

        if let (
            Some((width_mm, _)),
            Some((height_mm, _)),
            Some((content_width_mm, _)),
            Some((content_height_mm, _)),
            Some(cols),
        ) = (page_w, page_h, content_w, content_h, columns)
        {
            summaries.push(GridSummary { id, width_mm, height_mm, content_width_mm, content_height_mm, columns: cols });
        }
    }

    for &(req_id, _, _) in &REQUIRED_GRIDS {
        if !seen_ids.contains(req_id) {
            f.push(Finding::new(MISSING, req_id, format!("required grid `{req_id}` is missing")));
        }
    }

    summaries.sort_by(|a, b| a.id.cmp(&b.id));
    f.sort();
    f.dedup();
    PageReport { grids: summaries, findings: f }
}

fn fmt_um_as_mm(um: i64) -> String {
    let neg = um < 0;
    let abs = um.unsigned_abs();
    let whole = abs / 1_000;
    let frac = abs % 1_000;
    let sign = if neg { "-" } else { "" };
    if frac == 0 {
        format!("{sign}{whole}.0")
    } else if frac % 100 == 0 {
        format!("{sign}{whole}.{}", frac / 100)
    } else if frac % 10 == 0 {
        format!("{sign}{whole}.{:02}", frac / 10)
    } else {
        format!("{sign}{whole}.{frac:03}")
    }
}

impl PageReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let grids = self
            .grids
            .iter()
            .map(|g| {
                let mut m = BTreeMap::new();
                m.insert("columns".to_string(), CValue::Num(g.columns as f64));
                m.insert("content_height_mm".to_string(), CValue::Str(g.content_height_mm.clone()));
                m.insert("content_width_mm".to_string(), CValue::Str(g.content_width_mm.clone()));
                m.insert("height_mm".to_string(), CValue::Str(g.height_mm.clone()));
                m.insert("id".to_string(), CValue::Str(g.id.clone()));
                m.insert("width_mm".to_string(), CValue::Str(g.width_mm.clone()));
                CValue::Obj(m)
            })
            .collect();
        let mut m = BTreeMap::new();
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("grids".to_string(), CValue::Arr(grids));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!("fid-design page-check: {} grid(s)\n", self.grids.len());
        for g in &self.grids {
            s.push_str(&format!(
                "  {}: {} x {} mm (content {} x {} mm, {} columns)\n",
                g.id, g.width_mm, g.height_mm, g.content_width_mm, g.content_height_mm, g.columns
            ));
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decimal_mm_to_um() {
        assert_eq!(parse_mm_to_um("210"), Some(210_000));
        assert_eq!(parse_mm_to_um("210.0"), Some(210_000));
        assert_eq!(parse_mm_to_um("215.9"), Some(215_900));
        assert_eq!(parse_mm_to_um("19.25"), Some(19_250));
        assert_eq!(parse_mm_to_um("0.001"), Some(1));
        assert_eq!(parse_mm_to_um("0"), Some(0));
        assert_eq!(parse_mm_to_um("-1.0"), None);
        assert_eq!(parse_mm_to_um("1.0001"), None);
        assert_eq!(parse_mm_to_um("01.0"), None);
        assert_eq!(parse_mm_to_um("1e2"), None);
        assert_eq!(parse_mm_to_um(""), None);
    }
}
