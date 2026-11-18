//! `bots-check`: the bot register and restricted-path exclusion check (spec node
//! `ECO.CI.BOTS.REG`).
//!
//! `bots.toml` lists every automated reviewer as a `[[bot]]` entry. The check fails when an
//! entry is missing or malformed (`BOTS-SCHEMA`), was reviewed more than 365 days before
//! `--today` (`BOTS-STALE`), sends repository content out but its configuration does not
//! exclude every restricted path verbatim (`BOTS-EXCLUDE`) or has no canary date
//! (`BOTS-CANARY`), or when a forge-flagged bot account in the review export is not in the
//! register (`BOTS-UNREGISTERED`). Unreadable inputs are `BOTS-IO`.
//!
//! Determinism D0: the date is an input, never the clock; findings are sorted by code, then
//! subject (bot ID or account), then detail. Every input is size-bounded and parsed fail-closed.

mod yaml;

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Component, Path};

/// Largest accepted register, in bytes.
pub const MAX_REGISTER_BYTES: u64 = 64 * 1024;
/// Largest accepted bot configuration file, in bytes.
pub const MAX_CONFIG_BYTES: u64 = 256 * 1024;
/// Largest accepted review export, in bytes.
pub const MAX_EXPORT_BYTES: u64 = 10 * 1024 * 1024;
/// Largest accepted restricted-path list, in bytes.
pub const MAX_RESTRICTED_BYTES: u64 = 64 * 1024;
/// A review older than this many days before `--today` is stale.
pub const STALE_AFTER_DAYS: i64 = 365;
/// Deepest nesting accepted in a JSON or YAML configuration or export.
const MAX_DEPTH: usize = 64;

pub const SCHEMA: &str = "BOTS-SCHEMA";
pub const STALE: &str = "BOTS-STALE";
pub const EXCLUDE: &str = "BOTS-EXCLUDE";
pub const CANARY: &str = "BOTS-CANARY";
pub const UNREGISTERED: &str = "BOTS-UNREGISTERED";
pub const IO: &str = "BOTS-IO";

/// One finding: `<code> <subject>[ <detail>]`. The derived order is code, subject, detail.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: &'static str,
    /// Bot ID, `bot[<index>]` for an entry without a valid ID, an account, or an input name.
    pub subject: String,
    pub detail: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.detail.is_empty() {
            write!(f, "{} {}", self.code, self.subject)
        } else {
            write!(f, "{} {} {}", self.code, self.subject, self.detail)
        }
    }
}

fn finding(code: &'static str, subject: impl Into<String>, detail: impl Into<String>) -> Finding {
    Finding { code, subject: one_line(&subject.into()), detail: one_line(&detail.into()) }
}

/// Replaces control characters so that one finding is always one output line.
fn one_line(s: &str) -> String {
    s.chars().map(|c| if c.is_control() { ' ' } else { c }).collect()
}

/// A configuration file reduced to what the key-path lookup needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Node {
    Map(BTreeMap<String, Node>),
    Seq(Vec<Node>),
    Str(String),
    /// Numbers, booleans, dates and nulls: never a pattern.
    Other,
}

/// One valid register entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bot {
    pub id: String,
    pub kind: String,
    pub account: String,
    pub terms: String,
    pub data_handling: String,
    /// `reviewed_on` as days since 1970-01-01.
    pub reviewed_day: i64,
    pub approved_by: String,
    pub sends_content: bool,
    pub config: Option<String>,
    pub exclude_key: Option<String>,
    pub canary_verified_day: Option<i64>,
}

const BOT_KEYS: [&str; 11] = [
    "id",
    "kind",
    "account",
    "terms",
    "data_handling",
    "reviewed_on",
    "approved_by",
    "sends_content",
    "config",
    "exclude_key",
    "canary_verified_on",
];

/// Parses an ISO 8601 calendar date `YYYY-MM-DD` to days since 1970-01-01.
pub fn parse_date(s: &str) -> Option<i64> {
    let b = s.as_bytes();
    if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
        return None;
    }
    let num = |r: std::ops::Range<usize>| -> Option<i64> {
        let mut v = 0i64;
        for &c in b.get(r)? {
            if !c.is_ascii_digit() {
                return None;
            }
            v = v * 10 + i64::from(c - b'0');
        }
        Some(v)
    };
    let (y, m, d) = (num(0..4)?, num(5..7)?, num(8..10)?);
    let leap = (y % 4 == 0 && y % 100 != 0) || y % 400 == 0;
    let days_in_month = match m {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => return None,
    };
    if !(1..=days_in_month).contains(&d) {
        return None;
    }
    // Days from the civil date (proleptic Gregorian calendar, March-based year).
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    Some(era * 146_097 + doe - 719_468)
}

