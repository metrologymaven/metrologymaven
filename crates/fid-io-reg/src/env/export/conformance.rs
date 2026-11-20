//! The shared conformance kit for commands built from the export skeleton
//! (REQ-IO.REG.ENV.EXPORT-005). A format leaf calls [`check`] with the command under test
//! (`export.file@1` or its `export.<format>@1`, as an [`ExportRun`]), that command's registry
//! definition, a source, the digest its output must have (stored by the leaf's independent
//! reference) and an empty export root. The kit runs the real command path; only the sink is
//! faulted ([`Fault`]). It fails unless:
//!
//! 1. **definition**: the definition names the command, is W1 with `fs_effect: write_root`,
//!    D0, declares its own record as its effect, has a `path_like` `target`, a `dry_run`
//!    flag, and `format` exactly when the command takes one;
//! 2. **dry run**: a report with `dry_run: true` and the stored digest, no record, the root
//!    unchanged (so a writer whose bytes vary per process, by time or hash order, fails);
//! 3. **sink failure** after [`Fixture::fail_after`] bytes: the export fails, the root is
//!    unchanged (no file, no temp file);
//! 4. **cancel** before the start, and **mid-write** (right after the writer's first write):
//!    `cancelled`, root unchanged;
//! 5. **prepare, then place**: after prepare the target does not exist; after place the file
//!    has the stored digest, equal to the report's and the record's, and the record names the
//!    command and reads back from its canonical JSON;
//! 6. **prepare, then discard**: the root is unchanged;
//! 7. **no clobbering**: the same export again fails `target_exists`, the file unchanged;
//! 8. **replay** into the emptied root is `IDENTICAL`;
//! 9. **options**: every option the format's export descriptor declares (the `properties` of
//!    its options schema) is varied by some [`Fixture::option_variants`] entry, and each
//!    variant either changes the output digest or is refused (`bad_options`).
//!
//! The kit removes only the file it wrote.

use super::{
    ByteSink, ExportEnv, ExportError, ExportEvent, ExportParams, ExportRun, Fault, SourceRef, Writer, EXPORT_FILE,
};
use crate::detect::Registry;
use crate::error::{IoError, Position};
use crate::limits::Principal;
use crate::prov::VersionToken;
use crate::report::{FormatId, ReportBuilder};
use crate::roots::{Direction, RootSet};
use fid_canon::CValue;
use fid_registry::CommandDef;
use fid_replay::ReplayOutcome;
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::fmt;
use std::io;
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// A writer that itself fails (`io{Other}`, returned by the writer, not the sink) once its
/// output would pass `n` bytes; otherwise the wrapped writer. For AC-3.
#[derive(Debug, Clone, Copy)]
pub struct FailAfter<W> {
    /// The wrapped writer.
    pub inner: W,
    /// Bytes allowed before the failure.
    pub n: u64,
}

struct Cut<'a> {
    inner: &'a mut dyn ByteSink,
    n: u64,
    tripped: bool,
}

impl ByteSink for Cut<'_> {
    fn write_bytes(&mut self, buf: &[u8]) -> Result<(), IoError> {
        if self.tripped {
            return Ok(());
        }
        let room = usize::try_from(self.n.saturating_sub(self.inner.bytes_written())).unwrap_or(usize::MAX);
        if buf.len() > room {
            self.tripped = true;
            return self.inner.write_bytes(buf.get(..room).unwrap_or_default());
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
        let mut s = Cut { inner: sink, n: self.n, tripped: false };
        self.inner.write(src, opts, &mut s, report)?;
        if s.tripped {
            return Err(IoError::Io { kind: io::ErrorKind::Other, at: Position::byte(self.n) });
        }
        Ok(())
    }
}

/// What the kit runs against.
#[derive(Debug, Clone, Copy)]
pub struct Fixture<'a> {
    /// The format registry; the format must be registered for export.
    pub registry: &'a Registry,
    /// The roots; `root_id` must be an export root at `root_dir`.
    pub roots: &'a RootSet,
    /// The export root's ID.
    pub root_id: &'a str,
    /// The export root's folder; must be empty when the kit starts.
    pub root_dir: &'a Path,
    /// The format under test.
    pub format: &'a FormatId,
    /// The file name to export to (with the format's extension).
    pub file_name: &'a str,
    /// The `source` parameter recorded.
    pub source: &'a SourceRef,
    /// Writer options of the main export.
    pub options: &'a CValue,
    /// The SHA-256 the main export's file must have, from the leaf's independent reference.
    pub expected_sha256: &'a Digest,
    /// Option objects each differing from `options` in at least one declared option.
    pub option_variants: &'a [CValue],
    /// Bytes the faulted sink takes before failing; less than the full export.
    pub fail_after: u64,
}

