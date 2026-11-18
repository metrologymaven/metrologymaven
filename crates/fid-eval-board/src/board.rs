//! The part of a scoreboard bundle's `board.json` the claim check reads.
//!
//! `board.json` v1 is owned by spec node AGENT.BOARD.BUNDLE; this reader takes only what a
//! claim needs and ignores every other field:
//!
//! ```json
//! {"release": "r2026.1",
//!  "verdict": {"outcome": "pass"},
//!  "models": [{"model": "open-model-a",
//!              "by_task":   {"<task>":   {<figures>}},
//!              "by_family": {"<family>": {<figures>}},
//!              "by_app":    {"<app>":    {<figures>}}}]}
//! ```
//!
//! Figures are whole-number counts: `runs`, `successes`, `adoptable`, `human_reviewed`,
//! `human_adopted`, `violations` (each optional). Rates are formed from counts, so they are
//! exact: success = successes / runs, adoptability = adoptable / runs, human adoption =
//! human_adopted / human_reviewed.

use crate::register::{as_obj, MAX_DEPTH, MAX_INPUT_BYTES};
use crate::{Code, Finding, Fraction, Metric, ScopeKind};
use fid_canon::CValue;
use std::collections::BTreeMap;

/// The release's red-team verdict outcome (AGENT.REDTEAM.VERDICT).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    Pass,
    Block,
    Incomplete,
}

impl Outcome {
    pub fn parse(s: &str) -> Option<Outcome> {
        Some(match s {
            "pass" => Outcome::Pass,
            "block" => Outcome::Block,
            "incomplete" => Outcome::Incomplete,
            _ => return None,
        })
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Outcome::Pass => "pass",
            Outcome::Block => "block",
            Outcome::Incomplete => "incomplete",
        }
    }
}

/// The counts published for one task, family or app of one model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Figures {
    pub runs: Option<u64>,
    pub successes: Option<u64>,
    pub adoptable: Option<u64>,
    pub human_reviewed: Option<u64>,
    pub human_adopted: Option<u64>,
    pub violations: Option<u64>,
}

impl Figures {
    /// The exact figure for `metric`, or a reason it is unavailable.
    pub fn figure(&self, metric: Metric) -> Result<Fraction, String> {
        let rate = |num: Option<u64>, nn: &str, den: Option<u64>, dn: &str| -> Result<Fraction, String> {
            let n = num.ok_or_else(|| format!("board entry has no `{nn}`"))?;
            let d = den.ok_or_else(|| format!("board entry has no `{dn}`"))?;
            if n > d {
                return Err(format!("board entry has `{nn}` {n} above `{dn}` {d}"));
            }
            Fraction::new(n, d).ok_or_else(|| format!("board entry has `{dn}` 0"))
        };
        match metric {
            Metric::Success => rate(self.successes, "successes", self.runs, "runs"),
            Metric::Adoptability => rate(self.adoptable, "adoptable", self.runs, "runs"),
            Metric::HumanAdoption => rate(self.human_adopted, "human_adopted", self.human_reviewed, "human_reviewed"),
            Metric::Violations => self
                .violations
                .and_then(|v| Fraction::new(v, 1))
                .ok_or_else(|| "board entry has no `violations`".to_string()),
        }
    }
}

/// One model of the board.
#[derive(Debug, Clone, Default)]
pub struct ModelEntry {
    pub by_task: BTreeMap<String, Figures>,
    pub by_family: BTreeMap<String, Figures>,
    pub by_app: BTreeMap<String, Figures>,
}

impl ModelEntry {
    pub fn scope(&self, kind: ScopeKind) -> &BTreeMap<String, Figures> {
        match kind {
            ScopeKind::Task => &self.by_task,
            ScopeKind::Family => &self.by_family,
            ScopeKind::App => &self.by_app,
        }
    }
}

/// The parsed board.
#[derive(Debug, Clone)]
pub struct Board {
    pub file: String,
    pub release: String,
    pub verdict: Outcome,
    pub models: BTreeMap<String, ModelEntry>,
}

