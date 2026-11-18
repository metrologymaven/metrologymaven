//! Rule file format `fid-vocab-rules/1` (REQ-ECO.RULES.VOCAB.ENGINE-001).

use crate::text::Matcher;
use crate::{read_limited, VocabError, MAX_RULES_BYTES, RULES_FORMAT};
use regex::Regex;
use std::collections::BTreeSet;
use std::path::Path;

/// Default designator pattern: an organisation prefix (ISO, ASME, VDA, IEC, DIN or EN, possibly
/// stacked such as "DIN EN ISO"), a number with optional parts, and optionally an edition year.
pub const DEFAULT_DESIGNATOR_PATTERN: &str =
    r"\b(?:ISO|ASME|VDA|IEC|DIN|EN)(?:[ /][A-Z]{1,4})*\s?[A-Z]?\d+(?:[.\-]\d+)*(?:[:\-](?:19|20)\d{2})?\b";

/// Rule kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// A word form needs the rule's qualifiers in the same unit or sentence.
    Claim,
    /// A listed name is allowed only in nominative contexts.
    Name,
    /// A name plus a comparison word needs an allowed link.
    Cooccur,
    /// A non-preferred term; the finding names the preferred one.
    Preferred,
}

impl Kind {
    fn parse(s: &str) -> Option<Kind> {
        match s {
            "claim" => Some(Kind::Claim),
            "name" => Some(Kind::Name),
            "cooccur" => Some(Kind::Cooccur),
            "preferred" => Some(Kind::Preferred),
            _ => None,
        }
    }

    fn fields(self) -> &'static [&'static str] {
        match self {
            Kind::Claim => &["words", "phrases", "qualifiers", "designator_pattern"],
            Kind::Name => &["words", "phrases", "names_file", "allow_id_prefixes", "allow_phrases"],
            Kind::Cooccur => &["words", "phrases", "names_file", "comparison_words", "link_prefixes"],
            Kind::Preferred => &["words", "phrases", "preferred"],
        }
    }
}

/// Finding severity; only `Error` findings make `lint` exit 1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warn,
}

impl Severity {
    /// The rule-file and report spelling.
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Error => "error",
            Severity::Warn => "warn",
        }
    }
}

/// Where a rule's predicate is evaluated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scope {
    Unit,
    Sentence,
}

/// A required qualifier of a `claim` rule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Qualifier {
    /// The scope text matches the rule's designator pattern.
    Designator,
    /// The unit carries at least one link.
    Link,
}

/// One pass or fail example of a rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Example {
    pub id: String,
    pub text: String,
    pub links: Vec<String>,
}

/// A parsed rule.
#[derive(Debug, Clone)]
pub struct Rule {
    pub id: String,
    pub kind: Kind,
    pub severity: Severity,
    pub message: String,
    pub scope: Scope,
    pub qualifiers: Vec<Qualifier>,
    pub allow_id_prefixes: Vec<String>,
    pub link_prefixes: Vec<String>,
    pub preferred: Option<String>,
    pub pass: Vec<Example>,
    pub fail: Vec<Example>,
    pub(crate) terms: Matcher,
    pub(crate) allow_phrases: Matcher,
    pub(crate) comparison: Matcher,
    pub(crate) designator: Option<Regex>,
}

/// A parsed rule file, rules sorted by ID.
#[derive(Debug, Clone)]
pub struct RuleSet {
    pub version: String,
    pub rules: Vec<Rule>,
}

fn perr(msg: impl Into<String>) -> VocabError {
    VocabError::RulesParse(msg.into())
}

fn get_str(t: &toml::Table, key: &str, ctx: &str) -> Result<Option<String>, VocabError> {
    match t.get(key) {
        None => Ok(None),
        Some(toml::Value::String(s)) => Ok(Some(s.clone())),
        Some(_) => Err(perr(format!("{ctx}: `{key}` must be a string"))),
    }
}

fn req_str(t: &toml::Table, key: &str, ctx: &str) -> Result<String, VocabError> {
    match get_str(t, key, ctx)? {
        Some(s) if !s.trim().is_empty() => Ok(s),
        Some(_) => Err(perr(format!("{ctx}: `{key}` must not be empty"))),
        None => Err(perr(format!("{ctx}: missing `{key}`"))),
    }
}

fn get_list(t: &toml::Table, key: &str, ctx: &str) -> Result<Vec<String>, VocabError> {
    match t.get(key) {
        None => Ok(Vec::new()),
        Some(toml::Value::Array(a)) => a
            .iter()
            .map(|v| match v {
                toml::Value::String(s) if !s.trim().is_empty() => Ok(s.clone()),
                _ => Err(perr(format!("{ctx}: `{key}` must be a list of non-empty strings"))),
            })
            .collect(),
        Some(_) => Err(perr(format!("{ctx}: `{key}` must be a list of strings"))),
    }
}

