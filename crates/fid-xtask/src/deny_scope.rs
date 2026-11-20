//! Keeps the `cargo deny` advisory exceptions scoped (decision D-044).
//!
//! `deny.toml` ignores four "unmaintained" advisories (not vulnerabilities) that reach the
//! workspace only through the egui UI spike, `fid-spike-ui` (VIZ.SHELL.SPKTREE). This check
//! fails closed when that stops being true:
//!
//! - `unlisted_ignore`: `deny.toml` ignores an advisory that is not in [`EXCEPTIONS`]; any new
//!   ignore needs its own entry here and its own decision, never a widened one;
//! - `bad_reason`: an ignore entry has no `reason`, or its reason does not name the crate, the
//!   `only via fid-spike-ui` path, `unmaintained, not a vulnerability` and one
//!   `expires YYYY-MM-DD` date;
//! - `expired`: the expiry date has passed (today is later than it), or it is later than
//!   [`LATEST_EXPIRY`];
//! - `egui_upgraded`: the graph holds egui 0.34 or later, which ends every exception
//!   ("2027-03-01 or the egui >= 0.34 upgrade, whichever comes first");
//! - `escaped_scope`: a workspace crate other than [`SCOPED_ROOT`] reaches an advisory crate,
//!   directly or transitively, through normal, build or dev dependencies;
//! - `stale_ignore`: an advisory crate is no longer in the graph, so its ignore can go.
//!
//! The graph must be resolved for all targets and all features ([`Graph::load_all_targets`]):
//! `zvariant` and friends only appear on Linux, through `accesskit_unix`.

use super::Graph;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

/// The only workspace crate allowed to reach the advisory crates.
pub const SCOPED_ROOT: &str = "fid-spike-ui";

/// The latest expiry any exception may carry (D-044).
pub const LATEST_EXPIRY: Date = Date { y: 2027, m: 3, d: 1 };

/// Every exception ends when egui reaches this (major, minor) version.
pub const EGUI_END: (u64, u64) = (0, 34);

/// Advisory ID and the crates it names. Each entry is decided separately (D-044).
pub const EXCEPTIONS: &[(&str, &[&str])] = &[
    ("RUSTSEC-2026-0192", &["ttf-parser"]),
    ("RUSTSEC-2026-0370", &["zvariant"]),
    ("RUSTSEC-2026-0371", &["zbus_names"]),
    ("RUSTSEC-2026-0372", &["zvariant_derive"]),
];

/// A calendar date (proleptic Gregorian), ordered by (year, month, day).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Date {
    pub y: i64,
    pub m: u32,
    pub d: u32,
}

impl Date {
    /// Parses exactly `YYYY-MM-DD` with a valid month and day.
    pub fn parse(s: &str) -> Option<Date> {
        let b = s.as_bytes();
        if b.len() != 10 || b[4] != b'-' || b[7] != b'-' {
            return None;
        }
        let digits = |r: std::ops::Range<usize>| -> Option<u32> {
            let part = s.get(r)?;
            if !part.bytes().all(|c| c.is_ascii_digit()) {
                return None;
            }
            part.parse().ok()
        };
        let y = i64::from(digits(0..4)?);
        let m = digits(5..7)?;
        let d = digits(8..10)?;
        if !(1..=12).contains(&m) || d < 1 || d > days_in_month(y, m) {
            return None;
        }
        Some(Date { y, m, d })
    }

    /// The UTC date of `days` days after 1970-01-01 (H. Hinnant's `civil_from_days`).
    pub fn from_unix_days(days: i64) -> Date {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z.rem_euclid(146_097);
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        let y = yoe + era * 400 + i64::from(m <= 2);
        Date { y, m: m as u32, d: d as u32 }
    }
}

impl std::fmt::Display for Date {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.y, self.m, self.d)
    }
}

fn days_in_month(y: i64, m: u32) -> u32 {
    match m {
        2 if (y % 4 == 0 && y % 100 != 0) || y % 400 == 0 => 29,
        2 => 28,
        4 | 6 | 9 | 11 => 30,
        _ => 31,
    }
}

/// One `[advisories] ignore` entry of `deny.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ignore {
    pub id: String,
    /// `None` for a bare-string entry or a table without `reason`.
    pub reason: Option<String>,
}

/// Reads `[advisories] ignore` from `deny.toml`. Duplicate IDs and malformed entries are errors.
pub fn parse_ignores(deny_toml: &str) -> Result<Vec<Ignore>, String> {
    let t = deny_toml.parse::<toml::Table>().map_err(|e| format!("deny.toml: {e}"))?;
    let Some(adv) = t.get("advisories") else { return Ok(Vec::new()) };
    let adv = adv.as_table().ok_or("deny.toml: [advisories] is not a table")?;
    let Some(list) = adv.get("ignore") else { return Ok(Vec::new()) };
    let list = list.as_array().ok_or("deny.toml: advisories.ignore is not an array")?;
    let mut out = Vec::new();
    let mut seen = BTreeSet::new();
    for v in list {
        let ig = match v {
            toml::Value::String(id) => Ignore { id: id.clone(), reason: None },
            toml::Value::Table(e) => {
                let id = e.get("id").and_then(toml::Value::as_str).ok_or("deny.toml: ignore entry without id")?;
                let reason = match e.get("reason") {
                    None => None,
                    Some(r) => Some(r.as_str().ok_or("deny.toml: ignore reason is not a string")?.to_string()),
                };
                Ignore { id: id.to_string(), reason }
            }
            _ => return Err("deny.toml: ignore entry is neither a string nor a table".into()),
        };
        if !seen.insert(ig.id.clone()) {
            return Err(format!("deny.toml: duplicate ignore {}", ig.id));
        }
        out.push(ig);
    }
    Ok(out)
}

