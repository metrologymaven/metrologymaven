//! `fid-design pkg-check --root <dir> [--json]` and
//! `fid-design pkg-diff --from <dir> --to <dir> [--json]` (spec node `UX.SYSTEM.PKG`).
//!
//! Exit codes: 0 no findings, 1 findings, 2 unreadable package or bad usage.
//!
//! `pkg-diff` classifies every published name as `added`, `removed`, `renamed` (removed, with
//! a deprecation entry whose replacement is published in the new package), `value-changed`
//! (token `$type`/`$value` or kind differs), `deprecated` (newly listed in `deprecations.json`,
//! still published) or `unchanged`, then checks the version bump, the changelog sections
//! between the two versions and the deprecation timing.

use fid_canon::CValue;
use fid_design_pkg::changelog::Class;
use fid_design_pkg::{Code, Finding, Kind, LoadError, Package, Version};
use std::collections::BTreeMap;
use std::path::Path;

/// Report format IDs.
pub const CHECK_REPORT_FORMAT: &str = "fid-design-pkg-check@1";
pub const DIFF_REPORT_FORMAT: &str = "fid-design-pkg-diff@1";

/// Result of `pkg-check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckReport {
    pub version: Option<Version>,
    pub counts: BTreeMap<Kind, usize>,
    pub findings: Vec<Finding>,
}

/// Runs the structural package check.
pub fn check(root: &Path) -> Result<CheckReport, LoadError> {
    let p = Package::load(root)?;
    Ok(CheckReport { version: p.version(), counts: p.counts(), findings: p.findings })
}

fn findings_value(f: &[Finding]) -> CValue {
    CValue::Arr(
        f.iter()
            .map(|x| {
                let mut m = BTreeMap::new();
                m.insert("code".to_string(), CValue::Str(x.code.as_str().to_string()));
                m.insert("subject".to_string(), CValue::Str(x.subject.clone()));
                m.insert("detail".to_string(), CValue::Str(x.detail.clone()));
                CValue::Obj(m)
            })
            .collect(),
    )
}

fn version_value(v: Option<Version>) -> CValue {
    v.map_or(CValue::Null, |v| CValue::Str(v.to_string()))
}

fn findings_text(out: &mut String, f: &[Finding]) {
    for x in f {
        out.push_str(&x.to_string());
        out.push('\n');
    }
    if f.is_empty() {
        out.push_str("ok: no findings\n");
    } else {
        out.push_str(&format!("{} finding(s)\n", f.len()));
    }
}

impl CheckReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut counts = BTreeMap::new();
        for (k, n) in &self.counts {
            counts.insert(k.as_str().to_string(), CValue::Num(*n as f64));
        }
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(CHECK_REPORT_FORMAT.to_string()));
        m.insert("version".to_string(), version_value(self.version));
        m.insert("counts".to_string(), CValue::Obj(counts));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let v = self.version.map_or_else(|| "(unknown version)".to_string(), |v| v.to_string());
        let counts: Vec<String> = self.counts.iter().map(|(k, n)| format!("{} {n}", k.as_str())).collect();
        let mut s = format!("fid-design pkg-check: package {v}\nnames: {}\n", counts.join(", "));
        findings_text(&mut s, &self.findings);
        s
    }
}

/// Diff classification of one name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Change {
    Added,
    Removed,
    Renamed { replacement: String },
    ValueChanged,
    Deprecated { replacement: String },
    Unchanged,
}

impl Change {
    pub fn as_str(&self) -> &'static str {
        match self {
            Change::Added => "added",
            Change::Removed => "removed",
            Change::Renamed { .. } => "renamed",
            Change::ValueChanged => "value-changed",
            Change::Deprecated { .. } => "deprecated",
            Change::Unchanged => "unchanged",
        }
    }

    /// Changelog classes that agree with this classification.
    pub fn allowed_classes(&self) -> &'static [Class] {
        match self {
            Change::Added | Change::Deprecated { .. } => &[Class::Additive],
            Change::Removed | Change::Renamed { .. } => &[Class::Breaking],
            Change::ValueChanged => &[Class::Fix, Class::Breaking],
            Change::Unchanged => &[],
        }
    }
}

