//! Moderation log checks and quarterly counts (spec node `OUTR.COMM.MOD`).
//!
//! Moderators record each code-of-conduct report by hand in a **private** TOML log kept outside
//! every public repository (`community/moderation/PROCEDURE.md`). No command here writes the log.
//! Two read-only tools work on it:
//!
//! - `mod-check` ([`run_check`]) validates every entry and reports, by case ID only,
//!   `field_missing <field>`, `field_invalid <field>`, `conflict_not_handed_over`,
//!   `retention_overdue` and `moderator_unknown` (exit 1);
//! - `mod-stats` ([`run_stats`]) turns one quarter into counts per outcome category, the total
//!   and the number acknowledged within the target, with no case ID, date, channel or handle.
//!
//! Both refuse a log path inside the public repository checkout (`log_in_repo`, exit 2) before
//! opening any file. The checkout is `--repo-root`, or else the git top level of the working
//! directory; when neither exists there is no checkout to guard.
//!
//! Log format (every key required unless marked optional; any other key is reported as
//! `field_invalid unknown_key`, without its name, so the log cannot carry names, e-mail addresses
//! or the text of removed posts):
//!
//! ```toml
//! [[case]]
//! id = "MOD-2027-001"                       # MOD-<4 digits>-<3 digits>, unique
//! filed_at = "2027-01-04T09:00:00Z"         # UTC, string or TOML offset date-time
//! acknowledged_at = "2027-01-04T15:30:00Z"  # not before filed_at
//! channel = "forum"                         # one of CHANNELS (the IDs in RULES.md)
//! outcome = "warning"                       # no_action | warning | removal | ban
//! moderator = "handle"                      # GitHub login grammar, must be on the roster
//! parties_conflicted = []                   # roster handles with a conflict
//! handover = { from = "a", to = "handle" }  # optional; roster handles; `to` is the handler
//!
//! [[anon]]                                  # an anonymised case
//! quarter = "2024Q3"
//! outcome = "removal"
//! ack_within_target = true
//! ```
//!
//! Every handle in a case (handling moderator, `parties_conflicted`, `handover`) must be on the
//! roster (`moderator_unknown` otherwise), so no login of a non-moderator can be stored.
//!
//! Findings never echo a value or an unknown key's name, only the case ID (or `case#<n>` /
//! `anon#<n>`, 1-based, when the ID itself is missing or invalid) and a known field name or the
//! fixed token `unknown_key`.
//!
//! Determinism D0: dates come from the log and `--today`; there are no clock reads, and findings
//! are sorted.

#![deny(clippy::todo, clippy::unimplemented, clippy::unreachable)]

use crate::review_gate::is_valid_handle;
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::{ErrorKind, Read};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

/// Largest log file, in bytes.
pub const MAX_LOG_BYTES: usize = 1024 * 1024;
/// Largest roster file, in bytes.
pub const MAX_ROSTER_BYTES: usize = 64 * 1024;
/// Months after filing when an unanonymised case is overdue (PROCEDURE.md; a proposal).
pub const RETENTION_MONTHS: u32 = 24;
/// The channel IDs defined in `community/moderation/RULES.md`, the only values of `channel`.
pub const CHANNELS: [&str; 5] = ["chat", "forum", "issues", "mailing-list", "private"];
/// Field name reported for any key the format does not define (its own name is never echoed).
pub const UNKNOWN_KEY: &str = "unknown_key";
/// Largest accepted `--target-hours` (one year).
pub const MAX_TARGET_HOURS: u32 = 8784;

const CASE_KEYS: [&str; 8] =
    ["acknowledged_at", "channel", "filed_at", "handover", "id", "moderator", "outcome", "parties_conflicted"];
const ANON_KEYS: [&str; 3] = ["ack_within_target", "outcome", "quarter"];

// ------------------------------------------------------------------ errors

/// Conditions that stop a command (exit 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ModError {
    /// The log path lies inside the public repository checkout; nothing was read.
    LogInRepo { path: String },
    /// The log cannot be read, is too large, or is not UTF-8 TOML.
    LogParse { path: String, reason: String },
    /// The roster cannot be read or is not `moderators = [<handles>]`.
    RosterParse { path: String, reason: String },
    /// `--repo-root` (or the log's folder) cannot be resolved.
    Unreadable { path: String, reason: String },
}

