//! The versioned rule file `qif-norm.toml` (REQ-IO.QIF.NORM-001, -002).

use super::NormError;
use std::collections::{BTreeMap, BTreeSet};

/// The rule identifiers of `qif-norm@1`, in order (REQ-IO.QIF.NORM-001).
pub const RULE_IDS: [&str; 4] = ["c14n11", "xsd-collapse", "drop-ws-text", "ns-prefix-table"];

/// The rule file shipped with this crate.
pub const RULE_FILE: &str = include_str!("../../qif-norm.toml");

/// The lock file shipped with this crate: one `<version> <sha256 digest>` line per
/// version of the rule file, oldest first.
pub const LOCK_FILE: &str = include_str!("../../qif-norm.lock");

/// A loaded rule file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NormRules {
    version: String,
    version_number: u32,
    rules: Vec<String>,
    prefixes: BTreeMap<String, String>,
    simple_elements: BTreeSet<(String, String)>,
    simple_attributes: BTreeSet<(String, String)>,
}

impl NormRules {
    /// The rules shipped with this crate ([`RULE_FILE`]).
    pub fn builtin() -> Result<NormRules, NormError> {
        NormRules::parse(RULE_FILE)
    }

    /// Parses and validates a rule file.
    pub fn parse(text: &str) -> Result<NormRules, NormError> {
        let bad = |reason| NormError::RulesInvalid { reason };
        let table: toml::Table = text.parse().map_err(|_| bad("not valid TOML"))?;
        for key in table.keys() {
            if !matches!(key.as_str(), "version" | "rules" | "prefixes" | "simple_content") {
                return Err(bad("unknown top-level key"));
            }
        }
        let version = table.get("version").and_then(toml::Value::as_str).ok_or(bad("missing version"))?;
        let version_number = parse_version(version).ok_or(bad("version is not qif-norm@<n>"))?;
        let rules: Vec<String> = string_list(table.get("rules")).ok_or(bad("rules is not a list of strings"))?;
        if rules.iter().map(String::as_str).ne(RULE_IDS.iter().copied()) {
            return Err(bad("rules must be exactly c14n11, xsd-collapse, drop-ws-text, ns-prefix-table"));
        }
        let mut prefixes = BTreeMap::new();
        let mut seen = BTreeSet::new();
        let ptable = table.get("prefixes").and_then(toml::Value::as_table).ok_or(bad("missing [prefixes]"))?;
        for (uri, prefix) in ptable {
            let prefix = prefix.as_str().ok_or(bad("a prefix is not a string"))?;
            if uri.is_empty() || !valid_prefix(prefix) || !seen.insert(prefix.to_string()) {
                return Err(bad("invalid, reserved or duplicate prefix"));
            }
            prefixes.insert(uri.clone(), prefix.to_string());
        }
        let simple =
            table.get("simple_content").and_then(toml::Value::as_table).ok_or(bad("missing [simple_content]"))?;
        for key in simple.keys() {
            if !matches!(key.as_str(), "elements" | "attributes") {
                return Err(bad("unknown key in [simple_content]"));
            }
        }
        let simple_elements = clark_set(simple.get("elements")).ok_or(bad("invalid simple_content.elements"))?;
        let simple_attributes = clark_set(simple.get("attributes")).ok_or(bad("invalid simple_content.attributes"))?;
        Ok(NormRules {
            version: version.to_string(),
            version_number,
            rules,
            prefixes,
            simple_elements,
            simple_attributes,
        })
    }

    /// The version string, e.g. `qif-norm@1`.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// The version number `n` of `qif-norm@n`.
    pub fn version_number(&self) -> u32 {
        self.version_number
    }

    /// The rule identifiers, in order.
    pub fn rules(&self) -> &[String] {
        &self.rules
    }

    /// The fixed prefix for a namespace name, if the table has one.
    pub fn fixed_prefix(&self, uri: &str) -> Option<&str> {
        self.prefixes.get(uri).map(String::as_str)
    }

    /// True if the content of element `{uri}local` is collapsed.
    pub fn is_simple_element(&self, uri: &str, local: &str) -> bool {
        contains(&self.simple_elements, uri, local)
    }

    /// True if the value of attribute `{uri}local` is collapsed.
    pub fn is_simple_attribute(&self, uri: &str, local: &str) -> bool {
        contains(&self.simple_attributes, uri, local)
    }
}

fn contains(set: &BTreeSet<(String, String)>, uri: &str, local: &str) -> bool {
    // A borrowed lookup without allocating would need a custom Borrow impl; the sets are
    // small, so a range scan over the namespace is enough.
    set.range((uri.to_string(), String::new())..).take_while(|(u, _)| u == uri).any(|(_, l)| l == local)
}

/// `qif-norm@<n>` with `n` a positive decimal integer without leading zeros.
pub(crate) fn parse_version(v: &str) -> Option<u32> {
    let n = v.strip_prefix("qif-norm@")?;
    if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) || n.len() > 9 {
        return None;
    }
    n.parse().ok()
}

fn string_list(v: Option<&toml::Value>) -> Option<Vec<String>> {
    v?.as_array()?.iter().map(|x| x.as_str().map(str::to_string)).collect()
}

fn clark_set(v: Option<&toml::Value>) -> Option<BTreeSet<(String, String)>> {
    let mut out = BTreeSet::new();
    for name in string_list(v)? {
        let (uri, local) = match name.strip_prefix('{') {
            Some(rest) => {
                let (uri, local) = rest.split_once('}')?;
                if uri.is_empty() {
                    return None;
                }
                (uri.to_string(), local.to_string())
            }
            None => (String::new(), name.clone()),
        };
        if local.is_empty() || local.contains([':', '{', '}']) || !out.insert((uri, local)) {
            return None;
        }
    }
    Some(out)
}

/// A table prefix: ASCII letter or `_`, then ASCII letters, digits, `-`, `_` or `.`; not
/// starting with `xml` (reserved) and not of the form `n<digits>` (generated prefixes).
fn valid_prefix(p: &str) -> bool {
    let b = p.as_bytes();
    let Some(first) = b.first() else { return false };
    if !(first.is_ascii_alphabetic() || *first == b'_') {
        return false;
    }
    if !b.iter().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'-' | b'_' | b'.')) {
        return false;
    }
    if b.len() >= 3 && b[..3].eq_ignore_ascii_case(b"xml") {
        return false;
    }
    !is_generated_prefix(p)
}

/// `n` followed by one or more ASCII digits.
pub(crate) fn is_generated_prefix(p: &str) -> bool {
    p.strip_prefix('n').is_some_and(|d| !d.is_empty() && d.bytes().all(|c| c.is_ascii_digit()))
}
