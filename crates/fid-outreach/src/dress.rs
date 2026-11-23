//! Trade-dress distinctness list validation and Markdown renderer (spec node
//! `OUTR.BRAND.GUIDE.DRESS`, REQ-OUTR.BRAND.GUIDE.DRESS-001 .. -004).
//!
//! - Reads `brand/guidelines/trade-dress.toml` (`[[product]]` entries with `name`, `colours`,
//!   `icon_style`, `layout`, `instead`, optional `reviewed_by`, `reviewed_on`).
//! - Validates completeness against `rules/incumbents.txt` (`entry_missing`), per-field
//!   length <= 300 Unicode scalar values (`field_too_long`), and absence of images, HTML media
//!   tags, URLs or media file links (`media_reference`).
//! - Renders a deterministic D0 Markdown page using `{{FAMILY_NAME}}` for the family name,
//!   marking entries without both `reviewed_by` and `reviewed_on` as `[unverified]` and
//!   excluding them from the reviewer-facing summary.
//!
//! Exit codes: 0 valid (and rendered), 1 findings (nothing written), 2 unreadable input or
//! bad usage. No network, no `unsafe`, no panics.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::fs;
use std::io::Read;
use std::path::Path;

/// Largest TOML or incumbents file read (1 MiB).
pub const MAX_FILE_BYTES: u64 = 1024 * 1024;
/// Maximum allowed length of each text field in Unicode scalar values (REQ-003).
pub const MAX_FIELD_CHARS: usize = 300;
/// Most product entries accepted in one list.
pub const MAX_ENTRIES: usize = 500;

const ALLOWED_KEYS: [&str; 7] = ["colours", "icon_style", "instead", "layout", "name", "reviewed_by", "reviewed_on"];
const MEDIA_EXTS: [&str; 16] = [
    ".avif", ".bmp", ".gif", ".ico", ".jpeg", ".jpg", ".mov", ".mp3", ".mp4", ".pdf", ".png", ".svg", ".tif", ".tiff",
    ".webm", ".webp",
];

/// Stable finding codes (OUTR.BRAND.GUIDE.DRESS Design).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ErrorCode {
    EntryMissing,
    FieldTooLong,
    MediaReference,
    Parse,
}

impl ErrorCode {
    pub fn as_str(self) -> &'static str {
        match self {
            ErrorCode::EntryMissing => "entry_missing",
            ErrorCode::FieldTooLong => "field_too_long",
            ErrorCode::MediaReference => "media_reference",
            ErrorCode::Parse => "parse",
        }
    }
}

impl fmt::Display for ErrorCode {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// One validation finding on a trade-dress list.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: ErrorCode,
    pub entry: String,
    pub field: Option<String>,
    pub detail: String,
}

impl Finding {
    fn new(code: ErrorCode, entry: impl Into<String>, field: Option<&str>, detail: impl Into<String>) -> Self {
        Self { code, entry: entry.into(), field: field.map(String::from), detail: detail.into() }
    }
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(fld) => write!(f, "{} {}.{} — {}", self.code, self.entry, fld, self.detail),
            None => write!(f, "{} {} — {}", self.code, self.entry, self.detail),
        }
    }
}

/// One validated `[[product]]` entry in `trade-dress.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProductEntry {
    pub name: String,
    pub colours: String,
    pub icon_style: String,
    pub layout: String,
    pub instead: String,
    pub reviewed_by: Option<String>,
    pub reviewed_on: Option<String>,
}

impl ProductEntry {
    /// True iff both `reviewed_by` and `reviewed_on` are populated (REQ-002).
    pub fn is_reviewed(&self) -> bool {
        self.reviewed_by.is_some() && self.reviewed_on.is_some()
    }
}

/// Result of checking a trade-dress list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckResult {
    pub entries: Vec<ProductEntry>,
    pub findings: Vec<Finding>,
}

impl CheckResult {
    pub fn is_ok(&self) -> bool {
        self.findings.is_empty()
    }

    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }
}