/// A token without whitespace or control characters, at most 100 bytes.
fn valid_token(s: &str) -> bool {
    !s.is_empty() && s.len() <= 100 && s.chars().all(|c| c.is_ascii_graphic())
}

fn valid_id(s: &str) -> bool {
    valid_token(s) && s.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'))
}

/// A forge handle: letters, digits and hyphens.
fn valid_handle(s: &str) -> bool {
    valid_token(s) && s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-')
}

/// An SPDX licence expression (`MIT OR Apache-2.0`) or an http(s) URL.
fn valid_terms(s: &str) -> bool {
    if let Some(rest) = s.strip_prefix("https://").or_else(|| s.strip_prefix("http://")) {
        return !rest.is_empty() && rest.len() <= 500 && rest.chars().all(|c| c.is_ascii_graphic());
    }
    if s.is_empty() || s.len() > 500 {
        return false;
    }
    s.split(' ').all(|tok| {
        let t = tok.trim_matches(|c| c == '(' || c == ')');
        !t.is_empty() && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '+' | ':'))
    })
}

fn valid_line(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 500 && !s.chars().any(char::is_control)
}

/// A relative path made only of normal components, portable across platforms.
fn valid_rel_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 255
        && !s.contains('\\')
        && !s.contains(':')
        && Path::new(s).components().all(|c| matches!(c, Component::Normal(_)))
}

fn valid_key_path(s: &str) -> bool {
    !s.is_empty() && s.len() <= 255 && s.split('.').all(|seg| !seg.is_empty() && !seg.chars().any(char::is_control))
}

/// A date field: a TOML local date or a `"YYYY-MM-DD"` string.
fn date_field(v: &toml::Value) -> Option<i64> {
    match v {
        toml::Value::String(s) => parse_date(s),
        toml::Value::Datetime(d) => parse_date(&d.to_string()),
        _ => None,
    }
}

/// Parses one `[[bot]]` table. `Err` holds the schema problems found.
fn parse_bot(t: &toml::Table) -> Result<Bot, Vec<String>> {
    let mut errs = Vec::new();
    for k in t.keys() {
        if !BOT_KEYS.contains(&k.as_str()) {
            errs.push(format!("unknown field {k}"));
        }
    }
    let mut string = |k: &str, required: bool, ok: fn(&str) -> bool, what: &str| -> Option<String> {
        match t.get(k) {
            Some(toml::Value::String(s)) if ok(s.as_str()) => Some(s.clone()),
            Some(toml::Value::String(_)) => {
                errs.push(format!("{k} must be {what}"));
                None
            }
            Some(_) => {
                errs.push(format!("{k} must be a string"));
                None
            }
            None if required => {
                errs.push(format!("missing {k}"));
                None
            }
            None => None,
        }
    };
    let id = string("id", true, valid_id, "letters, digits, '-', '_' or '.'");
    let kind = string("kind", true, |s| s == "self-hosted" || s == "hosted", "self-hosted or hosted");
    let account = string("account", true, valid_token, "one token without spaces");
    let terms = string("terms", true, valid_terms, "an SPDX expression or a URL");
    let data_handling = string("data_handling", true, valid_line, "one non-empty line");
    let approved_by = string("approved_by", true, valid_handle, "a maintainer handle");
    let config = string("config", false, valid_rel_path, "a relative path without ..");
    let exclude_key = string("exclude_key", false, valid_key_path, "a dotted key path");
    let mut date = |k: &str, required: bool| -> Option<i64> {
        match t.get(k) {
            Some(v) => {
                let d = date_field(v);
                if d.is_none() {
                    errs.push(format!("{k} must be an ISO date"));
                }
                d
            }
            None => {
                if required {
                    errs.push(format!("missing {k}"));
                }
                None
            }
        }
    };
    let reviewed_day = date("reviewed_on", true);
    let canary_verified_day = date("canary_verified_on", false);
    let sends_content = match t.get("sends_content") {
        Some(toml::Value::Boolean(b)) => Some(*b),
        Some(_) => {
            errs.push("sends_content must be a boolean".to_string());
            None
        }
        None => {
            errs.push("missing sends_content".to_string());
            None
        }
    };
    if sends_content == Some(true) && (!t.contains_key("config") || !t.contains_key("exclude_key")) {
        errs.push("sends_content = true requires config and exclude_key".to_string());
    }
    // Every missing or malformed required field pushed an error, so with no errors all of
    // them are present and the defaults below are never used.
    if !errs.is_empty() {
        return Err(errs);
    }
    Ok(Bot {
        id: id.unwrap_or_default(),
        kind: kind.unwrap_or_default(),
        account: account.unwrap_or_default(),
        terms: terms.unwrap_or_default(),
        data_handling: data_handling.unwrap_or_default(),
        reviewed_day: reviewed_day.unwrap_or_default(),
        approved_by: approved_by.unwrap_or_default(),
        sends_content: sends_content.unwrap_or_default(),
        config,
        exclude_key,
        canary_verified_day,
    })
}

