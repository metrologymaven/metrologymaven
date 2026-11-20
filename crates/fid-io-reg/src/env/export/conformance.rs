//! The shared conformance kit for commands built from the export skeleton
//! (REQ-IO.REG.ENV.EXPORT-005). A format leaf calls [`check`] with its writer, a source and an
//! empty export root; the kit runs `export.file@1` and the format's `export.<format>@1` side
//! by side and fails unless both show the same behaviour:
//!
//! 1. **dry run**: a report with `dry_run: true`, no journal record, the root unchanged;
//! 2. **writer failure**: the writer wrapped in [`FailAfter`] fails after
//!    [`Fixture::fail_after`] bytes; the export fails, the root is unchanged (no file, no temp
//!    file) and there is no record;
//! 3. **cancel**: with the cancel flag set the export fails `cancelled`, root unchanged;
//! 4. **export**: the file appears, its SHA-256 and size equal the report's, the dry run's
//!    and the record's, and the record names the command;
//! 5. **no clobbering**: the same export again fails `target_exists`, the file unchanged;
//! 6. **replay**: after the file is removed, replaying the record is `IDENTICAL`.
//!
//! Both commands must produce the same report and the same record apart from its command ID.
//! The kit removes only the file it wrote.

use super::{
    ByteSink, ExportCommand, ExportEnv, ExportError, ExportEvent, ExportFile, ExportOutput, ExportParams, SourceRef,
    Writer,
};
use crate::detect::Registry;
use crate::error::{IoError, Position};
use crate::limits::Principal;
use crate::prov::VersionToken;
use crate::report::{FormatId, ReportBuilder};
use crate::roots::RootSet;
use fid_canon::CValue;
use fid_replay::ReplayOutcome;
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// A writer that fails with `io{Other}` once its output would pass `n` bytes; otherwise
/// the wrapped writer.
#[derive(Debug, Clone, Copy)]
pub struct FailAfter<W> {
    /// The wrapped writer.
    pub inner: W,
    /// Bytes allowed before the failure.
    pub n: u64,
}

struct FailSink<'a> {
    inner: &'a mut dyn ByteSink,
    n: u64,
}

impl ByteSink for FailSink<'_> {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        let at = self.inner.bytes_written();
        let room = usize::try_from(self.n.saturating_sub(at)).unwrap_or(usize::MAX);
        if buf.len() > room {
            self.inner.write_bytes(buf.get(..room).unwrap_or_default())?;
            return Err(IoError::Io { kind: io::ErrorKind::Other, at: Position::byte(self.n) });
        }
        self.inner.write_bytes(buf)
    }

    fn bytes_written(&self) -> u64 {
        self.inner.bytes_written()
    }
}

impl<W: Writer> Writer for FailAfter<W> {
    type Source = W::Source;

    fn format(&self) -> &FormatId {
        self.inner.format()
    }

    fn version(&self) -> &VersionToken {
        self.inner.version()
    }

    fn options(&self, given: &CValue) -> Result<CValue, &'static str> {
        self.inner.options(given)
    }

    fn write(
        &self,
        src: &Self::Source,
        opts: &CValue,
        sink: &mut dyn ByteSink,
        report: &mut ReportBuilder,
    ) -> Result<(), IoError> {
        let mut s = FailSink { inner: sink, n: self.n };
        self.inner.write(src, opts, &mut s, report)
    }
}

/// What the kit runs against.
#[derive(Debug, Clone, Copy)]
pub struct Fixture<'a> {
    /// The format registry; the writer's format must be registered for export.
    pub registry: &'a Registry,
    /// The roots; `root_id` must be an export root at `root_dir`.
    pub roots: &'a RootSet,
    /// The export root's ID.
    pub root_id: &'a str,
    /// The export root's folder; must be empty when the kit starts.
    pub root_dir: &'a Path,
    /// The file name to export to (with the format's extension).
    pub file_name: &'a str,
    /// The `source` parameter recorded.
    pub source: &'a SourceRef,
    /// Writer options.
    pub options: &'a CValue,
    /// Bytes the failing writer gets through; less than the full export.
    pub fail_after: u64,
}

/// A conformance failure: which step, for which command, and what was seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceError {
    /// The step (`dry_run`, `writer_failure`, `cancel`, `export`, `no_clobber`, `replay`,
    /// `setup`, `same_behaviour`).
    pub step: &'static str,
    /// The command ID.
    pub command: String,
    /// What went wrong.
    pub detail: String,
}

