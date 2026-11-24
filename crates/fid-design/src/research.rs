//! `fid-design research-check --root <dir> [--key <private csv>] [--json]` (spec node
//! `UX.RESEARCH.PROTO`, REQ-UX.RESEARCH.PROTO-002 .. -004). Tooling, nearest class R0; D0
//! (sorted walk, `BTreeMap`/`BTreeSet` only, sorted findings, no clock, no network).
//!
//! # Checks performed
//!
//! - **`RESEARCH-PII email`**: any text file contains an e-mail address (the address is never
//!   echoed).
//! - **`RESEARCH-PII phone`**: any text file contains a phone number (the number is never
//!   echoed): `+<cc>` international numbers, `(NNN) NNN-NNNN`, 3-3-4 groups with `-`, space or
//!   `.` (`555.019.2834`), 3-4 local numbers with `-` or space (`555-0192`), and national numbers
//!   with a trunk `0` prefix in other groupings (`07700 900123`, `030 12345678`).
//! - **`RESEARCH-PII name`**: any file or path contains a name from `--key <csv>`, or any part
//!   of one with at least [`MIN_NAME_PART_CHARS`] characters as a whole word (a surname alone),
//!   matched case-insensitively after Unicode NFKC and hidden-character stripping, using
//!   [`crate::review::names::scan_key`]. The key is read only from `--key`, never written, and
//!   no finding or path ever prints a matched name (matching path segments become `<withheld>`).
//!   Without `--key` names are not checked: the text report says so ([`NO_KEY_NOTICE`], also on
//!   stderr with `--json`) and the JSON carries `names_checked: false`.
//! - **`RESEARCH-IMAGE-PROVENANCE`**: every raster image (`.png`, `.jpg`, `.jpeg`, `.webp`,
//!   `.gif`, `.bmp`, `.tif`, `.tiff`) must carry a `<image>.capture.json` sidecar
//!   (`research-capture@1`: `format`, `image`, `command` naming `fid <app> capture`, `date`).
//! - **`RESEARCH-INCUMBENT`**: any file flagged as an incumbent artefact (via JSON field
//!   `incumbent: true` / `incumbent_artefact: true` / `origin: "incumbent"`, frontmatter marker
//!   `incumbent-artefact: true`, or listed in `incumbent.txt` / `incumbent-files.txt`).
//! - **`RESEARCH-CONSENT-REF`**: any study/session JSON record with `participant` or
//!   `participant_code` must use a valid `P-<study>-<NN>` code and carry a non-blank
//!   `consent_ref`.
//! - **`RESEARCH-SCHEMA`**: invalid JSON, refused untrusted text, orphan `.capture.json`,
//!   symbolic link inside `--root`, or an empty/malformed `--key` CSV.
//!
//! Exit codes: 0 clean, 1 findings, 2 unreadable input or bad usage.

use crate::finding::{findings_text, findings_value, read_text, Finding};
use crate::review::guard::{defects, path_defect, valid_date, value_defect};
use crate::review::names::{scan_key, WITHHELD};
use crate::review::walk;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// Report format ID.
pub const REPORT_FORMAT: &str = "fid-design-research-check@1";
/// Capture sidecar format ID.
pub const CAPTURE_FORMAT: &str = "research-capture@1";
/// Suffix of a capture provenance sidecar.
pub const CAPTURE_SUFFIX: &str = ".capture.json";

/// Finding codes (UX.RESEARCH.PROTO Design).
pub const PII: &str = "RESEARCH-PII";
pub const IMAGE_PROVENANCE: &str = "RESEARCH-IMAGE-PROVENANCE";
pub const INCUMBENT: &str = "RESEARCH-INCUMBENT";
pub const CONSENT_REF: &str = "RESEARCH-CONSENT-REF";
pub const SCHEMA: &str = "RESEARCH-SCHEMA";

/// Largest text file scanned (16 MiB).
pub const MAX_FILE_BYTES: u64 = 16 * 1024 * 1024;
/// Largest private key CSV read (1 MiB).
pub const MAX_KEY_BYTES: u64 = 1024 * 1024;
/// Largest JSON sidecar or record read (1 MiB).
pub const MAX_JSON_BYTES: u64 = 1024 * 1024;
/// Deepest JSON nesting accepted.
pub const MAX_JSON_DEPTH: usize = 32;
/// Most rows in `--key` CSV.
pub const MAX_KEY_ROWS: usize = 10_000;

