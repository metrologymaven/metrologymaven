//! Lifecycle states, retention and migrator records. Spec node `CORE.REG.VER.LIFE`.
//!
//! Every (ID, major) carries one [`Lifecycle`]. [`check_transition`] allows only the four
//! legal moves and enforces the minimum deprecation period; [`retained`] gives the
//! (ID, major) pairs the manifest lists; [`check_migrators`] checks the migrator records a
//! new major declares. Dates are calendar dates given by the release process; nothing
//! here reads a clock (determinism D0).

use crate::def::CommandDef;
use fid_canon::CValue;
use fid_kci::AlgorithmId;
use fid_types::{CommandId, CommandName};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

/// Minimum time in `deprecated` before `hidden`, in calendar months (REQ-003).
/// The figure is the parent node's `[unverified]` choice; change it by decision.
pub const DEPRECATION_MIN_MONTHS: u32 = 12;

/// Lifecycle state of one (ID, major) (REQ-001).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    Active,
    Deprecated,
    Hidden,
    Removed,
}

impl State {
    /// Every state, in lifecycle order.
    pub const ALL: [State; 4] = [State::Active, State::Deprecated, State::Hidden, State::Removed];

    /// The wire string.
    pub fn as_str(self) -> &'static str {
        match self {
            State::Active => "active",
            State::Deprecated => "deprecated",
            State::Hidden => "hidden",
            State::Removed => "removed",
        }
    }

    /// Parses the wire string.
    pub fn parse(s: &str) -> Result<State, LifeError> {
        State::ALL
            .into_iter()
            .find(|x| x.as_str() == s)
            .ok_or_else(|| LifeError::BadLifecycle(format!("unknown state {s:?}")))
    }
}

impl fmt::Display for State {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A calendar date (UTC), `YYYY-MM-DD`, years 0001..=9999. Orders chronologically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    year: u16,
    month: u8,
    day: u8,
}

fn days_in_month(year: u16, month: u8) -> u8 {
    match month {
        4 | 6 | 9 | 11 => 30,
        2 if (year % 4 == 0 && year % 100 != 0) || year % 400 == 0 => 29,
        2 => 28,
        _ => 31,
    }
}

impl Date {
    /// Builds a date, refusing impossible ones (e.g. 2025-02-29).
    pub fn new(year: u16, month: u8, day: u8) -> Result<Date, LifeError> {
        if !(1..=9999).contains(&year) || !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return Err(LifeError::BadLifecycle(format!("no such date {year:04}-{month:02}-{day:02}")));
        }
        Ok(Date { year, month, day })
    }

    /// Parses exactly `YYYY-MM-DD`.
    pub fn parse(s: &str) -> Result<Date, LifeError> {
        let bad = || LifeError::BadLifecycle(format!("date must be YYYY-MM-DD, got {s:?}"));
        let b = s.as_bytes();
        let digits = |r: std::ops::Range<usize>| -> Option<u16> {
            let part = b.get(r)?;
            if !part.iter().all(u8::is_ascii_digit) {
                return None;
            }
            Some(part.iter().fold(0u16, |acc, d| acc * 10 + u16::from(d - b'0')))
        };
        if b.len() != 10 || b.get(4) != Some(&b'-') || b.get(7) != Some(&b'-') {
            return Err(bad());
        }
        let (Some(y), Some(m), Some(d)) = (digits(0..4), digits(5..7), digits(8..10)) else {
            return Err(bad());
        };
        let m = u8::try_from(m).map_err(|_| bad())?;
        let d = u8::try_from(d).map_err(|_| bad())?;
        Date::new(y, m, d)
    }

    pub fn year(self) -> u16 {
        self.year
    }

    pub fn month(self) -> u8 {
        self.month
    }

    pub fn day(self) -> u8 {
        self.day
    }

    /// Whether at least `months` calendar months have passed from `self` to `later`:
    /// compares whole calendar months first, then the day of the month.
    pub fn months_until_at_least(self, later: Date, months: u32) -> bool {
        let index = |d: Date| i64::from(d.year) * 12 + i64::from(d.month);
        let elapsed = index(later) - index(self);
        let months = i64::from(months);
        elapsed > months || (elapsed == months && later.day >= self.day)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

/// Lifecycle of one (ID, major) (REQ-001). JSON form, the `lifecycle` field of a definition:
/// `{"state": "...", "since": "YYYY-MM-DD", "replaced_by": "name@major", "remove_after": "YYYY-MM-DD"}`;
/// the last two are optional and not allowed while `active`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Lifecycle {
    pub state: State,
    /// Release date on which the current state took effect.
    pub since: Date,
    pub replaced_by: Option<CommandId>,
    pub remove_after: Option<Date>,
}

