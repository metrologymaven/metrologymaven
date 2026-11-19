//! `registry-gate`: the build gate against the released baseline manifest (spec node
//! `CORE.REG.VER.GATE`, REQ-CORE.REG.VER-005, REQ-CORE.REG.VER-006, CONSTITUTION §3).
//!
//! The gate compares the current declarations with the manifest of the last release and
//! fails on:
//!
//! - any breaking change to an (ID, major) present in the baseline (REQ-001). The
//!   classification is `fid_registry::compat` ([`fid_registry::diff`] /
//!   [`fid_registry::classify`]) and nothing else: every change it marks
//!   [`Kind::Breaking`], `unclassified_construct` included, fails with its reason code;
//! - a change of `side_effect` or of the effective MCP projection
//!   ([`fid_registry::CommandDef::effective_projection`]) of a baseline (ID, major), even
//!   when the rest of the definition is unchanged (REQ-002);
//! - a baseline (ID, major) missing from the current declarations (`silent_removal`,
//!   REQ-003). A major retires by staying in the declarations with state `removed`, which
//!   is then checked as a lifecycle transition;
//! - any lifecycle change that `fid_registry::lifecycle` refuses, with that check's code
//!   (REQ-004). Every current lifecycle must also read as a valid lifecycle
//!   (`bad_lifecycle`).
//!
//! It exits 0 when nothing fails (REQ-005), 1 with a report when something does, and 2
//! when an input cannot be trusted: `baseline_invalid` (missing, unreadable, malformed or
//! failing its digest, REQ-006), `declarations_invalid`, `format_mismatch`, or
//! `release_date_required`.
//!
//! **Baseline digest.** The baseline manifest's digest is the SHA-256 of its exact bytes
//! (`CORE.REG.MAN` REQ-002). It is stated in a sidecar file next to it, the baseline path
//! plus `.sha256`, holding `sha256:<64 lowercase hex>` and at most one trailing LF. A
//! missing or malformed sidecar is `baseline_invalid`.
//!
//! **Release date.** A lifecycle transition is checked against the date of the release
//! that makes it, given as `--release-date`. When any lifecycle state differs from the
//! baseline, `--release-date` is mandatory: without it the gate exits 2 with
//! `release_date_required`, so a `since` written into the declarations (for example one far in
//! the future) can never stand in for the release date and defeat `deprecation_too_short` or
//! `removal_too_early`. Every changed state must carry the release date as its `since`, and
//! that date must be later than the baseline's `since` (else `bad_lifecycle` at
//! `/lifecycle/since`). A lifecycle edit that keeps the state is checked first and is always
//! LIFE's `illegal_transition`, with or without a release date.
//!
//! **Baseline contents.** A manifest never lists `removed` majors (LIFE `retained`), so a
//! baseline that does is `baseline_invalid`; every baseline (ID, major) missing from the
//! declarations is therefore a `silent_removal`.
//!
//! **Current declarations.** Until crates export `declarations()` (`CORE.REG.MAN`), the
//! current declarations are read from a manifest-shaped JSON file (`--current`), in the
//! format `fid_registry::Manifest::from_json` reads; unlike a manifest it may list
//! `removed` majors.
//!
//! **Report.** One line per failure, the RFC 8785 form of
//! `{"code", "command", "major", "pointer"}`, sorted by command, major, pointer and code
//! and deduplicated, then `registry-gate: fail (N)`; or `registry-gate: ok`. The report
//! depends only on the input bytes (determinism D0). Reads only; no network, no git.

use fid_canon::{to_canonical_bytes, CValue};
use fid_registry::{check_transition, diff, CommandRef, Date, Kind, LifecycleDecl, Manifest, Reason, State};
use fid_types::Digest;
use sha2::{Digest as _, Sha256};
use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

/// Default baseline location, relative to the repository root.
pub const DEFAULT_BASELINE: &str = "registry/baseline-manifest.json";
/// Suffix of the sidecar file stating the baseline digest.
pub const DIGEST_SUFFIX: &str = ".sha256";
/// Largest manifest or declarations file read (the `fid-canon` input limit).
pub const MAX_BYTES: usize = fid_canon::MAX_INPUT_BYTES;
/// Largest digest sidecar read: the digest plus one LF.
const MAX_SIDECAR_BYTES: usize = 72;