const RASTER_EXTS: [&str; 8] = ["bmp", "gif", "jpeg", "jpg", "png", "tif", "tiff", "webp"];
const CAPTURE_KEYS: [&str; 4] = ["command", "date", "format", "image"];

/// Shortest name part matched on its own (`Jordan Kestrel` also matches `Kestrel`, `Jordan`).
pub const MIN_NAME_PART_CHARS: usize = 3;

/// One private key entry (never logged or printed).
#[derive(Debug, Clone, PartialEq, Eq)]
struct KeyEntry {
    row: usize,
    /// Normalised, whitespace-collapsed comparison key.
    norm: String,
    /// Each word of `norm` with at least [`MIN_NAME_PART_CHARS`] characters, matched as a whole
    /// word on its own (a first name or surname alone still identifies a participant).
    parts: Vec<String>,
}

impl KeyEntry {
    fn new(row: usize, norm: String) -> Self {
        let mut parts: Vec<String> = norm
            .split(' ')
            .filter(|w| w.chars().count() >= MIN_NAME_PART_CHARS && *w != norm)
            .map(str::to_string)
            .collect();
        parts.sort();
        parts.dedup();
        Self { row, norm, parts }
    }

    /// Whether normalised text `hay` holds the full name or one of its parts as whole words.
    fn found_in(&self, hay: &str) -> bool {
        contains_whole_phrase(hay, &self.norm) || self.parts.iter().any(|w| contains_whole_phrase(hay, w))
    }
}

/// Result of `fid-design research-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub files_scanned: usize,
    pub images_checked: usize,
    /// Whether a private `--key` was given, so participant names were checked.
    pub names_checked: bool,
    pub findings: Vec<Finding>,
}

/// Whether `s` matches `P-<study>-<NN>` (`<study>`: `[a-z0-9][a-z0-9-]*`, `<NN>`: 2+ digits).
pub fn valid_participant_code(s: &str) -> bool {
    let Some(rest) = s.strip_prefix("P-") else {
        return false;
    };
    let Some((study, nn)) = rest.rsplit_once('-') else {
        return false;
    };
    let sb = study.as_bytes();
    let nb = nn.as_bytes();
    !sb.is_empty()
        && sb.len() <= 32
        && (sb[0].is_ascii_lowercase() || sb[0].is_ascii_digit())
        && sb.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
        && !study.ends_with('-')
        && nb.len() >= 2
        && nb.len() <= 8
        && nb.iter().all(u8::is_ascii_digit)
}

/// Whether `cmd` names the project's own capture command (`fid <subcommand> capture [...]`).
pub fn valid_capture_command(cmd: &str) -> bool {
    let t = cmd.trim();
    if t.is_empty() || t.len() > 256 || t != cmd {
        return false;
    }
    let parts: Vec<&str> = t.split_whitespace().collect();
    if parts.len() < 3 || parts[0] != "fid" || parts[2] != "capture" {
        return false;
    }
    let sub = parts[1].as_bytes();
    !sub.is_empty()
        && sub[0].is_ascii_lowercase()
        && sub.iter().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || *b == b'-')
}

fn normalise_words(s: &str) -> String {
    let folded = scan_key(s);
    folded.split_whitespace().collect::<Vec<_>>().join(" ")
}

fn contains_whole_phrase(hay: &str, needle: &str) -> bool {
    if needle.is_empty() || hay.len() < needle.len() {
        return false;
    }
    let mut start = 0usize;
    while let Some(rel) = hay.get(start..).and_then(|s| s.find(needle)) {
        let abs = start + rel;
        let end = abs + needle.len();
        let left_ok = hay.get(..abs).and_then(|p| p.chars().next_back()).is_none_or(|c| !c.is_alphanumeric());
        let right_ok = hay.get(end..).and_then(|p| p.chars().next()).is_none_or(|c| !c.is_alphanumeric());
        if left_ok && right_ok {
            return true;
        }
        let step = hay.get(abs..).and_then(|s| s.chars().next()).map_or(1, char::len_utf8);
        start = abs + step;
    }
    false
}