impl ModError {
    pub fn code(&self) -> &'static str {
        match self {
            ModError::LogInRepo { .. } => "log_in_repo",
            ModError::LogParse { .. } => "log_parse",
            ModError::RosterParse { .. } => "roster_parse",
            ModError::Unreadable { .. } => "input_unreadable",
        }
    }

    pub fn path(&self) -> &str {
        match self {
            ModError::LogInRepo { path }
            | ModError::LogParse { path, .. }
            | ModError::RosterParse { path, .. }
            | ModError::Unreadable { path, .. } => path,
        }
    }

    pub fn reason(&self) -> &str {
        match self {
            ModError::LogInRepo { .. } => "the moderation log must be kept outside the public repository",
            ModError::LogParse { reason, .. }
            | ModError::RosterParse { reason, .. }
            | ModError::Unreadable { reason, .. } => reason,
        }
    }

    /// Canonical JSON for `--json` on exit 2.
    pub fn to_json(&self) -> Vec<u8> {
        let mut e = BTreeMap::new();
        e.insert("code".to_string(), CValue::Str(self.code().to_string()));
        e.insert("path".to_string(), CValue::Str(self.path().to_string()));
        e.insert("detail".to_string(), CValue::Str(self.reason().to_string()));
        let mut o = BTreeMap::new();
        o.insert("error".to_string(), CValue::Obj(e));
        o.insert("ok".to_string(), CValue::Bool(false));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

impl fmt::Display for ModError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.code(), self.path(), self.reason())
    }
}

impl std::error::Error for ModError {}

// ------------------------------------------------------------------ findings

/// Finding codes (exit 1).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Code {
    FieldMissing,
    FieldInvalid,
    ConflictNotHandedOver,
    RetentionOverdue,
    ModeratorUnknown,
}

impl Code {
    pub fn as_str(self) -> &'static str {
        match self {
            Code::FieldMissing => "field_missing",
            Code::FieldInvalid => "field_invalid",
            Code::ConflictNotHandedOver => "conflict_not_handed_over",
            Code::RetentionOverdue => "retention_overdue",
            Code::ModeratorUnknown => "moderator_unknown",
        }
    }
}

/// One finding: a code, the field it concerns (field findings only) and the entry it is about
/// (a case ID, `case#<n>`, `anon#<n>` or `log` for the top level).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct Finding {
    pub code: Code,
    pub field: Option<String>,
    pub subject: String,
}

impl fmt::Display for Finding {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match &self.field {
            Some(field) => write!(f, "{} {} {}", self.code.as_str(), field, self.subject),
            None => write!(f, "{} {}", self.code.as_str(), self.subject),
        }
    }
}

impl Finding {
    fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("code".to_string(), CValue::Str(self.code.as_str().to_string()));
        if let Some(field) = &self.field {
            o.insert("field".to_string(), CValue::Str(field.clone()));
        }
        o.insert("subject".to_string(), CValue::Str(self.subject.clone()));
        CValue::Obj(o)
    }
}

// ------------------------------------------------------------------ dates

/// A day of the proleptic Gregorian calendar, years 1 to 9999.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub year: u32,
    pub month: u32,
    pub day: u32,
}

fn days_in_month(year: u32, month: u32) -> u32 {
    let leap = (year % 4 == 0 && year % 100 != 0) || year % 400 == 0;
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if leap => 29,
        2 => 28,
        _ => 0,
    }
}

fn digits(b: &[u8], from: usize, len: usize) -> Option<u32> {
    let part = b.get(from..from + len)?;
    if !part.iter().all(u8::is_ascii_digit) {
        return None;
    }
    Some(part.iter().fold(0u32, |n, c| n * 10 + u32::from(c - b'0')))
}

