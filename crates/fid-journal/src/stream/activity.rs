//! The P0 Activity Log writer (REQ-CORE.JRN.STREAM.ALOG-004..-006, -008).
//!
//! # Pinned rules
//!
//! - **Line.** One record is the RFC 8785 form of its entry header (`schema/entry.schema.json`)
//!   followed by `\n`. The stream is always [`ACTIVITY_STREAM`]; `seq` counts up from 1 across
//!   rotations; `prev` is always [`GENESIS_PREV`] (no previous-hash links). The detachable
//!   payload (salts and committed values) is not written: the Activity Log keeps what the
//!   header shows (command, class, principal kind, time, input and output digests).
//! - **Classes.** [`ActivityLog::append`] takes only `command` drafts of class R0 or R1; any
//!   other draft is refused with [`CLASS_REFUSED`], so a committed (W-class or other) command
//!   can never land in the Activity Log.
//! - **One writer per directory.** The first write takes `<dir>/activity.lock`, created with
//!   `create_new`, and holds it until the log is dropped, which removes it. A second
//!   `ActivityLog` on the same directory (in this or another process) gets
//!   [`UNAVAILABLE`] warnings while the lock exists. A lock left by a crashed process must be
//!   removed by hand (or by the host at start-up) before the directory is written again.
//! - **Files.** The current file is `<dir>/activity.log`. Rotation moves it to
//!   `activity.<n>.log`, `n` counting up from 1 (one more than the highest `n` present), without
//!   ever replacing a file: it hard-links the current file to the new name, trying the next `n`
//!   while the name is taken, then removes the old name. It then deletes the lowest-numbered
//!   rotated files beyond `keep`. A line is written whole with one
//!   `write_all`; a line that would take the current file past `max_bytes` goes to a fresh
//!   file, so no line is split across files. A line longer than [`MAX_LINE_BYTES`] is refused.
//! - **Flush.** Lines are not fsynced one by one (the Activity Log is not a record of state);
//!   the file is synced at rotation and at [`ActivityLog::close`] or drop.
//! - **Recovery.** The directory is opened lazily at the first write and again after any
//!   failure. On opening, a current file whose last byte is not `\n` (a torn write) gets one,
//!   and `seq` resumes after the last parseable line of the current file, else of the newest
//!   rotated file, else at 1.
//! - **Failures.** Every failure (directory not creatable, file not writable, rename, a draft
//!   that does not build) is an [`ActivityWarning`] with code [`UNAVAILABLE`]; it is returned
//!   and also counted ([`ActivityLog::warning_count`]) for callers that cannot take it, such as
//!   the job lifecycle sink. A failed deletion of an old rotated file is retried at the next
//!   rotation and is not a warning.

use crate::entry::{
    build, AiAssistance, CommandDraft, DraftBody, EntryDraft, EntryTime, Manifests, OsSalts, OutputDigest,
    PrincipalDraft, PrincipalKind, GENESIS_PREV,
};
use fid_canon::{digest_bytes, parse_with_limits, to_canonical_bytes, CValue, Digest};
use fid_types::{Determinism, SideEffect};
use std::collections::BTreeMap;
use std::fmt;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

/// The stream name written in every Activity Log line.
pub const ACTIVITY_STREAM: &str = "activity";
/// The current file's name.
pub const CURRENT_FILE: &str = "activity.log";
/// Default rotation size: 64 MiB (REQ-006).
pub const DEFAULT_MAX_BYTES: u64 = 64 * 1024 * 1024;
/// Default number of rotated files kept (REQ-006).
pub const DEFAULT_KEEP: usize = 8;
/// Longest line written, including its `\n`.
pub const MAX_LINE_BYTES: usize = 1024 * 1024;
/// The warning code of a failed write (the leaf's only error).
pub const UNAVAILABLE: &str = "activity_log_unavailable";
/// The code of a draft refused because it is not an R0 or R1 command.
pub const CLASS_REFUSED: &str = "activity_class_refused";
/// The lock file that makes one writer per directory.
pub const LOCK_FILE: &str = "activity.lock";
/// Most names tried for one rotation before giving up.
const MAX_ROTATION_TRIES: u64 = 1000;
/// Command ID of a job lifecycle record.
pub const LIFECYCLE_COMMAND: &str = "job.lifecycle";
/// Branch of a job lifecycle record (jobs are not branch-scoped).
pub const LIFECYCLE_BRANCH: &str = "main";
/// Identity committed for the principal of a lifecycle record: the scheduler (kind `system`).
pub const LIFECYCLE_IDENTITY: &str = "scheduler";