fn matcher(list: &[String], key: &str, ctx: &str) -> Result<Matcher, VocabError> {
    Matcher::new(list).map_err(|p| perr(format!("{ctx}: `{key}` entry {p:?} has no word characters")))
}

fn parse_examples(v: &toml::Value, ctx: &str) -> Result<Vec<Example>, VocabError> {
    let toml::Value::Array(items) = v else {
        return Err(perr(format!("{ctx}: must be a list")));
    };
    let mut out = Vec::new();
    for (i, item) in items.iter().enumerate() {
        let ictx = format!("{ctx}[{i}]");
        match item {
            toml::Value::String(s) => out.push(Example { id: "example".into(), text: s.clone(), links: Vec::new() }),
            toml::Value::Table(t) => {
                if let Some(k) = t.keys().find(|k| !matches!(k.as_str(), "text" | "id" | "links")) {
                    return Err(perr(format!("{ictx}: unknown key `{k}`")));
                }
                let text = req_str(t, "text", &ictx)?;
                let id = get_str(t, "id", &ictx)?.unwrap_or_else(|| "example".into());
                let links = get_list(t, "links", &ictx)?;
                out.push(Example { id, text, links });
            }
            _ => return Err(perr(format!("{ictx}: must be a string or a table"))),
        }
    }
    Ok(out)
}

fn read_names(base: Option<&Path>, rel: &str, ctx: &str) -> Result<Vec<String>, VocabError> {
    let Some(base) = base else {
        return Err(perr(format!("{ctx}: `names_file` needs a rule file on disk")));
    };
    let path = base.join(rel);
    let bytes = read_limited(&path, MAX_RULES_BYTES, "names file")?;
    let text = String::from_utf8(bytes).map_err(|_| perr(format!("{ctx}: names file {rel} is not UTF-8")))?;
    Ok(text.lines().map(str::trim).filter(|l| !l.is_empty() && !l.starts_with('#')).map(str::to_string).collect())
}

fn parse_rule(t: &toml::Table, base: Option<&Path>, index: usize) -> Result<Rule, VocabError> {
    let pre = format!("rule[{index}]");
    let id = req_str(t, "id", &pre)?;
    let ctx = format!("rule {id}");
    let kind_s = req_str(t, "kind", &ctx)?;
    let kind = Kind::parse(&kind_s)
        .ok_or_else(|| perr(format!("{ctx}: unknown kind {kind_s:?} (expected claim, name, cooccur or preferred)")))?;
    let common = ["id", "kind", "severity", "message", "scope", "examples"];
    if let Some(k) = t.keys().find(|k| !common.contains(&k.as_str()) && !kind.fields().contains(&k.as_str())) {
        return Err(perr(format!("{ctx}: key `{k}` is not allowed for kind {kind_s}")));
    }
    let severity = match req_str(t, "severity", &ctx)?.as_str() {
        "error" => Severity::Error,
        "warn" => Severity::Warn,
        other => return Err(perr(format!("{ctx}: severity {other:?} must be error or warn"))),
    };
    let message = req_str(t, "message", &ctx)?;
    let scope = match get_str(t, "scope", &ctx)?.as_deref() {
        None if kind == Kind::Cooccur => Scope::Sentence,
        None | Some("unit") => Scope::Unit,
        Some("sentence") => Scope::Sentence,
        Some(other) => return Err(perr(format!("{ctx}: scope {other:?} must be unit or sentence"))),
    };

    let mut terms = get_list(t, "words", &ctx)?;
    terms.extend(get_list(t, "phrases", &ctx)?);
    if let Some(rel) = get_str(t, "names_file", &ctx)? {
        terms.extend(read_names(base, &rel, &ctx)?);
    }
    if terms.is_empty() {
        let what = match kind {
            Kind::Name | Kind::Cooccur => "names (`words`, `phrases` or `names_file`)",
            _ => "`words` or `phrases`",
        };
        return Err(perr(format!("{ctx}: needs at least one entry in {what}")));
    }
    let terms = matcher(&terms, "words", &ctx)?;

    let mut qualifiers = BTreeSet::new();
    for q in get_list(t, "qualifiers", &ctx)? {
        match q.as_str() {
            "designator" => qualifiers.insert(Qualifier::Designator),
            "link" => qualifiers.insert(Qualifier::Link),
            other => return Err(perr(format!("{ctx}: qualifier {other:?} must be designator or link"))),
        };
    }
    let designator = if qualifiers.contains(&Qualifier::Designator) {
        let pat = get_str(t, "designator_pattern", &ctx)?.unwrap_or_else(|| DEFAULT_DESIGNATOR_PATTERN.to_string());
        Some(Regex::new(&pat).map_err(|e| perr(format!("{ctx}: designator_pattern: {e}")))?)
    } else {
        None
    };

    let comparison_list = get_list(t, "comparison_words", &ctx)?;
    if kind == Kind::Cooccur && comparison_list.is_empty() {
        return Err(perr(format!("{ctx}: needs at least one entry in `comparison_words`")));
    }
    let preferred = get_str(t, "preferred", &ctx)?;
    let has_preferred = matches!(preferred.as_deref(), Some(p) if !p.trim().is_empty());
    if kind == Kind::Preferred && !has_preferred {
        return Err(perr(format!("{ctx}: needs a non-empty `preferred` term")));
    }

    let (mut pass, mut fail) = (Vec::new(), Vec::new());
    match t.get("examples") {
        None => {}
        Some(toml::Value::Table(ex)) => {
            if let Some(k) = ex.keys().find(|k| !matches!(k.as_str(), "pass" | "fail")) {
                return Err(perr(format!("{ctx}: examples: unknown key `{k}`")));
            }
            if let Some(v) = ex.get("pass") {
                pass = parse_examples(v, &format!("{ctx}: examples.pass"))?;
            }
            if let Some(v) = ex.get("fail") {
                fail = parse_examples(v, &format!("{ctx}: examples.fail"))?;
            }
        }
        Some(_) => return Err(perr(format!("{ctx}: `examples` must be a table"))),
    }

    Ok(Rule {
        id,
        kind,
        severity,
        message,
        scope,
        qualifiers: qualifiers.into_iter().collect(),
        allow_id_prefixes: get_list(t, "allow_id_prefixes", &ctx)?,
        link_prefixes: get_list(t, "link_prefixes", &ctx)?,
        preferred,
        pass,
        fail,
        terms,
        allow_phrases: matcher(&get_list(t, "allow_phrases", &ctx)?, "allow_phrases", &ctx)?,
        comparison: matcher(&comparison_list, "comparison_words", &ctx)?,
        designator,
    })
}