/// Result of `pkg-diff`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DiffReport {
    pub from: Option<Version>,
    pub to: Option<Version>,
    pub changes: BTreeMap<String, Change>,
    pub findings: Vec<Finding>,
}

/// Classifies names between two packages and checks REQ-UX.SYSTEM.PKG-004..006.
pub fn diff(from_root: &Path, to_root: &Path) -> Result<DiffReport, LoadError> {
    let old = Package::load(from_root)?;
    let new = Package::load(to_root)?;
    Ok(diff_packages(&old, &new))
}

/// [`diff`] on loaded packages. Structural findings of either package are carried over
/// with the detail prefixed `from:` or `to:`.
pub fn diff_packages(old: &Package, new: &Package) -> DiffReport {
    let mut f: Vec<Finding> = Vec::new();
    for (side, p) in [("from", old), ("to", new)] {
        for x in &p.findings {
            f.push(Finding::new(x.code, &x.subject, format!("{side}: {}", x.detail)));
        }
    }
    let changes = classify(old, new);
    let (from, to) = (old.version(), new.version());
    if let (Some(a), Some(b)) = (from, to) {
        if b < a {
            f.push(Finding::new(Code::Schema, "package.toml", format!("--to version {b} is older than --from {a}")));
        }
        check_breaking(&changes, a, b, old, new, &mut f);
        check_changelog(&changes, a, b, new, &mut f);
    }
    f.sort();
    f.dedup();
    DiffReport { from, to, changes, findings: f }
}

fn classify(old: &Package, new: &Package) -> BTreeMap<String, Change> {
    let (on, nn) = (old.names(), new.names());
    let mut out = BTreeMap::new();
    let mut all: Vec<&str> = on.keys().chain(nn.keys()).copied().collect();
    all.sort_unstable();
    all.dedup();
    for name in all {
        let change = match (on.get(name), nn.get(name)) {
            (Some(_), None) => {
                let dep = old.deprecations.get(name).or_else(|| new.deprecations.get(name));
                match dep.and_then(|d| d.replacement.clone()) {
                    Some(r) if nn.contains_key(r.as_str()) => Change::Renamed { replacement: r },
                    _ => Change::Removed,
                }
            }
            (None, Some(_)) => Change::Added,
            (Some(k1), Some(k2)) => {
                let fp = |p: &Package| p.tokens.get(name).map(|t| t.fingerprint());
                if k1 != k2 || fp(old) != fp(new) {
                    Change::ValueChanged
                } else {
                    match (old.deprecations.get(name), new.deprecations.get(name)) {
                        (None, Some(d)) => Change::Deprecated { replacement: d.replacement.clone().unwrap_or_default() },
                        _ => Change::Unchanged,
                    }
                }
            }
            (None, None) => Change::Unchanged,
        };
        out.insert(name.to_string(), change);
    }
    out
}

fn check_breaking(
    changes: &BTreeMap<String, Change>,
    from: Version,
    to: Version,
    old: &Package,
    new: &Package,
    f: &mut Vec<Finding>,
) {
    for (name, c) in changes {
        if !matches!(c, Change::Removed | Change::Renamed { .. }) {
            continue;
        }
        if to.major <= from.major {
            f.push(Finding::new(
                Code::BreakingMinor,
                name,
                format!("{} from {from} to {to} without a major version increment", c.as_str()),
            ));
        }
        let Some(d) = old.deprecations.get(name).or_else(|| new.deprecations.get(name)) else {
            f.push(Finding::new(Code::DeprecationEarly, name, "removed without a prior deprecation entry"));
            continue;
        };
        if d.since > from {
            f.push(Finding::new(
                Code::DeprecationEarly,
                name,
                format!("deprecated since {} but removed by {to}; no released version listed it as deprecated", d.since),
            ));
        } else if to.line() <= d.since.line() {
            f.push(Finding::new(
                Code::DeprecationEarly,
                name,
                format!("removed in {to}, less than one minor version after deprecation in {}", d.since),
            ));
        } else if to < d.remove_after {
            f.push(Finding::new(
                Code::DeprecationEarly,
                name,
                format!("removed in {to}, before remove_after {}", d.remove_after),
            ));
        }
        if d.replacement.is_none() {
            f.push(Finding::new(Code::DeprecationNorepl, name, "removed name's deprecation entry has no replacement"));
        }
    }
}

