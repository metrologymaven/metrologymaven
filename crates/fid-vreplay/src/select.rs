//! Replayable-set selection and `NOT_REPLAYABLE` reasons. Spec node `TRUST.REPLAY.SELECT`.
//!
//! For each ResultRecord of a package, [`plan`] decides whether the verifier can recompute it
//! and builds the [`ReplayPlan`]: the kernel calls of every replayable result's dependency
//! closure, each once, in journal order, with algorithm ID, parameters, seed and input digests.
//! Every other result gets exactly one primary [`Reason`] and a detail string.
//!
//! The package is untrusted input. It is read through [`PackageView`] (the verified member
//! bytes of a `fid-pkg` reader), every member SELECT reads is bounded, and anything malformed
//! or unknown fails the whole selection with [`SelectError`] (fail closed), never a partial
//! plan.
//!
//! # Members read
//!
//! | Member | Use |
//! |---|---|
//! | `results/records.json` | JSON array of ResultRecords; `id`, `evaluation_basis`, `algorithm`, `journal_entry` |
//! | `journal/segment.jsonl` | one RFC 8785 line per entry: `{"header": {...}, "payload": {...}}`, payload optional |
//! | `points/<sha256 hex>.<ext>` | packaged point sets and other recorded outputs, by digest |
//! | `kernel/manifest.json` | the kernel manifest: the algorithm IDs the target kernel knows |
//!
//! Entry index = 0-based line number in the segment. Each entry's hash is recomputed (SHA-256
//! of the canonical header without `hash`) and must equal its `hash` field.
//!
//! # Rules
//!
//! A *kernel call* is a `command` entry that names exactly one algorithm. The producer of a
//! digest for a call at index `e` is the latest kernel call before `e` that lists the digest
//! among its outputs; only when no earlier kernel call outputs it is it the latest earlier
//! non-kernel `command` entry. So a non-kernel step that re-emits a kernel output can never
//! hide the kernel step. For one record:
//!
//! 1. basis `RESULTS_ONLY` → `BASIS_RESULTS_ONLY`; `DIRECT_READING` → `BASIS_DIRECT_READING`;
//!    otherwise, in a package of evidence tier `results` → `BASIS_RESULTS_ONLY`;
//! 2. the entry named by `journal_entry`: absent, not a `command`, or naming another algorithm
//!    than the record → `ENTRY_MISSING`; not a kernel call → `NON_KERNEL_STEP`;
//! 3. walk the closure from that call. For each kernel call: its algorithm must be in the
//!    kernel manifest (`ALGORITHM_UNKNOWN`), it must have been recorded under the packaged
//!    kernel manifest (`KERNEL_UNAVAILABLE`), and its payload must carry `args` reproducing
//!    `args_commit` (`ENTRY_MISSING`). For each input digest: a kernel-call producer joins the
//!    closure and the input is sourced from that call's output ([`InputSource::Call`]): kernel
//!    outputs are always recomputed, never taken from the package; otherwise a packaged
//!    `points/` member supplies the recorded output (REQ-003, [`InputSource::Packaged`]), and if
//!    a non-kernel step produced it the stand-in is disclosed in
//!    [`Selected::Replayable::substituted`]; otherwise the input is missing: `POINTS_MISSING`
//!    when there is no producer or the producer is recorded as class N, else `NON_KERNEL_STEP`;
//! 4. a closure of more than [`MAX_PLAN_CALLS`] calls → `PLAN_TOO_LARGE` (REQ-005);
//! 5. the kernel artifact must be obtainable ([`ArtifactSource`]) → `KERNEL_UNAVAILABLE`.
//!
//! Rules 1 and 2 decide before any walk, and rule 4 aborts the walk: so a result whose own entry
//! is `ENTRY_MISSING` or `NON_KERNEL_STEP` (`not_found`, `not_a_command`, `algorithm_mismatch`,
//! `root_not_kernel`), or whose closure is `PLAN_TOO_LARGE`, gets that reason even if its
//! closure would also have shown an earlier-listed one. Otherwise the walk visits the whole
//! closure and, when several reasons apply, the primary one is the first in [`Reason::ALL`]
//! (the order REQ-001 lists them, then `PLAN_TOO_LARGE`), with the smallest detail string of
//! that reason, so the choice never depends on walk order.
//!
//! Record IDs must be unique: a repeated ID is `pkg.malformed` `duplicate_record_id`.
//!
//! **No kernel attested.** A package whose `kernel_manifest_digest` is the reserved
//! [`NO_KERNEL_MANIFEST`] (`CORE.JRN.ENTRY`) is refused as `pkg.malformed` `kernel_unattested`
//! before any member is read or [`ArtifactSource::is_available`] is called: it attests no
//! kernel to replay under. A kernel call recorded under the reserved digest was not recorded
//! under the packaged kernel (rule 3, `kernel_manifest_mismatch`).
//!
//! # What is trusted
//!
//! Which results are replayable never rests on a class the package declares. A step is a
//! kernel call because it names an algorithm that the kernel manifest lists (and RUN replays
//! it there); the recorded `determinism` and side-effect class of an entry are read only to
//! choose between two `NOT_REPLAYABLE` labels (rule 3), never to make a result replayable. A
//! step that is not a kernel call is never recomputed by the verifier, so its packaged output
//! is the only thing replay could use; it is substituted only when that exact digest is
//! packaged. The algorithm ID of a replayable result ([`Selected::Replayable`]) is the one its
//! journal entry names and the kernel manifest lists; `TRUST.REPLAY.RUN` must take the
//! determinism class and quantity kind for `compare` from the algorithm catalogue by that ID,
//! never from the record.
//!
//! Determinism D0: no clocks, no randomness, no hash-ordered collections.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;

use fid_canon::{digest_bytes, to_canonical_bytes, CValue};
use fid_types::Digest;

use crate::compare::ResultInput;

/// Path of the ResultRecords member.
pub const RECORDS_PATH: &str = "results/records.json";
/// Path of the journal segment member.
pub const SEGMENT_PATH: &str = "journal/segment.jsonl";
/// Path of the kernel manifest member.
pub const KERNEL_MANIFEST_PATH: &str = "kernel/manifest.json";
/// Folder of packaged point sets and recorded outputs.
pub const POINTS_DIR: &str = "points/";