/// A conformance failure: which step, for which command, and what was seen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConformanceError {
    /// The step (`definition`, `setup`, `dry_run`, `sink_failure`, `cancel`,
    /// `cancel_mid_write`, `prepare_place`, `prepare_discard`, `no_clobber`, `replay`,
    /// `options`).
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

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn prop<'v>(schema: &'v CValue, name: &str) -> Option<&'v CValue> {
    match schema {
        CValue::Obj(o) => match o.get("properties") {
            Some(CValue::Obj(p)) => p.get(name),
            _ => None,
        },
        _ => None,
    }
}

fn check_def(def: &CommandDef, id: &str, takes_format: bool) -> Result<(), String> {
    if format!("{}@{}", def.id, def.major) != id {
        return Err(format!("the definition is {}@{}", def.id, def.major));
    }
    if def.side_effect.as_str() != "W1" || def.fs_effect.as_str() != "write_root" || def.determinism.as_str() != "D0" {
        return Err("not W1 / write_root / D0".into());
    }
    if def.effects != [id.to_string()] {
        return Err("the effects are not exactly the command's record".into());
    }
    let path_like = matches!(prop(&def.params_schema, "target"), Some(CValue::Obj(t)) if t.get("path_like") == Some(&CValue::Bool(true)));
    if !path_like || prop(&def.params_schema, "dry_run").is_none() {
        return Err("target is not path_like or dry_run is missing".into());
    }
    if prop(&def.params_schema, "format").is_some() != takes_format {
        return Err("format parameter does not match the command".into());
    }
    Ok(())
}

/// The option names the format's export descriptor declares.
fn declared_options(reg: &Registry, format: &FormatId) -> Vec<String> {
    match reg.get(format, Direction::Export).map(|d| d.options_schema()) {
        Some(CValue::Obj(o)) => match o.get("properties") {
            Some(CValue::Obj(p)) => p.keys().cloned().collect(),
            _ => Vec::new(),
        },
        _ => Vec::new(),
    }
}

