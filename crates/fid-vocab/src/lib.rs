//! `fid-vocab`: the project's one vocabulary and claim lint engine (spec node
//! `ECO.RULES.VOCAB.ENGINE`). Repository tooling, not a product or registry crate; reads only
//! (R0) and is deterministic (D0: BTreeMap only, no clock).
//!
//! A versioned rule file ([`RuleSet`], format `fid-vocab-rules/1`) is applied to text units
//! ([`Unit`]) that adapters (product catalogue, outreach Markdown and HTML) supply. [`lint`]
//! returns a sorted [`Report`]; [`test_rules`] checks that every rule's examples behave.

pub mod rules;
pub mod text;
pub mod units;

pub use rules::{
    Example, Kind, Qualifier, Rule, RuleSet, Scope, Severity, DEFAULT_DESIGNATOR_PATTERN, MAX_REGEX_BYTES,
    MAX_REGEX_NEST,
};
pub use units::{parse_units, Suppression, Unit};

use fid_canon::CValue;
use std::collections::BTreeMap;
use std::fmt;
use std::io::Read;
use std::path::Path;
use text::{nfc, sentences, tokenize, Token};

/// The only rule-file format this engine reads.
pub const RULES_FORMAT: &str = "fid-vocab-rules/1";
/// Largest accepted rule file (and names file): 1 MiB.
pub const MAX_RULES_BYTES: usize = 1024 * 1024;
/// Most rules accepted in one rule file.
pub const MAX_RULES: usize = 1000;
/// Largest accepted unit stream: 50 MiB.
pub const MAX_UNITS_BYTES: usize = 50 * 1024 * 1024;

/// Errors; every one makes the command exit 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VocabError {
    /// The rule file (or a names file) is unreadable as TOML or breaks the format rules.
    RulesParse(String),
    /// The rule file declares a format other than [`RULES_FORMAT`].
    UnsupportedFormat(String),
    /// A unit line (1-based) cannot be parsed.
    UnitParse { line: usize, reason: String },
    /// An input is over its size limit.
    InputTooLarge { what: String, limit: usize },
    /// A file cannot be read.
    Io { path: String, reason: String },
}

impl VocabError {
    /// Stable machine-readable error code.
    pub fn code(&self) -> &'static str {
        match self {
            VocabError::RulesParse(_) => "rules_parse",
            VocabError::UnsupportedFormat(_) => "unsupported_format",
            VocabError::UnitParse { .. } => "unit_parse",
            VocabError::InputTooLarge { .. } => "input_too_large",
            VocabError::Io { .. } => "io",
        }
    }
}

impl fmt::Display for VocabError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            VocabError::RulesParse(m) => write!(f, "rule file: {m}"),
            VocabError::UnsupportedFormat(v) => {
                write!(f, "rule file: unsupported format {v:?} (this engine reads {RULES_FORMAT:?})")
            }
            VocabError::UnitParse { line, reason } => write!(f, "units line {line}: {reason}"),
            VocabError::InputTooLarge { what, limit } => write!(f, "{what} is larger than {limit} bytes"),
            VocabError::Io { path, reason } => write!(f, "cannot read {path}: {reason}"),
        }
    }
}

impl std::error::Error for VocabError {}

/// Reads at most `limit` bytes of `path`; a larger file is [`VocabError::InputTooLarge`].
pub fn read_limited(path: &Path, limit: usize, what: &str) -> Result<Vec<u8>, VocabError> {
    let io = |e: std::io::Error| VocabError::Io { path: path.display().to_string(), reason: e.to_string() };
    let file = std::fs::File::open(path).map_err(io)?;
    let mut buf = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    file.take(cap).read_to_end(&mut buf).map_err(io)?;
    if buf.len() > limit {
        return Err(VocabError::InputTooLarge { what: format!("{what} {}", path.display()), limit });
    }
    Ok(buf)
}

