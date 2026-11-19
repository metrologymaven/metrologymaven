//! `bot-blocking`: the maintainer-marked blocking findings check (spec node
//! `ECO.CI.BOTS.BLOCK`).
//!
//! Bot findings are advisory until a maintainer replies `/blocking` on the bot's review
//! thread (undone by a later `/advisory`). The check reads an exported list of review
//! threads, the bot register (`bots.toml`, ECO.CI.BOTS.REG) and the maintainer list, and
//! fails while any blocking thread is unresolved. Marks by accounts that are not
//! maintainers are ignored and reported as `ignored-mark` notes. Any problem with the inputs
//! (missing, malformed, larger than 10 MiB) fails closed with `BLOCK-INPUT`.
//!
//! Determinism D0: no clock reads; output is sorted by thread ID.

use super::{parse_date, parse_register, read_bounded, valid_token, MAX_EXPORT_BYTES, MAX_REGISTER_BYTES};
use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

/// The fail-closed input error code.
pub const INPUT: &str = "BLOCK-INPUT";
/// Largest accepted maintainer list, in bytes.
pub const MAX_MAINTAINERS_BYTES: u64 = 64 * 1024;
/// Deepest nesting accepted in the thread export.
const MAX_DEPTH: usize = 16;
/// Largest accepted thread ID or path, in bytes.
const MAX_ID_BYTES: usize = 200;
const MAX_PATH_BYTES: usize = 4096;
/// Largest accepted line number.
const MAX_LINE: f64 = 1.0e9;

/// The reply that marks a bot thread blocking.
pub const MARK_BLOCKING: &str = "/blocking";
/// The reply that returns a bot thread to advisory.
pub const MARK_ADVISORY: &str = "/advisory";

/// An unresolved thread that a maintainer marked blocking.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocking {
    pub id: String,
    pub path: String,
    /// `None` when the export has `"line": null` (a thread on a whole file or an outdated diff).
    pub line: Option<u64>,
    /// The maintainer whose latest mark is `/blocking`.
    pub by: String,
}

/// A `/blocking` or `/advisory` reply by an account that is not a maintainer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IgnoredMark {
    pub id: String,
    pub mark: &'static str,
    pub author: String,
}

/// The result of a successful evaluation.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Report {
    /// Sorted by thread ID.
    pub blocking: Vec<Blocking>,
    /// Sorted by thread ID, then by reply time.
    pub ignored: Vec<IgnoredMark>,
}

impl Report {
    /// The output lines: blocking threads, then `ignored-mark` notes, then a summary.
    pub fn lines(&self) -> Vec<String> {
        let mut out = Vec::new();
        for b in &self.blocking {
            let line = b.line.map_or_else(|| "-".to_string(), |l| l.to_string());
            out.push(format!("blocking {} {}:{} by {}", b.id, quote_path(&b.path), line, b.by));
        }
        for n in &self.ignored {
            out.push(format!("ignored-mark {} {} by {}", n.id, n.mark, n.author));
        }
        if self.blocking.is_empty() {
            out.push("bot-blocking: ok".to_string());
        } else {
            out.push(format!("bot-blocking: {} unresolved blocking thread(s)", self.blocking.len()));
        }
        out
    }

    /// 1 when any blocking thread is unresolved, else 0.
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.blocking.is_empty())
    }
}