/// A failed Activity Log write. The call that produced the record still succeeds (REQ-008).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityWarning {
    /// [`UNAVAILABLE`], or [`CLASS_REFUSED`] for a draft that may not be written here.
    pub code: &'static str,
    /// What failed, for the log of the host.
    pub detail: String,
}

impl ActivityWarning {
    fn new(detail: impl Into<String>) -> ActivityWarning {
        ActivityWarning { code: UNAVAILABLE, detail: detail.into() }
    }
}

impl fmt::Display for ActivityWarning {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for ActivityWarning {}

/// Rotation settings (REQ-006).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActivityLogConfig {
    /// The current file rotates before a line would take it past this many bytes.
    pub max_bytes: u64,
    /// How many rotated files are kept; older ones are deleted.
    pub keep: usize,
}

impl Default for ActivityLogConfig {
    fn default() -> Self {
        ActivityLogConfig { max_bytes: DEFAULT_MAX_BYTES, keep: DEFAULT_KEEP }
    }
}

/// Wall-clock source for the `time.utc` of lifecycle records.
pub trait UtcClock: Send + Sync {
    /// Time since the Unix epoch (UTC).
    fn now(&self) -> Duration;
}

/// The system clock.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemUtc;

impl UtcClock for SystemUtc {
    fn now(&self) -> Duration {
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default()
    }
}

/// RFC 3339 UTC with nine fractional digits, e.g. `2026-11-19T08:00:00.000000000Z`.
pub fn format_utc(since_epoch: Duration) -> String {
    let secs = since_epoch.as_secs();
    let (y, m, d) = civil_from_days(secs / 86_400);
    let rem = secs % 86_400;
    format!(
        "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}.{:09}Z",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60,
        since_epoch.subsec_nanos()
    )
}

/// Proleptic Gregorian date of a day count since 1970-01-01 (H. Hinnant's algorithm).
fn civil_from_days(days: u64) -> (u64, u64, u64) {
    let z = days.saturating_add(719_468);
    let era = z / 146_097;
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe.saturating_add(era.saturating_mul(400));
    (if m <= 2 { y.saturating_add(1) } else { y }, m, d)
}

/// The name of rotated file number `n`: `activity.<n>.log`.
pub fn rotated_file_name(n: u64) -> String {
    format!("activity.{n}.log")
}

fn parse_rotated(name: &str) -> Option<u64> {
    let digits = name.strip_prefix("activity.")?.strip_suffix(".log")?;
    if digits.is_empty() || !digits.bytes().all(|b| b.is_ascii_digit()) || (digits.len() > 1 && digits.starts_with('0'))
    {
        return None;
    }
    digits.parse().ok()
}

/// One job lifecycle change, as plain strings (`fid-jobs` maps its `LifecycleEvent` to this).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct JobChange<'a> {
    /// The job ID.
    pub job: &'a str,
    /// The job's command ID, e.g. `model.rebuild@1`.
    pub command: &'a str,
    /// The kind of principal that submitted the job.
    pub principal_kind: &'a str,
    /// The previous state; `None` for the submission.
    pub from: Option<&'a str>,
    /// The new state.
    pub to: &'a str,
    /// The error code, for `failed` and `cancelled`.
    pub error: Option<&'a str>,
    /// Monotonic clock time of the change, in nanoseconds.
    pub at_ns: u64,
    /// Wall-clock time of the change since the Unix epoch, when the source knows it; written as
    /// `time.utc`. `None` stamps the write time instead.
    pub wall: Option<Duration>,
}