impl Date {
    /// Parses `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
            return None;
        }
        let (year, month, day) = (digits(b, 0, 4)?, digits(b, 5, 2)?, digits(b, 8, 2)?);
        if year == 0 || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date { year, month, day })
    }

    /// Days since 1970-01-01 (H. Hinnant's `days_from_civil`).
    fn days(self) -> i64 {
        let m = i64::from(self.month);
        let y = i64::from(self.year) - i64::from(m <= 2);
        let era = y / 400; // y >= 0 for years 1..=9999
        let yoe = y - era * 400;
        let mp = (m + 9) % 12;
        let doy = (153 * mp + 2) / 5 + i64::from(self.day) - 1;
        let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
        era * 146_097 + doe - 719_468
    }

    /// The same day `n` months later, clamped to the last day of a shorter month.
    pub fn add_months(self, n: u32) -> Date {
        let total = self.year * 12 + (self.month - 1) + n;
        let (year, month) = (total / 12, total % 12 + 1);
        Date { year, month, day: self.day.min(days_in_month(year, month)) }
    }

    pub fn quarter(self) -> Quarter {
        Quarter { year: self.year, q: (self.month - 1) / 3 + 1 }
    }
}

/// A UTC instant `YYYY-MM-DDTHH:MM:SSZ`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Stamp {
    pub date: Date,
    /// Seconds since 1970-01-01T00:00:00Z.
    pub secs: i64,
}

impl Stamp {
    pub fn parse(s: &str) -> Option<Stamp> {
        let b = s.as_bytes();
        if b.len() != 20 || b.get(10) != Some(&b'T') || b.get(13) != Some(&b':') || b.get(16) != Some(&b':') {
            return None;
        }
        if b.get(19) != Some(&b'Z') {
            return None;
        }
        let date = Date::parse(s.get(0..10)?)?;
        let (h, m, sec) = (digits(b, 11, 2)?, digits(b, 14, 2)?, digits(b, 17, 2)?);
        if h > 23 || m > 59 || sec > 59 {
            return None;
        }
        let secs = date.days() * 86_400 + i64::from(h * 3600 + m * 60 + sec);
        Some(Stamp { date, secs })
    }
}

/// A calendar quarter `YYYYQn`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Quarter {
    pub year: u32,
    pub q: u32,
}

impl Quarter {
    pub fn parse(s: &str) -> Option<Quarter> {
        let b = s.as_bytes();
        if b.len() != 6 || b.get(4) != Some(&b'Q') {
            return None;
        }
        let (year, q) = (digits(b, 0, 4)?, digits(b, 5, 1)?);
        if year == 0 || !(1..=4).contains(&q) {
            return None;
        }
        Some(Quarter { year, q })
    }
}

impl fmt::Display for Quarter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}Q{}", self.year, self.q)
    }
}

// ------------------------------------------------------------------ log

/// Outcome categories, in output order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Outcome {
    NoAction,
    Warning,
    Removal,
    Ban,
}

impl Outcome {
    pub const ALL: [Outcome; 4] = [Outcome::NoAction, Outcome::Warning, Outcome::Removal, Outcome::Ban];

    pub fn parse(s: &str) -> Option<Outcome> {
        Outcome::ALL.into_iter().find(|o| o.as_str() == s)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::NoAction => "no_action",
            Outcome::Warning => "warning",
            Outcome::Removal => "removal",
            Outcome::Ban => "ban",
        }
    }
}

/// A hand-over from a conflicted (or unavailable) moderator to the handling one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Handover {
    pub from: String,
    pub to: String,
}

/// One valid `[[case]]` entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub id: String,
    pub filed_at: Stamp,
    pub acknowledged_at: Stamp,
    pub channel: String,
    pub outcome: Outcome,
    pub moderator: String,
    pub parties_conflicted: BTreeSet<String>,
    pub handover: Option<Handover>,
}

/// One valid `[[anon]]` entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Anon {
    pub quarter: Quarter,
    pub outcome: Outcome,
    pub ack_within_target: bool,
}

/// The entries that passed field validation, and the field findings of those that did not.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Log {
    pub cases: Vec<Case>,
    pub anon: Vec<Anon>,
    /// Field findings (`field_missing`, `field_invalid`), sorted.
    pub findings: Vec<Finding>,
    /// Number of `[[case]]` entries, valid or not.
    pub case_entries: usize,
    /// Number of `[[anon]]` entries, valid or not.
    pub anon_entries: usize,
}