/// Most kernel calls in one result's closure (REQ-005) [unverified limit].
pub const MAX_PLAN_CALLS: usize = 100_000;
/// Default total walk work (calls visited plus inputs examined) over all results; beyond it
/// the remaining results are `PLAN_TOO_LARGE` with detail `work_budget`.
pub const WORK_BUDGET: u64 = 10_000_000;
/// Largest `results/records.json`, in bytes.
pub const MAX_RECORDS_BYTES: u64 = 64 * 1024 * 1024;
/// Largest `journal/segment.jsonl`, in bytes.
pub const MAX_SEGMENT_BYTES: u64 = 256 * 1024 * 1024;
/// Largest segment line, in bytes.
pub const MAX_LINE_BYTES: usize = 16 * 1024 * 1024;
/// Largest `kernel/manifest.json`, in bytes (the kernel manifest's own input limit).
pub const MAX_KERNEL_MANIFEST_BYTES: u64 = 4 * 1024 * 1024;
/// The reserved `manifests.kernel` digest meaning "no kernel manifest attested" (spec node
/// `CORE.JRN.ENTRY`, `fid_journal::NO_KERNEL_MANIFEST`): the SHA-256 of the empty string.
/// Repeated here so the verifier does not link the journal crate; a test pins it.
pub const NO_KERNEL_MANIFEST: Digest = Digest::from_bytes([
    0xe3, 0xb0, 0xc4, 0x42, 0x98, 0xfc, 0x1c, 0x14, 0x9a, 0xfb, 0xf4, 0xc8, 0x99, 0x6f, 0xb9, 0x24, 0x27, 0xae, 0x41,
    0xe4, 0x64, 0x9b, 0x93, 0x4c, 0xa4, 0x95, 0x99, 0x1b, 0x78, 0x52, 0xb8, 0x55,
]);
/// Most ResultRecords in a package.
pub const MAX_RECORDS: usize = 1_000_000;
/// Most entries in a segment.
pub const MAX_ENTRIES: usize = 1_000_000;
/// Most inputs, outputs or algorithms of one entry.
pub const MAX_DIGESTS_PER_ENTRY: usize = 4096;
/// Most algorithms in a kernel manifest.
pub const MAX_ALGORITHMS: usize = 4096;
/// Most JSON elements parsed from one member or line.
const MAX_ELEMENTS: usize = 8 * 1024 * 1024;
/// Deepest JSON nesting accepted.
const MAX_DEPTH: usize = 64;

// ------------------------------------------------------------------ the package

/// One member listed in the package manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberInfo {
    /// Member path.
    pub path: String,
    /// SHA-256 the manifest lists (the reader checks the bytes against it).
    pub sha256: Digest,
}

/// An error from the package reader, passed through unchanged (`pkg.*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackageError {
    /// Stable code, for example `pkg.digest_mismatch`.
    pub code: String,
    /// Human-readable detail.
    pub detail: String,
}

/// The package as SELECT reads it: a `fid-pkg` reader (`TRUST.PKG.READER`) that has opened
/// and validated the container and manifest, and hands out member bytes only after checking
/// them against the manifest.
pub trait PackageView {
    /// The manifest's `evidence_tier` wire string (`results`, `features` or `points`).
    fn evidence_tier(&self) -> String;
    /// The manifest's `kernel_manifest_digest`.
    fn kernel_manifest_digest(&self) -> Digest;
    /// Every manifest member.
    fn members(&self) -> Vec<MemberInfo>;
    /// The verified bytes of a listed member; `None` if absent. More than `max_bytes` is an
    /// error.
    fn read_member(&mut self, path: &str, max_bytes: u64) -> Result<Option<Vec<u8>>, PackageError>;
}

/// Whether the kernel artifact for a kernel manifest can be obtained (implemented by
/// `TRUST.REPLAY.KART`; offline unless the caller opted in).
pub trait ArtifactSource {
    /// `true` if the WASM reference artifact the manifest names can be obtained.
    fn is_available(&self, kernel_manifest_digest: &Digest) -> bool;
}

// ------------------------------------------------------------------ errors

/// Why a selection failed as a whole.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SelectError {
    /// The package reader failed; its code is passed through.
    Package(PackageError),
    /// A member SELECT reads is malformed (`pkg.malformed`).
    Malformed {
        /// The member.
        member: String,
        /// Stable reason, for example `entry_hash_mismatch`.
        reason: &'static str,
        /// Detail.
        detail: String,
    },
    /// A member exceeds a SELECT limit (`pkg.limit`).
    Limit {
        /// The member.
        member: String,
        /// What was exceeded.
        what: &'static str,
    },
}

impl SelectError {
    /// Stable code: the reader's own code, `pkg.malformed` or `pkg.limit`.
    pub fn code(&self) -> &str {
        match self {
            SelectError::Package(e) => &e.code,
            SelectError::Malformed { .. } => "pkg.malformed",
            SelectError::Limit { .. } => "pkg.limit",
        }
    }

    /// Stable reason for `pkg.malformed` and `pkg.limit`; `None` for a passed-through error.
    pub fn reason(&self) -> Option<&'static str> {
        match self {
            SelectError::Package(_) => None,
            SelectError::Malformed { reason, .. } => Some(reason),
            SelectError::Limit { what, .. } => Some(what),
        }
    }
}

impl fmt::Display for SelectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SelectError::Package(e) => write!(f, "{}: {}", e.code, e.detail),
            SelectError::Malformed { member, reason, detail } => {
                write!(f, "pkg.malformed: {member}: {reason}: {detail}")
            }
            SelectError::Limit { member, what } => write!(f, "pkg.limit: {member}: {what}"),
        }
    }
}

impl std::error::Error for SelectError {}

fn bad(member: &str, reason: &'static str, detail: impl Into<String>) -> SelectError {
    SelectError::Malformed { member: member.to_string(), reason, detail: detail.into() }
}

// ------------------------------------------------------------------ results

/// Primary reason a result is not replayable, in precedence order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Reason {
    /// The record's basis is `RESULTS_ONLY`, or the package carries results only.
    BasisResultsOnly,
    /// The record's basis is `DIRECT_READING`.
    BasisDirectReading,
    /// An input point set or recorded output is not in the package.
    PointsMissing,
    /// The producing journal entry, or a kernel call's parameters, are not in the package.
    EntryMissing,
    /// A kernel call names an algorithm the kernel manifest does not list.
    AlgorithmUnknown,
    /// The kernel artifact cannot be obtained, or a call was recorded under another kernel.
    KernelUnavailable,
    /// A step the verifier cannot recompute and whose output is not packaged.
    NonKernelStep,
    /// The closure exceeds the call limit or the walk budget.
    PlanTooLarge,
}

impl Reason {
    /// Every reason, in precedence order.
    pub const ALL: [Reason; 8] = [
        Reason::BasisResultsOnly,
        Reason::BasisDirectReading,
        Reason::PointsMissing,
        Reason::EntryMissing,
        Reason::AlgorithmUnknown,
        Reason::KernelUnavailable,
        Reason::NonKernelStep,
        Reason::PlanTooLarge,
    ];