/// Redacts any `/`-separated path segment that contains a private key name.
fn redact_path(rel: &str, keys: &[KeyEntry]) -> String {
    if keys.is_empty() {
        return rel.to_string();
    }
    rel.split('/')
        .map(|seg| {
            let norm = normalise_words(seg);
            if keys.iter().any(|k| k.found_in(&norm) || norm.contains(&k.norm)) {
                WITHHELD.to_string()
            } else {
                seg.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("/")
}

/// Detects whether a line contains an e-mail address without flagging `@<version>` IDs.
pub fn line_has_email(line: &str) -> bool {
    let bytes = line.as_bytes();
    for (i, &b) in bytes.iter().enumerate() {
        if b != b'@' {
            continue;
        }
        // Scan left for local part.
        let mut l = i;
        while l > 0 {
            let c = bytes[l - 1];
            if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'_' | b'%' | b'+' | b'-') {
                l -= 1;
            } else {
                break;
            }
        }
        let local = &bytes[l..i];
        if local.is_empty()
            || local.len() > 64
            || local[0] == b'.'
            || local[local.len() - 1] == b'.'
            || !local.iter().any(u8::is_ascii_alphanumeric)
            || local.windows(2).any(|w| w == b"..")
        {
            continue;
        }
        // Scan right for domain part.
        let mut r = i + 1;
        while r < bytes.len() {
            let c = bytes[r];
            if c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-') {
                r += 1;
            } else {
                break;
            }
        }
        let mut domain = &bytes[i + 1..r];
        while domain.last() == Some(&b'.') {
            domain = &domain[..domain.len() - 1];
        }
        if domain.len() < 4 || domain.len() > 253 {
            continue;
        }
        let labels: Vec<&[u8]> = domain.split(|&c| c == b'.').collect();
        if labels.len() < 2 {
            continue;
        }
        let labels_ok = labels.iter().all(|lab| {
            !lab.is_empty()
                && lab.len() <= 63
                && lab[0] != b'-'
                && lab[lab.len() - 1] != b'-'
                && lab.iter().all(|c| c.is_ascii_alphanumeric() || *c == b'-')
        });
        let tld_ok =
            labels.last().is_some_and(|tld| (2..=24).contains(&tld.len()) && tld.iter().all(u8::is_ascii_alphabetic));
        if labels_ok && tld_ok {
            return true;
        }
    }
    false
}

/// Detects whether a line contains a phone number while ignoring ISO dates, version IDs and
/// plain numbers.
pub fn line_has_phone(line: &str) -> bool {
    let bytes = line.as_bytes();
    let mut i = 0usize;
    while i < bytes.len() {
        let b = bytes[i];
        let starts = b == b'+' || b == b'(' || b.is_ascii_digit();
        let left_boundary = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
        if !starts || !left_boundary {
            i += 1;
            continue;
        }
        let mut j = i;
        while j < bytes.len() {
            let c = bytes[j];
            if c.is_ascii_digit() || matches!(c, b'+' | b'(' | b')' | b'-' | b' ' | b'.') {
                j += 1;
            } else {
                break;
            }
        }
        let mut end = j;
        while end > i && matches!(bytes[end - 1], b' ' | b'-' | b'.') {
            end -= 1;
        }
        let right_boundary = end == bytes.len() || !bytes[end].is_ascii_alphanumeric();
        if right_boundary && end > i {
            if let Ok(cand) = std::str::from_utf8(&bytes[i..end]) {
                if is_phone_candidate(cand) {
                    return true;
                }
            }
        }
        i = j.max(i + 1);
    }
    false
}

fn is_phone_candidate(s: &str) -> bool {
    let digits: usize = s.bytes().filter(u8::is_ascii_digit).count();
    if !(7..=15).contains(&digits) {
        return false;
    }
    // Reject ISO dates YYYY-MM-DD (even if followed by numbers).
    let groups: Vec<usize> = s.split(|c: char| !c.is_ascii_digit()).filter(|g| !g.is_empty()).map(str::len).collect();
    if groups == [4, 2, 2] {
        return false;
    }
    // International form: `+<cc> ...` with at least 2 groups and >= 8 digits.
    if let Some(rest) = s.strip_prefix('+') {
        if rest.bytes().next().is_some_and(|b| b.is_ascii_digit()) && groups.len() >= 2 && digits >= 8 {
            return true;
        }
    }
    // Parenthesized area code: `(NNN) NNN-NNNN` etc.
    if s.starts_with('(') && s.contains(')') && groups.len() >= 2 && digits >= 7 {
        return true;
    }
    // 3-group national format like `555-019-2834`, `555 019 2834` or `555.019.2834` (3-3-4,
    // 3-4-4 or 1-3-3-4).
    if matches!(groups.as_slice(), [3, 3, 4] | [3, 4, 4] | [1, 3, 3, 4])
        && (s.contains('-') || s.contains(' ') || s.contains('.'))
    {
        return true;
    }
    // Local number `555-0192` / `555 0192` (3-4). Not with `.`: `125.4500` is a measurement.
    if groups == [3, 4] && !s.contains('.') {
        return true;
    }
    // National numbers with a trunk prefix in other groupings: `07700 900123`, `030 12345678`,
    // `0800 123 4567`, `0049 30 12345678`. The first group starts with `0` and has 2..=5 digits;
    // space or `-` separators only (a `.` would make `012.345678` a decimal).
    let first_trunk = s
        .split(|c: char| !c.is_ascii_digit())
        .find(|g| !g.is_empty())
        .is_some_and(|g| g.starts_with('0') && (2..=5).contains(&g.len()));
    if first_trunk && !s.contains('.') && (2..=4).contains(&groups.len()) && digits >= 9 && s.starts_with('0') {
        return true;
    }
    false
}

fn parse_csv_row(line: &str) -> Result<Vec<String>, ()> {
    let mut fields = Vec::new();
    let mut cur = String::new();
    let mut chars = line.chars().peekable();
    let mut in_quotes = false;
    let mut at_field_start = true;
    while let Some(c) = chars.next() {
        if in_quotes {
            if c == '"' {
                if chars.peek() == Some(&'"') {
                    chars.next();
                    cur.push('"');
                } else {
                    in_quotes = false;
                }
            } else {
                cur.push(c);
            }
        } else if c == '"' && at_field_start {
            in_quotes = true;
            at_field_start = false;
        } else if c == ',' {
            fields.push(cur.trim().to_string());
            cur.clear();
            at_field_start = true;
        } else {
            if !c.is_whitespace() {
                at_field_start = false;
            }
            cur.push(c);
        }
    }
    if in_quotes {
        return Err(());
    }
    fields.push(cur.trim().to_string());
    Ok(fields)
}

/// Parses a private key CSV (`code,name[,extra...]`). Never quotes any entry in findings.
fn parse_key_csv(text: &str, f: &mut Vec<Finding>) -> Vec<KeyEntry> {
    let mut entries = Vec::new();
    let mut data_rows = 0usize;
    let mut saw_first = false;
    for (idx, raw) in text.split('\n').enumerate() {
        let row = idx + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw).trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Ok(cols) = parse_csv_row(line) else {
            f.push(Finding::new(SCHEMA, "key", format!("row {row}: malformed CSV quoting")));
            continue;
        };
        if !saw_first {
            saw_first = true;
            let c0 = cols.first().map(|s| s.to_ascii_lowercase());
            if matches!(c0.as_deref(), Some("code" | "participant_code" | "participant")) {
                continue;
            }
        }
        data_rows += 1;
        if data_rows > MAX_KEY_ROWS {
            f.push(Finding::new(SCHEMA, "key", format!("more than {MAX_KEY_ROWS} rows")));
            break;
        }
        if cols.len() < 2 {
            f.push(Finding::new(SCHEMA, "key", format!("row {row}: expected at least 2 columns (code, name)")));
            continue;
        }
        if !valid_participant_code(&cols[0]) {
            f.push(Finding::new(
                SCHEMA,
                "key",
                format!("row {row}: invalid participant code (expected P-<study>-<NN>)"),
            ));
            continue;
        }
        let mut added_for_row = false;
        for col in cols.iter().skip(1) {
            if col.is_empty() {
                continue;
            }
            if !defects(col).is_empty() {
                f.push(Finding::new(SCHEMA, "key", format!("row {row}: entry refused (untrusted text)")));
                continue;
            }
            let norm = normalise_words(col);
            if norm.len() >= 2 {
                entries.push(KeyEntry::new(row, norm));
                added_for_row = true;
            }
        }
        if !added_for_row {
            f.push(Finding::new(SCHEMA, "key", format!("row {row}: missing participant name")));
        }
    }
    if entries.is_empty() {
        f.push(Finding::new(SCHEMA, "key", "key file holds no participant names"));
    }
    entries
}