/// True for a case ID `MOD-<4 digits>-<3 digits>` (`^MOD-[0-9]{4}-[0-9]{3}$`).
pub fn is_case_id(s: &str) -> bool {
    let b = s.as_bytes();
    b.len() == 12
        && b.starts_with(b"MOD-")
        && b.get(8) == Some(&b'-')
        && b.iter().enumerate().all(|(i, c)| i < 4 || i == 8 || c.is_ascii_digit())
}

fn stamp_text(v: &toml::Value) -> Option<String> {
    match v {
        toml::Value::String(s) => Some(s.clone()),
        toml::Value::Datetime(d) if d.date.is_some() && d.time.is_some() && d.offset.is_some() => Some(d.to_string()),
        _ => None,
    }
}

struct EntryCtx<'a> {
    subject: String,
    findings: &'a mut Vec<Finding>,
}

impl EntryCtx<'_> {
    fn push(&mut self, code: Code, field: &str) {
        self.findings.push(Finding { code, field: Some(field.to_string()), subject: self.subject.clone() });
    }

    /// Looks up `key`; records `field_missing` when absent and `field_invalid` when `conv` fails.
    fn get<T>(&mut self, t: &toml::Table, key: &str, conv: impl Fn(&toml::Value) -> Option<T>) -> Option<T> {
        match t.get(key) {
            None => {
                self.push(Code::FieldMissing, key);
                None
            }
            Some(v) => {
                let out = conv(v);
                if out.is_none() {
                    self.push(Code::FieldInvalid, key);
                }
                out
            }
        }
    }
}

fn handle_of(v: &toml::Value) -> Option<String> {
    v.as_str().filter(|s| is_valid_handle(s)).map(str::to_string)
}

fn outcome_of(v: &toml::Value) -> Option<Outcome> {
    v.as_str().and_then(Outcome::parse)
}

fn parse_case(t: &toml::Table, n: usize, ids: &mut BTreeSet<String>, findings: &mut Vec<Finding>) -> Option<Case> {
    let id_ok = t.get("id").and_then(toml::Value::as_str).filter(|s| is_case_id(s));
    let subject = id_ok.map_or_else(|| format!("case#{n}"), str::to_string);
    let mut cx = EntryCtx { subject, findings };
    let before = cx.findings.len();
    for key in t.keys() {
        if !CASE_KEYS.contains(&key.as_str()) {
            cx.push(Code::FieldInvalid, UNKNOWN_KEY);
        }
    }
    let id = cx.get(t, "id", |v| v.as_str().filter(|s| is_case_id(s)).map(str::to_string));
    if let Some(id) = &id {
        if !ids.insert(id.clone()) {
            cx.push(Code::FieldInvalid, "id");
        }
    }
    let filed_at = cx.get(t, "filed_at", |v| stamp_text(v).and_then(|s| Stamp::parse(&s)));
    let acknowledged_at = cx.get(t, "acknowledged_at", |v| stamp_text(v).and_then(|s| Stamp::parse(&s)));
    if let (Some(f), Some(a)) = (filed_at, acknowledged_at) {
        if a.secs < f.secs {
            cx.push(Code::FieldInvalid, "acknowledged_at");
        }
    }
    let channel = cx.get(t, "channel", |v| v.as_str().filter(|s| CHANNELS.contains(s)).map(str::to_string));
    let outcome = cx.get(t, "outcome", outcome_of);
    let moderator = cx.get(t, "moderator", handle_of);
    let parties_conflicted = cx.get(t, "parties_conflicted", |v| {
        let mut set = BTreeSet::new();
        for h in v.as_array()? {
            if !set.insert(handle_of(h)?) {
                return None;
            }
        }
        Some(set)
    });
    let handover = match t.get("handover") {
        None => None,
        Some(v) => {
            let h = v.as_table().and_then(|h| {
                if h.keys().any(|k| k != "from" && k != "to") {
                    return None;
                }
                let from = h.get("from").and_then(handle_of)?;
                let to = h.get("to").and_then(handle_of)?;
                let to_handler = match &moderator {
                    Some(m) => *m == to,
                    None => true,
                };
                (from != to && to_handler).then_some(Handover { from, to })
            });
            if h.is_none() {
                cx.push(Code::FieldInvalid, "handover");
            }
            h
        }
    };
    if cx.findings.len() != before {
        return None;
    }
    Some(Case {
        id: id?,
        filed_at: filed_at?,
        acknowledged_at: acknowledged_at?,
        channel: channel?,
        outcome: outcome?,
        moderator: moderator?,
        parties_conflicted: parties_conflicted?,
        handover,
    })
}