    /// Wire code, for example `POINTS_MISSING`. `compare` reports it as
    /// `NOT_REPLAYABLE:<code>`.
    pub fn as_str(self) -> &'static str {
        match self {
            Reason::BasisResultsOnly => "BASIS_RESULTS_ONLY",
            Reason::BasisDirectReading => "BASIS_DIRECT_READING",
            Reason::PointsMissing => "POINTS_MISSING",
            Reason::EntryMissing => "ENTRY_MISSING",
            Reason::AlgorithmUnknown => "ALGORITHM_UNKNOWN",
            Reason::KernelUnavailable => "KERNEL_UNAVAILABLE",
            Reason::NonKernelStep => "NON_KERNEL_STEP",
            Reason::PlanTooLarge => "PLAN_TOO_LARGE",
        }
    }

    /// Parses a wire code.
    pub fn parse(s: &str) -> Option<Reason> {
        Reason::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

impl fmt::Display for Reason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A result that cannot be replayed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NotReplayable {
    /// ResultRecord ID.
    pub record_id: String,
    /// The one primary reason.
    pub reason: Reason,
    /// Detail, for example the missing digest or `not_found:<entry hash>`.
    pub detail: String,
}

impl From<&NotReplayable> for ResultInput {
    fn from(n: &NotReplayable) -> ResultInput {
        ResultInput::NotReplayable { record_id: n.record_id.clone(), reason: n.reason.as_str().to_string() }
    }
}

/// Index into [`ReplayPlan::calls`].
pub type CallIndex = usize;

/// Where a kernel call's input comes from during replay.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum InputSource {
    /// Output number `output` of an earlier call of the plan (its recomputed value).
    Call {
        /// The producing call.
        call: CallIndex,
        /// Index into that call's [`KernelCall::outputs`].
        output: usize,
    },
    /// The verified bytes of the packaged member `points/<hex>.<ext>` with this digest.
    Packaged(Digest),
}

/// One kernel call to replay.
#[derive(Debug, Clone, PartialEq)]
pub struct KernelCall {
    /// 0-based line number of the entry in `journal/segment.jsonl`.
    pub entry_index: usize,
    /// The entry's hash.
    pub entry_hash: Digest,
    /// Algorithm ID with exact version, as the kernel manifest lists it.
    pub algorithm_id: String,
    /// Parameters: the entry's committed `args`.
    pub params: CValue,
    /// Recorded seed.
    pub seed: Option<u64>,
    /// Input digests, in recorded order.
    pub inputs: Vec<Digest>,
    /// Source of each input, parallel to [`KernelCall::inputs`].
    pub sources: Vec<InputSource>,
    /// Output digests as recorded, in recorded order.
    pub outputs: Vec<Digest>,
}

impl KernelCall {
    /// RFC 8785 bytes of [`KernelCall::params`] (the KCI parameter bytes).
    pub fn params_bytes(&self) -> Vec<u8> {
        to_canonical_bytes(&self.params).unwrap_or_default()
    }
}

/// The replay plan.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReplayPlan {
    /// Kernel calls in journal order, each once.
    pub calls: Vec<KernelCall>,
    /// Per replayable result, in record order: the calls of its closure, ascending. The last
    /// one is the result's own call.
    pub results: Vec<(String, Vec<CallIndex>)>,
}

/// The selection of one result.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Selected {
    /// Replayable.
    Replayable {
        /// ResultRecord ID.
        record_id: String,
        /// Algorithm ID of the result's own call (from the journal, listed by the kernel
        /// manifest): the key for the determinism class and quantity kind.
        algorithm_id: String,
        /// The calls of its closure, ascending; the last one is its own call.
        calls: Vec<CallIndex>,
        /// Every recorded intermediate that stands in for a step the verifier does not
        /// recompute: (packaged digest, hash of the non-kernel entry that produced it), sorted.
        /// Non-empty means the confirmation rests on recorded intermediates.
        substituted: Vec<(Digest, Digest)>,
    },
    /// Not replayable.
    NotReplayable(NotReplayable),
}

impl Selected {
    /// ResultRecord ID.
    pub fn record_id(&self) -> &str {
        match self {
            Selected::Replayable { record_id, .. } => record_id,
            Selected::NotReplayable(n) => &n.record_id,
        }
    }
}

/// The selection for a package.
#[derive(Debug, Clone, PartialEq)]
pub struct Selection {
    /// The packaged kernel manifest digest the plan targets.
    pub kernel_manifest_digest: Digest,
    /// The replay plan.
    pub plan: ReplayPlan,
    /// Per result, in `records.json` order.
    pub results: Vec<Selected>,
}

impl Selection {
    /// The results SELECT marked not replayable, in record order.
    pub fn not_replayable(&self) -> impl Iterator<Item = &NotReplayable> {
        self.results.iter().filter_map(|r| match r {
            Selected::NotReplayable(n) => Some(n),
            Selected::Replayable { .. } => None,
        })
    }