/// One finding.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Finding {
    pub unit: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
    /// Offset of the first matched word inside the unit, counted in Unicode scalar values
    /// (characters, not bytes) of the unit text after NFC normalisation.
    pub offset: u32,
    pub rule: String,
    pub severity: Severity,
    pub message: String,
    /// The matched words and phrases, as written in the unit, in text order.
    pub matched: Vec<String>,
    /// The preferred term (`preferred` rules only).
    pub preferred: Option<String>,
}

type SortKey<'a> = (&'a str, u32, u32, &'a str, &'a str, u32);

impl Finding {
    fn key(&self) -> SortKey<'_> {
        (self.file.as_str(), self.line, self.col, self.unit.as_str(), self.rule.as_str(), self.offset)
    }

    fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("unit".to_string(), CValue::Str(self.unit.clone()));
        o.insert("file".to_string(), CValue::Str(self.file.clone()));
        o.insert("line".to_string(), CValue::Num(f64::from(self.line)));
        o.insert("col".to_string(), CValue::Num(f64::from(self.col)));
        o.insert("offset".to_string(), CValue::Num(f64::from(self.offset)));
        o.insert("rule".to_string(), CValue::Str(self.rule.clone()));
        o.insert("severity".to_string(), CValue::Str(self.severity.as_str().to_string()));
        o.insert("message".to_string(), CValue::Str(self.message.clone()));
        o.insert("matched".to_string(), CValue::Arr(self.matched.iter().cloned().map(CValue::Str).collect()));
        if let Some(p) = &self.preferred {
            o.insert("preferred".to_string(), CValue::Str(p.clone()));
        }
        CValue::Obj(o)
    }
}

/// A suppression as listed in the report, with how many findings it silenced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SuppressionEntry {
    pub unit: String,
    pub file: String,
    pub line: u32,
    pub col: u32,
    pub rule: String,
    pub reason: String,
    pub suppressed: u32,
}

impl SuppressionEntry {
    fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("unit".to_string(), CValue::Str(self.unit.clone()));
        o.insert("file".to_string(), CValue::Str(self.file.clone()));
        o.insert("line".to_string(), CValue::Num(f64::from(self.line)));
        o.insert("col".to_string(), CValue::Num(f64::from(self.col)));
        o.insert("rule".to_string(), CValue::Str(self.rule.clone()));
        o.insert("reason".to_string(), CValue::Str(self.reason.clone()));
        o.insert("suppressed".to_string(), CValue::Num(f64::from(self.suppressed)));
        CValue::Obj(o)
    }
}

/// The lint report.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    pub rules_version: String,
    pub findings: Vec<Finding>,
    pub suppressions: Vec<SuppressionEntry>,
}

impl Report {
    /// Number of unsuppressed error-severity findings.
    pub fn error_count(&self) -> usize {
        self.findings.iter().filter(|f| f.severity == Severity::Error).count()
    }

    /// Exit code of `fid-vocab lint` for this report: 1 with any error finding, else 0.
    pub fn exit_code(&self) -> u8 {
        u8::from(self.error_count() > 0)
    }

    /// The report as a JSON value.
    pub fn to_cvalue(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("rules_version".to_string(), CValue::Str(self.rules_version.clone()));
        o.insert("findings".to_string(), CValue::Arr(self.findings.iter().map(Finding::to_cvalue).collect()));
        o.insert(
            "suppressions".to_string(),
            CValue::Arr(self.suppressions.iter().map(SuppressionEntry::to_cvalue).collect()),
        );
        CValue::Obj(o)
    }

    /// RFC 8785 canonical JSON bytes.
    pub fn to_json(&self) -> Result<Vec<u8>, fid_canon::CanonError> {
        fid_canon::to_canonical_bytes(&self.to_cvalue())
    }