impl JobChange<'_> {
    /// The transition object `{command, error, from, job, principal_kind, to}`.
    pub fn to_value(&self) -> CValue {
        let opt = |v: Option<&str>| v.map_or(CValue::Null, |s| CValue::Str(s.to_string()));
        let mut o = BTreeMap::new();
        o.insert("command".to_string(), CValue::Str(self.command.to_string()));
        o.insert("error".to_string(), opt(self.error));
        o.insert("from".to_string(), opt(self.from));
        o.insert("job".to_string(), CValue::Str(self.job.to_string()));
        o.insert("principal_kind".to_string(), CValue::Str(self.principal_kind.to_string()));
        o.insert("to".to_string(), CValue::Str(self.to.to_string()));
        CValue::Obj(o)
    }

    /// SHA-256 over the RFC 8785 transition object: the output digest of the lifecycle line.
    pub fn digest(&self) -> Result<Digest, ActivityWarning> {
        to_canonical_bytes(&self.to_value())
            .map(|b| digest_bytes(&b))
            .map_err(|e| ActivityWarning::new(format!("lifecycle change: {e}")))
    }
}

struct State {
    /// The lock file, once this log holds it.
    lock: Option<PathBuf>,
    file: Option<File>,
    size: u64,
    next_seq: u64,
    seq_known: bool,
    next_rotation: u64,
}

/// The local, unchained, rotating Activity Log.
pub struct ActivityLog {
    dir: PathBuf,
    config: ActivityLogConfig,
    manifests: Manifests,
    clock: Arc<dyn UtcClock>,
    state: Mutex<State>,
    warnings: AtomicU64,
    last_warning: Mutex<Option<ActivityWarning>>,
}

impl fmt::Debug for ActivityLog {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ActivityLog").field("dir", &self.dir).field("config", &self.config).finish_non_exhaustive()
    }
}

impl ActivityLog {
    /// An Activity Log in `dir`. Nothing is touched until the first write, so this never fails.
    /// `manifests` are the registry and kernel manifests stamped on lifecycle records.
    pub fn new(dir: impl Into<PathBuf>, config: ActivityLogConfig, manifests: Manifests) -> ActivityLog {
        ActivityLog::with_clock(dir, config, manifests, Arc::new(SystemUtc))
    }

    /// [`ActivityLog::new`] with an injected wall clock.
    pub fn with_clock(
        dir: impl Into<PathBuf>,
        config: ActivityLogConfig,
        manifests: Manifests,
        clock: Arc<dyn UtcClock>,
    ) -> ActivityLog {
        ActivityLog {
            dir: dir.into(),
            config,
            manifests,
            clock,
            state: Mutex::new(State {
                lock: None,
                file: None,
                size: 0,
                next_seq: 1,
                seq_known: false,
                next_rotation: 1,
            }),
            warnings: AtomicU64::new(0),
            last_warning: Mutex::new(None),
        }
    }

    /// The directory holding the log files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// The rotation settings.
    pub fn config(&self) -> ActivityLogConfig {
        self.config
    }

    /// Writes one record built from `draft` (its `stream` is replaced by `activity`) and
    /// returns its entry hash. Only `command` drafts of class R0 or R1 are taken; anything else
    /// is refused with [`CLASS_REFUSED`] and nothing is written. A failure is a warning: the
    /// caller's call still succeeds.
    pub fn append(&self, draft: &EntryDraft) -> Result<Digest, ActivityWarning> {
        let r = match &draft.body {
            DraftBody::Command(c) if matches!(c.class, SideEffect::R0 | SideEffect::R1) => self.append_inner(draft),
            DraftBody::Command(c) => Err(ActivityWarning {
                code: CLASS_REFUSED,
                detail: format!("class {} never goes to the Activity Log", c.class),
            }),
            _ => {
                Err(ActivityWarning { code: CLASS_REFUSED, detail: format!("a {} entry is not a call", draft.kind()) })
            }
        };
        if let Err(w) = &r {
            self.note(w);
        }
        r
    }