/// Parses the register text. Malformed entries become `BOTS-SCHEMA` findings and are left
/// out of the returned list; so are later entries that repeat an ID or an account.
pub fn parse_register(text: &str, findings: &mut BTreeSet<Finding>) -> Vec<Bot> {
    let table = match text.parse::<toml::Table>() {
        Ok(t) => t,
        Err(e) => {
            findings.insert(finding(SCHEMA, "register", format!("not valid TOML: {}", e.message())));
            return Vec::new();
        }
    };
    let mut bots = Vec::new();
    for (key, value) in &table {
        if key != "bot" {
            findings.insert(finding(SCHEMA, "register", format!("unknown top-level key {key}")));
            continue;
        }
        let toml::Value::Array(items) = value else {
            findings.insert(finding(SCHEMA, "register", "bot must be an array of tables ([[bot]])"));
            continue;
        };
        for (i, item) in items.iter().enumerate() {
            let fallback = format!("bot[{i}]");
            let toml::Value::Table(t) = item else {
                findings.insert(finding(SCHEMA, fallback, "not a table"));
                continue;
            };
            let subject = match t.get("id") {
                Some(toml::Value::String(s)) if valid_id(s) => s.clone(),
                _ => fallback,
            };
            match parse_bot(t) {
                Ok(bot) => bots.push(bot),
                Err(errs) => {
                    for e in errs {
                        findings.insert(finding(SCHEMA, subject.clone(), e));
                    }
                }
            }
        }
    }
    let mut ids = BTreeSet::new();
    let mut accounts = BTreeSet::new();
    let mut unique = Vec::new();
    for bot in bots {
        if !ids.insert(bot.id.clone()) {
            findings.insert(finding(SCHEMA, bot.id.clone(), "duplicate id"));
        } else if !accounts.insert(bot.account.clone()) {
            findings.insert(finding(SCHEMA, bot.id.clone(), format!("duplicate account {}", bot.account)));
        } else {
            unique.push(bot);
        }
    }
    unique
}

/// Reads at most `max` bytes of UTF-8 text.
fn read_bounded(path: &Path, max: u64) -> Result<String, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut buf = Vec::new();
    file.take(max + 1).read_to_end(&mut buf).map_err(|e| format!("{}: {e}", path.display()))?;
    if buf.len() as u64 > max {
        return Err(format!("{}: larger than {max} bytes", path.display()));
    }
    String::from_utf8(buf).map_err(|_| format!("{}: not UTF-8", path.display()))
}

/// The restricted patterns: one per line, surrounding whitespace removed; blank lines and
/// lines starting with `#` are skipped.
pub fn parse_restricted(text: &str) -> BTreeSet<String> {
    text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(String::from).collect()
}

fn from_toml(v: &toml::Value) -> Node {
    match v {
        toml::Value::String(s) => Node::Str(s.clone()),
        toml::Value::Array(a) => Node::Seq(a.iter().map(from_toml).collect()),
        toml::Value::Table(t) => Node::Map(t.iter().map(|(k, v)| (k.clone(), from_toml(v))).collect()),
        _ => Node::Other,
    }
}

fn from_json(v: &fid_canon::CValue) -> Node {
    use fid_canon::CValue;
    match v {
        CValue::Str(s) => Node::Str(s.clone()),
        CValue::Arr(a) => Node::Seq(a.iter().map(from_json).collect()),
        CValue::Obj(o) => Node::Map(o.iter().map(|(k, v)| (k.clone(), from_json(v))).collect()),
        _ => Node::Other,
    }
}