impl fmt::Display for ConformanceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} {}: {}", self.command, self.step, self.detail)
    }
}

impl std::error::Error for ConformanceError {}

/// The folder's entry names, sorted.
fn listing(dir: &Path) -> Result<Vec<String>, String> {
    let mut names = Vec::new();
    for e in std::fs::read_dir(dir).map_err(|e| format!("cannot list the root: {:?}", e.kind()))? {
        let e = e.map_err(|e| format!("cannot list the root: {:?}", e.kind()))?;
        names.push(e.file_name().to_string_lossy().into_owned());
    }
    names.sort();
    Ok(names)
}

type Run<'r, S> = dyn Fn(&ExportEnv<'_>, &ExportParams, &S) -> Result<ExportOutput, ExportError> + 'r;
type Replay<'r, S> = dyn Fn(&ExportEnv<'_>, &ExportEvent, &S) -> Result<ReplayOutcome, ExportError> + 'r;

/// One command under test: how to run it normally, with the failing writer, and replay it.
struct Subject<'r, S: ?Sized> {
    id: String,
    with_format: bool,
    run: Box<Run<'r, S>>,
    run_failing: Box<Run<'r, S>>,
    replay: Box<Replay<'r, S>>,
}

struct Seen {
    dry: ExportOutput,
    real: ExportOutput,
}

fn run_subject<S: ?Sized>(
    fx: &Fixture<'_>,
    format: &FormatId,
    s: &Subject<'_, S>,
    src: &S,
) -> Result<Seen, ConformanceError> {
    let fail = |step: &'static str, detail: String| ConformanceError { step, command: s.id.clone(), detail };
    let before = listing(fx.root_dir).map_err(|d| fail("setup", d))?;
    if !before.is_empty() {
        return Err(fail("setup", "the export root is not empty".into()));
    }
    let unchanged = |step: &'static str| -> Result<(), ConformanceError> {
        let now = listing(fx.root_dir).map_err(|d| fail(step, d))?;
        if now == before {
            Ok(())
        } else {
            Err(fail(step, format!("the export root changed: {now:?}")))
        }
    };
    let target = format!("root://{}/{}", fx.root_id, fx.file_name);
    let mut params = ExportParams::new(fx.source.clone(), s.with_format.then(|| format.clone()), &target);
    params.options = fx.options.clone();
    let env = ExportEnv::new(fx.registry, fx.roots, Principal::Script);

    // 1. Dry run.
    let mut dry_params = params.clone();
    dry_params.dry_run = true;
    let dry = (s.run)(&env, &dry_params, src).map_err(|e| fail("dry_run", format!("failed: {e}")))?;
    if dry.event.is_some() || !dry.report.dry_run() {
        return Err(fail("dry_run", "a dry run must report dry_run and journal nothing".into()));
    }
    unchanged("dry_run")?;

    // 2. Writer failure.
    match (s.run_failing)(&env, &params, src) {
        Ok(_) => return Err(fail("writer_failure", "the failing writer's export succeeded".into())),
        Err(ExportError::Io(IoError::Io { .. })) => {}
        Err(e) => return Err(fail("writer_failure", format!("unexpected error {}", e.code()))),
    }
    unchanged("writer_failure")?;

    // 3. Cancel.
    let flag = AtomicBool::new(true);
    let cancel_env = ExportEnv { cancel: Some(&flag), ..env };
    match (s.run)(&cancel_env, &params, src) {
        Err(ExportError::Cancelled) => {}
        Ok(_) => return Err(fail("cancel", "a cancelled export succeeded".into())),
        Err(e) => return Err(fail("cancel", format!("unexpected error {}", e.code()))),
    }
    unchanged("cancel")?;

    // 4. Export.
    let real = (s.run)(&env, &params, src).map_err(|e| fail("export", format!("failed: {e}")))?;
    let path = fx.root_dir.join(fx.file_name);
    let bytes = std::fs::read(&path).map_err(|e| fail("export", format!("no file: {:?}", e.kind())))?;
    let sha = format!("sha256:{}", hex(&Sha256::digest(&bytes)));
    let Some(event) = &real.event else { return Err(fail("export", "no journal record".into())) };
    let same = real.report.sha256().as_str() == sha
        && real.report.bytes() == bytes.len() as u64
        && event.sha256.as_str() == sha
        && event.bytes == real.report.bytes()
        && dry.report.sha256() == real.report.sha256()
        && dry.report.bytes() == real.report.bytes()
        && event.command.as_str() == s.id
        && event.lossy == *real.report.lossy();
    if !same {
        return Err(fail("export", "file, report, dry run and record disagree".into()));
    }
    let after = listing(fx.root_dir).map_err(|d| fail("export", d))?;
    if after != [fx.file_name.to_string()] {
        return Err(fail("export", format!("unexpected files: {after:?}")));
    }
    if ExportEvent::from_json(&event.to_json().map_err(|e| fail("export", e.to_string()))?).as_ref() != Ok(event) {
        return Err(fail("export", "the record does not read back".into()));
    }

    // 5. No clobbering.
    match (s.run)(&env, &params, src) {
        Err(ExportError::Io(IoError::TargetExists)) => {}
        Ok(_) => return Err(fail("no_clobber", "a second export overwrote the file".into())),
        Err(e) => return Err(fail("no_clobber", format!("unexpected error {}", e.code()))),
    }
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        return Err(fail("no_clobber", "the file changed".into()));
    }

    // 6. Replay into the emptied root.
    std::fs::remove_file(&path).map_err(|e| fail("replay", format!("cannot remove: {:?}", e.kind())))?;
    match (s.replay)(&env, event, src) {
        Ok(ReplayOutcome::Identical) => {}
        Ok(o) => return Err(fail("replay", format!("outcome {}", o.as_str()))),
        Err(e) => return Err(fail("replay", format!("failed: {e}"))),
    }
    std::fs::remove_file(&path).map_err(|e| fail("replay", format!("cannot remove: {:?}", e.kind())))?;
    Ok(Seen { dry, real })
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

