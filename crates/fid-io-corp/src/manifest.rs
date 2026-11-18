//! The per-format corpus manifest `corpus/<format-id>.toml` and the registry listing.

use crate::error::CorpError;
use fid_canon::CValue;
use fid_types::Digest;
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Longest accepted format ID, in bytes.
const MAX_FORMAT_ID: usize = 64;

/// A registry format ID such as `mesh.stl`: lowercase ASCII letters, digits, `.`, `_`, `-`,
/// starting with a letter or digit and never containing `..`. It doubles as a file stem.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FormatId(String);

impl FormatId {
    /// Parses a format ID; `None` if it breaks the rules above.
    pub fn parse(s: &str) -> Option<FormatId> {
        let b = s.as_bytes();
        let first_ok = b.first().is_some_and(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        let ok = first_ok
            && b.len() <= MAX_FORMAT_ID
            && b.iter().all(|&c| c.is_ascii_lowercase() || c.is_ascii_digit() || matches!(c, b'.' | b'_' | b'-'))
            && !s.contains("..");
        ok.then(|| FormatId(s.to_string()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for FormatId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Where an entry's bytes live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Location {
    /// Stored in the repository, relative to the corpus directory (`/`-separated).
    Path(String),
    /// Referenced by URL and fetched in CI (`https://` or `http://` only).
    Url(String),
}

impl Location {
    /// The path or URL text, used to name the entry in findings.
    pub fn as_str(&self) -> &str {
        match self {
            Location::Path(s) | Location::Url(s) => s,
        }
    }
}

/// The expected outcome of importing an entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Expect {
    /// Accepted, with expected entity counts by kind.
    Accept { counts: BTreeMap<String, u64> },
    /// Rejected with this typed error code.
    Reject { error: String },
}

/// One corpus file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub location: Location,
    pub source: String,
    /// An SPDX licence ID, or `unknown`.
    pub licence: String,
    pub sha256: Digest,
    pub expect: Expect,
    pub tags: Vec<String>,
}

/// A parsed `corpus/<format-id>.toml`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Manifest {
    pub format: FormatId,
    pub entries: Vec<Entry>,
}

fn is_spdx_id(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_alphanumeric() || matches!(c, b'.' | b'-' | b'+'))
}

fn is_code(s: &str) -> bool {
    !s.is_empty() && s.len() <= 64 && s.bytes().all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
}

/// A safe relative path: `/`-separated, no empty, `.` or `..` segments, no backslash or colon.
fn is_safe_path(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 512
        && !s.contains(['\\', ':', '\0'])
        && s.split('/').all(|seg| !seg.is_empty() && seg != "." && seg != "..")
}

type Table = toml::Table;

fn only_keys(t: &Table, allowed: &[&str], at: &str) -> Result<(), CorpError> {
    match t.keys().find(|k| !allowed.contains(&k.as_str())) {
        Some(k) => Err(CorpError::malformed(at, format!("unknown key {k:?}"))),
        None => Ok(()),
    }
}

fn string<'a>(t: &'a Table, key: &str, at: &str) -> Result<&'a str, CorpError> {
    match t.get(key) {
        Some(toml::Value::String(s)) => Ok(s),
        Some(_) => Err(CorpError::malformed(at, format!("{key} must be a string"))),
        None => Err(CorpError::malformed(at, format!("missing {key}"))),
    }
}

fn table<'a>(v: &'a toml::Value, what: &str, at: &str) -> Result<&'a Table, CorpError> {
    match v {
        toml::Value::Table(t) => Ok(t),
        _ => Err(CorpError::malformed(at, format!("{what} must be a table"))),
    }
}

fn parse_expect(v: &toml::Value, at: &str) -> Result<Expect, CorpError> {
    let t = table(v, "expect", at)?;
    only_keys(t, &["accept", "reject"], at)?;
    match (t.get("accept"), t.get("reject")) {
        (Some(a), None) => {
            let a = table(a, "expect.accept", at)?;
            only_keys(a, &["counts"], at)?;
            let mut counts = BTreeMap::new();
            if let Some(c) = a.get("counts") {
                for (k, n) in table(c, "expect.accept.counts", at)? {
                    let n = match n {
                        toml::Value::Integer(n) => u64::try_from(*n).ok(),
                        _ => None,
                    };
                    let Some(n) = n.filter(|_| is_code(k)) else {
                        return Err(CorpError::malformed(at, format!("bad count {k:?}")));
                    };
                    counts.insert(k.clone(), n);
                }
            }
            Ok(Expect::Accept { counts })
        }
        (None, Some(r)) => {
            let r = table(r, "expect.reject", at)?;
            only_keys(r, &["error"], at)?;
            let error = string(r, "error", at)?;
            if !is_code(error) {
                return Err(CorpError::malformed(at, format!("bad error code {error:?}")));
            }
            Ok(Expect::Reject { error: error.to_string() })
        }
        _ => Err(CorpError::malformed(at, "expect needs exactly one of accept or reject")),
    }
}