/// Parses a configuration file by its extension: `.toml`, `.yml`/`.yaml` or `.json`.
pub fn parse_config(name: &str, text: &str) -> Result<Node, String> {
    let ext = Path::new(name).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "toml" => {
            let t: toml::Table = text.parse().map_err(|e: toml::de::Error| format!("not valid TOML: {}", e.message()))?;
            Ok(from_toml(&toml::Value::Table(t)))
        }
        "yml" | "yaml" => yaml::parse(text, MAX_DEPTH).map_err(|e| format!("YAML: {e}")),
        "json" => {
            let v = fid_canon::parse_with_limits(text.as_bytes(), MAX_CONFIG_BYTES as usize, MAX_DEPTH)
                .map_err(|e| format!("not valid JSON: {}", e.code()))?;
            Ok(from_json(&v))
        }
        _ => Err(format!("unsupported config type .{ext} (expected .toml, .yml, .yaml or .json)")),
    }
}

/// The strings in the list at the dotted `key` path; empty when the path is missing or
/// does not lead to a list.
pub fn exclusion_list(root: &Node, key: &str) -> BTreeSet<String> {
    let mut cur = root;
    for seg in key.split('.') {
        match cur {
            Node::Map(m) => match m.get(seg) {
                Some(next) => cur = next,
                None => return BTreeSet::new(),
            },
            _ => return BTreeSet::new(),
        }
    }
    match cur {
        Node::Seq(items) => items
            .iter()
            .filter_map(|n| match n {
                Node::Str(s) => Some(s.clone()),
                _ => None,
            })
            .collect(),
        _ => BTreeSet::new(),
    }
}

/// Checks the register entries against `today` (days since 1970-01-01) and the restricted
/// patterns. `base` is the directory that `config` paths are relative to.
pub fn check_bots(bots: &[Bot], base: &Path, restricted: &BTreeSet<String>, today: i64) -> BTreeSet<Finding> {
    let mut out = BTreeSet::new();
    for bot in bots {
        if today - bot.reviewed_day > STALE_AFTER_DAYS {
            out.insert(finding(STALE, bot.id.clone(), ""));
        }
        if !bot.sends_content {
            continue;
        }
        if bot.canary_verified_day.is_none() {
            out.insert(finding(CANARY, bot.id.clone(), ""));
        }
        let (Some(config), Some(key)) = (&bot.config, &bot.exclude_key) else {
            // parse_bot refuses this combination; kept fail-closed.
            out.insert(finding(EXCLUDE, bot.id.clone(), "no config"));
            continue;
        };
        let tree = read_bounded(&base.join(config), MAX_CONFIG_BYTES).and_then(|text| parse_config(config, &text));
        match tree {
            Err(e) => {
                out.insert(finding(IO, bot.id.clone(), format!("config {config}: {e}")));
            }
            Ok(tree) => {
                let list = exclusion_list(&tree, key);
                for p in restricted {
                    if !list.contains(p) {
                        out.insert(finding(EXCLUDE, bot.id.clone(), p.clone()));
                    }
                }
            }
        }
    }
    out
}

/// REQ-005: every account the forge marks as a bot in the review export must be registered.
pub fn check_export(text: &str, accounts: &BTreeSet<String>) -> BTreeSet<Finding> {
    use fid_canon::CValue;
    let mut out = BTreeSet::new();
    let root = match fid_canon::parse_with_limits(text.as_bytes(), MAX_EXPORT_BYTES as usize, MAX_DEPTH) {
        Ok(v) => v,
        Err(e) => {
            out.insert(finding(SCHEMA, "review-export", format!("not valid JSON: {}", e.code())));
            return out;
        }
    };
    let CValue::Arr(items) = root else {
        out.insert(finding(SCHEMA, "review-export", "must be an array of {author, author_is_bot}"));
        return out;
    };
    for (i, item) in items.iter().enumerate() {
        let fields = match item {
            CValue::Obj(o) => (o.get("author"), o.get("author_is_bot")),
            _ => (None, None),
        };
        match fields {
            (Some(CValue::Str(author)), Some(CValue::Bool(is_bot))) if valid_token(author) => {
                if *is_bot && !accounts.contains(author) {
                    out.insert(finding(UNREGISTERED, author.clone(), ""));
                }
            }
            _ => {
                // One schema finding is enough: the export is machine-made, so one bad item
                // means the exporter is wrong.
                out.insert(finding(
                    SCHEMA,
                    "review-export",
                    format!("item {i} needs a string author without spaces and a boolean author_is_bot"),
                ));
                return out;
            }
        }
    }
    out
}