    /// Human-readable report; the first line names the rule-file version.
    pub fn to_text(&self) -> String {
        let mut s = format!("fid-vocab: rules version {}\n", self.rules_version);
        for f in &self.findings {
            s.push_str(&format!(
                "{}:{}:{}: {} {} [{}] {} (matched: {})\n",
                f.file,
                f.line,
                f.col,
                f.severity.as_str(),
                f.rule,
                f.unit,
                f.message,
                f.matched.join(", ")
            ));
        }
        for x in &self.suppressions {
            s.push_str(&format!(
                "{}:{}:{}: suppressed {} [{}] x{}: {}\n",
                x.file, x.line, x.col, x.rule, x.unit, x.suppressed, x.reason
            ));
        }
        let warns = self.findings.len() - self.error_count();
        s.push_str(&format!(
            "{} error(s), {} warning(s), {} suppression(s)\n",
            self.error_count(),
            warns,
            self.suppressions.len()
        ));
        s
    }
}

/// A unit prepared for matching: NFC text, tokens, the character offset of each token start
/// (one forward pass) and sentence ranges.
struct Prepared {
    text: String,
    toks: Vec<Token>,
    tok_chars: Vec<u32>,
    sents: Vec<(usize, usize)>,
}

impl Prepared {
    fn new(raw: &str) -> Prepared {
        let text = nfc(raw);
        let toks = tokenize(&text);
        let mut tok_chars = Vec::with_capacity(toks.len());
        let mut chars = 0usize;
        let mut byte = 0usize;
        for t in &toks {
            chars += text[byte..t.start].chars().count();
            byte = t.start;
            tok_chars.push(u32::try_from(chars).unwrap_or(u32::MAX));
        }
        let sents = sentences(&text);
        Prepared { text, toks, tok_chars, sents }
    }

    /// Token index range lying inside the byte range `[s, e)` (binary search; tokens are sorted
    /// and never cross a sentence boundary).
    fn token_range(&self, s: usize, e: usize) -> (usize, usize) {
        let lo = self.toks.partition_point(|t| t.start < s);
        let hi = lo + self.toks[lo..].partition_point(|t| t.end <= e);
        (lo, hi)
    }
}

/// One raw hit of a rule: char offset of its first match and the matched words.
struct Hit {
    offset: u32,
    matched: Vec<String>,
}

fn eval_rule(rule: &Rule, id: &str, links: &[String], p: &Prepared) -> Vec<Hit> {
    let scopes: Vec<(usize, usize)> = match rule.scope {
        Scope::Unit => vec![(0, p.text.len())],
        Scope::Sentence => p.sents.clone(),
    };
    let link_ok = || links.iter().any(|l| rule.link_prefixes.iter().any(|pre| l.starts_with(pre.as_str())));
    // Evaluated at most once per unit, so sentence scope stays linear in the unit size.
    let mut nominative: Option<bool> = None;
    let mut is_nominative = || {
        *nominative.get_or_insert_with(|| {
            rule.allow_id_prefixes.iter().any(|pre| id.starts_with(pre.as_str()))
                || !rule.allow_phrases.find(&p.toks).is_empty()
        })
    };
    let mut hits = Vec::new();
    for (s, e) in scopes {
        let (lo, hi) = p.token_range(s, e);
        let toks = &p.toks[lo..hi];
        let mut spans = rule.terms.find(toks);
        if spans.is_empty() {
            continue;
        }
        let report = match rule.kind {
            Kind::Claim => {
                rule.qualifiers.is_empty()
                    || rule.qualifiers.iter().any(|q| match q {
                        Qualifier::Designator => !rule.designator.as_ref().is_some_and(|re| re.is_match(&p.text[s..e])),
                        Qualifier::Link => links.is_empty(),
                    })
            }
            Kind::Name => !is_nominative(),
            Kind::Cooccur => {
                let cmp = rule.comparison.find(toks);
                if cmp.is_empty() || link_ok() {
                    false
                } else {
                    spans.extend(cmp);
                    spans.sort();
                    true
                }
            }
            Kind::Preferred => true,
        };
        if !report {
            continue;
        }
        let matched: Vec<String> =
            spans.iter().map(|&(i, j)| p.text[toks[i].start..toks[j - 1].end].to_string()).collect();
        let offset = spans.first().map_or(0, |&(i, _)| p.tok_chars[lo + i]);
        hits.push(Hit { offset, matched });
    }
    hits
}