    /// The selection as a JSON value; [`Selection::canonical_json`] is its RFC 8785 form.
    pub fn to_value(&self) -> CValue {
        let s = |x: &str| CValue::Str(x.to_string());
        let calls = self
            .plan
            .calls
            .iter()
            .map(|c| {
                let mut m = BTreeMap::new();
                m.insert("entry_index".to_string(), s(&c.entry_index.to_string()));
                m.insert("entry_hash".to_string(), s(c.entry_hash.as_str()));
                m.insert("algorithm_id".to_string(), s(&c.algorithm_id));
                m.insert("params".to_string(), c.params.clone());
                m.insert("seed".to_string(), c.seed.map_or(CValue::Null, |x| s(&x.to_string())));
                m.insert("inputs".to_string(), CValue::Arr(c.inputs.iter().map(|d| s(d.as_str())).collect()));
                m.insert("outputs".to_string(), CValue::Arr(c.outputs.iter().map(|d| s(d.as_str())).collect()));
                let sources = c
                    .sources
                    .iter()
                    .map(|src| {
                        let mut o = BTreeMap::new();
                        match src {
                            InputSource::Call { call, output } => {
                                o.insert("call".to_string(), s(&call.to_string()));
                                o.insert("output".to_string(), s(&output.to_string()));
                            }
                            InputSource::Packaged(d) => {
                                o.insert("packaged".to_string(), s(d.as_str()));
                            }
                        }
                        CValue::Obj(o)
                    })
                    .collect();
                m.insert("sources".to_string(), CValue::Arr(sources));
                CValue::Obj(m)
            })
            .collect();
        let results = self
            .results
            .iter()
            .map(|r| {
                let mut m = BTreeMap::new();
                m.insert("record_id".to_string(), s(r.record_id()));
                match r {
                    Selected::Replayable { algorithm_id, calls, substituted, .. } => {
                        let subst = substituted
                            .iter()
                            .map(|(d, h)| {
                                let mut o = BTreeMap::new();
                                o.insert("digest".to_string(), s(d.as_str()));
                                o.insert("producer".to_string(), s(h.as_str()));
                                CValue::Obj(o)
                            })
                            .collect();
                        m.insert("substituted".to_string(), CValue::Arr(subst));
                        m.insert("replayable".to_string(), CValue::Bool(true));
                        m.insert("algorithm_id".to_string(), s(algorithm_id));
                        m.insert("calls".to_string(), CValue::Arr(calls.iter().map(|i| s(&i.to_string())).collect()));
                    }
                    Selected::NotReplayable(n) => {
                        m.insert("replayable".to_string(), CValue::Bool(false));
                        m.insert("reason".to_string(), s(n.reason.as_str()));
                        m.insert("detail".to_string(), s(&n.detail));
                    }
                }
                CValue::Obj(m)
            })
            .collect();
        let mut root = BTreeMap::new();
        root.insert("kernel_manifest_digest".to_string(), s(self.kernel_manifest_digest.as_str()));
        root.insert("calls".to_string(), CValue::Arr(calls));
        root.insert("results".to_string(), CValue::Arr(results));
        CValue::Obj(root)
    }

    /// RFC 8785 bytes of [`Selection::to_value`].
    pub fn canonical_json(&self) -> Vec<u8> {
        to_canonical_bytes(&self.to_value()).unwrap_or_default()
    }
}

// ------------------------------------------------------------------ options

/// Selection limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    /// Most kernel calls in one closure ([`MAX_PLAN_CALLS`]).
    pub max_plan_calls: usize,
    /// Total walk work over all results ([`WORK_BUDGET`]).
    pub work_budget: u64,
}

impl Default for Limits {
    fn default() -> Limits {
        Limits { max_plan_calls: MAX_PLAN_CALLS, work_budget: WORK_BUDGET }
    }
}

// ------------------------------------------------------------------ parsed content

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tier {
    Results,
    Features,
    Points,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Basis {
    Points,
    Features,
    ResultsOnly,
    DirectReading,
}

struct Record {
    id: String,
    basis: Basis,
    algorithm: String,
    journal_entry: Digest,
}

struct Command {
    determinism_n: bool,
    seed: u64,
    inputs: Vec<Digest>,
    outputs: Vec<Digest>,
    algorithms: Vec<String>,
    /// `Some(Ok(args))` when the payload's args reproduce `args_commit`; `Some(Err(()))` when
    /// they do not; `None` when absent.
    args: Option<Result<CValue, ()>>,
}

struct Entry {
    hash: Digest,
    kernel: Digest,
    command: Option<Command>,
}

impl Entry {
    fn kernel_algorithm(&self) -> Option<&str> {
        match &self.command {
            Some(c) if c.algorithms.len() == 1 => c.algorithms.first().map(String::as_str),
            _ => None,
        }
    }
}

// ------------------------------------------------------------------ JSON helpers

type Obj = BTreeMap<String, CValue>;

fn parse_json(member: &str, bytes: &[u8]) -> Result<CValue, SelectError> {
    fid_canon::parse_with_budget(bytes, bytes.len(), MAX_DEPTH, MAX_ELEMENTS)
        .map_err(|e| bad(member, "json", e.to_string()))
}

fn as_obj<'a>(member: &str, v: &'a CValue, what: &str) -> Result<&'a Obj, SelectError> {
    match v {
        CValue::Obj(o) => Ok(o),
        _ => Err(bad(member, "shape", format!("{what}: not an object"))),
    }
}

fn get<'a>(member: &str, o: &'a Obj, key: &str, reason: &'static str) -> Result<&'a CValue, SelectError> {
    o.get(key).ok_or_else(|| bad(member, reason, format!("{key}: missing")))
}

fn get_str<'a>(member: &str, o: &'a Obj, key: &str, reason: &'static str) -> Result<&'a str, SelectError> {
    match get(member, o, key, reason)? {
        CValue::Str(s) => Ok(s),
        _ => Err(bad(member, reason, format!("{key}: not a string"))),
    }
}

fn get_digest(member: &str, o: &Obj, key: &str, reason: &'static str) -> Result<Digest, SelectError> {
    let s = get_str(member, o, key, reason)?;
    Digest::parse(s).map_err(|_| bad(member, reason, format!("{key}: not a sha256: digest")))
}

fn get_arr<'a>(member: &str, o: &'a Obj, key: &str, reason: &'static str) -> Result<&'a [CValue], SelectError> {
    match get(member, o, key, reason)? {
        CValue::Arr(a) if a.len() <= MAX_DIGESTS_PER_ENTRY => Ok(a),
        CValue::Arr(_) => Err(SelectError::Limit { member: member.to_string(), what: "entry_list" }),
        _ => Err(bad(member, reason, format!("{key}: not an array"))),
    }
}

fn sha256(bytes: &[u8]) -> Digest {
    Digest::from_bytes(digest_bytes(bytes).0)
}

fn canonical(member: &str, v: &CValue) -> Result<Vec<u8>, SelectError> {
    to_canonical_bytes(v).map_err(|e| bad(member, "json", e.to_string()))
}

/// Algorithm ID with exact version: `<module>.<feature>.<method>@<major>.<minor>.<patch>`
/// (KRN.MAN.CARD), each name `[a-z][a-z0-9_]*`, each version part a u32 without leading zeros.
fn is_algorithm_id(s: &str) -> bool {
    if s.len() > 128 {
        return false;
    }
    let Some((name, version)) = s.split_once('@') else { return false };
    let names: Vec<&str> = name.split('.').collect();
    let name_ok = names.len() == 3
        && names.iter().all(|n| {
            let mut b = n.bytes();
            matches!(b.next(), Some(b'a'..=b'z'))
                && b.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'_')
        });
    let parts: Vec<&str> = version.split('.').collect();
    let version_ok = parts.len() == 3
        && parts.iter().all(|p| {
            !p.is_empty()
                && p.bytes().all(|c| c.is_ascii_digit())
                && (p.len() == 1 || !p.starts_with('0'))
                && p.parse::<u32>().is_ok()
        });
    name_ok && version_ok
}