/// Runs the kit for `writer` (see the module documentation).
pub fn check<W: Writer>(writer: &W, src: &W::Source, fx: &Fixture<'_>) -> Result<(), ConformanceError> {
    let format = writer.format().clone();
    let setup = |detail: String| ConformanceError { step: "setup", command: String::new(), detail };
    let mut file = ExportFile::new();
    file.add(Box::new(writer)).map_err(|e| setup(e.to_string()))?;
    let mut failing = ExportFile::new();
    failing.add(Box::new(FailAfter { inner: writer, n: fx.fail_after })).map_err(|e| setup(e.to_string()))?;
    let cmd = ExportCommand::new(writer).map_err(|e| setup(e.to_string()))?;
    let failing_cmd =
        ExportCommand::new(FailAfter { inner: writer, n: fx.fail_after }).map_err(|e| setup(e.to_string()))?;

    let generic = Subject {
        id: super::EXPORT_FILE.to_string(),
        with_format: true,
        run: Box::new(|env: &ExportEnv<'_>, p: &ExportParams, s: &W::Source| file.run(env, p, s)),
        run_failing: Box::new(|env: &ExportEnv<'_>, p: &ExportParams, s: &W::Source| failing.run(env, p, s)),
        replay: Box::new(|env: &ExportEnv<'_>, e: &ExportEvent, s: &W::Source| file.replay(env, e, s)),
    };
    let specific = Subject {
        id: cmd.id().as_str().to_string(),
        with_format: false,
        run: Box::new(|env: &ExportEnv<'_>, p: &ExportParams, s: &W::Source| cmd.run(env, p, s)),
        run_failing: Box::new(|env: &ExportEnv<'_>, p: &ExportParams, s: &W::Source| failing_cmd.run(env, p, s)),
        replay: Box::new(|env: &ExportEnv<'_>, e: &ExportEvent, s: &W::Source| cmd.replay(env, e, s)),
    };
    let a = run_subject(fx, &format, &generic, src)?;
    let b = run_subject(fx, &format, &specific, src)?;

    let differ = |detail: &str| ConformanceError {
        step: "same_behaviour",
        command: specific.id.clone(),
        detail: detail.to_string(),
    };
    if a.dry.report != b.dry.report || a.real.report != b.real.report {
        return Err(differ("the reports differ from export.file@1's"));
    }
    match (a.real.event, b.real.event) {
        (Some(mut x), Some(y)) => {
            x.command = y.command.clone();
            if x != y {
                return Err(differ("the journal records differ from export.file@1's beyond the command ID"));
            }
        }
        _ => return Err(differ("a journal record is missing")),
    }
    Ok(())
}