/// Failure code of REQ-003.
pub const SILENT_REMOVAL: &str = "silent_removal";
/// Exit-2 code of REQ-006.
pub const BASELINE_INVALID: &str = "baseline_invalid";
/// Exit-2 code: the current declarations cannot be read.
pub const DECLARATIONS_INVALID: &str = "declarations_invalid";
/// Exit-2 code: a lifecycle state changed but no `--release-date` was given.
pub const RELEASE_DATE_REQUIRED: &str = "release_date_required";
/// Exit-2 code: baseline and declarations use different manifest formats.
pub const FORMAT_MISMATCH: &str = "format_mismatch";
/// `fid_registry::lifecycle` code for an unreadable or inconsistent lifecycle.
const BAD_LIFECYCLE: &str = "bad_lifecycle";

pub const USAGE: &str = "usage: cargo xtask registry-gate --current <declarations.json> \
                         [--baseline <registry/baseline-manifest.json>] [--release-date <YYYY-MM-DD>]";

/// One reason the gate fails. Orders by command (ID, then major), pointer, code.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GateFailure {
    pub command: CommandRef,
    /// RFC 6901 pointer into the definition's JSON form; empty for the whole definition.
    pub pointer: String,
    pub code: String,
}

impl GateFailure {
    fn new(command: &CommandRef, pointer: &str, code: &str) -> GateFailure {
        GateFailure { command: command.clone(), pointer: pointer.to_string(), code: code.to_string() }
    }

    /// `{"code", "command", "major", "pointer"}`.
    pub fn to_value(&self) -> CValue {
        let CValue::Obj(mut o) = self.command.to_value() else { return CValue::Null };
        o.insert("pointer".into(), CValue::Str(self.pointer.clone()));
        o.insert("code".into(), CValue::Str(self.code.clone()));
        CValue::Obj(o)
    }
}

/// The gate's findings, sorted and without duplicates.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GateReport {
    pub failures: Vec<GateFailure>,
}

impl GateReport {
    /// True when nothing failed.
    pub fn passed(&self) -> bool {
        self.failures.is_empty()
    }

    /// The printed report (see the module documentation).
    pub fn render(&self) -> String {
        if self.passed() {
            return "registry-gate: ok\n".to_string();
        }
        let mut out = String::new();
        for f in &self.failures {
            // Every value is a string or a u32, so canonicalisation cannot fail; the
            // fallback still names the failure.
            match to_canonical_bytes(&f.to_value()).map(String::from_utf8) {
                Ok(Ok(line)) => out.push_str(&line),
                _ => out.push_str(&format!("{} {}@{} {:?}", f.code, f.command.id, f.command.major, f.pointer)),
            }
            out.push('\n');
        }
        out.push_str(&format!("registry-gate: fail ({})\n", self.failures.len()));
        out
    }
}

/// An input the gate cannot trust: exit 2.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InputError {
    pub code: &'static str,
    pub detail: String,
}

impl fmt::Display for InputError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.detail)
    }
}

impl std::error::Error for InputError {}

fn read_capped(path: &Path, limit: usize) -> Result<Vec<u8>, String> {
    let file = std::fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut bytes = Vec::new();
    let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
    file.take(cap).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    if bytes.len() > limit {
        return Err(format!("{}: larger than {limit} bytes", path.display()));
    }
    Ok(bytes)
}

/// The sidecar path stating the digest of `baseline`.
pub fn digest_path(baseline: &Path) -> PathBuf {
    let mut s = baseline.as_os_str().to_owned();
    s.push(DIGEST_SUFFIX);
    PathBuf::from(s)
}

/// SHA-256 of exact bytes, in wire form.
pub fn bytes_digest(bytes: &[u8]) -> Digest {
    Digest::from_bytes(Sha256::digest(bytes).into())
}