// ------------------------------------------------------------------ readers

fn read(
    pkg: &mut dyn PackageView,
    listed: &BTreeSet<String>,
    path: &str,
    max: u64,
) -> Result<Option<Vec<u8>>, SelectError> {
    if !listed.contains(path) {
        return Ok(None);
    }
    match pkg.read_member(path, max) {
        Ok(Some(b)) if u64::try_from(b.len()).is_ok_and(|n| n <= max) => Ok(Some(b)),
        Ok(Some(_)) => Err(SelectError::Limit { member: path.to_string(), what: "member_size" }),
        Ok(None) => Err(bad(path, "missing_member", "listed in the manifest but not readable")),
        Err(e) => Err(SelectError::Package(e)),
    }
}

fn parse_records(bytes: &[u8]) -> Result<Vec<Record>, SelectError> {
    let m = RECORDS_PATH;
    let CValue::Arr(items) = parse_json(m, bytes)? else {
        return Err(bad(m, "shape", "not an array"));
    };
    if items.len() > MAX_RECORDS {
        return Err(SelectError::Limit { member: m.to_string(), what: "records" });
    }
    let mut out = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let o = as_obj(m, item, &format!("record {i}"))?;
        let id = get_str(m, o, "id", "record_field")?;
        if id.is_empty() {
            return Err(bad(m, "record_field", format!("record {i}: empty id")));
        }
        let basis = match get_str(m, o, "evaluation_basis", "record_field")? {
            "POINTS" => Basis::Points,
            "FEATURES" => Basis::Features,
            "RESULTS_ONLY" => Basis::ResultsOnly,
            "DIRECT_READING" => Basis::DirectReading,
            other => return Err(bad(m, "unknown_basis", format!("record {id:?}: {other:?}"))),
        };
        let algorithm = get_str(m, o, "algorithm", "record_field")?;
        if !is_algorithm_id(algorithm) {
            return Err(bad(m, "record_field", format!("record {id:?}: algorithm {algorithm:?}")));
        }
        let journal_entry = get_digest(m, o, "journal_entry", "record_field")?;
        out.push(Record { id: id.to_string(), basis, algorithm: algorithm.to_string(), journal_entry });
    }
    Ok(out)
}

fn parse_kernel_manifest(bytes: &[u8]) -> Result<BTreeSet<String>, SelectError> {
    let m = KERNEL_MANIFEST_PATH;
    let v = parse_json(m, bytes)?;
    if canonical(m, &v)? != bytes {
        return Err(bad(m, "kernel_manifest", "not in RFC 8785 canonical form"));
    }
    let o = as_obj(m, &v, "manifest")?;
    let keys: Vec<&str> = o.keys().map(String::as_str).collect();
    if keys != ["artifacts", "core", "core_digest"] {
        return Err(bad(m, "kernel_manifest", format!("top-level fields {keys:?}")));
    }
    let core = get(m, o, "core", "kernel_manifest")?;
    let core_digest = get_digest(m, o, "core_digest", "kernel_manifest")?;
    if sha256(&canonical(m, core)?) != core_digest {
        return Err(bad(m, "kernel_manifest", "core_digest does not match core"));
    }
    let core = as_obj(m, core, "core")?;
    const CORE_KEYS: [&str; 14] = [
        "algorithms",
        "facade_selftest_digest",
        "lockfile_digest",
        "name",
        "native_gate",
        "numeric_profile",
        "reference_results_digest",
        "reference_suite_digest",
        "replay_runtime",
        "schema_version",
        "semver",
        "solver_limits_digest",
        "source_tree_digest",
        "toolchain_digest",
    ];
    let core_keys: Vec<&str> = core.keys().map(String::as_str).collect();
    if core_keys != CORE_KEYS {
        return Err(bad(m, "kernel_manifest", format!("core fields {core_keys:?}")));
    }
    if get_str(m, core, "schema_version", "kernel_manifest")? != "1" {
        return Err(bad(m, "kernel_manifest", "schema_version is not \"1\""));
    }
    let CValue::Arr(algs) = get(m, core, "algorithms", "kernel_manifest")? else {
        return Err(bad(m, "kernel_manifest", "algorithms: not an array"));
    };
    if algs.len() > MAX_ALGORITHMS {
        return Err(SelectError::Limit { member: m.to_string(), what: "algorithms" });
    }
    let mut ids = BTreeSet::new();
    for a in algs {
        let a = as_obj(m, a, "algorithm")?;
        let keys: Vec<&str> = a.keys().map(String::as_str).collect();
        if keys != ["card_digest", "id", "impl_digest"] {
            return Err(bad(m, "kernel_manifest", format!("algorithm fields {keys:?}")));
        }
        get_digest(m, a, "card_digest", "kernel_manifest")?;
        get_digest(m, a, "impl_digest", "kernel_manifest")?;
        let id = get_str(m, a, "id", "kernel_manifest")?;
        if !is_algorithm_id(id) || id.starts_with("kernel.") {
            return Err(bad(m, "kernel_manifest", format!("algorithm id {id:?}")));
        }
        if !ids.insert(id.to_string()) {
            return Err(bad(m, "kernel_manifest", format!("algorithm {id:?} listed twice")));
        }
    }
    Ok(ids)
}

fn parse_salt(s: &str) -> Option<[u8; 16]> {
    if s.len() != 32 || !s.bytes().all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)) {
        return None;
    }
    let mut out = [0u8; 16];
    for (i, o) in out.iter_mut().enumerate() {
        *o = u8::from_str_radix(s.get(2 * i..2 * i + 2)?, 16).ok()?;
    }
    Some(out)
}

/// The payload's `args` checked against the header's `args_commit` (CORE.JRN.ENTRY: SHA-256
/// over the 16-byte salt followed by the RFC 8785 value).
fn payload_args(
    m: &str,
    payload: Option<&CValue>,
    args_commit: &Digest,
) -> Result<Option<Result<CValue, ()>>, SelectError> {
    let Some(p) = payload else { return Ok(None) };
    let p = as_obj(m, p, "payload")?;
    let salts = match p.get("salts") {
        Some(CValue::Obj(s)) => s,
        Some(_) => return Err(bad(m, "shape", "payload.salts: not an object")),
        None => return Ok(None),
    };
    let (Some(value), Some(salt)) = (p.get("args"), salts.get("args")) else { return Ok(None) };
    let CValue::Str(salt) = salt else { return Err(bad(m, "shape", "payload.salts.args: not a string")) };
    let Some(salt) = parse_salt(salt) else { return Err(bad(m, "shape", "payload.salts.args: not a salt")) };
    let mut bytes = salt.to_vec();
    bytes.extend_from_slice(&canonical(m, value)?);
    Ok(Some(if sha256(&bytes) == *args_commit { Ok(value.clone()) } else { Err(()) }))
}