fn parse_anon(t: &toml::Table, n: usize, findings: &mut Vec<Finding>) -> Option<Anon> {
    let mut cx = EntryCtx { subject: format!("anon#{n}"), findings };
    let before = cx.findings.len();
    for key in t.keys() {
        if !ANON_KEYS.contains(&key.as_str()) {
            cx.push(Code::FieldInvalid, UNKNOWN_KEY);
        }
    }
    let quarter = cx.get(t, "quarter", |v| v.as_str().and_then(Quarter::parse));
    let outcome = cx.get(t, "outcome", outcome_of);
    let ack = cx.get(t, "ack_within_target", toml::Value::as_bool);
    if cx.findings.len() != before {
        return None;
    }
    Some(Anon { quarter: quarter?, outcome: outcome?, ack_within_target: ack? })
}

/// Validates the fields of every entry of a parsed log (REQ-OUTR.COMM.MOD-002).
pub fn parse_log(table: &toml::Table) -> Log {
    let mut log = Log::default();
    for key in table.keys() {
        if key != "case" && key != "anon" {
            let field = Some(UNKNOWN_KEY.to_string());
            log.findings.push(Finding { code: Code::FieldInvalid, field, subject: "log".into() });
        }
    }
    let mut ids = BTreeSet::new();
    for (key, is_case) in [("case", true), ("anon", false)] {
        let Some(v) = table.get(key) else { continue };
        let Some(arr) = v.as_array() else {
            log.findings.push(Finding { code: Code::FieldInvalid, field: Some(key.into()), subject: "log".into() });
            continue;
        };
        if is_case {
            log.case_entries = arr.len();
        } else {
            log.anon_entries = arr.len();
        }
        for (i, entry) in arr.iter().enumerate() {
            let n = i + 1;
            let Some(t) = entry.as_table() else {
                let subject = format!("{key}#{n}");
                log.findings.push(Finding { code: Code::FieldInvalid, field: Some(key.into()), subject });
                continue;
            };
            if is_case {
                if let Some(c) = parse_case(t, n, &mut ids, &mut log.findings) {
                    log.cases.push(c);
                }
            } else if let Some(a) = parse_anon(t, n, &mut log.findings) {
                log.anon.push(a);
            }
        }
    }
    log.findings.sort();
    log.findings.dedup();
    log
}

// ------------------------------------------------------------------ roster