/// Runs the kit for one command (see the module documentation).
pub fn check<S: ?Sized>(
    cmd: &dyn ExportRun<S>,
    def: &CommandDef,
    src: &S,
    fx: &Fixture<'_>,
) -> Result<(), ConformanceError> {
    let id = cmd.command_id().map(|c| c.as_str().to_string()).unwrap_or_default();
    let fail = |step: &'static str, detail: String| ConformanceError { step, command: id.clone(), detail };
    check_def(def, &id, cmd.takes_format()).map_err(|d| fail("definition", d))?;
    if cmd.takes_format() != (id == EXPORT_FILE) {
        return Err(fail("definition", "only export.file@1 takes format".into()));
    }

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
    let mut params = ExportParams::new(fx.source.clone(), cmd.takes_format().then(|| fx.format.clone()), &target);
    params.options = fx.options.clone();
    let env = ExportEnv::new(fx.registry, fx.roots, Principal::Script);
    let dry_of = |p: &ExportParams| {
        let mut d = p.clone();
        d.dry_run = true;
        d
    };

    // 2. Dry run, against the stored digest.
    let dry = cmd.run(&env, &dry_of(&params), src).map_err(|e| fail("dry_run", format!("failed: {e}")))?;
    if dry.event.is_some() || !dry.report.dry_run() {
        return Err(fail("dry_run", "a dry run must report dry_run and journal nothing".into()));
    }
    if dry.report.sha256() != fx.expected_sha256 {
        return Err(fail("dry_run", format!("digest {} is not the stored one", dry.report.sha256().as_str())));
    }
    unchanged("dry_run")?;

    // 3. Sink failure.
    let faulted = ExportEnv { fault: Some(Fault::FailAfter(fx.fail_after)), ..env };
    match cmd.run(&faulted, &params, src) {
        Ok(_) => return Err(fail("sink_failure", "the export succeeded on a failing sink".into())),
        Err(ExportError::Io(IoError::Io { .. })) => {}
        Err(e) => return Err(fail("sink_failure", format!("unexpected error {}", e.code()))),
    }
    unchanged("sink_failure")?;

    // 4. Cancel before the start and mid-write.
    let flag = AtomicBool::new(true);
    for (step, env) in [
        ("cancel", ExportEnv { cancel: Some(&flag), ..env }),
        ("cancel_mid_write", ExportEnv { fault: Some(Fault::CancelMidWrite), ..env }),
    ] {
        for p in [&params, &dry_of(&params)] {
            match cmd.run(&env, p, src) {
                Err(ExportError::Cancelled) => {}
                Ok(_) => return Err(fail(step, "a cancelled export succeeded".into())),
                Err(e) => return Err(fail(step, format!("unexpected error {}", e.code()))),
            }
        }
        unchanged(step)?;
    }

    // 5. Prepare, then place.
    let path = fx.root_dir.join(fx.file_name);
    let prepared = cmd.prepare(&env, &params, src).map_err(|e| fail("prepare_place", format!("failed: {e}")))?;
    if path.exists() {
        return Err(fail("prepare_place", "prepare touched the target".into()));
    }
    let real = prepared.place().map_err(|e| fail("prepare_place", format!("place failed: {e}")))?;
    let bytes = std::fs::read(&path).map_err(|e| fail("prepare_place", format!("no file: {:?}", e.kind())))?;
    let sha = format!("sha256:{}", hex(&Sha256::digest(&bytes)));
    let Some(event) = &real.event else { return Err(fail("prepare_place", "no journal record".into())) };
    let same = sha == fx.expected_sha256.as_str()
        && real.report.sha256().as_str() == sha
        && real.report.bytes() == bytes.len() as u64
        && event.sha256.as_str() == sha
        && event.bytes == real.report.bytes()
        && dry.report.bytes() == real.report.bytes()
        && event.command.as_str() == id
        && event.lossy == *real.report.lossy();
    if !same {
        return Err(fail("prepare_place", "file, stored digest, report, dry run and record disagree".into()));
    }
    if listing(fx.root_dir).map_err(|d| fail("prepare_place", d))? != [fx.file_name.to_string()] {
        return Err(fail("prepare_place", "unexpected files in the root".into()));
    }
    let json = event.to_json().map_err(|e| fail("prepare_place", e.to_string()))?;
    if ExportEvent::from_json(&json).as_ref() != Ok(event) {
        return Err(fail("prepare_place", "the record does not read back".into()));
    }

    // 7. No clobbering.
    match cmd.run(&env, &params, src) {
        Err(ExportError::Io(IoError::TargetExists)) => {}
        Ok(_) => return Err(fail("no_clobber", "a second export overwrote the file".into())),
        Err(e) => return Err(fail("no_clobber", format!("unexpected error {}", e.code()))),
    }
    if std::fs::read(&path).ok().as_deref() != Some(bytes.as_slice()) {
        return Err(fail("no_clobber", "the file changed".into()));
    }

    // 8. Replay into the emptied root.
    std::fs::remove_file(&path).map_err(|e| fail("replay", format!("cannot remove: {:?}", e.kind())))?;
    match cmd.replay(&env, event, src) {
        Ok(ReplayOutcome::Identical) => {}
        Ok(o) => return Err(fail("replay", format!("outcome {}", o.as_str()))),
        Err(e) => return Err(fail("replay", format!("failed: {e}"))),
    }
    std::fs::remove_file(&path).map_err(|e| fail("replay", format!("cannot remove: {:?}", e.kind())))?;

    // 6. Prepare, then discard.
    let prepared = cmd.prepare(&env, &params, src).map_err(|e| fail("prepare_discard", format!("failed: {e}")))?;
    prepared.discard();
    unchanged("prepare_discard")?;

    // 9. Declared options change the output or are refused.
    for name in declared_options(fx.registry, fx.format) {
        let covered = fx.option_variants.iter().any(|v| match (v, fx.options) {
            (CValue::Obj(v), CValue::Obj(base)) => v.get(&name).is_some() && v.get(&name) != base.get(&name),
            _ => false,
        });
        if !covered {
            return Err(fail("options", format!("declared option {name} is not varied by any variant")));
        }
    }
    for (i, v) in fx.option_variants.iter().enumerate() {
        let mut p = dry_of(&params);
        p.options = v.clone();
        match cmd.run(&env, &p, src) {
            Ok(out) if out.report.sha256() != dry.report.sha256() => {}
            Ok(_) => return Err(fail("options", format!("variant {i} does not change the output"))),
            Err(ExportError::BadOptions(_)) => {}
            Err(e) => return Err(fail("options", format!("variant {i}: unexpected error {}", e.code()))),
        }
    }
    unchanged("options")?;
    Ok(())
}