fn is_raster_image(rel: &str) -> bool {
    rel.rsplit('/')
        .next()
        .and_then(|f| f.rsplit_once('.'))
        .is_some_and(|(stem, ext)| !stem.is_empty() && RASTER_EXTS.contains(&ext.to_ascii_lowercase().as_str()))
}

fn check_capture_sidecar(root: &Path, img_rel: &str, shown_rel: &str, f: &mut Vec<Finding>) -> Result<(), String> {
    let sidecar = format!("{img_rel}{CAPTURE_SUFFIX}");
    let text = match crate::review::kind(root, &sidecar)? {
        crate::review::Kind::Missing => {
            f.push(Finding::new(
                IMAGE_PROVENANCE,
                shown_rel,
                format!("raster image lacks capture sidecar `{shown_rel}{CAPTURE_SUFFIX}`"),
            ));
            return Ok(());
        }
        crate::review::Kind::File => read_text(&root.join(&sidecar), MAX_JSON_BYTES)?,
        _ => {
            f.push(Finding::new(IMAGE_PROVENANCE, shown_rel, "capture sidecar is not a regular file"));
            return Ok(());
        }
    };
    let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_JSON_BYTES as usize, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            f.push(Finding::new(
                IMAGE_PROVENANCE,
                shown_rel,
                format!("capture sidecar is not valid JSON: {}", e.code()),
            ));
            return Ok(());
        }
    };
    if let Some(d) = value_defect(&val) {
        f.push(Finding::new(IMAGE_PROVENANCE, shown_rel, format!("capture sidecar text refused at {d}")));
        return Ok(());
    }
    let CValue::Obj(m) = &val else {
        f.push(Finding::new(IMAGE_PROVENANCE, shown_rel, "capture sidecar must be a JSON object"));
        return Ok(());
    };
    let keys: Vec<&str> = m.keys().map(String::as_str).collect();
    if keys != CAPTURE_KEYS {
        f.push(Finding::new(
            IMAGE_PROVENANCE,
            shown_rel,
            format!("capture sidecar must have exactly the keys {}", CAPTURE_KEYS.join(", ")),
        ));
        return Ok(());
    }
    if m.get("format") != Some(&CValue::Str(CAPTURE_FORMAT.to_string())) {
        f.push(Finding::new(
            IMAGE_PROVENANCE,
            shown_rel,
            format!("capture sidecar `format` must be \"{CAPTURE_FORMAT}\""),
        ));
    }
    if m.get("image") != Some(&CValue::Str(img_rel.to_string())) {
        f.push(Finding::new(IMAGE_PROVENANCE, shown_rel, "capture sidecar `image` must equal the image path"));
    }
    match m.get("command") {
        Some(CValue::Str(cmd)) if valid_capture_command(cmd) => {}
        _ => f.push(Finding::new(
            IMAGE_PROVENANCE,
            shown_rel,
            "capture sidecar `command` must name the project's capture command (`fid <app> capture`)",
        )),
    }
    match m.get("date") {
        Some(CValue::Str(d)) if valid_date(d) => {}
        _ => f.push(Finding::new(IMAGE_PROVENANCE, shown_rel, "capture sidecar `date` must be YYYY-MM-DD")),
    }
    Ok(())
}

