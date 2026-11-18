//! First-divergence search and attribution. Spec node `CORE.RPL.BISECT`.
//!
//! When a replay is `DIVERGENT` this module says where and why:
//! * [`first_divergent`] / [`first_divergent_by_seq`] scan per-entry outcomes (REQ-001);
//! * [`bisect`] finds the first divergent entry of a span when only a predicate
//!   "the state after entry k diverges" is known (REQ-002, REQ-006);
//! * [`attribute`] names one primary cause and every other matching cause (REQ-003..005);
//! * [`report_fragment`] gives the canonical JSON fragment for the Replay Report.
//!
//! Everything here is D0: no clocks, no randomness, no hash-ordered collections.

use crate::classify::ReplayOutcome;
use fid_canon::{CValue, CanonError};
use fid_types::{CommandId, Digest};
use std::collections::BTreeMap;
use std::fmt;

/// Lowest index whose outcome is `DIVERGENT` (REQ-001). The slice index is the entry's
/// sequence number within the replayed span.
pub fn first_divergent(outcomes: &[ReplayOutcome]) -> Option<u64> {
    outcomes.iter().position(|o| *o == ReplayOutcome::Divergent).map(|i| i as u64)
}

/// Lowest sequence number whose outcome is `DIVERGENT` (REQ-001), for entries given as
/// `(seq, outcome)` pairs in any order.
pub fn first_divergent_by_seq(entries: &[(u64, ReplayOutcome)]) -> Option<u64> {
    entries.iter().filter(|(_, o)| *o == ReplayOutcome::Divergent).map(|(s, _)| *s).min()
}

/// Result of a [`bisect`] search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BisectResult {
    /// Index of the first divergent entry of the span, or `None` when no entry diverges.
    pub first: Option<u64>,
    /// Number of predicate evaluations made.
    pub evaluations: u64,
    /// Set when the monotonicity spot check failed and the answer came from a linear scan
    /// (REQ-006; reported, not fatal).
    pub non_monotone: bool,
}

impl BisectResult {
    /// Stable note codes for the report (`non_monotone` when set).
    pub fn notes(&self) -> Vec<String> {
        if self.non_monotone {
            vec!["non_monotone".to_string()]
        } else {
            Vec::new()
        }
    }
}

/// Failure of a [`bisect`] search.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BisectError<E> {
    /// The span has no entries.
    EmptySpan,
    /// The predicate itself failed; the search stops.
    Predicate(E),
}

impl<E> BisectError<E> {
    /// Stable code.
    pub fn code(&self) -> &'static str {
        match self {
            BisectError::EmptySpan => "empty_span",
            BisectError::Predicate(_) => "predicate_failed",
        }
    }
}

impl<E: fmt::Display> fmt::Display for BisectError<E> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BisectError::EmptySpan => f.write_str("empty_span: the span has no entries"),
            BisectError::Predicate(e) => write!(f, "predicate_failed: {e}"),
        }
    }
}

impl<E: fmt::Debug + fmt::Display> std::error::Error for BisectError<E> {}

/// Finds the first divergent entry of a span of `n` entries (REQ-002, REQ-006).
///
/// `pred(k)` answers "does the state after entry `k` (entries `0..=k` applied) diverge?"
/// and is expected to be monotone: false up to the first divergent entry, true from it on.
///
/// Algorithm: a lower-bound binary search over `[0, n-1)` that assumes entry `n-1` diverges
/// (at most `ceil(log2 n)` evaluations), then one spot check evaluating entry `n-1`
/// (total at most `ceil(log2 n) + 1`). Every evaluation is kept; if any is inconsistent with
/// the found boundary (a true before it or a false at or after it) the predicate is not
/// monotone, `non_monotone` is set and a linear scan from entry 0 gives the answer, reusing
/// the values already known. If every evaluation is false, no entry diverges.
///
/// The spot check sees a violation only when an evaluated point contradicts the boundary;
/// a non-monotone predicate that agrees with a monotone one on every evaluated point is
/// not detected (no `O(log n)` search can do better).
pub fn bisect<E>(n: u64, mut pred: impl FnMut(u64) -> Result<bool, E>) -> Result<BisectResult, BisectError<E>> {
    if n == 0 {
        return Err(BisectError::EmptySpan);
    }
    let mut m = Memo { seen: BTreeMap::new(), evaluations: 0 };

    let last = n - 1;
    let (mut lo, mut hi) = (0u64, last);
    while lo < hi {
        let mid = lo + (hi - lo) / 2;
        if m.eval(mid, &mut pred)? {
            hi = mid;
        } else {
            lo = mid + 1;
        }
    }
    // Spot check: under monotonicity the last entry diverges whenever any entry does.
    m.eval(last, &mut pred)?;

    if !m.seen.values().any(|v| *v) {
        return Ok(BisectResult { first: None, evaluations: m.evaluations, non_monotone: false });
    }
    let k = lo;
    if m.seen.iter().all(|(&j, &v)| v == (j >= k)) {
        return Ok(BisectResult { first: Some(k), evaluations: m.evaluations, non_monotone: false });
    }
    // REQ-006: not monotone; fall back to a linear scan.
    let mut found = None;
    for j in 0..n {
        if m.eval(j, &mut pred)? {
            found = Some(j);
            break;
        }
    }
    Ok(BisectResult { first: found, evaluations: m.evaluations, non_monotone: true })
}

/// Predicate values already known, so no entry is evaluated twice.
struct Memo {
    seen: BTreeMap<u64, bool>,
    evaluations: u64,
}