/// Parses a sidecar: exactly `sha256:<64 lowercase hex>`, optionally followed by one LF.
fn parse_sidecar(bytes: &[u8]) -> Result<Digest, String> {
    let body = bytes.strip_suffix(b"\n").unwrap_or(bytes);
    let text = std::str::from_utf8(body).map_err(|_| "digest file is not UTF-8".to_string())?;
    Digest::parse(text).map_err(|_| "digest file must hold sha256:<64 lowercase hex> and at most one LF".to_string())
}

/// Loads and verifies the baseline (REQ-006): the sidecar digest must match the exact
/// bytes, the bytes must be a valid manifest, every lifecycle must read, and no entry may be
/// `removed` (a manifest lists only retained majors).
pub fn load_baseline(path: &Path) -> Result<Manifest, InputError> {
    let bad = |detail: String| InputError { code: BASELINE_INVALID, detail };
    let bytes = read_capped(path, MAX_BYTES).map_err(bad)?;
    let sidecar = digest_path(path);
    let stated = read_capped(&sidecar, MAX_SIDECAR_BYTES)
        .and_then(|b| parse_sidecar(&b).map_err(|e| format!("{}: {e}", sidecar.display())))
        .map_err(bad)?;
    let actual = bytes_digest(&bytes);
    if actual != stated {
        return Err(bad(format!(
            "{}: digest mismatch: stated {}, bytes are {}",
            path.display(),
            stated.as_str(),
            actual.as_str()
        )));
    }
    let manifest = Manifest::from_json(&bytes).map_err(|e| bad(format!("{}: {e}", path.display())))?;
    for (key, def) in &manifest.commands {
        let decl = LifecycleDecl::from_def(def).map_err(|e| bad(format!("{}@{}: {e}", key.id, key.major)))?;
        if decl.lifecycle.state == State::Removed {
            return Err(bad(format!("{}@{}: a manifest never lists a removed major", key.id, key.major)));
        }
    }
    Ok(manifest)
}

/// Loads the current declarations (a manifest-shaped file; see the module documentation).
pub fn load_declarations(path: &Path) -> Result<Manifest, InputError> {
    let bad = |detail: String| InputError { code: DECLARATIONS_INVALID, detail };
    let bytes = read_capped(path, MAX_BYTES).map_err(bad)?;
    Manifest::from_json(&bytes).map_err(|e| bad(format!("{}: {e}", path.display())))
}