/// Bounded UTF-8 file reader (no symlinks, <= `MAX_FILE_BYTES`).
pub fn read_bounded(path: &Path) -> Result<String, String> {
    let md = fs::symlink_metadata(path).map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if md.file_type().is_symlink() || !md.is_file() {
        return Err(format!("{} must be a regular file", path.display()));
    }
    if md.len() > MAX_FILE_BYTES {
        return Err(format!("{} exceeds {MAX_FILE_BYTES} bytes", path.display()));
    }
    let file = fs::File::open(path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
    let mut buf = Vec::new();
    file.take(MAX_FILE_BYTES.saturating_add(1))
        .read_to_end(&mut buf)
        .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
    if (buf.len() as u64) > MAX_FILE_BYTES {
        return Err(format!("{} exceeds {MAX_FILE_BYTES} bytes", path.display()));
    }
    String::from_utf8(buf).map_err(|e| format!("{} is not valid UTF-8: {e}", path.display()))
}

/// Parses `rules/incumbents.txt` (non-empty lines excluding `#` comments).
pub fn parse_incumbents(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if seen.insert(line.to_string()) {
            out.push(line.to_string());
        }
    }
    out
}

/// Detects Markdown images, HTML media tags, URLs, or media file extensions in a field (REQ-003).
pub fn has_media_reference(s: &str) -> bool {
    let lower = s.to_ascii_lowercase();
    if lower.contains("![")
        || lower.contains("<img")
        || lower.contains("<svg")
        || lower.contains("<video")
        || lower.contains("<audio")
        || lower.contains("<picture")
        || lower.contains("<source")
        || lower.contains("http://")
        || lower.contains("https://")
        || lower.contains("ftp://")
        || lower.contains("file://")
        || lower.contains("data:image/")
    {
        return true;
    }
    for token in lower
        .split(|c: char| c.is_whitespace() || matches!(c, '(' | ')' | '[' | ']' | '<' | '>' | '"' | '\'' | ',' | ';'))
    {
        let trimmed = token.trim_end_matches(['.', ':', '?', '#']);
        if MEDIA_EXTS.iter().any(|ext| trimmed.ends_with(ext) && trimmed.len() > ext.len()) {
            return true;
        }
    }
    false
}

fn valid_date(s: &str) -> bool {
    let b = s.as_bytes();
    if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
        return false;
    }
    let (Ok(y), Ok(m), Ok(d)) = (s[0..4].parse::<u32>(), s[5..7].parse::<u32>(), s[8..10].parse::<u32>()) else {
        return false;
    };
    if !(1900..=9999).contains(&y) || !(1..=12).contains(&m) || d == 0 {
        return false;
    }
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let max_d = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if leap {
                29
            } else {
                28
            }
        }
        _ => 0,
    };
    d <= max_d
}

fn non_blank_opt(v: Option<&toml::Value>, entry: &str, field: &str, f: &mut Vec<Finding>) -> Option<String> {
    match v {
        None => None,
        Some(toml::Value::String(s)) => {
            let t = s.trim();
            if t.is_empty() {
                None
            } else {
                Some(t.to_string())
            }
        }
        Some(_) => {
            f.push(Finding::new(ErrorCode::Parse, entry, Some(field), "must be a string"));
            None
        }
    }
}

fn check_field_rules(entry: &str, field: &str, val: &str, f: &mut Vec<Finding>) {
    let chars = val.chars().count();
    if chars > MAX_FIELD_CHARS {
        f.push(Finding::new(
            ErrorCode::FieldTooLong,
            entry,
            Some(field),
            format!("field is {chars} characters, maximum is {MAX_FIELD_CHARS}"),
        ));
    }
    if has_media_reference(val) {
        f.push(Finding::new(
            ErrorCode::MediaReference,
            entry,
            Some(field),
            "images, media links and URLs are forbidden in trade-dress entries",
        ));
    }
}