fn finding(rule: &Rule, unit: &Unit, hit: Hit) -> Finding {
    let message = match &rule.preferred {
        Some(pref) if rule.kind == Kind::Preferred => format!("{} (preferred term: {pref})", rule.message),
        _ => rule.message.clone(),
    };
    Finding {
        unit: unit.id.clone(),
        file: unit.file.clone(),
        line: unit.line,
        col: unit.col,
        offset: hit.offset,
        rule: rule.id.clone(),
        severity: rule.severity,
        message,
        matched: hit.matched,
        preferred: if rule.kind == Kind::Preferred { rule.preferred.clone() } else { None },
    }
}

/// Applies every rule to every unit, honours suppressions and returns the sorted report.
/// The result does not depend on the order of `units`.
pub fn lint(rules: &RuleSet, units: &[Unit]) -> Report {
    let mut findings = Vec::new();
    let mut suppressions = Vec::new();
    for unit in units {
        let p = Prepared::new(&unit.text);
        let mut silenced: BTreeMap<&str, u32> = BTreeMap::new();
        for rule in &rules.rules {
            let suppressed = unit.suppress.iter().any(|s| s.rule == rule.id);
            for hit in eval_rule(rule, &unit.id, &unit.links, &p) {
                if suppressed {
                    *silenced.entry(rule.id.as_str()).or_insert(0) += 1;
                } else {
                    findings.push(finding(rule, unit, hit));
                }
            }
        }
        for s in &unit.suppress {
            suppressions.push(SuppressionEntry {
                unit: unit.id.clone(),
                file: unit.file.clone(),
                line: unit.line,
                col: unit.col,
                rule: s.rule.clone(),
                reason: s.reason.clone(),
                suppressed: silenced.get(s.rule.as_str()).copied().unwrap_or(0),
            });
        }
    }
    findings.sort_by(|a, b| a.key().cmp(&b.key()).then_with(|| a.matched.cmp(&b.matched)));
    suppressions.sort_by(|a, b| {
        (&a.file, a.line, a.col, &a.unit, &a.rule, &a.reason)
            .cmp(&(&b.file, b.line, b.col, &b.unit, &b.rule, &b.reason))
    });
    Report { rules_version: rules.version.clone(), findings, suppressions }
}

/// Findings of one rule on one example.
fn example_hits(rule: &Rule, ex: &Example) -> usize {
    eval_rule(rule, &ex.id, &ex.links, &Prepared::new(&ex.text)).len()
}

/// Checks every rule's examples (REQ-ECO.RULES.VOCAB.ENGINE-007). Returns one problem per
/// line, each naming the rule; empty when all examples behave.
pub fn test_rules(rules: &RuleSet) -> Vec<String> {
    let mut problems = Vec::new();
    for rule in &rules.rules {
        if rule.pass.is_empty() {
            problems.push(format!("rule {}: no pass example", rule.id));
        }
        if rule.fail.is_empty() {
            problems.push(format!("rule {}: no fail example", rule.id));
        }
        for (i, ex) in rule.pass.iter().enumerate() {
            if example_hits(rule, ex) > 0 {
                problems.push(format!("rule {}: pass example {} yields a finding: {:?}", rule.id, i + 1, ex.text));
            }
        }
        for (i, ex) in rule.fail.iter().enumerate() {
            if example_hits(rule, ex) == 0 {
                problems.push(format!("rule {}: fail example {} yields no finding: {:?}", rule.id, i + 1, ex.text));
            }
        }
    }
    problems
}