fn parse_entry(t: &Table, at: &str) -> Result<Entry, CorpError> {
    only_keys(t, &["path", "url", "source", "licence", "sha256", "expect", "tags"], at)?;
    let location = match (t.contains_key("path"), t.contains_key("url")) {
        (true, false) => {
            let p = string(t, "path", at)?;
            if !is_safe_path(p) {
                return Err(CorpError::malformed(at, format!("unsafe path {p:?}")));
            }
            Location::Path(p.to_string())
        }
        (false, true) => {
            let u = string(t, "url", at)?;
            if !crate::fetch::is_fetchable_url(u) {
                return Err(CorpError::malformed(at, format!("bad url {u:?}")));
            }
            Location::Url(u.to_string())
        }
        _ => return Err(CorpError::malformed(at, "entry needs exactly one of path or url")),
    };
    let at = format!("{at} ({})", location.as_str());
    let at = at.as_str();
    let source = string(t, "source", at)?;
    if source.trim().is_empty() {
        return Err(CorpError::malformed(at, "empty source"));
    }
    let licence = string(t, "licence", at)?;
    if !is_spdx_id(licence) {
        return Err(CorpError::malformed(at, format!("licence {licence:?} is neither an SPDX ID nor `unknown`")));
    }
    let sha256 = string(t, "sha256", at)?;
    let sha256 = Digest::parse(sha256)
        .map_err(|_| CorpError::malformed(at, format!("sha256 {sha256:?} is not sha256:<hex>")))?;
    let expect = parse_expect(t.get("expect").ok_or_else(|| CorpError::malformed(at, "missing expect"))?, at)?;
    let mut tags = Vec::new();
    match t.get("tags") {
        None => {}
        Some(toml::Value::Array(items)) => {
            for item in items {
                match item {
                    toml::Value::String(s) if is_code(&s.replace('-', "_")) => tags.push(s.clone()),
                    _ => return Err(CorpError::malformed(at, "tags must be lowercase identifiers")),
                }
            }
        }
        Some(_) => return Err(CorpError::malformed(at, "tags must be an array")),
    }
    Ok(Entry { location, source: source.to_string(), licence: licence.to_string(), sha256, expect, tags })
}

impl Manifest {
    /// Parses a manifest. `file_stem` is the file name without `.toml`; the `format` key must equal it.
    pub fn parse(text: &str, file_stem: &str) -> Result<Manifest, CorpError> {
        let at = format!("{file_stem}.toml");
        let root: Table = text.parse().map_err(|e: toml::de::Error| CorpError::malformed(&at, e.message()))?;
        only_keys(&root, &["format", "entry"], &at)?;
        let format = string(&root, "format", &at)?;
        let format = FormatId::parse(format).filter(|f| f.as_str() == file_stem).ok_or_else(|| {
            CorpError::malformed(&at, format!("format {format:?} must be a format ID equal to the file stem"))
        })?;
        let mut entries = Vec::new();
        match root.get("entry") {
            None => {}
            Some(toml::Value::Array(items)) => {
                for (i, item) in items.iter().enumerate() {
                    let at = format!("{at} entry {}", i + 1);
                    entries.push(parse_entry(table(item, "entry", &at)?, &at)?);
                }
            }
            Some(_) => return Err(CorpError::malformed(&at, "entry must be an array of tables ([[entry]])")),
        }
        Ok(Manifest { format, entries })
    }
}