/// The path as printed: unchanged when it has no whitespace, double quote or backslash;
/// otherwise in double quotes with each double quote and backslash escaped by a backslash, so
/// every output line splits unambiguously.
pub fn quote_path(path: &str) -> String {
    if !path.chars().any(|c| c.is_whitespace() || c == '"' || c == '\\') {
        return path.to_string();
    }
    let mut out = String::from("\"");
    for c in path.chars() {
        if c == '"' || c == '\\' {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('"');
    out
}

/// The reply's mark: its body's first whitespace-separated token, when that is a mark.
pub fn mark_of(body: &str) -> Option<&'static str> {
    match body.split_whitespace().next() {
        Some(MARK_BLOCKING) => Some(MARK_BLOCKING),
        Some(MARK_ADVISORY) => Some(MARK_ADVISORY),
        _ => None,
    }
}

/// Parses an RFC 3339 UTC timestamp `YYYY-MM-DDThh:mm:ss[.f{1,9}]Z` into a sortable key
/// (days since 1970-01-01, second of day, nanoseconds).
pub fn parse_timestamp(s: &str) -> Option<(i64, u32, u32)> {
    let date = s.get(..10)?;
    let rest = s.get(10..)?;
    let days = parse_date(date)?;
    let rest = rest.strip_prefix('T')?.strip_suffix('Z')?;
    let (hms, frac) = match rest.split_once('.') {
        Some((hms, frac)) => (hms, Some(frac)),
        None => (rest, None),
    };
    let b = hms.as_bytes();
    if b.len() != 8 || b[2] != b':' || b[5] != b':' {
        return None;
    }
    let two = |i: usize| -> Option<u32> {
        let (h, l) = (*b.get(i)?, *b.get(i + 1)?);
        if !h.is_ascii_digit() || !l.is_ascii_digit() {
            return None;
        }
        Some(u32::from(h - b'0') * 10 + u32::from(l - b'0'))
    };
    let (h, m, sec) = (two(0)?, two(3)?, two(6)?);
    // 60 admits a leap second.
    if h > 23 || m > 59 || sec > 60 {
        return None;
    }
    let mut nanos = 0u32;
    if let Some(f) = frac {
        if f.is_empty() || f.len() > 9 || !f.bytes().all(|c| c.is_ascii_digit()) {
            return None;
        }
        for i in 0..9 {
            let d = f.as_bytes().get(i).map_or(0, |c| u32::from(c - b'0'));
            nanos = nanos * 10 + d;
        }
    }
    Some((days, h * 3600 + m * 60 + sec, nanos))
}

struct Comment {
    author: String,
    at: (i64, u32, u32),
    body: String,
}

struct Thread {
    path: String,
    line: Option<u64>,
    resolved: bool,
    comments: Vec<Comment>,
}

fn field<'a>(o: &'a BTreeMap<String, CValue>, k: &str, ctx: &str) -> Result<&'a CValue, String> {
    o.get(k).ok_or_else(|| format!("{ctx}: missing {k}"))
}

fn parse_comment(v: &CValue, ctx: &str) -> Result<Comment, String> {
    let CValue::Obj(o) = v else {
        return Err(format!("{ctx}: not an object"));
    };
    let author = match field(o, "author", ctx)? {
        CValue::Str(s) if valid_token(s) => s.clone(),
        _ => return Err(format!("{ctx}: author must be one token without spaces")),
    };
    let at = match field(o, "at", ctx)? {
        CValue::Str(s) => parse_timestamp(s).ok_or_else(|| format!("{ctx}: at must be YYYY-MM-DDThh:mm:ssZ"))?,
        _ => return Err(format!("{ctx}: at must be a string")),
    };
    let body = match field(o, "body", ctx)? {
        CValue::Str(s) => s.clone(),
        _ => return Err(format!("{ctx}: body must be a string")),
    };
    Ok(Comment { author, at, body })
}