    /// Writes one job lifecycle change, whatever the recording flag (REQ-004).
    ///
    /// The record is a `command` entry of [`LIFECYCLE_COMMAND`] (class R0, determinism N) by
    /// principal kind `system` ([`LIFECYCLE_IDENTITY`]) on [`LIFECYCLE_BRANCH`]. Its committed
    /// arguments are the transition object ([`JobChange::to_value`]); its one output names the
    /// job as entity, with [`JobChange::digest`] as digest. `time.mono_ns` is the change's
    /// clock time; `time.utc` is [`JobChange::wall`], the change's own wall-clock time, or the
    /// injected wall clock's reading at write time when the change carries none.
    pub fn record_lifecycle(&self, change: &JobChange<'_>) -> Result<Digest, ActivityWarning> {
        let r = self.lifecycle_draft(change).and_then(|d| self.append_inner(&d));
        if let Err(w) = &r {
            self.note(w);
        }
        r
    }

    /// The draft [`ActivityLog::record_lifecycle`] writes.
    pub fn lifecycle_draft(&self, change: &JobChange<'_>) -> Result<EntryDraft, ActivityWarning> {
        let digest = change.digest()?;
        Ok(EntryDraft {
            stream: ACTIVITY_STREAM.to_string(),
            branch: LIFECYCLE_BRANCH.to_string(),
            logical_parent: None,
            time: EntryTime {
                utc: format_utc(change.wall.unwrap_or_else(|| self.clock.now())),
                mono_ns: change.at_ns,
                sync: "unknown".to_string(),
            },
            principal: PrincipalDraft {
                kind: PrincipalKind::System,
                identity: CValue::Str(LIFECYCLE_IDENTITY.to_string()),
                on_behalf_of: None,
            },
            ai_assistance: AiAssistance::None,
            manifests: self.manifests,
            reason: None,
            human_assertion: None,
            agent_coedited: false,
            body: DraftBody::Command(CommandDraft {
                id: LIFECYCLE_COMMAND.to_string(),
                major: 1,
                class: SideEffect::R0,
                determinism: Determinism::N,
                seed: 0,
                args: change.to_value(),
                events: Vec::new(),
                inputs: Vec::new(),
                consumed_r1: Vec::new(),
                outputs: vec![OutputDigest { entity: Some(change.job.to_string()), digest }],
                algorithms: Vec::new(),
                event_sources: Vec::new(),
            }),
        })
    }

    /// Syncs and closes the current file. The next write opens it again.
    pub fn close(&self) -> Result<(), ActivityWarning> {
        let mut st = self.lock();
        let r = match st.file.take() {
            Some(f) => f.sync_all().map_err(|e| ActivityWarning::new(format!("sync {CURRENT_FILE}: {e}"))),
            None => Ok(()),
        };
        drop(st);
        if let Err(w) = &r {
            self.note(w);
        }
        r
    }

    /// How many writes have failed since this log was created.
    pub fn warning_count(&self) -> u64 {
        self.warnings.load(Ordering::SeqCst)
    }

    /// The most recent warning, cleared by this call.
    pub fn take_last_warning(&self) -> Option<ActivityWarning> {
        self.last_warning.lock().unwrap_or_else(PoisonError::into_inner).take()
    }