impl Lifecycle {
    /// Reads a lifecycle object, refusing unknown fields and inconsistent metadata.
    pub fn from_map(map: &BTreeMap<String, CValue>) -> Result<Lifecycle, LifeError> {
        let bad = LifeError::BadLifecycle;
        if let Some(k) = map.keys().find(|k| !matches!(k.as_str(), "state" | "since" | "replaced_by" | "remove_after"))
        {
            return Err(bad(format!("unknown field {k}")));
        }
        let text = |k: &str| -> Result<Option<&str>, LifeError> {
            match map.get(k) {
                None => Ok(None),
                Some(CValue::Str(s)) => Ok(Some(s.as_str())),
                Some(_) => Err(bad(format!("{k} must be a string"))),
            }
        };
        let state = State::parse(text("state")?.ok_or_else(|| bad("state is required".into()))?)?;
        let since = Date::parse(text("since")?.ok_or_else(|| bad("since is required".into()))?)?;
        let replaced_by = text("replaced_by")?
            .map(|s| CommandId::parse(s).map_err(|_| bad(format!("replaced_by is not a command ID: {s:?}"))))
            .transpose()?;
        let remove_after = text("remove_after")?.map(Date::parse).transpose()?;
        let life = Lifecycle { state, since, replaced_by, remove_after };
        life.validate()?;
        Ok(life)
    }

    /// Checks the metadata rules: `active` carries neither `replaced_by` nor
    /// `remove_after`, and `remove_after` is not before `since`.
    pub fn validate(&self) -> Result<(), LifeError> {
        if self.state == State::Active && (self.replaced_by.is_some() || self.remove_after.is_some()) {
            return Err(LifeError::BadLifecycle("an active major has no replaced_by or remove_after".into()));
        }
        if let Some(r) = self.remove_after {
            if r < self.since {
                return Err(LifeError::BadLifecycle(format!("remove_after {r} is before since {}", self.since)));
            }
        }
        Ok(())
    }

    /// The JSON value of this lifecycle; absent options are left out.
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("state".to_string(), CValue::Str(self.state.as_str().to_string()));
        o.insert("since".to_string(), CValue::Str(self.since.to_string()));
        if let Some(r) = &self.replaced_by {
            o.insert("replaced_by".to_string(), CValue::Str(r.to_string()));
        }
        if let Some(r) = self.remove_after {
            o.insert("remove_after".to_string(), CValue::Str(r.to_string()));
        }
        CValue::Obj(o)
    }
}

/// Checks a lifecycle transition of one major against the date of the release that
/// makes it (REQ-002, REQ-003). Allowed: `active → deprecated`, `deprecated → hidden`
/// (only once [`DEPRECATION_MIN_MONTHS`] have passed since `old.since`), `hidden → removed`
/// (only on or after `old.remove_after` when that date is set, else `removal_too_early`)
/// and `deprecated → active`. Everything else, including staying in the same state, is
/// `illegal_transition`.
pub fn check_transition(old: &Lifecycle, new: State, release_date: Date) -> Result<(), LifeError> {
    use State::{Active, Deprecated, Hidden, Removed};
    match (old.state, new) {
        (Active, Deprecated) | (Deprecated, Active) => Ok(()),
        (Hidden, Removed) => match old.remove_after {
            Some(after) if release_date < after => Err(LifeError::RemovalTooEarly { after, release: release_date }),
            _ => Ok(()),
        },
        (Deprecated, Hidden) => {
            if old.since.months_until_at_least(release_date, DEPRECATION_MIN_MONTHS) {
                Ok(())
            } else {
                Err(LifeError::DeprecationTooShort { since: old.since, release: release_date })
            }
        }
        (from, to) => Err(LifeError::IllegalTransition { from, to }),
    }
}