fn parse_thread(i: usize, v: &CValue) -> Result<(String, Thread), String> {
    let ctx = format!("thread {i}");
    let CValue::Obj(o) = v else {
        return Err(format!("{ctx}: not an object"));
    };
    let id = match field(o, "id", &ctx)? {
        CValue::Str(s) if !s.is_empty() && s.len() <= MAX_ID_BYTES && s.chars().all(|c| c.is_ascii_graphic()) => {
            s.clone()
        }
        _ => return Err(format!("{ctx}: id must be one printable ASCII token")),
    };
    let ctx = format!("thread {id}");
    let path = match field(o, "path", &ctx)? {
        CValue::Str(s) if !s.is_empty() && s.len() <= MAX_PATH_BYTES && !s.chars().any(char::is_control) => s.clone(),
        _ => return Err(format!("{ctx}: path must be a non-empty string without control characters")),
    };
    let line = match field(o, "line", &ctx)? {
        CValue::Null => None,
        // The JSON reader keeps every number as f64; a line is a whole number from 1 to 1e9,
        // so the conversion below is exact.
        CValue::Num(f) if *f >= 1.0 && *f <= MAX_LINE && f.fract() == 0.0 => {
            #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
            let l = *f as u64;
            Some(l)
        }
        _ => return Err(format!("{ctx}: line must be a whole number from 1 or null")),
    };
    let resolved = match field(o, "resolved", &ctx)? {
        CValue::Bool(b) => *b,
        _ => return Err(format!("{ctx}: resolved must be a boolean")),
    };
    let comments: Vec<Comment> = match field(o, "comments", &ctx)? {
        CValue::Arr(a) if !a.is_empty() => a
            .iter()
            .enumerate()
            .map(|(j, c)| parse_comment(c, &format!("{ctx} comment {j}")))
            .collect::<Result<_, _>>()?,
        _ => return Err(format!("{ctx}: comments must be a non-empty array")),
    };
    Ok((id, Thread { path, line, resolved, comments }))
}

/// Parses the thread export. Errors are the `BLOCK-INPUT` detail.
fn parse_threads(text: &str) -> Result<BTreeMap<String, Thread>, String> {
    let root = fid_canon::parse_with_limits(text.as_bytes(), MAX_EXPORT_BYTES as usize, MAX_DEPTH)
        .map_err(|e| format!("threads: not valid JSON: {}", e.code()))?;
    let CValue::Arr(items) = root else {
        return Err("threads: must be an array of {id, path, line, resolved, comments}".to_string());
    };
    let mut out = BTreeMap::new();
    for (i, item) in items.iter().enumerate() {
        let (id, thread) = parse_thread(i, item)?;
        if out.insert(id.clone(), thread).is_some() {
            return Err(format!("threads: duplicate id {id}"));
        }
    }
    Ok(out)
}

/// The maintainer handles: one per line, surrounding whitespace removed; blank lines and
/// lines starting with `#` are skipped. Forge logins are case-insensitive ASCII, so the
/// handles are returned lowercased, and two entries that differ only in case are an input
/// error. An empty list is an input error (fail closed).
pub fn parse_maintainers(text: &str) -> Result<BTreeSet<String>, String> {
    let mut out = BTreeSet::new();
    for (i, raw) in text.lines().enumerate() {
        let l = raw.trim();
        if l.is_empty() || l.starts_with('#') {
            continue;
        }
        if !valid_token(l) {
            return Err(format!("maintainers line {}: not a single account name", i + 1));
        }
        if !out.insert(l.to_ascii_lowercase()) {
            return Err(format!("maintainers line {}: {l} is listed twice (ignoring case)", i + 1));
        }
    }
    if out.is_empty() {
        return Err("maintainers: no accounts".to_string());
    }
    Ok(out)
}