const COUNT_FIELDS: [&str; 6] = ["adoptable", "human_adopted", "human_reviewed", "runs", "successes", "violations"];

/// Parses `board.json` `bytes`; `file` is the path shown in findings.
pub fn parse_board(bytes: &[u8], file: &str) -> Result<Board, Vec<Finding>> {
    let err = |d: String| Finding::new(Code::InvalidBoard, None, file, 0, d);
    if bytes.len() > MAX_INPUT_BYTES {
        return Err(vec![err("file larger than 16 MiB".into())]);
    }
    let v = fid_canon::parse_with_limits(bytes, MAX_INPUT_BYTES, MAX_DEPTH)
        .map_err(|e| vec![err(format!("not valid JSON: {e}"))])?;
    let Some(top) = as_obj(&v) else {
        return Err(vec![err("top level is not an object".into())]);
    };
    let mut errs = Vec::new();
    let release = match top.get("release") {
        Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
        _ => {
            errs.push(err("release must be a non-empty string".into()));
            String::new()
        }
    };
    let verdict = match top.get("verdict").and_then(as_obj).and_then(|m| m.get("outcome")) {
        Some(CValue::Str(s)) => Outcome::parse(s),
        _ => None,
    };
    if verdict.is_none() {
        errs.push(err("verdict.outcome must be pass, block or incomplete".into()));
    }
    let mut models = BTreeMap::new();
    match top.get("models") {
        Some(CValue::Arr(items)) => {
            for (i, item) in items.iter().enumerate() {
                let at = format!("models[{i}]");
                let Some(m) = as_obj(item) else {
                    errs.push(err(format!("{at}: not an object")));
                    continue;
                };
                let name = match m.get("model") {
                    Some(CValue::Str(s)) if !s.is_empty() => s.clone(),
                    _ => {
                        errs.push(err(format!("{at}.model must be a non-empty string")));
                        continue;
                    }
                };
                let mut entry = ModelEntry::default();
                for (key, slot) in [
                    ("by_task", &mut entry.by_task),
                    ("by_family", &mut entry.by_family),
                    ("by_app", &mut entry.by_app),
                ] {
                    match m.get(key) {
                        None => {}
                        Some(CValue::Obj(groups)) => {
                            for (gid, g) in groups {
                                match parse_figures(g, &format!("{at}.{key}.{gid}")) {
                                    Ok(f) => {
                                        slot.insert(gid.clone(), f);
                                    }
                                    Err(ds) => errs.extend(ds.into_iter().map(&err)),
                                }
                            }
                        }
                        Some(_) => errs.push(err(format!("{at}.{key} must be an object"))),
                    }
                }
                if models.insert(name.clone(), entry).is_some() {
                    errs.push(err(format!("{at}: model {name:?} listed twice")));
                }
            }
        }
        _ => errs.push(err("models must be an array".into())),
    }
    match (errs.is_empty(), verdict) {
        (true, Some(verdict)) => Ok(Board { file: file.to_string(), release, verdict, models }),
        _ => {
            errs.sort();
            Err(errs)
        }
    }
}

fn parse_figures(v: &CValue, at: &str) -> Result<Figures, Vec<String>> {
    let Some(m) = as_obj(v) else {
        return Err(vec![format!("{at}: not an object")]);
    };
    let mut ds = Vec::new();
    let mut vals = [None; 6];
    for (slot, key) in vals.iter_mut().zip(COUNT_FIELDS) {
        match m.get(key) {
            None => {}
            Some(CValue::Num(n)) if is_count(*n) => *slot = Some(*n as u64),
            Some(_) => ds.push(format!("{at}.{key} must be a whole number from 0 to 2^53")),
        }
    }
    if !ds.is_empty() {
        return Err(ds);
    }
    let [adoptable, human_adopted, human_reviewed, runs, successes, violations] = vals;
    Ok(Figures { runs, successes, adoptable, human_reviewed, human_adopted, violations })
}

fn is_count(n: f64) -> bool {
    (0.0..=9_007_199_254_740_992.0).contains(&n) && n.fract() == 0.0
}