fn check_json_record(shown_rel: &str, text: &str, f: &mut Vec<Finding>) {
    let val = match fid_canon::parse_with_limits(text.as_bytes(), MAX_JSON_BYTES as usize, MAX_JSON_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            f.push(Finding::new(SCHEMA, shown_rel, format!("not valid JSON: {}", e.code())));
            return;
        }
    };
    if let Some(d) = value_defect(&val) {
        f.push(Finding::new(SCHEMA, shown_rel, format!("text refused at {d}")));
        return;
    }
    let mut stack = vec![&val];
    let mut flagged_incumbent = false;
    while let Some(v) = stack.pop() {
        match v {
            CValue::Arr(a) => stack.extend(a.iter()),
            CValue::Obj(m) => {
                let inc_bool = matches!(m.get("incumbent"), Some(CValue::Bool(true)))
                    || matches!(m.get("incumbent_artefact"), Some(CValue::Bool(true)));
                let inc_str = matches!(m.get("origin"), Some(CValue::Str(s)) if s.eq_ignore_ascii_case("incumbent"))
                    || matches!(m.get("source_kind"), Some(CValue::Str(s)) if s.eq_ignore_ascii_case("incumbent"));
                if (inc_bool || inc_str) && !flagged_incumbent {
                    flagged_incumbent = true;
                    f.push(Finding::new(INCUMBENT, shown_rel, "file is flagged as an incumbent artefact"));
                }
                if let Some(p_val) = m.get("participant").or_else(|| m.get("participant_code")) {
                    match p_val {
                        CValue::Str(code) if valid_participant_code(code) => {}
                        _ => f.push(Finding::new(
                            CONSENT_REF,
                            shown_rel,
                            "participant identifier must follow `P-<study>-<NN>`",
                        )),
                    }
                    match m.get("consent_ref") {
                        Some(CValue::Str(c)) if !c.trim().is_empty() && c.len() <= 128 => {}
                        _ => f.push(Finding::new(
                            CONSENT_REF,
                            shown_rel,
                            "participant record must carry a non-blank `consent_ref`",
                        )),
                    }
                }
                stack.extend(m.values());
            }
            _ => {}
        }
    }
}