impl Memo {
    fn eval<E>(&mut self, k: u64, pred: &mut impl FnMut(u64) -> Result<bool, E>) -> Result<bool, BisectError<E>> {
        if let Some(v) = self.seen.get(&k) {
            return Ok(*v);
        }
        self.evaluations += 1;
        let v = pred(k).map_err(BisectError::Predicate)?;
        self.seen.insert(k, v);
        Ok(v)
    }
}

/// Cause of a divergence, in precedence order (REQ-004): the first variant wins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Cause {
    Input,
    Parameter,
    DefaultsProfile,
    CommandMigration,
    Kernel,
}

impl Cause {
    /// All causes in precedence order.
    pub const ALL: [Cause; 5] =
        [Cause::Input, Cause::Parameter, Cause::DefaultsProfile, Cause::CommandMigration, Cause::Kernel];

    /// Wire name, e.g. `defaults_profile`.
    pub fn as_str(self) -> &'static str {
        match self {
            Cause::Input => "input",
            Cause::Parameter => "parameter",
            Cause::DefaultsProfile => "defaults_profile",
            Cause::CommandMigration => "command_migration",
            Cause::Kernel => "kernel",
        }
    }

    /// Parses a wire name.
    pub fn parse(s: &str) -> Option<Cause> {
        Cause::ALL.iter().copied().find(|c| c.as_str() == s)
    }
}

impl fmt::Display for Cause {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// What the journal records (or the replay produced) for one entry, as compared by
/// [`attribute`] (REQ-003).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EntryFacts {
    /// Journal sequence number of the entry.
    pub seq: u64,
    /// Input digests, in the entry's order.
    pub inputs: Vec<Digest>,
    /// Argument commitments, in the entry's order.
    pub arguments: Vec<Digest>,
    /// Defaults-profile digest.
    pub defaults_profile: Digest,
    /// Command `name@major`.
    pub command: CommandId,
    /// Migrator applied to the recorded arguments, if any.
    pub migrator: Option<String>,
    /// Kernel manifest digest.
    pub kernel_manifest: Digest,
}

/// Note set when no compared item differs (REQ-005).
pub const NO_INPUT_DIFFERENCE: &str = "no_input_difference";

/// Why the first divergent entry diverged.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attribution {
    pub entry_seq: u64,
    pub primary: Cause,
    /// Every other matching cause, in precedence order.
    pub secondary: Vec<Cause>,
    /// Sorted note codes, e.g. `no_input_difference`.
    pub notes: Vec<String>,
}

/// Attributes the divergence at an entry to one primary cause (REQ-003..005).
///
/// Compared items: input digests (`input`), argument commitments (`parameter`),
/// defaults-profile digest (`defaults_profile`), command ID or migrator
/// (`command_migration`) and kernel manifest digest (`kernel`). The first differing item
/// in that order is primary, the others are secondary. When nothing differs the primary
/// cause is `kernel` with note `no_input_difference`. `entry_seq` is the recorded entry's.
pub fn attribute(recorded: &EntryFacts, replayed: &EntryFacts) -> Attribution {
    let differs = |c: Cause| match c {
        Cause::Input => recorded.inputs != replayed.inputs,
        Cause::Parameter => recorded.arguments != replayed.arguments,
        Cause::DefaultsProfile => recorded.defaults_profile != replayed.defaults_profile,
        Cause::CommandMigration => recorded.command != replayed.command || recorded.migrator != replayed.migrator,
        Cause::Kernel => recorded.kernel_manifest != replayed.kernel_manifest,
    };
    let mut matching = Cause::ALL.iter().copied().filter(|c| differs(*c));
    match matching.next() {
        Some(primary) => {
            Attribution { entry_seq: recorded.seq, primary, secondary: matching.collect(), notes: Vec::new() }
        }
        None => Attribution {
            entry_seq: recorded.seq,
            primary: Cause::Kernel,
            secondary: Vec::new(),
            notes: vec![NO_INPUT_DIFFERENCE.to_string()],
        },
    }
}

fn strings(v: &[String]) -> CValue {
    CValue::Arr(v.iter().map(|s| CValue::Str(s.clone())).collect())
}

/// The fragment written into the Replay Report, as a canonical-JSON value:
/// `{"first_divergence": {"entry_seq", "search": {"evaluations", "notes"},
/// "attribution": {"primary", "secondary", "notes"}}}`. `search` is `null` when the entry
/// came from per-entry outcomes rather than [`bisect`].
pub fn report_value(search: Option<&BisectResult>, attribution: &Attribution) -> CValue {
    let mut att = BTreeMap::new();
    att.insert("primary".to_string(), CValue::Str(attribution.primary.as_str().to_string()));
    att.insert(
        "secondary".to_string(),
        CValue::Arr(attribution.secondary.iter().map(|c| CValue::Str(c.as_str().to_string())).collect()),
    );
    att.insert("notes".to_string(), strings(&attribution.notes));

    let search = match search {
        Some(s) => {
            let mut m = BTreeMap::new();
            m.insert("evaluations".to_string(), CValue::U64(s.evaluations));
            m.insert("notes".to_string(), strings(&s.notes()));
            CValue::Obj(m)
        }
        None => CValue::Null,
    };

    let mut fd = BTreeMap::new();
    fd.insert("entry_seq".to_string(), CValue::U64(attribution.entry_seq));
    fd.insert("search".to_string(), search);
    fd.insert("attribution".to_string(), CValue::Obj(att));
    let mut root = BTreeMap::new();
    root.insert("first_divergence".to_string(), CValue::Obj(fd));
    CValue::Obj(root)
}

/// [`report_value`] as canonical JSON bytes (RFC 8785 via `fid-canon`); byte-identical for
/// identical inputs (AC-5).
pub fn report_fragment(search: Option<&BisectResult>, attribution: &Attribution) -> Result<Vec<u8>, CanonError> {
    fid_canon::to_canonical_bytes(&report_value(search, attribution))
}