    fn note(&self, w: &ActivityWarning) {
        self.warnings.fetch_add(1, Ordering::SeqCst);
        *self.last_warning.lock().unwrap_or_else(PoisonError::into_inner) = Some(w.clone());
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn append_inner(&self, draft: &EntryDraft) -> Result<Digest, ActivityWarning> {
        let mut st = self.lock();
        if st.file.is_none() {
            self.open_current(&mut st)?;
        }
        let mut d = draft.clone();
        d.stream = ACTIVITY_STREAM.to_string();
        let entry = build(&d, st.next_seq, GENESIS_PREV, &mut OsSalts)
            .map_err(|e| ActivityWarning::new(format!("record does not build: {e}")))?;
        let mut line = to_canonical_bytes(&entry.header.to_value())
            .map_err(|e| ActivityWarning::new(format!("record does not serialise: {e}")))?;
        line.push(b'\n');
        if line.len() > MAX_LINE_BYTES {
            return Err(ActivityWarning::new(format!("line of {} bytes exceeds {MAX_LINE_BYTES}", line.len())));
        }
        let len = u64::try_from(line.len()).unwrap_or(u64::MAX);
        if st.size > 0 && st.size.saturating_add(len) > self.config.max_bytes {
            self.rotate(&mut st)?;
        }
        let Some(file) = st.file.as_mut() else {
            return Err(ActivityWarning::new("no current file"));
        };
        if let Err(e) = file.write_all(&line) {
            // A torn line is repaired with a newline when the file is opened again.
            st.file = None;
            return Err(ActivityWarning::new(format!("write {CURRENT_FILE}: {e}")));
        }
        st.size = st.size.saturating_add(len);
        st.next_seq = st.next_seq.saturating_add(1);
        Ok(entry.header.hash)
    }

    /// Opens (creating if needed) the directory and the current file.
    fn open_current(&self, st: &mut State) -> Result<(), ActivityWarning> {
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| ActivityWarning::new(format!("create {}: {e}", self.dir.display())))?;
        if st.lock.is_none() {
            let lock = self.dir.join(LOCK_FILE);
            OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(&lock)
                .map_err(|e| ActivityWarning::new(format!("{} (one writer per directory): {e}", lock.display())))?;
            st.lock = Some(lock);
        }
        let rotated = self.rotated()?;
        st.next_rotation = rotated.last().map_or(1, |(n, _)| n.saturating_add(1));
        let path = self.dir.join(CURRENT_FILE);
        let mut file = OpenOptions::new()
            .read(true)
            .append(true)
            .create(true)
            .open(&path)
            .map_err(|e| ActivityWarning::new(format!("open {}: {e}", path.display())))?;
        let mut size = file.metadata().map_err(|e| ActivityWarning::new(format!("stat {CURRENT_FILE}: {e}")))?.len();
        if size > 0 && last_byte(&mut file, size)? != b'\n' {
            file.write_all(b"\n").map_err(|e| ActivityWarning::new(format!("repair {CURRENT_FILE}: {e}")))?;
            size = size.saturating_add(1);
        }
        if !st.seq_known {
            let mut last = last_seq(&mut file, size);
            if last.is_none() {
                if let Some((_, p)) = rotated.last() {
                    last = File::open(p).ok().and_then(|mut f| {
                        let len = f.metadata().ok()?.len();
                        last_seq(&mut f, len)
                    });
                }
            }
            st.next_seq = last.map_or(1, |s| s.saturating_add(1));
            st.seq_known = true;
        }
        st.size = size;
        st.file = Some(file);
        Ok(())
    }

    /// Rotated files present, sorted by number.
    fn rotated(&self) -> Result<Vec<(u64, PathBuf)>, ActivityWarning> {
        let rd = std::fs::read_dir(&self.dir)
            .map_err(|e| ActivityWarning::new(format!("list {}: {e}", self.dir.display())))?;
        let mut out = Vec::new();
        for entry in rd.flatten() {
            if let Some(n) = entry.file_name().to_str().and_then(parse_rotated) {
                out.push((n, entry.path()));
            }
        }
        out.sort();
        Ok(out)
    }