fn scan_text_file(shown_rel: &str, text: &str, keys: &[KeyEntry], f: &mut Vec<Finding>) {
    let mut flagged_incumbent = false;
    for (idx, raw) in text.split('\n').enumerate() {
        let line_no = idx + 1;
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        if line_has_email(line) {
            f.push(Finding::new(PII, "email", format!("{shown_rel}:{line_no}: e-mail address detected")));
        }
        if line_has_phone(line) {
            f.push(Finding::new(PII, "phone", format!("{shown_rel}:{line_no}: phone number detected")));
        }
        if !keys.is_empty() {
            let norm_line = normalise_words(line);
            for k in keys {
                if k.found_in(&norm_line) {
                    f.push(Finding::new(
                        PII,
                        "name",
                        format!(
                            "{shown_rel}:{line_no}: name or name part from private key matched (key row {})",
                            k.row
                        ),
                    ));
                }
            }
        }
        if !flagged_incumbent {
            let low = line.trim().to_ascii_lowercase();
            if low == "incumbent-artefact: true" || low == "incumbent_artefact: true" {
                flagged_incumbent = true;
                f.push(Finding::new(INCUMBENT, shown_rel, format!("line {line_no}: flagged as an incumbent artefact")));
            }
        }
    }
}

/// Runs `fid-design research-check` on `<root>` with optional private `<key>` CSV.
/// `Err` is an unreadable input or bad path (exit 2).
pub fn check(root: &Path, key_path: Option<&Path>) -> Result<Report, String> {
    let root_md = std::fs::symlink_metadata(root).map_err(|e| format!("cannot read --root {}: {e}", root.display()))?;
    if root_md.file_type().is_symlink() || !root_md.is_dir() {
        return Err(format!("--root {} must be a plain directory", root.display()));
    }

    let mut f = Vec::new();
    let keys = match key_path {
        Some(p) => {
            let text = read_text(p, MAX_KEY_BYTES)?;
            parse_key_csv(&text, &mut f)
        }
        None => Vec::new(),
    };

    let mut wf = Vec::new();
    let entries = walk::walk(root, "", &mut wf)?;
    for w in wf {
        let subj = redact_path(&w.subject, &keys);
        f.push(Finding::new(SCHEMA, subj, w.detail));
    }

    let mut all_files: BTreeSet<String> = BTreeSet::new();
    let mut raster_files: Vec<String> = Vec::new();
    let mut capture_files: Vec<String> = Vec::new();
    let mut incumbent_listed: BTreeSet<String> = BTreeSet::new();
    let mut files_scanned = 0usize;

    for e in &entries {
        if e.is_dir {
            continue;
        }
        let shown = redact_path(&e.rel, &keys);
        if shown != e.rel {
            f.push(Finding::new(PII, "name", format!("{shown}: file path contains a name from the private key")));
        }
        if let Some(d) = path_defect(&e.rel) {
            f.push(Finding::new(SCHEMA, shown, format!("path refused: {d}")));
            continue;
        }
        all_files.insert(e.rel.clone());
        if is_raster_image(&e.rel) {
            raster_files.push(e.rel.clone());
            continue;
        }
        if e.rel.ends_with(CAPTURE_SUFFIX) {
            capture_files.push(e.rel.clone());
        }
        let text = read_text(&e.path, MAX_FILE_BYTES)?;
        files_scanned += 1;
        scan_text_file(&shown, &text, &keys, &mut f);

        if e.rel == "incumbent.txt" || e.rel == "incumbent-files.txt" {
            for raw in text.lines() {
                let item = raw.trim();
                if !item.is_empty() && !item.starts_with('#') {
                    incumbent_listed.insert(item.to_string());
                }
            }
        } else if e.rel.ends_with(".json") && !e.rel.ends_with(CAPTURE_SUFFIX) && !e.rel.ends_with(".origin.json") {
            check_json_record(&shown, &text, &mut f);
        }
    }

    for img in &raster_files {
        let shown = redact_path(img, &keys);
        check_capture_sidecar(root, img, &shown, &mut f)?;
    }
    for cap in &capture_files {
        let img = &cap[..cap.len() - CAPTURE_SUFFIX.len()];
        if !all_files.contains(img) {
            let shown = redact_path(cap, &keys);
            f.push(Finding::new(SCHEMA, shown, "capture sidecar for an image that does not exist"));
        }
    }
    for inc in incumbent_listed {
        let shown = redact_path(&inc, &keys);
        f.push(Finding::new(INCUMBENT, shown, "listed as an incumbent artefact"));
    }

    f.sort();
    f.dedup();
    Ok(Report { files_scanned, images_checked: raster_files.len(), names_checked: key_path.is_some(), findings: f })
}

