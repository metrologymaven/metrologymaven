//! `CHANGELOG.md`: `## <version>` headings, each followed by entries
//! `- <class>: <name> - <one line>` with class `breaking`, `additive` or `fix`.
//! Other lines (the title, prose, blank lines, deeper headings) are ignored.

use crate::version::Version;
use crate::{is_valid_name, Code, Finding};
use std::collections::BTreeMap;

/// Change class of a changelog entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Class {
    Breaking,
    Additive,
    Fix,
}

impl Class {
    pub fn as_str(self) -> &'static str {
        match self {
            Class::Breaking => "breaking",
            Class::Additive => "additive",
            Class::Fix => "fix",
        }
    }

    pub fn parse(s: &str) -> Option<Class> {
        match s {
            "breaking" => Some(Class::Breaking),
            "additive" => Some(Class::Additive),
            "fix" => Some(Class::Fix),
            _ => None,
        }
    }
}

/// One changelog entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub class: Class,
    pub name: String,
    pub text: String,
    /// 1-based line number in `CHANGELOG.md`.
    pub line: usize,
}

/// The parsed changelog, keyed by release version.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Changelog {
    pub sections: BTreeMap<Version, Vec<Entry>>,
}

const FILE: &str = "CHANGELOG.md";

fn bad(line: usize, msg: &str) -> Finding {
    Finding::new(Code::Schema, FILE, format!("line {line}: {msg}"))
}

/// Parses the changelog text; malformed headings and entries become `PKG-SCHEMA` findings.
pub fn parse(text: &str) -> (Changelog, Vec<Finding>) {
    let mut log = Changelog::default();
    let mut findings = Vec::new();
    let mut current: Option<Version> = None;
    for (i, raw) in text.lines().enumerate() {
        let n = i + 1;
        let line = raw.trim_end();
        if let Some(rest) = line.strip_prefix("## ") {
            match Version::parse(rest.trim()) {
                Some(v) if log.sections.contains_key(&v) => {
                    findings.push(bad(n, &format!("duplicate section ## {v}")));
                    current = None;
                }
                Some(v) => {
                    log.sections.insert(v, Vec::new());
                    current = Some(v);
                }
                None => {
                    findings.push(bad(n, "section heading is not ## MAJOR.MINOR.PATCH"));
                    current = None;
                }
            }
            continue;
        }
        let Some(body) = line.strip_prefix("- ") else {
            continue;
        };
        let Some(v) = current else {
            findings.push(bad(n, "entry outside a ## <version> section"));
            continue;
        };
        match parse_entry(body, n) {
            Ok(e) => {
                if let Some(s) = log.sections.get_mut(&v) {
                    s.push(e);
                }
            }
            Err(msg) => findings.push(bad(n, &msg)),
        }
    }
    (log, findings)
}

fn parse_entry(body: &str, line: usize) -> Result<Entry, String> {
    let shape = "entry is not `- breaking|additive|fix: <name> - <text>`";
    let (class, rest) = body.split_once(": ").ok_or_else(|| shape.to_string())?;
    let class = Class::parse(class).ok_or_else(|| format!("unknown class {class:?}"))?;
    let (name, text) = rest.split_once(" - ").ok_or_else(|| shape.to_string())?;
    if !is_valid_name(name) {
        return Err(format!("entry name {name:?} is not a valid published name"));
    }
    if text.trim().is_empty() {
        return Err("entry has no description".to_string());
    }
    Ok(Entry { class, name: name.to_string(), text: text.trim().to_string(), line })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_sections_and_entries() {
        let text = "# Changelog\n\n## 1.1.0\n\nProse.\n- additive: a.b - added\n- fix: c - fixed it\n\n## 1.0.0\n";
        let (log, f) = parse(text);
        assert!(f.is_empty(), "{f:?}");
        let s = &log.sections[&Version::new(1, 1, 0)];
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].class, Class::Additive);
        assert_eq!(s[0].name, "a.b");
        assert_eq!(s[1].line, 7);
        assert!(log.sections[&Version::new(1, 0, 0)].is_empty());
    }

    #[test]
    fn malformed_lines_are_schema_findings() {
        let text = "- fix: a - early\n## one\n## 1.0.0\n- removed: a - x\n- fix: a\n- fix: A - x\n## 1.0.0\n";
        let (_, f) = parse(text);
        assert_eq!(f.len(), 6, "{f:?}");
        assert!(f.iter().all(|x| x.code == Code::Schema));
    }
}