fn parse_entry(index: usize, line: &[u8]) -> Result<Entry, SelectError> {
    let m = SEGMENT_PATH;
    if line.len() > MAX_LINE_BYTES {
        return Err(SelectError::Limit { member: m.to_string(), what: "line_size" });
    }
    let v = parse_json(m, line).map_err(|e| match e {
        SelectError::Malformed { member, reason, detail } => {
            SelectError::Malformed { member, reason, detail: format!("line {index}: {detail}") }
        }
        other => other,
    })?;
    if canonical(m, &v)? != line {
        return Err(bad(m, "non_canonical", format!("line {index}")));
    }
    let o = as_obj(m, &v, "line")?;
    if o.keys().any(|k| k != "header" && k != "payload") {
        return Err(bad(m, "shape", format!("line {index}: unknown field")));
    }
    let header = get(m, o, "header", "shape")?;
    let h = as_obj(m, header, "header")?;
    match h.get("schema_version") {
        Some(CValue::Num(x)) if *x == 1.0 => {}
        _ => return Err(bad(m, "shape", format!("line {index}: schema_version is not 1"))),
    }
    let hash = get_digest(m, h, "hash", "shape")?;
    let mut without = h.clone();
    without.remove("hash");
    if sha256(&canonical(m, &CValue::Obj(without))?) != hash {
        return Err(bad(m, "entry_hash_mismatch", format!("line {index}")));
    }
    let manifests = as_obj(m, get(m, h, "manifests", "shape")?, "manifests")?;
    let kernel = get_digest(m, manifests, "kernel", "shape")?;
    let kind = get_str(m, h, "kind", "shape")?;
    let command = match kind {
        "session_header" | "principal_upgrade" | "head_move" => None,
        "command" => {
            let c = as_obj(m, get(m, h, "command", "shape")?, "command")?;
            let determinism_n = match get_str(m, c, "determinism", "shape")? {
                "D0" | "D1" | "D2" | "D3" => false,
                "N" => true,
                other => return Err(bad(m, "shape", format!("line {index}: determinism {other:?}"))),
            };
            let seed_s = get_str(m, c, "seed", "shape")?;
            let seed_ok = !seed_s.is_empty()
                && seed_s.bytes().all(|b| b.is_ascii_digit())
                && (seed_s.len() == 1 || !seed_s.starts_with('0'));
            let seed = match seed_s.parse::<u64>() {
                Ok(s) if seed_ok => s,
                _ => return Err(bad(m, "shape", format!("line {index}: seed {seed_s:?}"))),
            };
            let args_commit = get_digest(m, c, "args_commit", "shape")?;
            let mut inputs = Vec::new();
            for i in get_arr(m, c, "inputs", "shape")? {
                inputs.push(get_digest(m, as_obj(m, i, "input")?, "digest", "shape")?);
            }
            let mut outputs = Vec::new();
            for i in get_arr(m, c, "outputs", "shape")? {
                outputs.push(get_digest(m, as_obj(m, i, "output")?, "digest", "shape")?);
            }
            let mut algorithms = Vec::new();
            for a in get_arr(m, c, "algorithms", "shape")? {
                match a {
                    CValue::Str(s) if is_algorithm_id(s) => algorithms.push(s.clone()),
                    _ => return Err(bad(m, "shape", format!("line {index}: algorithm {a:?}"))),
                }
            }
            let args = payload_args(m, o.get("payload"), &args_commit)?;
            Some(Command { determinism_n, seed, inputs, outputs, algorithms, args })
        }
        other => return Err(bad(m, "shape", format!("line {index}: kind {other:?}"))),
    };
    Ok(Entry { hash, kernel, command })
}

fn parse_segment(bytes: &[u8]) -> Result<Vec<Entry>, SelectError> {
    let m = SEGMENT_PATH;
    if bytes.is_empty() {
        return Ok(Vec::new());
    }
    let Some(body) = bytes.strip_suffix(b"\n") else {
        return Err(bad(m, "truncated", "the last line has no newline"));
    };
    let mut entries = Vec::new();
    for (i, line) in body.split(|&b| b == b'\n').enumerate() {
        if line.is_empty() {
            return Err(bad(m, "empty_line", format!("line {i}")));
        }
        if entries.len() >= MAX_ENTRIES {
            return Err(SelectError::Limit { member: m.to_string(), what: "entries" });
        }
        entries.push(parse_entry(i, line)?);
    }
    Ok(entries)
}

/// Packaged digests: every `points/<hex>.<ext>` member, whose name must equal its digest.
fn packaged_points(members: &[MemberInfo]) -> Result<BTreeSet<Digest>, SelectError> {
    let mut out = BTreeSet::new();
    for mem in members {
        let Some(name) = mem.path.strip_prefix(POINTS_DIR) else { continue };
        let ok = match name.split_once('.') {
            Some((hex, ext)) => {
                hex == mem.sha256.hex()
                    && !ext.is_empty()
                    && ext.bytes().all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
            }
            None => false,
        };
        if !ok {
            return Err(bad(&mem.path, "points_name", "not points/<sha256 hex of the member>.<ext>"));
        }
        if !out.insert(mem.sha256) {
            return Err(bad(&mem.path, "points_name", "the same point set is packaged twice"));
        }
    }
    Ok(out)
}

// ------------------------------------------------------------------ selection

/// [`plan_with`] with the default [`Limits`].
pub fn plan(pkg: &mut dyn PackageView, artifacts: &dyn ArtifactSource) -> Result<Selection, SelectError> {
    plan_with(pkg, artifacts, Limits::default())
}