/// The single `expires YYYY-MM-DD` date in a reason, if there is exactly one and it is valid.
pub fn expiry(reason: &str) -> Option<Date> {
    let mut found = None;
    for (i, _) in reason.match_indices("expires ") {
        let rest = reason.get(i + "expires ".len()..)?;
        let date = Date::parse(rest.get(..10)?)?;
        if found.replace(date).is_some() {
            return None;
        }
    }
    found
}

fn version_of(v: &str) -> Option<(u64, u64)> {
    let mut it = v.split('.');
    Some((it.next()?.parse().ok()?, it.next()?.parse().ok()?))
}

/// First path (crate names) from `root` to a crate named in `targets`, over every resolved
/// edge (dev-dependencies included), visiting dependencies in (name, ID) order.
fn first_path(g: &Graph, root: &str, targets: &BTreeSet<&str>) -> Option<Vec<String>> {
    let name = |id: &str| g.names.get(id).cloned().unwrap_or_else(|| id.to_string());
    let mut parent: BTreeMap<&str, &str> = BTreeMap::new();
    let mut seen: BTreeSet<&str> = BTreeSet::from([root]);
    let mut queue: VecDeque<&str> = VecDeque::from([root]);
    while let Some(node) = queue.pop_front() {
        let mut next: Vec<&str> = g.edges.get(node).into_iter().flatten().map(|e| e.to.as_str()).collect();
        next.sort_by_key(|id| (name(id), id.to_string()));
        for n in next {
            if !seen.insert(n) {
                continue;
            }
            parent.insert(n, node);
            if targets.contains(name(n).as_str()) {
                let mut path = vec![name(n)];
                let mut cur = n;
                while let Some(&p) = parent.get(cur) {
                    path.push(name(p));
                    cur = p;
                }
                path.reverse();
                return Some(path);
            }
            queue.push_back(n);
        }
    }
    None
}

/// Checks `deny.toml`'s ignores against [`EXCEPTIONS`], the date `today` and the graph.
/// Returns sorted findings, one per line: `<advisory or crate>: <rule> <detail>`.
pub fn check(deny_toml: &str, g: &Graph, today: Date) -> Result<Vec<String>, String> {
    let ignores = parse_ignores(deny_toml)?;
    let table: BTreeMap<&str, &[&str]> = EXCEPTIONS.iter().copied().collect();
    let mut out = BTreeSet::new();
    let present: BTreeSet<&str> = g.names.values().map(String::as_str).collect();

    let egui: Vec<(u64, u64)> = g
        .names
        .iter()
        .filter(|(_, n)| *n == "egui")
        .filter_map(|(id, _)| g.versions.get(id))
        .filter_map(|v| version_of(v.as_str()))
        .collect();

    let mut targets: BTreeSet<&str> = BTreeSet::new();
    for ig in &ignores {
        let Some(crates) = table.get(ig.id.as_str()) else {
            out.insert(format!("{}: unlisted_ignore not in fid_xtask::deny_scope::EXCEPTIONS", ig.id));
            continue;
        };
        targets.extend(crates.iter().copied());
        let Some(reason) = &ig.reason else {
            out.insert(format!("{}: bad_reason missing", ig.id));
            continue;
        };
        for need in ["only via fid-spike-ui", "unmaintained, not a vulnerability"] {
            if !reason.contains(need) {
                out.insert(format!("{}: bad_reason lacks \"{need}\"", ig.id));
            }
        }
        for c in crates.iter() {
            if !reason.contains(c) {
                out.insert(format!("{}: bad_reason does not name {c}", ig.id));
            }
        }
        match expiry(reason) {
            None => {
                out.insert(format!("{}: bad_reason needs exactly one \"expires YYYY-MM-DD\"", ig.id));
            }
            Some(exp) => {
                if exp > LATEST_EXPIRY {
                    out.insert(format!("{}: expired date {exp} is past the D-044 limit {LATEST_EXPIRY}", ig.id));
                }
                if today > exp {
                    out.insert(format!("{}: expired on {exp} (today {today})", ig.id));
                }
            }
        }
        if egui.iter().any(|v| *v >= EGUI_END) {
            out.insert(format!("{}: egui_upgraded egui >= {}.{} is in the graph", ig.id, EGUI_END.0, EGUI_END.1));
        }
        for c in crates.iter() {
            if !present.contains(c) {
                out.insert(format!("{c}: stale_ignore {} names a crate no longer in the graph", ig.id));
            }
        }
    }

    if !targets.is_empty() {
        let mut roots: Vec<(&str, &str)> =
            g.members.iter().map(|id| (g.names.get(id).map(String::as_str).unwrap_or(id), id.as_str())).collect();
        roots.sort();
        for (root_name, root_id) in roots {
            if root_name == SCOPED_ROOT {
                continue;
            }
            if let Some(path) = first_path(g, root_id, &targets) {
                out.insert(format!("{root_name}: escaped_scope {}", path.join(" -> ")));
            }
        }
    }
    Ok(out.into_iter().collect())
}