/// Applies REQ-001..004 to a verified baseline and the current declarations. Pure and
/// deterministic. `release_date` is the date of the release being gated; it is required
/// (else `release_date_required`) when any lifecycle state differs from the baseline.
pub fn gate(baseline: &Manifest, current: &Manifest, release_date: Option<Date>) -> Result<GateReport, InputError> {
    let d = diff(baseline, current);
    if release_date.is_none() {
        if let Some(lc) = d.lifecycle_changed.iter().find(|lc| lc.old_state != lc.new_state) {
            return Err(InputError {
                code: RELEASE_DATE_REQUIRED,
                detail: format!(
                    "{}@{} changes lifecycle state; pass --release-date <YYYY-MM-DD>",
                    lc.command.id, lc.command.major
                ),
            });
        }
    }
    let mut out: BTreeSet<GateFailure> = BTreeSet::new();

    // Every current lifecycle must be a valid lifecycle (CORE.REG.VER.LIFE).
    let mut decls = BTreeMap::new();
    for (key, def) in &current.commands {
        match LifecycleDecl::from_def(def) {
            Ok(decl) => {
                decls.insert(key, decl);
            }
            Err(e) => {
                out.insert(GateFailure::new(key, "/lifecycle", e.code()));
            }
        }
    }

    // REQ-001: COMPAT is the only classifier; every breaking change fails with its reason.
    for c in d.changed.iter().filter(|c| c.kind == Kind::Breaking) {
        out.insert(GateFailure::new(&c.command, &c.pointer, c.reason.as_str()));
    }

    // REQ-002: class and effective projection are frozen within a major.
    for (key, old) in &baseline.commands {
        let Some(new) = current.commands.get(key) else { continue };
        if old.side_effect != new.side_effect {
            out.insert(GateFailure::new(key, "/side_effect", Reason::SideEffectChanged.as_str()));
        }
        if old.effective_projection() != new.effective_projection() {
            out.insert(GateFailure::new(key, "/mcp_projection", Reason::McpProjectionChanged.as_str()));
        }
    }

    // REQ-003: a baseline major may only leave through a legal transition to `removed`
    // (the baseline holds no `removed` majors; `load_baseline` refuses them).
    for key in &d.removed {
        out.insert(GateFailure::new(key, "", SILENT_REMOVAL));
    }

    // REQ-004: every lifecycle change must pass the lifecycle check.
    for lc in &d.lifecycle_changed {
        let key = &lc.command;
        // A current lifecycle that does not read was reported as bad_lifecycle above.
        let Some(new) = decls.get(key) else { continue };
        let old = match baseline.commands.get(key).map(LifecycleDecl::from_def) {
            Some(Ok(old)) => old,
            Some(Err(e)) => {
                out.insert(GateFailure::new(key, "/lifecycle", e.code()));
                continue;
            }
            None => {
                out.insert(GateFailure::new(key, "/lifecycle", BAD_LIFECYCLE));
                continue;
            }
        };
        let since = new.lifecycle.since;
        // Same state, other metadata: LIFE refuses staying in a state (`illegal_transition`)
        // whatever the date, so this is checked before any date rule.
        let release = match release_date {
            Some(r) if old.lifecycle.state != new.lifecycle.state => r,
            _ => {
                if let Err(e) = check_transition(&old.lifecycle, new.lifecycle.state, since) {
                    out.insert(GateFailure::new(key, "/lifecycle", e.code()));
                } else {
                    // Unreachable while LIFE refuses same-state moves; fail closed anyway.
                    out.insert(GateFailure::new(key, "/lifecycle", BAD_LIFECYCLE));
                }
                continue;
            }
        };
        if since != release || since <= old.lifecycle.since {
            out.insert(GateFailure::new(key, "/lifecycle/since", BAD_LIFECYCLE));
            continue;
        }
        if let Err(e) = check_transition(&old.lifecycle, new.lifecycle.state, release) {
            out.insert(GateFailure::new(key, "/lifecycle", e.code()));
        }
    }

    Ok(GateReport { failures: out.into_iter().collect() })
}

/// Loads both inputs and runs the gate.
pub fn run(baseline: &Path, current: &Path, release_date: Option<Date>) -> Result<GateReport, InputError> {
    let base = load_baseline(baseline)?;
    let cur = load_declarations(current)?;
    if base.format != cur.format {
        return Err(InputError {
            code: FORMAT_MISMATCH,
            detail: format!("baseline format {}, declarations format {}", base.format, cur.format),
        });
    }
    gate(&base, &cur, release_date)
}

/// The exit status of a run: 0 pass, 1 failures, 2 untrusted input.
pub fn exit_code(result: &Result<GateReport, InputError>) -> u8 {
    match result {
        Ok(r) if r.passed() => 0,
        Ok(_) => 1,
        Err(_) => 2,
    }
}

/// The text a run prints on stdout.
pub fn render(result: &Result<GateReport, InputError>) -> String {
    match result {
        Ok(r) => r.render(),
        Err(e) => format!("registry-gate: {e}\n"),
    }
}

/// `cargo xtask registry-gate`.
pub fn cli(args: &[String]) -> ExitCode {
    let (mut baseline, mut current, mut release) = (PathBuf::from(DEFAULT_BASELINE), None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--baseline", Some(v)) => baseline = PathBuf::from(v),
            ("--current", Some(v)) => current = Some(PathBuf::from(v)),
            ("--release-date", Some(v)) => match Date::parse(v) {
                Ok(d) => release = Some(d),
                Err(e) => {
                    eprintln!("registry-gate: --release-date: {e}\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(current) = current else {
        eprintln!("registry-gate: --current is required\n{USAGE}");
        return ExitCode::from(2);
    };
    let result = run(&baseline, &current, release);
    print!("{}", render(&result));
    ExitCode::from(exit_code(&result))
}