/// Evaluates the thread export against the bot accounts and maintainers. Accounts are
/// compared ASCII-case-insensitively, as forge logins are. `Err` is the `BLOCK-INPUT` detail.
pub fn evaluate(threads: &str, bots: &BTreeSet<String>, maintainers: &BTreeSet<String>) -> Result<Report, String> {
    let lower = |set: &BTreeSet<String>| -> BTreeSet<String> { set.iter().map(|a| a.to_ascii_lowercase()).collect() };
    let (bots, maintainers) = (lower(bots), lower(maintainers));
    let threads = parse_threads(threads)?;
    let mut report = Report::default();
    for (id, t) in threads {
        let mut comments = t.comments.iter();
        let Some(root) = comments.next() else { continue };
        let mut replies: Vec<&Comment> = comments.collect();
        // The root is the first comment of the export; a reply older than it means the
        // export is not in thread order, so which comment opened the thread is unknown.
        if replies.iter().any(|r| r.at < root.at) {
            return Err(format!("thread {id}: a reply is older than the root comment"));
        }
        // Stable: replies with equal timestamps keep their export order.
        replies.sort_by_key(|c| c.at);
        let mut last: Option<(&'static str, &str)> = None;
        for r in replies {
            let Some(mark) = mark_of(&r.body) else { continue };
            if maintainers.contains(&r.author.to_ascii_lowercase()) {
                last = Some((mark, r.author.as_str()));
            } else {
                report.ignored.push(IgnoredMark { id: id.clone(), mark, author: r.author.clone() });
            }
        }
        // REQ-004: only threads opened by a registered bot can block.
        if !bots.contains(&root.author.to_ascii_lowercase()) || t.resolved {
            continue;
        }
        if let Some((MARK_BLOCKING, by)) = last {
            report.blocking.push(Blocking { id, path: t.path, line: t.line, by: by.to_string() });
        }
    }
    Ok(report)
}

/// Runs the whole check from files. `Err` is the `BLOCK-INPUT` detail.
pub fn check(threads: &Path, register: &Path, maintainers: &Path) -> Result<Report, String> {
    let register_text = read_bounded(register, MAX_REGISTER_BYTES)?;
    let mut findings = BTreeSet::new();
    let bots = parse_register(&register_text, &mut findings);
    if let Some(f) = findings.into_iter().next() {
        return Err(format!("register: {f}"));
    }
    let accounts: BTreeSet<String> = bots.into_iter().map(|b| b.account.to_ascii_lowercase()).collect();
    if accounts.is_empty() {
        // No bots would make every thread advisory: fail closed, like an empty maintainer list.
        return Err("register: no bots".to_string());
    }
    let maintainers = parse_maintainers(&read_bounded(maintainers, MAX_MAINTAINERS_BYTES)?)?;
    let threads = read_bounded(threads, MAX_EXPORT_BYTES)?;
    evaluate(&threads, &accounts, &maintainers)
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn marks() {
        assert_eq!(mark_of("/blocking"), Some(MARK_BLOCKING));
        assert_eq!(mark_of("  /advisory\nnot needed"), Some(MARK_ADVISORY));
        assert_eq!(mark_of("/blocking: please fix"), None);
        assert_eq!(mark_of("this is /blocking"), None);
        assert_eq!(mark_of("/Blocking"), None);
        assert_eq!(mark_of(""), None);
    }

    #[test]
    fn timestamps() {
        let a = parse_timestamp("2026-11-17T10:00:00Z").unwrap();
        let b = parse_timestamp("2026-11-17T10:00:00.5Z").unwrap();
        let c = parse_timestamp("2026-11-17T10:00:01Z").unwrap();
        assert!(a < b && b < c);
        assert_eq!(b.2, 500_000_000);
        for bad in [
            "2026-11-17 10:00:00Z",
            "2026-11-17T10:00:00",
            "2026-11-17T10:00:00+01:00",
            "2026-11-17T24:00:00Z",
            "2026-11-17T10:00:00.Z",
            "2026-11-17T10:0:00Z",
            "2026-02-30T10:00:00Z",
            "",
        ] {
            assert_eq!(parse_timestamp(bad), None, "{bad}");
        }
    }

    #[test]
    fn maintainers() {
        let m = parse_maintainers("# list\n\n alice \nbob\n").unwrap();
        assert_eq!(m, BTreeSet::from(["alice".to_string(), "bob".to_string()]));
        assert!(parse_maintainers("# none\n").is_err());
        assert!(parse_maintainers("two words\n").is_err());
        assert_eq!(parse_maintainers("Ada-Quill\n").unwrap(), set(&["ada-quill"]));
        assert!(parse_maintainers("ada-quill\nAda-Quill\n").is_err());
        assert!(parse_maintainers("ada-quill\nada-quill\n").is_err());
    }

    #[test]
    fn accounts_match_ignoring_case() {
        let json = r#"[{"id":"T1","path":"a.rs","line":3,"resolved":false,"comments":[
            {"author":"Fid-Review[bot]","at":"2026-01-01T00:00:00Z","body":"finding"},
            {"author":"ADA-quill","at":"2026-01-01T01:00:00Z","body":"/blocking"}]}]"#;
        let r = evaluate(json, &set(&["fid-REVIEW[bot]"]), &set(&["Ada-Quill"])).unwrap();
        assert_eq!(r.lines(), ["blocking T1 a.rs:3 by ADA-quill", "bot-blocking: 1 unresolved blocking thread(s)"]);
        assert!(r.ignored.is_empty());
    }

    #[test]
    fn reply_older_than_root_is_input_error() {
        let json = r#"[{"id":"T1","path":"a.rs","line":3,"resolved":false,"comments":[
            {"author":"bot[bot]","at":"2026-01-01T01:00:00Z","body":"finding"},
            {"author":"alice","at":"2026-01-01T00:00:00Z","body":"/blocking"}]}]"#;
        let e = evaluate(json, &set(&["bot[bot]"]), &set(&["alice"])).unwrap_err();
        assert!(e.contains("older than the root"), "{e}");
    }

    #[test]
    fn paths_are_quoted_when_ambiguous() {
        assert_eq!(quote_path("src/a.rs"), "src/a.rs");
        assert_eq!(quote_path("docs/my file.md"), "\"docs/my file.md\"");
        assert_eq!(quote_path("a\"b\\c"), "\"a\\\"b\\\\c\"");
    }

    fn set(xs: &[&str]) -> BTreeSet<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reply_order_is_by_time_not_position() {
        // The /advisory reply is listed first but is older, so /blocking is the latest mark.
        let json = r#"[{"id":"T1","path":"a.rs","line":3,"resolved":false,"comments":[
            {"author":"bot[bot]","at":"2026-01-01T00:00:00Z","body":"finding"},
            {"author":"alice","at":"2026-01-01T02:00:00Z","body":"/blocking"},
            {"author":"alice","at":"2026-01-01T01:00:00Z","body":"/advisory"}]}]"#;
        let r = evaluate(json, &set(&["bot[bot]"]), &set(&["alice"])).unwrap();
        assert_eq!(r.blocking.len(), 1);
        assert_eq!(r.lines()[0], "blocking T1 a.rs:3 by alice");
    }

    #[test]
    fn bad_exports() {
        let bots = set(&["bot[bot]"]);
        let m = set(&["alice"]);
        for bad in [
            "{}",
            "[1]",
            r#"[{"id":"T","path":"a","line":1,"resolved":false,"comments":[]}]"#,
            r#"[{"id":"T","path":"a","line":0,"resolved":false,"comments":[{"author":"a","at":"2026-01-01T00:00:00Z","body":""}]}]"#,
            r#"[{"id":"T","path":"a","line":1.5,"resolved":false,"comments":[{"author":"a","at":"2026-01-01T00:00:00Z","body":""}]}]"#,
            r#"[{"id":"T","path":"a","line":1,"resolved":"no","comments":[{"author":"a","at":"2026-01-01T00:00:00Z","body":""}]}]"#,
            r#"[{"id":"T","path":"a","line":1,"resolved":false,"comments":[{"author":"a b","at":"2026-01-01T00:00:00Z","body":""}]}]"#,
            r#"[{"id":"T","path":"a","line":1,"resolved":false,"comments":[{"author":"a","at":"yesterday","body":""}]}]"#,
            r#"[{"id":"T","path":"a","line":1,"resolved":false,"comments":[{"author":"a","at":"2026-01-01T00:00:00Z"}]}]"#,
            r#"[{"id":"T","path":"a","line":1,"resolved":false,"comments":[{"author":"a","at":"2026-01-01T00:00:00Z","body":""}]},
               {"id":"T","path":"b","line":1,"resolved":false,"comments":[{"author":"a","at":"2026-01-01T00:00:00Z","body":""}]}]"#,
        ] {
            assert!(evaluate(bad, &bots, &m).is_err(), "{bad}");
        }
        let null_line = r#"[{"id":"T","path":"a","line":null,"resolved":false,"comments":[
            {"author":"bot[bot]","at":"2026-01-01T00:00:00Z","body":"x"},
            {"author":"alice","at":"2026-01-01T00:00:01Z","body":"/blocking"}]}]"#;
        assert_eq!(evaluate(null_line, &bots, &m).unwrap().lines()[0], "blocking T a:- by alice");
    }
}