/// Selects the replayable results of a package and builds the replay plan.
pub fn plan_with(
    pkg: &mut dyn PackageView,
    artifacts: &dyn ArtifactSource,
    limits: Limits,
) -> Result<Selection, SelectError> {
    let tier = match pkg.evidence_tier().as_str() {
        "results" => Tier::Results,
        "features" => Tier::Features,
        "points" => Tier::Points,
        other => return Err(bad("manifest.json", "evidence_tier", format!("{other:?}"))),
    };
    let kernel_digest = pkg.kernel_manifest_digest();
    if kernel_digest == NO_KERNEL_MANIFEST {
        return Err(bad(
            "manifest.json",
            "kernel_unattested",
            "kernel_manifest_digest is the reserved no-kernel digest: no kernel is attested",
        ));
    }
    let members = pkg.members();
    let listed: BTreeSet<String> = members.iter().map(|m| m.path.clone()).collect();
    let points = packaged_points(&members)?;

    let records = match read(pkg, &listed, RECORDS_PATH, MAX_RECORDS_BYTES)? {
        Some(b) => parse_records(&b)?,
        None => return Err(bad(RECORDS_PATH, "records_missing", "the package has no records")),
    };
    let algorithms = match read(pkg, &listed, KERNEL_MANIFEST_PATH, MAX_KERNEL_MANIFEST_BYTES)? {
        Some(b) => {
            if sha256(&b) != kernel_digest {
                return Err(bad(
                    KERNEL_MANIFEST_PATH,
                    "kernel_manifest_digest",
                    "does not hash to the package manifest's kernel_manifest_digest",
                ));
            }
            Some(parse_kernel_manifest(&b)?)
        }
        None => None,
    };
    let entries = match read(pkg, &listed, SEGMENT_PATH, MAX_SEGMENT_BYTES)? {
        Some(b) => parse_segment(&b)?,
        None => Vec::new(),
    };

    let mut by_hash: BTreeMap<Digest, usize> = BTreeMap::new();
    // Producers by output digest, kernel calls and other commands apart (ascending indices).
    let mut kernel_producers: BTreeMap<Digest, Vec<usize>> = BTreeMap::new();
    let mut other_producers: BTreeMap<Digest, Vec<usize>> = BTreeMap::new();
    for (i, e) in entries.iter().enumerate() {
        if by_hash.insert(e.hash, i).is_some() {
            return Err(bad(SEGMENT_PATH, "duplicate_entry_hash", format!("line {i}: {}", e.hash)));
        }
        if let Some(c) = &e.command {
            let producers = if e.kernel_algorithm().is_some() { &mut kernel_producers } else { &mut other_producers };
            for o in &c.outputs {
                let list = producers.entry(*o).or_default();
                if list.last() != Some(&i) {
                    list.push(i);
                }
            }
        }
    }

    let mut w = Walker {
        entries: &entries,
        kernel_producers: &kernel_producers,
        other_producers: &other_producers,
        points: &points,
        algorithms: algorithms.as_ref(),
        kernel_digest,
        available: None,
        artifacts,
        limits,
        budget: limits.work_budget,
        exhausted: false,
    };

    let mut results = Vec::with_capacity(records.len());
    let mut closures: Vec<(usize, BTreeSet<usize>)> = Vec::new();
    let mut seen_ids: BTreeSet<&str> = BTreeSet::new();
    for r in &records {
        if !seen_ids.insert(&r.id) {
            return Err(bad(RECORDS_PATH, "duplicate_record_id", format!("{:?}", r.id)));
        }
        let nr = |reason: Reason, detail: String| {
            Selected::NotReplayable(NotReplayable { record_id: r.id.clone(), reason, detail })
        };
        let sel = match (r.basis, tier) {
            (Basis::ResultsOnly, _) => nr(Reason::BasisResultsOnly, "evaluation_basis=RESULTS_ONLY".into()),
            (Basis::DirectReading, _) => nr(Reason::BasisDirectReading, "evaluation_basis=DIRECT_READING".into()),
            (_, Tier::Results) => nr(Reason::BasisResultsOnly, "evidence_tier=results".into()),
            (Basis::Points | Basis::Features, Tier::Points | Tier::Features) => match w.record(r, &by_hash) {
                Ok((closure, substituted)) => {
                    closures.push((results.len(), closure));
                    Selected::Replayable {
                        record_id: r.id.clone(),
                        algorithm_id: r.algorithm.clone(),
                        calls: Vec::new(),
                        substituted: substituted.into_iter().collect(),
                    }
                }
                Err((reason, detail)) => nr(reason, detail),
            },
        };
        results.push(sel);
    }

    let used: BTreeSet<usize> = closures.iter().flat_map(|(_, c)| c.iter().copied()).collect();
    let position: BTreeMap<usize, CallIndex> = used.iter().enumerate().map(|(k, &e)| (e, k)).collect();
    let mut calls = Vec::with_capacity(used.len());
    for &e in &used {
        let Some(entry) = entries.get(e) else { continue };
        let (Some(c), Some(alg)) = (&entry.command, entry.kernel_algorithm()) else { continue };
        let Some(Ok(params)) = &c.args else { continue };
        let mut sources = Vec::with_capacity(c.inputs.len());
        for d in &c.inputs {
            let (kernel, _) = w.producer(d, e);
            let source = match kernel {
                Some(p) => {
                    let call = position.get(&p).copied();
                    let output = entries
                        .get(p)
                        .and_then(|pe| pe.command.as_ref())
                        .and_then(|pc| pc.outputs.iter().position(|o| o == d));
                    match (call, output) {
                        (Some(call), Some(output)) => InputSource::Call { call, output },
                        _ => return Err(bad(SEGMENT_PATH, "plan", format!("line {e}: producer of {d} not planned"))),
                    }
                }
                None if points.contains(d) => InputSource::Packaged(*d),
                None => return Err(bad(SEGMENT_PATH, "plan", format!("line {e}: input {d} has no source"))),
            };
            sources.push(source);
        }
        calls.push(KernelCall {
            entry_index: e,
            entry_hash: entry.hash,
            algorithm_id: alg.to_string(),
            params: params.clone(),
            seed: Some(c.seed),
            inputs: c.inputs.clone(),
            sources,
            outputs: c.outputs.clone(),
        });
    }
    let mut plan_results = Vec::with_capacity(closures.len());
    for (k, closure) in &closures {
        let idx: Vec<CallIndex> = closure.iter().filter_map(|e| position.get(e).copied()).collect();
        if let Some(Selected::Replayable { record_id, calls, .. }) = results.get_mut(*k) {
            calls.clone_from(&idx);
            plan_results.push((record_id.clone(), idx));
        }
    }
    Ok(Selection { kernel_manifest_digest: kernel_digest, plan: ReplayPlan { calls, results: plan_results }, results })
}

type Fail = (Reason, String);
/// (packaged digest, producing non-kernel entry hash).
type Stand = (Digest, Digest);

struct Walker<'a> {
    entries: &'a [Entry],
    kernel_producers: &'a BTreeMap<Digest, Vec<usize>>,
    other_producers: &'a BTreeMap<Digest, Vec<usize>>,
    points: &'a BTreeSet<Digest>,
    algorithms: Option<&'a BTreeSet<String>>,
    kernel_digest: Digest,
    available: Option<bool>,
    artifacts: &'a dyn ArtifactSource,
    limits: Limits,
    budget: u64,
    exhausted: bool,
}