/// Parses the public roster `moderators = ["handle", ...]` (handles only, unique).
pub fn parse_roster(table: &toml::Table, path: &str) -> Result<BTreeSet<String>, ModError> {
    let err = |reason: &str| ModError::RosterParse { path: path.to_string(), reason: reason.to_string() };
    if table.keys().any(|k| k != "moderators") {
        return Err(err("only `moderators` is allowed"));
    }
    let arr = table.get("moderators").and_then(toml::Value::as_array).ok_or_else(|| err("no `moderators` array"))?;
    let mut out = BTreeSet::new();
    for v in arr {
        let h = handle_of(v).ok_or_else(|| err("a roster entry is not a handle"))?;
        if !out.insert(h) {
            return Err(err("duplicate handle"));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ checks

/// Roster, conflict and retention checks on the valid cases (REQ-OUTR.COMM.MOD-004, -006, -007).
/// Every handle in a case (moderator, `parties_conflicted`, `handover`) must be on the roster.
pub fn check(log: &Log, roster: &BTreeSet<String>, today: Date) -> Vec<Finding> {
    let mut out = log.findings.clone();
    for c in &log.cases {
        let finding = |code| Finding { code, field: None, subject: c.id.clone() };
        let mut handles: Vec<&String> = vec![&c.moderator];
        handles.extend(c.parties_conflicted.iter());
        if let Some(h) = &c.handover {
            handles.push(&h.from);
            handles.push(&h.to);
        }
        if handles.iter().any(|h| !roster.contains(*h)) {
            out.push(finding(Code::ModeratorUnknown));
        }
        if c.parties_conflicted.contains(&c.moderator) {
            out.push(finding(Code::ConflictNotHandedOver));
        }
        if today > c.filed_at.date.add_months(RETENTION_MONTHS) {
            out.push(finding(Code::RetentionOverdue));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Quarterly counts (REQ-OUTR.COMM.MOD-001, -005): cases filed in `quarter` (UTC) plus
/// anonymised entries of that quarter. Contains no case ID, date, channel or handle.
pub fn stats(log: &Log, quarter: Quarter, target_hours: u32) -> CValue {
    let mut by: BTreeMap<Outcome, u64> = Outcome::ALL.into_iter().map(|o| (o, 0)).collect();
    let (mut total, mut within) = (0u64, 0u64);
    let limit = i64::from(target_hours) * 3600;
    let rows = log
        .cases
        .iter()
        .filter(|c| c.filed_at.date.quarter() == quarter)
        .map(|c| (c.outcome, c.acknowledged_at.secs - c.filed_at.secs <= limit))
        .chain(log.anon.iter().filter(|a| a.quarter == quarter).map(|a| (a.outcome, a.ack_within_target)));
    for (outcome, ok) in rows {
        total += 1;
        within += u64::from(ok);
        if let Some(n) = by.get_mut(&outcome) {
            *n += 1;
        }
    }
    let num = |n: u64| CValue::Num(n as f64);
    let by_outcome = by.into_iter().map(|(o, n)| (o.as_str().to_string(), num(n))).collect();
    let mut ack = BTreeMap::new();
    ack.insert("count".to_string(), num(within));
    ack.insert("total".to_string(), num(total));
    ack.insert("target_hours".to_string(), num(u64::from(target_hours)));
    let mut o = BTreeMap::new();
    o.insert("quarter".to_string(), CValue::Str(quarter.to_string()));
    o.insert("total".to_string(), num(total));
    o.insert("by_outcome".to_string(), CValue::Obj(by_outcome));
    o.insert("ack_within_target".to_string(), CValue::Obj(ack));
    CValue::Obj(o)
}

// ------------------------------------------------------------------ repository guard

/// `git rev-parse --show-toplevel` in the working directory, if it succeeds.
pub fn git_toplevel() -> Option<PathBuf> {
    let out = Command::new("git").args(["rev-parse", "--show-toplevel"]).output().ok()?;
    if !out.status.success() {
        return None;
    }
    let text = String::from_utf8(out.stdout).ok()?;
    let text = text.trim();
    (!text.is_empty()).then(|| PathBuf::from(text))
}

/// The absolute, symlink-free form of `p`; for a file that does not exist, its folder is
/// resolved instead and the file name appended.
fn resolve(p: &Path) -> Result<PathBuf, ModError> {
    let unreadable = |e: std::io::Error| ModError::Unreadable { path: p.display().to_string(), reason: e.to_string() };
    match std::fs::canonicalize(p) {
        Ok(c) => Ok(c),
        Err(e) if e.kind() == ErrorKind::NotFound => {
            let parent = match p.parent() {
                Some(d) if !d.as_os_str().is_empty() => d,
                _ => Path::new("."),
            };
            let name = p.file_name().ok_or_else(|| unreadable(e))?;
            Ok(std::fs::canonicalize(parent).map_err(unreadable)?.join(name))
        }
        Err(e) => Err(unreadable(e)),
    }
}

fn same_component(a: Component<'_>, b: Component<'_>) -> bool {
    if cfg!(any(windows, target_os = "macos")) {
        a.as_os_str().to_string_lossy().to_lowercase() == b.as_os_str().to_string_lossy().to_lowercase()
    } else {
        a == b
    }
}

/// Fails with `log_in_repo` when `log` lies inside `root` (REQ-OUTR.COMM.MOD-003). Opens no file.
pub fn guard(log: &Path, root: Option<&Path>) -> Result<(), ModError> {
    let Some(root) = root else { return Ok(()) };
    let root = resolve(root)?;
    let log_abs = resolve(log)?;
    let mut inside = log_abs.components();
    let is_inside = root.components().all(|r| inside.next().is_some_and(|l| same_component(r, l)));
    if is_inside {
        return Err(ModError::LogInRepo { path: log.display().to_string() });
    }
    Ok(())
}

// ------------------------------------------------------------------ reading

fn read_toml(path: &Path, limit: usize, err: fn(String, String) -> ModError) -> Result<toml::Table, ModError> {
    let shown = path.display().to_string();
    let mut bytes = Vec::new();
    std::fs::File::open(path)
        .and_then(|f| f.take(limit as u64 + 1).read_to_end(&mut bytes))
        .map_err(|e| err(shown.clone(), e.to_string()))?;
    if bytes.len() > limit {
        return Err(err(shown, format!("larger than {limit} bytes")));
    }
    let text = String::from_utf8(bytes).map_err(|_| err(shown.clone(), "not UTF-8".to_string()))?;
    // `message()` only: the full error quotes the offending line, which may hold case details.
    text.parse::<toml::Table>().map_err(|e| err(shown, e.message().to_string()))
}

fn log_err(path: String, reason: String) -> ModError {
    ModError::LogParse { path, reason }
}

fn roster_err(path: String, reason: String) -> ModError {
    ModError::RosterParse { path, reason }
}

/// Guards the path, then reads and validates the log.
pub fn read_log(log: &Path, repo_root: Option<&Path>) -> Result<Log, ModError> {
    let root = match repo_root {
        Some(r) => Some(r.to_path_buf()),
        None => git_toplevel(),
    };
    guard(log, root.as_deref())?;
    Ok(parse_log(&read_toml(log, MAX_LOG_BYTES, log_err)?))
}

// ------------------------------------------------------------------ commands

/// `mod-check` inputs.
#[derive(Debug, Clone)]
pub struct CheckOptions {
    pub log: PathBuf,
    pub roster: PathBuf,
    pub today: Date,
    pub repo_root: Option<PathBuf>,
}

/// `mod-check` result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    /// Sorted.
    pub findings: Vec<Finding>,
    pub cases: usize,
    pub anon: usize,
}

impl CheckReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn render(&self) -> String {
        let mut s: String = self.findings.iter().map(|f| format!("{f}\n")).collect();
        s.push_str(&format!(
            "mod-check: {} case(s), {} anonymised, {} finding(s)\n",
            self.cases,
            self.anon,
            self.findings.len()
        ));
        s
    }

    pub fn to_json(&self) -> Vec<u8> {
        let mut o = BTreeMap::new();
        o.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        o.insert("cases".to_string(), CValue::Num(self.cases as f64));
        o.insert("anon".to_string(), CValue::Num(self.anon as f64));
        o.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(Finding::to_cvalue).collect()));
        fid_canon::to_canonical_bytes(&CValue::Obj(o)).unwrap_or_default()
    }
}

/// Runs `mod-check`.
pub fn run_check(opts: &CheckOptions) -> Result<CheckReport, ModError> {
    let log = read_log(&opts.log, opts.repo_root.as_deref())?;
    let roster_table = read_toml(&opts.roster, MAX_ROSTER_BYTES, roster_err)?;
    let roster = parse_roster(&roster_table, &opts.roster.display().to_string())?;
    let findings = check(&log, &roster, opts.today);
    Ok(CheckReport { findings, cases: log.case_entries, anon: log.anon_entries })
}

/// `mod-stats` inputs.
#[derive(Debug, Clone)]
pub struct StatsOptions {
    pub log: PathBuf,
    pub quarter: Quarter,
    pub target_hours: u32,
    pub repo_root: Option<PathBuf>,
}

/// `mod-stats` result: either field findings (nothing to write, exit 1) or the canonical counts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StatsResult {
    Invalid(Vec<Finding>),
    Counts(Vec<u8>),
}

/// Runs `mod-stats`. A log with field findings yields no counts.
pub fn run_stats(opts: &StatsOptions) -> Result<StatsResult, ModError> {
    let log = read_log(&opts.log, opts.repo_root.as_deref())?;
    if !log.findings.is_empty() {
        return Ok(StatsResult::Invalid(log.findings));
    }
    let v = stats(&log, opts.quarter, opts.target_hours);
    let bytes = fid_canon::to_canonical_bytes(&v)
        .map_err(|e| ModError::LogParse { path: opts.log.display().to_string(), reason: e.to_string() })?;
    Ok(StatsResult::Counts(bytes))
}