/// Validates TOML trade-dress text, optionally checking completeness against `incumbents`.
pub fn check_str(list_text: &str, incumbents: Option<&[String]>) -> CheckResult {
    let mut f = Vec::new();
    let mut entries = Vec::new();

    let table: toml::Table = match toml::from_str(list_text) {
        Ok(t) => t,
        Err(e) => {
            f.push(Finding::new(ErrorCode::Parse, "trade-dress.toml", None, format!("invalid TOML: {e}")));
            return CheckResult { entries, findings: f };
        }
    };

    for k in table.keys() {
        if k != "product" {
            f.push(Finding::new(ErrorCode::Parse, "trade-dress.toml", Some(k), "unknown top-level key"));
        }
    }

    let Some(toml::Value::Array(arr)) = table.get("product") else {
        f.push(Finding::new(ErrorCode::Parse, "trade-dress.toml", Some("product"), "missing `[[product]]` array"));
        return CheckResult { entries, findings: f };
    };
    if arr.len() > MAX_ENTRIES {
        f.push(Finding::new(
            ErrorCode::Parse,
            "trade-dress.toml",
            Some("product"),
            format!("exceeds {MAX_ENTRIES} product entries"),
        ));
        return CheckResult { entries, findings: f };
    }

    let mut seen_names = BTreeSet::new();
    for (idx, item) in arr.iter().enumerate() {
        let fallback = format!("product[{idx}]");
        let Some(tbl) = item.as_table() else {
            f.push(Finding::new(ErrorCode::Parse, fallback, None, "product entry must be a TOML table"));
            continue;
        };
        let name = match tbl.get("name") {
            Some(toml::Value::String(s)) if !s.trim().is_empty() => s.trim().to_string(),
            _ => {
                f.push(Finding::new(ErrorCode::Parse, fallback, Some("name"), "missing non-blank string `name`"));
                continue;
            }
        };
        if !seen_names.insert(name.clone()) {
            f.push(Finding::new(ErrorCode::Parse, &name, Some("name"), "duplicate product entry"));
        }
        check_field_rules(&name, "name", &name, &mut f);

        for k in tbl.keys() {
            if !ALLOWED_KEYS.contains(&k.as_str()) {
                f.push(Finding::new(ErrorCode::Parse, &name, Some(k), "unknown field in product entry"));
            }
        }

        let mut req = |field: &str| -> Option<String> {
            match tbl.get(field) {
                Some(toml::Value::String(s)) if !s.trim().is_empty() => {
                    let val = s.trim().to_string();
                    check_field_rules(&name, field, &val, &mut f);
                    Some(val)
                }
                _ => {
                    f.push(Finding::new(ErrorCode::Parse, &name, Some(field), "missing non-blank required field"));
                    None
                }
            }
        };

        let colours = req("colours");
        let icon_style = req("icon_style");
        let layout = req("layout");
        let instead = req("instead");

        let reviewed_by = non_blank_opt(tbl.get("reviewed_by"), &name, "reviewed_by", &mut f);
        if let Some(ref rb) = reviewed_by {
            check_field_rules(&name, "reviewed_by", rb, &mut f);
        }
        let reviewed_on = non_blank_opt(tbl.get("reviewed_on"), &name, "reviewed_on", &mut f);
        if let Some(ref ro) = reviewed_on {
            check_field_rules(&name, "reviewed_on", ro, &mut f);
            if !valid_date(ro) {
                f.push(Finding::new(ErrorCode::Parse, &name, Some("reviewed_on"), "must be YYYY-MM-DD"));
            }
        }

        if let (Some(colours), Some(icon_style), Some(layout), Some(instead)) = (colours, icon_style, layout, instead) {
            entries.push(ProductEntry { name, colours, icon_style, layout, instead, reviewed_by, reviewed_on });
        }
    }

    if let Some(inc) = incumbents {
        for name in inc {
            if !seen_names.contains(name) {
                f.push(Finding::new(
                    ErrorCode::EntryMissing,
                    name,
                    None,
                    format!("incumbent `{name}` has no [[product]] entry"),
                ));
            }
        }
    }

    f.sort();
    f.dedup();
    CheckResult { entries, findings: f }
}