impl<'a> Walker<'a> {
    fn spend(&mut self) -> bool {
        if self.budget == 0 {
            self.exhausted = true;
        } else {
            self.budget -= 1;
        }
        !self.exhausted
    }

    /// (latest earlier kernel-call producer, latest earlier non-kernel producer) of `d`. A
    /// kernel producer, when there is one, is always the producer.
    fn producer(&self, d: &Digest, before: usize) -> (Option<usize>, Option<usize>) {
        let latest = |map: &BTreeMap<Digest, Vec<usize>>| {
            let list = map.get(d)?;
            let n = list.partition_point(|&i| i < before);
            n.checked_sub(1).and_then(|k| list.get(k).copied())
        };
        (latest(self.kernel_producers), latest(self.other_producers))
    }

    /// The closure of a record's call, or its primary reason.
    fn record(
        &mut self,
        r: &Record,
        by_hash: &BTreeMap<Digest, usize>,
    ) -> Result<(BTreeSet<usize>, BTreeSet<Stand>), Fail> {
        let entries: &'a [Entry] = self.entries;
        let budget_fail = || (Reason::PlanTooLarge, "work_budget".to_string());
        if self.exhausted {
            return Err(budget_fail());
        }
        let Some(&root) = by_hash.get(&r.journal_entry) else {
            return Err((Reason::EntryMissing, format!("not_found:{}", r.journal_entry)));
        };
        let Some(entry) = entries.get(root) else {
            return Err((Reason::EntryMissing, format!("not_found:{}", r.journal_entry)));
        };
        if entry.command.is_none() {
            return Err((Reason::EntryMissing, format!("not_a_command:{}", entry.hash)));
        }
        let Some(alg) = entry.kernel_algorithm() else {
            return Err((Reason::NonKernelStep, format!("root_not_kernel:{}", entry.hash)));
        };
        if alg != r.algorithm {
            return Err((Reason::EntryMissing, format!("algorithm_mismatch:{}", entry.hash)));
        }

        let mut fails: BTreeMap<Reason, BTreeSet<String>> = BTreeMap::new();
        let mut add = |reason: Reason, detail: String| {
            fails.entry(reason).or_default().insert(detail);
        };
        let mut visited: BTreeSet<usize> = BTreeSet::new();
        let mut substituted: BTreeSet<Stand> = BTreeSet::new();
        let mut stack = vec![root];
        while let Some(e) = stack.pop() {
            if !visited.insert(e) {
                continue;
            }
            if !self.spend() {
                return Err(budget_fail());
            }
            if visited.len() > self.limits.max_plan_calls {
                return Err((Reason::PlanTooLarge, format!("closure_calls>{}", self.limits.max_plan_calls)));
            }
            let Some(entry) = entries.get(e) else { continue };
            let (Some(c), Some(alg)) = (&entry.command, entry.kernel_algorithm()) else { continue };
            if !self.algorithms.is_some_and(|a| a.contains(alg)) {
                add(Reason::AlgorithmUnknown, alg.to_string());
            }
            if entry.kernel != self.kernel_digest || entry.kernel == NO_KERNEL_MANIFEST {
                add(Reason::KernelUnavailable, format!("kernel_manifest_mismatch:{}", entry.hash));
            }
            match &c.args {
                None => add(Reason::EntryMissing, format!("args_missing:{}", entry.hash)),
                Some(Err(())) => add(Reason::EntryMissing, format!("args_commitment:{}", entry.hash)),
                Some(Ok(_)) => {}
            }
            for d in &c.inputs {
                if !self.spend() {
                    return Err(budget_fail());
                }
                let (kernel, other) = self.producer(d, e);
                if let Some(p) = kernel {
                    stack.push(p);
                    continue;
                }
                let prod_entry = other.and_then(|p| entries.get(p));
                match prod_entry {
                    _ if self.points.contains(d) => {
                        if let Some(pe) = prod_entry {
                            substituted.insert((*d, pe.hash));
                        }
                    }
                    None => add(Reason::PointsMissing, d.to_string()),
                    Some(pe) => match &pe.command {
                        Some(pc) if !pc.determinism_n => {
                            add(Reason::NonKernelStep, format!("not_recomputable:{}", pe.hash));
                        }
                        _ => add(Reason::PointsMissing, d.to_string()),
                    },
                }
            }
        }
        if !self.artifact_available() {
            add(Reason::KernelUnavailable, format!("artifact_unavailable:{}", self.kernel_digest));
        }
        for reason in Reason::ALL {
            if let Some(detail) = fails.get(&reason).and_then(|s| s.iter().next()) {
                return Err((reason, detail.clone()));
            }
        }
        Ok((visited, substituted))
    }

    fn artifact_available(&mut self) -> bool {
        match self.available {
            Some(a) => a,
            None => {
                // `plan_with` refuses the reserved digest first; never ask a source for it.
                let a = self.kernel_digest != NO_KERNEL_MANIFEST && self.artifacts.is_available(&self.kernel_digest);
                self.available = Some(a);
                a
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reason_order_is_the_spec_list_then_plan_too_large() {
        let codes: Vec<&str> = Reason::ALL.iter().map(|r| r.as_str()).collect();
        assert_eq!(
            codes,
            [
                "BASIS_RESULTS_ONLY",
                "BASIS_DIRECT_READING",
                "POINTS_MISSING",
                "ENTRY_MISSING",
                "ALGORITHM_UNKNOWN",
                "KERNEL_UNAVAILABLE",
                "NON_KERNEL_STEP",
                "PLAN_TOO_LARGE",
            ]
        );
        for r in Reason::ALL {
            assert_eq!(Reason::parse(r.as_str()), Some(r));
        }
        assert_eq!(MAX_PLAN_CALLS, 100_000);
    }

    #[test]
    fn algorithm_ids() {
        assert!(is_algorithm_id("fit.circle.ls@1.0.0"));
        assert!(is_algorithm_id("a.b.c@4294967295.0.10"));
        for bad in [
            "fit.circle@1.0.0",
            "fit.circle.ls@01.0.0",
            "Fit.circle.ls@1.0.0",
            "fit.circle.ls@1.0",
            "fit.circle.ls@4294967296.0.0",
            "fit.circle.ls",
        ] {
            assert!(!is_algorithm_id(bad), "{bad}");
        }
    }

    #[test]
    fn salts() {
        assert_eq!(parse_salt("000102030405060708090a0b0c0d0e0f").map(|s| s[15]), Some(15));
        assert!(parse_salt("000102030405060708090A0B0C0D0E0F").is_none());
        assert!(parse_salt("00").is_none());
    }
}