    fn rotate(&self, st: &mut State) -> Result<(), ActivityWarning> {
        if let Some(f) = st.file.take() {
            // The flush point of the leaf's Design: rotation and close.
            f.sync_all().map_err(|e| ActivityWarning::new(format!("sync {CURRENT_FILE}: {e}")))?;
        }
        let from = self.dir.join(CURRENT_FILE);
        // No-clobber move: a hard link fails on an existing name, so a rotated file is never
        // replaced; a taken name moves on to the next number.
        let mut linked = false;
        for _ in 0..MAX_ROTATION_TRIES {
            let to = self.dir.join(rotated_file_name(st.next_rotation));
            st.next_rotation = st.next_rotation.saturating_add(1);
            match std::fs::hard_link(&from, &to) {
                Ok(()) => {
                    linked = true;
                    break;
                }
                Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(ActivityWarning::new(format!("link {CURRENT_FILE} to {}: {e}", to.display()))),
            }
        }
        if !linked {
            return Err(ActivityWarning::new("no free rotated file name"));
        }
        std::fs::remove_file(&from).map_err(|e| ActivityWarning::new(format!("remove old {CURRENT_FILE}: {e}")))?;
        if let Ok(rotated) = self.rotated() {
            let excess = rotated.len().saturating_sub(self.config.keep);
            for (_, p) in rotated.iter().take(excess) {
                // A failed deletion is retried at the next rotation.
                let _ = std::fs::remove_file(p);
            }
        }
        self.open_current(st)
    }
}

impl Drop for ActivityLog {
    fn drop(&mut self) {
        let mut st = self.lock();
        if let Some(f) = st.file.take() {
            let _ = f.sync_all();
        }
        if let Some(lock) = st.lock.take() {
            let _ = std::fs::remove_file(lock);
        }
    }
}

fn last_byte(file: &mut File, size: u64) -> Result<u8, ActivityWarning> {
    let mut b = [0u8; 1];
    file.seek(SeekFrom::Start(size.saturating_sub(1)))
        .and_then(|_| file.read_exact(&mut b))
        .map_err(|e| ActivityWarning::new(format!("read {CURRENT_FILE}: {e}")))?;
    Ok(b[0])
}

/// `seq` of the last parseable Activity Log line among the file's last [`MAX_LINE_BYTES`] + 1
/// bytes; `None` when there is none.
fn last_seq(file: &mut File, size: u64) -> Option<u64> {
    let window = u64::try_from(MAX_LINE_BYTES).ok()?.saturating_add(1).min(size);
    let start = size - window;
    file.seek(SeekFrom::Start(start)).ok()?;
    let mut buf = Vec::new();
    Read::take(&mut *file, window).read_to_end(&mut buf).ok()?;
    let mut parts: Vec<&[u8]> = buf.split(|b| *b == b'\n').collect();
    if start > 0 && !parts.is_empty() {
        // The first piece may be the tail of an earlier line.
        parts.remove(0);
    }
    parts.iter().rev().filter(|p| !p.is_empty()).find_map(|p| line_seq(p))
}

fn line_seq(line: &[u8]) -> Option<u64> {
    let CValue::Obj(o) = parse_with_limits(line, MAX_LINE_BYTES, 64).ok()? else { return None };
    if o.get("stream") != Some(&CValue::Str(ACTIVITY_STREAM.to_string())) {
        return None;
    }
    match o.get("seq") {
        Some(CValue::Str(s)) => s.parse().ok(),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn utc_formatting_matches_known_instants() {
        assert_eq!(format_utc(Duration::ZERO), "1970-01-01T00:00:00.000000000Z");
        // 2000-02-29T12:34:56Z (a leap day) and 2030-01-01T00:00:00Z.
        assert_eq!(format_utc(Duration::new(951_827_696, 7)), "2000-02-29T12:34:56.000000007Z");
        assert_eq!(format_utc(Duration::from_secs(1_893_456_000)), "2030-01-01T00:00:00.000000000Z");
        assert_eq!(format_utc(Duration::from_secs(4_107_542_399)), "2100-02-28T23:59:59.000000000Z");
    }

    #[test]
    fn rotated_names_round_trip() {
        assert_eq!(parse_rotated(&rotated_file_name(12)), Some(12));
        for bad in ["activity.log", "activity..log", "activity.01.log", "activity.x.log", "activity.1.txt"] {
            assert_eq!(parse_rotated(bad), None, "{bad}");
        }
    }
}
