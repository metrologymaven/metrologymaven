//! Minimal Cargo-style version requirements for `kernel-allowlist.toml` entries.
//!
//! Supported: `^`, `~`, `=`, `>`, `>=`, `<`, `<=`, a bare version (caret), `*` and `x`
//! wildcards, and comma-separated comparators (all must hold). Build metadata is ignored.
//! A pre-release version matches only if some comparator names the same `major.minor.patch`
//! with a pre-release, as Cargo does; everything unparseable is rejected (fail closed).

use std::cmp::Ordering;

/// A parsed `major.minor.patch[-pre][+build]` version.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
    /// Pre-release identifiers, empty for a release.
    pub pre: Vec<String>,
}

fn num(s: &str) -> Option<u64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) || (s.len() > 1 && s.starts_with('0')) {
        return None;
    }
    s.parse().ok()
}

fn parse_pre(s: &str) -> Option<Vec<String>> {
    let ids: Vec<String> = s.split('.').map(str::to_string).collect();
    let ok = ids.iter().all(|i| !i.is_empty() && i.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-'));
    ok.then_some(ids)
}

impl Version {
    /// Parses a full semantic version.
    pub fn parse(s: &str) -> Option<Version> {
        let s = s.split_once('+').map_or(s, |(v, _)| v);
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, parse_pre(p)?),
            None => (s, Vec::new()),
        };
        let parts: Vec<&str> = core.split('.').collect();
        match parts.as_slice() {
            [a, b, c] => Some(Version { major: num(a)?, minor: num(b)?, patch: num(c)?, pre }),
            _ => None,
        }
    }

    fn triple(&self) -> (u64, u64, u64) {
        (self.major, self.minor, self.patch)
    }
}

fn cmp_pre(a: &[String], b: &[String]) -> Ordering {
    match (a.is_empty(), b.is_empty()) {
        (true, true) => return Ordering::Equal,
        (true, false) => return Ordering::Greater,
        (false, true) => return Ordering::Less,
        _ => {}
    }
    for (x, y) in a.iter().zip(b.iter()) {
        let o = match (num(x), num(y)) {
            (Some(p), Some(q)) => p.cmp(&q),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => x.cmp(y),
        };
        if o != Ordering::Equal {
            return o;
        }
    }
    a.len().cmp(&b.len())
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        self.triple().cmp(&other.triple()).then_with(|| cmp_pre(&self.pre, &other.pre))
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Op {
    Exact,
    Greater,
    GreaterEq,
    Less,
    LessEq,
    Tilde,
    Caret,
    Wildcard,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Comparator {
    op: Op,
    major: u64,
    minor: Option<u64>,
    patch: Option<u64>,
    pre: Vec<String>,
}

/// A parsed version requirement (comparators joined by AND; empty means `*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VersionReq {
    text: String,
    comparators: Vec<Comparator>,
}

fn v(major: u64, minor: u64, patch: u64) -> Version {
    Version { major, minor, patch, pre: Vec::new() }
}

impl Comparator {
    fn parse(s: &str) -> Option<Comparator> {
        let s = s.trim();
        let (op, rest) = [
            (">=", Op::GreaterEq),
            ("<=", Op::LessEq),
            (">", Op::Greater),
            ("<", Op::Less),
            ("=", Op::Exact),
            ("~", Op::Tilde),
            ("^", Op::Caret),
        ]
        .iter()
        .find_map(|(p, op)| s.strip_prefix(p).map(|r| (*op, r.trim_start())))
        .unwrap_or((Op::Caret, s));
        let bare = rest == s;
        let rest = rest.split_once('+').map_or(rest, |(v, _)| v);
        let (core, pre) = match rest.split_once('-') {
            Some((c, p)) => (c, parse_pre(p)?),
            None => (rest, Vec::new()),
        };
        let parts: Vec<&str> = core.split('.').collect();
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        let wild = |p: &str| p == "*" || p == "x" || p == "X";
        let mut nums: Vec<u64> = Vec::new();
        let mut saw_wild = false;
        for p in &parts {
            if wild(p) {
                saw_wild = true;
            } else if saw_wild {
                return None;
            } else {
                nums.push(num(p)?);
            }
        }
        let op = if saw_wild {
            // `1.*` style needs no operator (or `=`); a wildcard major alone is handled by the caller.
            if !(bare || op == Op::Exact) || nums.is_empty() {
                return None;
            }
            Op::Wildcard
        } else {
            op
        };
        if !pre.is_empty() && nums.len() != 3 {
            return None;
        }
        let major = *nums.first()?;
        Some(Comparator { op, major, minor: nums.get(1).copied(), patch: nums.get(2).copied(), pre })
    }

    fn matches(&self, ver: &Version) -> bool {
        let (m, mi, pa) = (self.major, self.minor, self.patch);
        let full = Version { major: m, minor: mi.unwrap_or(0), patch: pa.unwrap_or(0), pre: self.pre.clone() };
        let next_major = v(m.saturating_add(1), 0, 0);
        match (self.op, mi, pa) {
            (Op::Exact, Some(_), Some(_)) => *ver == full,
            (Op::Exact | Op::Wildcard, Some(j), None) => ver.major == m && ver.minor == j,
            (Op::Exact | Op::Wildcard, None, _) => ver.major == m,
            (Op::Wildcard, Some(j), Some(_)) => ver.major == m && ver.minor == j,
            (Op::Greater, Some(_), Some(_)) => *ver > full,
            (Op::Greater, Some(j), None) => *ver >= v(m, j.saturating_add(1), 0),
            (Op::Greater, None, _) => *ver >= next_major,
            (Op::GreaterEq, _, _) => *ver >= full,
            (Op::Less, _, _) => *ver < full,
            (Op::LessEq, Some(_), Some(_)) => *ver <= full,
            (Op::LessEq, Some(j), None) => *ver < v(m, j.saturating_add(1), 0),
            (Op::LessEq, None, _) => *ver < next_major,
            (Op::Tilde, Some(j), _) => *ver >= full && *ver < v(m, j.saturating_add(1), 0),
            (Op::Tilde, None, _) => *ver >= full && *ver < next_major,
            (Op::Caret, _, _) => {
                let upper = match (m, mi, pa) {
                    (0, Some(0), Some(p)) => v(0, 0, p.saturating_add(1)),
                    (0, Some(j), _) => v(0, j.saturating_add(1), 0),
                    _ => next_major,
                };
                *ver >= full && *ver < upper
            }
        }
    }
}

impl VersionReq {
    /// Parses a requirement such as `^1.2`, `>=0.3, <0.5` or `*`.
    pub fn parse(s: &str) -> Option<VersionReq> {
        let t = s.trim();
        if t.is_empty() {
            return None;
        }
        let comparators =
            if t == "*" { Vec::new() } else { t.split(',').map(Comparator::parse).collect::<Option<Vec<_>>>()? };
        Some(VersionReq { text: t.to_string(), comparators })
    }

    /// The requirement as written (trimmed).
    pub fn as_str(&self) -> &str {
        &self.text
    }

    /// True if `ver` satisfies every comparator (and the pre-release rule).
    pub fn matches(&self, ver: &Version) -> bool {
        if !ver.pre.is_empty() {
            let named = self.comparators.iter().any(|c| {
                !c.pre.is_empty() && c.major == ver.major && c.minor == Some(ver.minor) && c.patch == Some(ver.patch)
            });
            if !named {
                return false;
            }
        }
        self.comparators.iter().all(|c| c.matches(ver))
    }
}