fn check_changelog(changes: &BTreeMap<String, Change>, from: Version, to: Version, new: &Package, f: &mut Vec<Finding>) {
    // Entries of every release after `from` up to and including `to`, read from the new
    // package's changelog (it carries the history).
    let mut entries: BTreeMap<&str, Vec<(Version, Class, usize)>> = BTreeMap::new();
    for (v, list) in &new.changelog.sections {
        if *v <= from || *v > to {
            continue;
        }
        for e in list {
            entries.entry(e.name.as_str()).or_default().push((*v, e.class, e.line));
        }
    }
    for (name, c) in changes {
        let listed = entries.get(name.as_str());
        if *c == Change::Unchanged {
            continue;
        }
        let Some(list) = listed else {
            f.push(Finding::new(
                Code::ChangelogMissing,
                name,
                format!("{} between {from} and {to} but no CHANGELOG.md entry", c.as_str()),
            ));
            continue;
        };
        for (v, class, line) in list {
            if !c.allowed_classes().contains(class) {
                let want: Vec<&str> = c.allowed_classes().iter().map(|k| k.as_str()).collect();
                f.push(Finding::new(
                    Code::ChangelogClass,
                    name,
                    format!(
                        "CHANGELOG.md line {line} ({v}) says {} but the diff says {} (expected {})",
                        class.as_str(),
                        c.as_str(),
                        want.join(" or ")
                    ),
                ));
            }
        }
    }
    for (name, list) in &entries {
        let unchanged = match changes.get(*name) {
            Some(c) => *c == Change::Unchanged,
            None => true,
        };
        if unchanged {
            for (v, class, line) in list {
                f.push(Finding::new(
                    Code::ChangelogClass,
                    *name,
                    format!("CHANGELOG.md line {line} ({v}) says {} but the diff finds no change", class.as_str()),
                ));
            }
        }
    }
}

impl DiffReport {
    pub fn exit_code(&self) -> u8 {
        u8::from(!self.findings.is_empty())
    }

    pub fn to_cvalue(&self) -> CValue {
        let mut names = BTreeMap::new();
        for (n, c) in &self.changes {
            let mut m = BTreeMap::new();
            m.insert("change".to_string(), CValue::Str(c.as_str().to_string()));
            if let Change::Renamed { replacement } | Change::Deprecated { replacement } = c {
                m.insert("replacement".to_string(), CValue::Str(replacement.clone()));
            }
            names.insert(n.clone(), CValue::Obj(m));
        }
        let mut m = BTreeMap::new();
        m.insert("format".to_string(), CValue::Str(DIFF_REPORT_FORMAT.to_string()));
        m.insert("from".to_string(), version_value(self.from));
        m.insert("to".to_string(), version_value(self.to));
        m.insert("names".to_string(), CValue::Obj(names));
        m.insert("findings".to_string(), findings_value(&self.findings));
        m.insert("ok".to_string(), CValue::Bool(self.findings.is_empty()));
        CValue::Obj(m)
    }

    /// RFC 8785 canonical JSON.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    pub fn to_text(&self) -> String {
        let v = |x: Option<Version>| x.map_or_else(|| "(unknown)".to_string(), |v| v.to_string());
        let mut s = format!("fid-design pkg-diff: {} -> {}\n", v(self.from), v(self.to));
        for (n, c) in &self.changes {
            match c {
                Change::Unchanged => {}
                Change::Renamed { replacement } | Change::Deprecated { replacement } => {
                    s.push_str(&format!("{} {n} -> {replacement}\n", c.as_str()));
                }
                _ => s.push_str(&format!("{} {n}\n", c.as_str())),
            }
        }
        findings_text(&mut s, &self.findings);
        s
    }
}