/// The lifecycle of one declared (ID, major).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LifecycleDecl {
    pub command: CommandId,
    pub lifecycle: Lifecycle,
}

impl LifecycleDecl {
    /// Reads the lifecycle of a definition.
    pub fn from_def(def: &CommandDef) -> Result<LifecycleDecl, LifeError> {
        let command = CommandName::parse(&def.id)
            .and_then(|n| CommandId::new(&n, def.major))
            .map_err(|e| LifeError::BadLifecycle(format!("bad command ID: {}", e.code())))?;
        let lifecycle = Lifecycle::from_map(&def.lifecycle)
            .map_err(|e| LifeError::BadLifecycle(format!("{command}: {}", e.detail())))?;
        let decl = LifecycleDecl { command, lifecycle };
        decl.validate()?;
        Ok(decl)
    }

    /// Checks the lifecycle metadata and that a `replaced_by` under the same name points
    /// to a higher major. A different name (a rename) is allowed.
    pub fn validate(&self) -> Result<(), LifeError> {
        self.lifecycle.validate()?;
        if let Some(r) = &self.lifecycle.replaced_by {
            if r.name() == self.command.name() && r.major() <= self.command.major() {
                let me = &self.command;
                return Err(LifeError::BadLifecycle(format!("{me}: replaced_by {r} must be a higher major")));
            }
        }
        Ok(())
    }
}

fn key(id: &CommandId) -> (&str, u32) {
    (id.name(), id.major())
}

/// The state of each declared (ID, major); a pair declared twice is `bad_lifecycle`.
fn states(decls: &[LifecycleDecl]) -> Result<BTreeMap<(&str, u32), State>, LifeError> {
    let mut out = BTreeMap::new();
    for d in decls {
        if out.insert(key(&d.command), d.lifecycle.state).is_some() {
            return Err(LifeError::BadLifecycle(format!("{} is declared more than once", d.command)));
        }
    }
    Ok(out)
}

/// The (ID, major) pairs the manifest lists (REQ-004): every declaration whose state is
/// not `removed`, `hidden` included, ordered by name then major. A pair declared twice is
/// refused, since each (ID, major) carries exactly one state.
pub fn retained(decls: &[LifecycleDecl]) -> Result<Vec<&LifecycleDecl>, LifeError> {
    states(decls)?;
    let mut out: Vec<&LifecycleDecl> = decls.iter().filter(|d| d.lifecycle.state != State::Removed).collect();
    out.sort_by(|a, b| key(&a.command).cmp(&key(&b.command)));
    Ok(out)
}

/// A migrator a newer major declares to translate an older major's arguments (REQ-005).
/// JSON form: `{"algorithm": "a.b.c@1.0.0", "command": "name", "from_major": 1, "to_major": 2}`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MigratorRecord {
    pub algorithm: AlgorithmId,
    /// Command name without the major; the majors are `from_major` and `to_major`.
    pub command: CommandName,
    pub from_major: u32,
    pub to_major: u32,
}

impl MigratorRecord {
    /// Reads a migrator record; any malformed field is `migrator_invalid`.
    pub fn from_value(v: &CValue) -> Result<MigratorRecord, LifeError> {
        let bad = LifeError::MigratorInvalid;
        let CValue::Obj(o) = v else { return Err(bad("must be an object".into())) };
        if let Some(k) = o.keys().find(|k| !matches!(k.as_str(), "algorithm" | "command" | "from_major" | "to_major")) {
            return Err(bad(format!("unknown field {k}")));
        }
        let text = |k: &str| match o.get(k) {
            Some(CValue::Str(s)) => Ok(s.as_str()),
            _ => Err(bad(format!("{k} must be a string"))),
        };
        let major = |k: &str| match o.get(k) {
            Some(CValue::Num(n)) if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(n) => Ok(*n as u32),
            _ => Err(bad(format!("{k} must be an integer 0..2^32"))),
        };
        let algorithm = AlgorithmId::parse(text("algorithm")?).map_err(|_| bad("bad algorithm ID".into()))?;
        let command = CommandName::parse(text("command")?).map_err(|_| bad("bad command name".into()))?;
        Ok(MigratorRecord { algorithm, command, from_major: major("from_major")?, to_major: major("to_major")? })
    }