/// Reads and validates a trade-dress TOML file and optional incumbents file.
pub fn check_files(list_path: &Path, incumbents_path: Option<&Path>) -> Result<CheckResult, String> {
    let list_text = read_bounded(list_path)?;
    let inc = match incumbents_path {
        Some(p) => Some(parse_incumbents(&read_bounded(p)?)),
        None => None,
    };
    Ok(check_str(&list_text, inc.as_deref()))
}

/// Renders validated entries as deterministic D0 Markdown using `{{FAMILY_NAME}}` (REQ-002, AC-4).
///
/// Reviewed entries (both `reviewed_by` and `reviewed_on` set) appear in the reviewer summary.
/// Unreviewed entries are marked `[unverified]` and omitted from the reviewer summary.
pub fn render_markdown(entries: &[ProductEntry]) -> String {
    let mut ordered: BTreeMap<String, &ProductEntry> = BTreeMap::new();
    for e in entries {
        ordered.insert(e.name.clone(), e);
    }

    let mut out = String::new();
    out.push_str("# {{FAMILY_NAME}}: trade-dress distinctness list\n\n");
    out.push_str(
        "Generated by `fid-outreach dress-render` from `brand/guidelines/trade-dress.toml` (spec node `OUTR.BRAND.GUIDE.DRESS`). \
         This page uses `{{FAMILY_NAME}}` for the family name; entries without maintainer review are marked `[unverified]` and omitted from the reviewer summary.\n\n",
    );

    out.push_str("## Reviewer summary\n\n");
    let reviewed: Vec<&ProductEntry> = ordered.values().copied().filter(|e| e.is_reviewed()).collect();
    if reviewed.is_empty() {
        out.push_str("_No entries have completed maintainer review yet._\n\n");
    } else {
        out.push_str("| Product | What {{FAMILY_NAME}} does instead | Reviewed |\n");
        out.push_str("|---|---|---|\n");
        for e in reviewed {
            let by = e.reviewed_by.as_deref().unwrap_or("");
            let on = e.reviewed_on.as_deref().unwrap_or("");
            out.push_str(&format!("| {} | {} | {} ({}) |\n", e.name, e.instead, by, on));
        }
        out.push('\n');
    }

    out.push_str("## Entries\n\n");
    for e in ordered.values() {
        if e.is_reviewed() {
            let by = e.reviewed_by.as_deref().unwrap_or("");
            let on = e.reviewed_on.as_deref().unwrap_or("");
            out.push_str(&format!("### {}\n\n", e.name));
            out.push_str(&format!("- **Status:** reviewed by {by} on {on}\n"));
        } else {
            out.push_str(&format!("### {} [unverified]\n\n", e.name));
            out.push_str("- **Status:** [unverified] (pending maintainer review)\n");
        }
        out.push_str(&format!("- **Colours:** {}\n", e.colours));
        out.push_str(&format!("- **Icon style:** {}\n", e.icon_style));
        out.push_str(&format!("- **Layout:** {}\n", e.layout));
        out.push_str(&format!("- **{{{{FAMILY_NAME}}}} does instead:** {}\n\n", e.instead));
    }
    out
}

/// Validates `list_path` (and optional `incumbents_path`) and writes the rendered Markdown to
/// `out_path` only if there are no findings.
pub fn render_to_file(
    list_path: &Path,
    out_path: &Path,
    incumbents_path: Option<&Path>,
) -> Result<CheckResult, String> {
    let result = check_files(list_path, incumbents_path)?;
    if result.is_ok() {
        let md = render_markdown(&result.entries);
        if let Some(parent) = out_path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent).map_err(|e| format!("cannot create directory {}: {e}", parent.display()))?;
            }
        }
        fs::write(out_path, md.as_bytes()).map_err(|e| format!("cannot write {}: {e}", out_path.display()))?;
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_detection() {
        assert!(has_media_reference("see ![logo](logo.png)"));
        assert!(has_media_reference("uses <img src='x'>"));
        assert!(has_media_reference("from https://example.com/screen.png"));
        assert!(has_media_reference("check screenshot.jpg for layout"));
        assert!(!has_media_reference("Use surface.base tokens and 0.5 px monochrome grid."));
    }
}