/// What `research-check` prints when it ran without `--key`: names cannot be checked without the
/// private key, so a clean result says nothing about them.
pub const NO_KEY_NOTICE: &str =
    "notice: no --key given: participant names were NOT checked (only e-mail, phone, images, consent and incumbent flags)";

impl Report {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(REPORT_FORMAT.to_string()));
        m.insert("files_scanned".to_string(), CValue::Num(self.files_scanned as f64));
        m.insert("images_checked".to_string(), CValue::Num(self.images_checked as f64));
        m.insert("names_checked".to_string(), CValue::Bool(self.names_checked));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let mut s = format!(
            "fid-design research-check: {} file(s) scanned, {} image(s) checked\n",
            self.files_scanned, self.images_checked
        );
        if !self.names_checked {
            s.push_str(NO_KEY_NOTICE);
            s.push('\n');
        }
        findings_text(&mut s, &self.findings);
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detectors_and_validators() {
        assert!(valid_participant_code("P-inq01-01"));
        assert!(valid_participant_code("P-r01-12"));
        assert!(!valid_participant_code("P-inq01-1"));
        assert!(!valid_participant_code("inq01-01"));
        assert!(!valid_participant_code("P-INQ-01"));

        assert!(valid_capture_command("fid viz capture"));
        assert!(valid_capture_command("fid report capture --out a.png"));
        assert!(!valid_capture_command("scrot screen.png"));
        assert!(!valid_capture_command("fid viz export"));

        assert!(line_has_email("Contact alice.smith@example.com for details."));
        assert!(line_has_email("user+tag@sub.company.co.uk"));
        assert!(!line_has_email("format: ux-research-protocol@1"));
        assert!(!line_has_email("command align.run_bestfit@1"));

        assert!(line_has_phone("Call +1-555-019-2834 today"));
        assert!(line_has_phone("Office: (555) 019-2834"));
        assert!(line_has_phone("Direct 555-019-2834"));
        assert!(!line_has_phone("Date: 2026-11-22"));
        assert!(!line_has_phone("Participant P-inq01-01 measured 12.3456 mm"));

        // `.` separators, 3-4 local numbers, and trunk-prefix national groupings.
        assert!(line_has_phone("Call 555.019.2834 after six"));
        assert!(line_has_phone("Extension line 555-0192."));
        assert!(line_has_phone("Mobile 07700 900123"));
        assert!(line_has_phone("Berlin office 030 12345678"));
        assert!(line_has_phone("Freephone 0800 123 4567"));
        assert!(line_has_phone("Dial 0049 30 12345678"));
        assert!(line_has_phone("+44 7700 900123"));
        // Not phones: decimals, versions, IP-like runs, dates, standards, short pairs.
        assert!(!line_has_phone("Flatness 125.4500 mm, 0.0125 mm band"));
        assert!(!line_has_phone("Version 1.2.3, build 2026.11.22"));
        assert!(!line_has_phone("Host 192.168.100.200"));
        assert!(!line_has_phone("ISO 2768-1 and ISO 1101"));
        assert!(!line_has_phone("Took 120 s for 42 parts"));
        assert!(!line_has_phone("Grid 0.5 1200"));
    }
}