/// Reads the first-party format IDs from `import.list_formats@1` output JSON.
///
/// Accepts the result envelope (`{"result": [...]}`), `{"formats": [...]}` or a bare array.
/// Items are descriptor objects with `id` (and optional `origin`) or bare ID strings. Items whose
/// `origin` is present and not `first_party` are skipped. The result is sorted and deduplicated.
pub fn parse_format_listing(json: &[u8]) -> Result<Vec<FormatId>, CorpError> {
    const AT: &str = "format listing";
    let value = fid_canon::parse(json).map_err(|e| CorpError::malformed(AT, format!("JSON: {}", e.code())))?;
    let items = match &value {
        CValue::Arr(a) => a,
        CValue::Obj(o) => match o.get("result").or_else(|| o.get("formats")) {
            Some(CValue::Arr(a)) => a,
            _ => return Err(CorpError::malformed(AT, "object without a result or formats array")),
        },
        _ => return Err(CorpError::malformed(AT, "expected an array or an object")),
    };
    let mut out = BTreeSet::new();
    for item in items {
        let id = match item {
            CValue::Str(s) => s,
            CValue::Obj(o) => {
                match o.get("origin") {
                    None => {}
                    Some(CValue::Str(origin)) if origin == "first_party" => {}
                    Some(CValue::Str(_)) => continue,
                    Some(_) => return Err(CorpError::malformed(AT, "origin must be a string")),
                }
                match o.get("id") {
                    Some(CValue::Str(s)) => s,
                    _ => return Err(CorpError::malformed(AT, "descriptor without a string id")),
                }
            }
            _ => return Err(CorpError::malformed(AT, "items must be descriptors or strings")),
        };
        let id = FormatId::parse(id).ok_or_else(|| CorpError::malformed(AT, format!("bad format ID {id:?}")))?;
        out.insert(id);
    }
    Ok(out.into_iter().collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    const D: &str = "sha256:0000000000000000000000000000000000000000000000000000000000000001";

    fn one(entry: &str) -> Result<Manifest, CorpError> {
        Manifest::parse(&format!("format = \"mesh.stl\"\n[[entry]]\n{entry}\n"), "mesh.stl")
    }

    #[test]
    fn format_ids() {
        for ok in ["mesh.stl", "qif-xml", "e57", "a_b.c"] {
            assert!(FormatId::parse(ok).is_some(), "{ok}");
        }
        for bad in ["", "Mesh.stl", ".stl", "a..b", "a/b", "a\\b", "a b", &"a".repeat(65)] {
            assert!(FormatId::parse(bad).is_none(), "{bad}");
        }
    }

    #[test]
    fn full_entry_parses() {
        let m = one(&format!(
            "path = \"files/a.stl\"\nsource = \"s\"\nlicence = \"CC0-1.0\"\nsha256 = \"{D}\"\n\
             expect = {{ accept = {{ counts = {{ facets = 4 }} }} }}\ntags = [\"p0-subset\", \"hostile\"]"
        ))
        .unwrap();
        assert_eq!(m.format.as_str(), "mesh.stl");
        let e = &m.entries[0];
        assert_eq!(e.location, Location::Path("files/a.stl".into()));
        assert_eq!(e.expect, Expect::Accept { counts: [("facets".to_string(), 4)].into_iter().collect() });
        assert_eq!(e.tags, ["p0-subset", "hostile"]);
        let r = one(&format!(
            "url = \"https://x.invalid/a\"\nsource = \"s\"\nlicence = \"unknown\"\nsha256 = \"{D}\"\n\
             expect = {{ reject = {{ error = \"truncated_file\" }} }}"
        ))
        .unwrap();
        assert_eq!(r.entries[0].expect, Expect::Reject { error: "truncated_file".into() });
        assert!(r.entries[0].tags.is_empty());
    }

    #[test]
    fn malformed_entries() {
        let base =
            format!("source = \"s\"\nlicence = \"MIT\"\nsha256 = \"{D}\"\nexpect = {{ reject = {{ error = \"x\" }} }}");
        for bad in [
            base.clone(),
            format!("path = \"a\"\nurl = \"https://x/a\"\n{base}"),
            format!("path = \"../a\"\n{base}"),
            format!("path = \"/a\"\n{base}"),
            format!("path = \"C:/a\"\n{base}"),
            format!("path = \"a\\\\b\"\n{base}"),
            format!("url = \"ftp://x/a\"\n{base}"),
            format!("url = \"file:///etc/passwd\"\n{base}"),
            format!("url = \"file:////host/share/x\"\n{base}"),
            format!("url = \"https://x/[1-2]\"\n{base}"),
            format!("path = \"a\"\n{base}\nextra = 1"),
            format!("path = \"a\"\n{}", base.replace("MIT", "MIT OR Apache-2.0")),
            format!("path = \"a\"\n{}", base.replace(D, "sha256:ABC")),
            format!("path = \"a\"\n{}", base.replace("reject = { error = \"x\" }", "")),
            format!("path = \"a\"\n{}", base.replace("{ error = \"x\" }", "{ error = \"x\" }, accept = {}")),
            format!("path = \"a\"\n{}", base.replace("reject = { error = \"x\" }", "accept = { counts = { f = -1 } }")),
            format!("path = \"a\"\n{base}\ntags = \"x\""),
        ] {
            assert_eq!(one(&bad).map_err(|e| e.code()), Err("malformed"), "{bad}");
        }
        assert_eq!(Manifest::parse("format = \"mesh.ply\"", "mesh.stl").map_err(|e| e.code()), Err("malformed"));
        assert_eq!(Manifest::parse("format = ", "mesh.stl").map_err(|e| e.code()), Err("malformed"));
        assert_eq!(
            Manifest::parse("format = \"mesh.stl\"\nentry = 1", "mesh.stl").map_err(|e| e.code()),
            Err("malformed")
        );
    }

    #[test]
    fn listings() {
        let ids =
            |s: &str| parse_format_listing(s.as_bytes()).map(|v| v.iter().map(|f| f.to_string()).collect::<Vec<_>>());
        assert_eq!(ids(r#"["b","a","a"]"#).unwrap(), ["a", "b"]);
        assert_eq!(
            ids(r#"{"result":[{"id":"x","origin":"plugin"},{"id":"y","origin":"first_party"},{"id":"z"}]}"#).unwrap(),
            ["y", "z"]
        );
        assert_eq!(ids(r#"{"formats":["m"]}"#).unwrap(), ["m"]);
        for bad in
            [r#"{}"#, r#"1"#, r#"[1]"#, r#"[{"origin":"first_party"}]"#, r#"["A"]"#, r#"[{"id":"a","origin":1}]"#, "["]
        {
            assert_eq!(ids(bad).map_err(|e| e.code()), Err("malformed"), "{bad}");
        }
    }
}