    /// The JSON value the manifest records (REQ-CORE.REG.VER-007).
    pub fn to_value(&self) -> CValue {
        let mut o = BTreeMap::new();
        o.insert("algorithm".to_string(), CValue::Str(self.algorithm.to_string()));
        o.insert("command".to_string(), CValue::Str(self.command.to_string()));
        o.insert("from_major".to_string(), CValue::Num(f64::from(self.from_major)));
        o.insert("to_major".to_string(), CValue::Num(f64::from(self.to_major)));
        CValue::Obj(o)
    }
}

/// Checks migrator records against the declarations (REQ-005, REQ-006). Fails with
/// `migrator_invalid` when the target major is not greater than the source, the source or
/// target (ID, major) is not declared or is `removed`, or two records share a command,
/// source and target. A pair declared twice in `decls` is `bad_lifecycle`, as in [`retained`].
/// A `hidden` target is accepted: the spec only excludes removed or undeclared majors.
pub fn check_migrators(decls: &[LifecycleDecl], migrators: &[MigratorRecord]) -> Result<(), LifeError> {
    let states = states(decls)?;
    let mut seen = BTreeSet::new();
    for m in migrators {
        let name = m.command.as_str();
        let label = format!("{name}@{} -> {name}@{}", m.from_major, m.to_major);
        if m.to_major <= m.from_major {
            return Err(LifeError::MigratorInvalid(format!("{label}: target major must be greater than source")));
        }
        for (end, major) in [("source", m.from_major), ("target", m.to_major)] {
            match states.get(&(name, major)) {
                None => return Err(LifeError::MigratorInvalid(format!("{label}: {end} is not declared"))),
                Some(State::Removed) => return Err(LifeError::MigratorInvalid(format!("{label}: {end} is removed"))),
                Some(_) => {}
            }
        }
        if !seen.insert((name, m.from_major, m.to_major)) {
            return Err(LifeError::MigratorInvalid(format!("{label}: declared more than once")));
        }
    }
    Ok(())
}

/// Lifecycle errors with stable codes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifeError {
    IllegalTransition {
        from: State,
        to: State,
    },
    DeprecationTooShort {
        since: Date,
        release: Date,
    },
    /// `hidden → removed` before the promised `remove_after` date.
    RemovalTooEarly {
        after: Date,
        release: Date,
    },
    MigratorInvalid(String),
    /// A lifecycle object, date or declaration list is malformed.
    BadLifecycle(String),
}

impl LifeError {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            LifeError::IllegalTransition { .. } => "illegal_transition",
            LifeError::DeprecationTooShort { .. } => "deprecation_too_short",
            LifeError::RemovalTooEarly { .. } => "removal_too_early",
            LifeError::MigratorInvalid(_) => "migrator_invalid",
            LifeError::BadLifecycle(_) => "bad_lifecycle",
        }
    }

    fn detail(&self) -> String {
        match self {
            LifeError::IllegalTransition { from, to } => format!("{from} -> {to} is not allowed"),
            LifeError::DeprecationTooShort { since, release } => {
                format!("deprecated since {since}; hidden needs {DEPRECATION_MIN_MONTHS} months, release is {release}")
            }
            LifeError::RemovalTooEarly { after, release } => {
                format!("removal promised not before {after}, release is {release}")
            }
            LifeError::MigratorInvalid(s) | LifeError::BadLifecycle(s) => s.clone(),
        }
    }
}

impl fmt::Display for LifeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code(), self.detail())
    }
}

impl std::error::Error for LifeError {}