/// Runs the whole check. `Err` is a usage error (an invalid `--today`); everything else
/// is a finding. An empty result means the register passes.
pub fn check(register: &Path, restricted: &Path, today: &str, review_export: Option<&Path>) -> Result<Vec<Finding>, String> {
    let today = parse_date(today).ok_or_else(|| format!("--today {today} is not an ISO date (YYYY-MM-DD)"))?;
    let mut findings = BTreeSet::new();
    let patterns = match read_bounded(restricted, MAX_RESTRICTED_BYTES) {
        Ok(text) => Some(parse_restricted(&text)),
        Err(e) => {
            findings.insert(finding(IO, "restricted", e));
            None
        }
    };
    let bots = match read_bounded(register, MAX_REGISTER_BYTES) {
        Ok(text) => parse_register(&text, &mut findings),
        Err(e) => {
            findings.insert(finding(IO, "register", e));
            Vec::new()
        }
    };
    if let Some(patterns) = &patterns {
        let base = register.parent().unwrap_or(Path::new("."));
        findings.extend(check_bots(&bots, base, patterns, today));
    }
    if let Some(export) = review_export {
        match read_bounded(export, MAX_EXPORT_BYTES) {
            Ok(text) => {
                let accounts: BTreeSet<String> = bots.iter().map(|b| b.account.clone()).collect();
                findings.extend(check_export(&text, &accounts));
            }
            Err(e) => {
                findings.insert(finding(IO, "review-export", e));
            }
        }
    }
    Ok(findings.into_iter().collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(parse_date("1970-01-01"), Some(0));
        assert_eq!(parse_date("1970-01-02"), Some(1));
        assert_eq!(parse_date("2000-03-01"), Some(11_017));
        let d = |s| parse_date(s).unwrap();
        // AC-3 boundary: 2028 is a leap year, so 2027-06-01 is 366 days before 2028-06-01.
        assert_eq!(d("2028-06-01") - d("2027-06-01"), 366);
        assert_eq!(d("2028-06-01") - d("2027-06-02"), 365);
        assert!(parse_date("2028-02-29").is_some());
        for bad in ["2027-02-29", "2027-13-01", "2027-00-10", "2027-1-01", "2027/01/01", "20270101xx", "２027-01-01"] {
            assert_eq!(parse_date(bad), None, "{bad}");
        }
    }

    #[test]
    fn terms() {
        assert!(valid_terms("MIT"));
        assert!(valid_terms("(MIT OR Apache-2.0) AND LicenseRef-x"));
        assert!(valid_terms("https://example.invalid/terms"));
        assert!(!valid_terms(""));
        assert!(!valid_terms("https://"));
        assert!(!valid_terms("see website!"));
    }

    #[test]
    fn rel_paths() {
        assert!(valid_rel_path("a/b.yml"));
        for bad in ["", "../a.yml", "/etc/x.yml", "a/../b.yml", "a\\b.yml", "C:x.yml", "./a.yml"] {
            assert!(!valid_rel_path(bad), "{bad}");
        }
    }

    #[test]
    fn bad_register_text() {
        let mut f = BTreeSet::new();
        assert!(parse_register("[[bot]\n", &mut f).is_empty());
        assert!(f.iter().all(|x| x.code == SCHEMA && x.subject == "register"), "{f:?}");
        let mut f = BTreeSet::new();
        parse_register("bot = 1\nother = 2\n", &mut f);
        assert_eq!(f.len(), 2, "{f:?}");
    }

    #[test]
    fn export_schema() {
        let none = BTreeSet::new();
        assert!(check_export("[]", &none).is_empty());
        for bad in ["{}", "[1]", "[{\"author\":\"a b\",\"author_is_bot\":true}]", "[{\"author\":\"x\"}]", "nope"] {
            let f = check_export(bad, &none);
            assert_eq!(f.len(), 1, "{bad}");
            assert!(f.iter().all(|x| x.code == SCHEMA), "{bad}: {f:?}");
        }
    }

    #[test]
    fn lookup() {
        let tree = parse_config("x.json", r#"{"a":{"b":["p",1,"q"]},"c":"s"}"#).unwrap();
        assert_eq!(exclusion_list(&tree, "a.b"), BTreeSet::from(["p".to_string(), "q".to_string()]));
        assert!(exclusion_list(&tree, "a").is_empty());
        assert!(exclusion_list(&tree, "c").is_empty());
        assert!(exclusion_list(&tree, "a.b.c").is_empty());
        assert!(exclusion_list(&tree, "z").is_empty());
        assert!(parse_config("x.ini", "").is_err());
    }
}