impl RuleSet {
    /// Parses a rule file. `base` is the directory that `names_file` paths are relative to.
    /// Missing examples are accepted here and reported by [`crate::test_rules`].
    pub fn parse(text: &str, base: Option<&Path>) -> Result<RuleSet, VocabError> {
        if text.len() > MAX_RULES_BYTES {
            return Err(VocabError::InputTooLarge { what: "rule file".into(), limit: MAX_RULES_BYTES });
        }
        let table: toml::Table =
            text.parse().map_err(|e: toml::de::Error| perr(format!("TOML: {}", e.to_string().trim_end())))?;
        match get_str(&table, "format", "rule file")? {
            Some(f) if f == RULES_FORMAT => {}
            Some(f) => return Err(VocabError::UnsupportedFormat(f)),
            None => return Err(perr(format!("rule file: missing `format` (expected {RULES_FORMAT:?})"))),
        }
        if let Some(k) = table.keys().find(|k| !matches!(k.as_str(), "format" | "version" | "rule")) {
            return Err(perr(format!("rule file: unknown key `{k}`")));
        }
        let version = req_str(&table, "version", "rule file")?;
        let items = match table.get("rule") {
            None => Vec::new(),
            Some(toml::Value::Array(a)) => a.clone(),
            Some(_) => return Err(perr("rule file: `rule` must be an array of tables ([[rule]])")),
        };
        let mut rules: Vec<Rule> = Vec::new();
        let mut seen = BTreeSet::new();
        for (i, item) in items.iter().enumerate() {
            let toml::Value::Table(t) = item else {
                return Err(perr(format!("rule[{i}]: must be a table")));
            };
            let rule = parse_rule(t, base, i)?;
            if !seen.insert(rule.id.clone()) {
                return Err(perr(format!("rule {}: duplicate rule ID", rule.id)));
            }
            rules.push(rule);
        }
        if rules.is_empty() {
            return Err(perr("rule file: no [[rule]] tables"));
        }
        rules.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(RuleSet { version, rules })
    }

    /// Reads and parses a rule file from disk (at most 1 MiB).
    pub fn load(path: &Path) -> Result<RuleSet, VocabError> {
        let bytes = read_limited(path, MAX_RULES_BYTES, "rule file")?;
        let text = String::from_utf8(bytes).map_err(|_| perr(format!("{}: not UTF-8", path.display())))?;
        RuleSet::parse(&text, path.parent())
    }
}
