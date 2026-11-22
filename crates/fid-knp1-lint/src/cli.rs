//! CLI entry point and self-test for `fid-knp1-lint` (spec node `KRN.NUM.LINT.CLI`).
//!
//! Subcommands:
//! - `fid-knp1-lint check --config <toml> [--waivers <toml>] [--denylist <json>] [--skip-typed] --out <json>`
//! - `fid-knp1-lint selftest [--seeded <dir>] [--data-ws <toml>]`
//!
//! Exit codes (REQ-KRN.NUM.LINT.CLI-002):
//! - `0`: no unwaived findings and no fatal errors
//! - `1`: unwaived findings exist (report written) or fatal rule/waiver/selftest failure
//! - `2`: tool or input error (`config_invalid`, `input_missing`, `driver_missing`), writing no report
//!
//! No network access; writes only `--out` (and, when TYPED runs, a temporary scratch directory)
//! (REQ-KRN.NUM.LINT.CLI-005).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use crate::data::{self, DataErrorCode};
use crate::findings::{
    apply_waivers, catalogue, concrete_family_rules, input_digest, load_waivers, sort_findings, CrateRoots, ErrorCode,
    Finding, InputDigests, Report, Rule, TypedStatus,
};
use crate::flags::wasm::SimdPolicy;
use crate::flags::{self, BUILD_CONFIG_CRATE, BUILD_CONFIG_VERSION};
use crate::graph::{self, Closure, CrateEntry, GraphErrorCode, SourceKind};
use crate::syn_scan::{self, FileRef, SynErrorCode};
use crate::typed::{self, CrateRef, MapCtx, TypedErrorCode};

static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

/// Tool version embedded in reports (crate version).
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exit code for a clean run (no unwaived findings, no errors).
pub const EXIT_CLEAN: i32 = 0;
/// Exit code when unwaived findings exist or a fatal rule/waiver/selftest failure occurred.
pub const EXIT_FINDINGS: i32 = 1;
/// Exit code on any tool or input error (fail closed, no success report written).
pub const EXIT_TOOL_ERROR: i32 = 2;

/// The 12 fixed syntax/API rules whose seeded crates live under `fixtures/knp1/seeded/<RULE>/`.
pub const SEEDED_SYN_RULES: [&str; 12] = [
    "KNP1-CLOCK",
    "KNP1-ENV",
    "KNP1-FMA",
    "KNP1-HASH",
    "KNP1-IO-FS",
    "KNP1-IO-NET",
    "KNP1-LIBM",
    "KNP1-MINMAX",
    "KNP1-POW",
    "KNP1-SIMD",
    "KNP1-THREAD",
    "KNP1-UNSAFE",
];

/// Typed error codes for exit-2 tool/input failures (`KRN.NUM.LINT.CLI` Design).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CliErrorCode {
    ConfigInvalid,
    InputMissing,
    DriverMissing,
}

impl CliErrorCode {
    /// Stable machine-readable error code.
    pub fn as_str(self) -> &'static str {
        match self {
            CliErrorCode::ConfigInvalid => "config_invalid",
            CliErrorCode::InputMissing => "input_missing",
            CliErrorCode::DriverMissing => "driver_missing",
        }
    }
}

/// A tool or input error (exit code 2).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliError {
    pub code: CliErrorCode,
    pub message: String,
}

impl CliError {
    pub fn new(code: CliErrorCode, message: impl Into<String>) -> Self {
        Self { code, message: message.into() }
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code.as_str(), self.message)
    }
}

impl std::error::Error for CliError {}

/// Parsed CLI command.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    Check(CheckArgs),
    Selftest(SelftestArgs),
}

/// Arguments for `fid-knp1-lint check`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckArgs {
    pub config: PathBuf,
    pub waivers: Option<PathBuf>,
    pub denylist: Option<PathBuf>,
    pub skip_typed: bool,
    pub out: PathBuf,
}

/// Arguments for `fid-knp1-lint selftest`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelftestArgs {
    pub seeded_dir: PathBuf,
    pub data_ws_config: PathBuf,
}

/// Result of running [`run_check`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckOutcome {
    pub exit_code: i32,
    pub report: Option<Report>,
    pub messages: Vec<String>,
}

/// Result of running [`run_selftest`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelftestOutcome {
    pub passed: bool,
    pub checked_rules: Vec<String>,
    pub failures: Vec<String>,
}

/// Parses command-line arguments (excluding `argv[0]`).
pub fn parse_args(args: &[String]) -> Result<Command, CliError> {
    let Some(sub) = args.first().map(String::as_str) else {
        return Err(CliError::new(CliErrorCode::ConfigInvalid, "missing subcommand; expected `check` or `selftest`"));
    };
    let rest = args.get(1..).unwrap_or_default();
    match sub {
        "check" => parse_check_args(rest).map(Command::Check),
        "selftest" => parse_selftest_args(rest).map(Command::Selftest),
        other => Err(CliError::new(
            CliErrorCode::ConfigInvalid,
            format!("unknown subcommand `{other}`; expected `check` or `selftest`"),
        )),
    }
}

fn parse_check_args(args: &[String]) -> Result<CheckArgs, CliError> {
    let mut config: Option<PathBuf> = None;
    let mut waivers: Option<PathBuf> = None;
    let mut denylist: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut skip_typed = false;

    let mut i = 0usize;
    while i < args.len() {
        let flag = args.get(i).map(String::as_str).unwrap_or_default();
        match flag {
            "--config" => {
                if config.is_some() {
                    return Err(CliError::new(CliErrorCode::ConfigInvalid, "duplicate --config"));
                }
                let val = next_value(args, &mut i, "--config")?;
                config = Some(PathBuf::from(val));
            }
            "--waivers" => {
                if waivers.is_some() {
                    return Err(CliError::new(CliErrorCode::ConfigInvalid, "duplicate --waivers"));
                }
                let val = next_value(args, &mut i, "--waivers")?;
                waivers = Some(PathBuf::from(val));
            }
            "--denylist" => {
                if denylist.is_some() {
                    return Err(CliError::new(CliErrorCode::ConfigInvalid, "duplicate --denylist"));
                }
                let val = next_value(args, &mut i, "--denylist")?;
                denylist = Some(PathBuf::from(val));
            }
            "--out" => {
                if out.is_some() {
                    return Err(CliError::new(CliErrorCode::ConfigInvalid, "duplicate --out"));
                }
                let val = next_value(args, &mut i, "--out")?;
                out = Some(PathBuf::from(val));
            }
            "--skip-typed" => {
                skip_typed = true;
            }
            other => {
                return Err(CliError::new(CliErrorCode::ConfigInvalid, format!("unknown argument `{other}`")));
            }
        }
        i += 1;
    }

    let config =
        config.ok_or_else(|| CliError::new(CliErrorCode::ConfigInvalid, "missing required --config <toml>"))?;
    let out = out.ok_or_else(|| CliError::new(CliErrorCode::ConfigInvalid, "missing required --out <json>"))?;
    Ok(CheckArgs { config, waivers, denylist, skip_typed, out })
}

fn parse_selftest_args(args: &[String]) -> Result<SelftestArgs, CliError> {
    let mut seeded_dir = PathBuf::from("fixtures/knp1/seeded");
    let mut data_ws_config = PathBuf::from("fixtures/knp1/data/ws/knp1-lint.toml");

    let mut i = 0usize;
    while i < args.len() {
        let flag = args.get(i).map(String::as_str).unwrap_or_default();
        match flag {
            "--seeded" => {
                let val = next_value(args, &mut i, "--seeded")?;
                seeded_dir = PathBuf::from(val);
            }
            "--data-ws" => {
                let val = next_value(args, &mut i, "--data-ws")?;
                data_ws_config = PathBuf::from(val);
            }
            other => {
                return Err(CliError::new(CliErrorCode::ConfigInvalid, format!("unknown selftest argument `{other}`")));
            }
        }
        i += 1;
    }
    Ok(SelftestArgs { seeded_dir, data_ws_config })
}

fn next_value<'a>(args: &'a [String], i: &mut usize, flag: &str) -> Result<&'a str, CliError> {
    *i += 1;
    match args.get(*i) {
        Some(v) if !v.is_empty() => Ok(v.as_str()),
        _ => Err(CliError::new(CliErrorCode::ConfigInvalid, format!("{flag} requires a value"))),
    }
}

fn read_required_file(path: &Path, label: &str) -> Result<Vec<u8>, CliError> {
    if !path.exists() {
        return Err(CliError::new(CliErrorCode::InputMissing, format!("{label} not found: {}", path.display())));
    }
    std::fs::read(path)
        .map_err(|e| CliError::new(CliErrorCode::InputMissing, format!("cannot read {label} {}: {e}", path.display())))
}

/// Runs `fid-knp1-lint check` in the pinned order GRAPH -> SYN -> TYPED -> FLAGS -> DATA,
/// applies waivers, and writes `args.out` (REQ-KRN.NUM.LINT.CLI-001..003, -005).
pub fn run_check(args: &CheckArgs) -> Result<CheckOutcome, CliError> {
    let config_bytes = read_required_file(&args.config, "config")?;
    let config_text = std::str::from_utf8(&config_bytes).map_err(|_| {
        CliError::new(CliErrorCode::ConfigInvalid, format!("{}: not valid UTF-8", args.config.display()))
    })?;
    let config_table: toml::Table = config_text
        .parse()
        .map_err(|e: toml::de::Error| CliError::new(CliErrorCode::ConfigInvalid, format!("knp1-lint.toml: {e}")))?;
    let config_dir = args.config.parent().unwrap_or_else(|| Path::new("."));

    // Pre-read waivers if specified so a missing/malformed waiver file fails closed (exit 2)
    // before writing any report.
    let (waivers_bytes, waivers) = match &args.waivers {
        Some(p) => {
            let bytes = read_required_file(p, "waivers")?;
            let parsed = load_waivers(&bytes)
                .map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("waivers: {e}")))?;
            (bytes, parsed)
        }
        None => (Vec::new(), Vec::new()),
    };

    // Validate `[data]` table keys whenever `[data]` is present (even if `--denylist` overrides it).
    let config_denylist: Option<PathBuf> = match config_table.get("data") {
        Some(toml::Value::Table(t)) => {
            if let Some(k) = t.keys().find(|k| k.as_str() != "denylist") {
                return Err(CliError::new(
                    CliErrorCode::ConfigInvalid,
                    format!("knp1-lint.toml: unknown key data.{k}"),
                ));
            }
            match t.get("denylist") {
                Some(toml::Value::String(s)) if !s.is_empty() => Some(config_dir.join(s)),
                _ => {
                    return Err(CliError::new(
                        CliErrorCode::ConfigInvalid,
                        "knp1-lint.toml: data.denylist must name the denylist file",
                    ))
                }
            }
        }
        Some(_) => {
            return Err(CliError::new(CliErrorCode::ConfigInvalid, "knp1-lint.toml: [data] must be a table"));
        }
        None => None,
    };

    // Fail closed if neither `--denylist` nor `[data].denylist` is provided.
    let denylist_path: PathBuf = match (&args.denylist, config_denylist) {
        (Some(p), _) => p.clone(),
        (None, Some(p)) => p,
        (None, None) => {
            return Err(CliError::new(
                CliErrorCode::ConfigInvalid,
                "knp1-lint.toml: missing required [data] table (and no --denylist flag provided)",
            ))
        }
    };
    if !denylist_path.exists() {
        return Err(CliError::new(
            CliErrorCode::InputMissing,
            format!("denylist not found: {}", denylist_path.display()),
        ));
    }
    let denylist = data::load_denylist_file(&denylist_path).map_err(map_data_err)?;
    let denylist_digest = denylist.digest.clone();

    // Fail closed if `[flags]` is missing or invalid.
    let flags_cfg = flags::parse_config(config_text, config_dir)
        .map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("flags: {e}")))?;

    let mut all_findings: Vec<Finding> = Vec::new();

    // 1. GRAPH: dependency closure, allowlist and SPDX licence check.
    let graph_run = graph::run(&args.config).map_err(map_graph_err)?;
    all_findings.extend(graph_run.findings);

    // 2. SYN: syntax-level banned-API scan over the kernel closure.
    let syn_cfg = syn_scan::parse_config(config_text).map_err(map_syn_err)?;
    let syn_out = syn_scan::scan_closure(&graph_run.closure, &syn_cfg).map_err(map_syn_err)?;
    all_findings.extend(syn_out.findings);

    // 3. TYPED: type-aware check unless `--skip-typed`.
    let typed_status = if args.skip_typed {
        TypedStatus::Skipped
    } else {
        let typed_cfg = typed::parse_config(config_text).map_err(map_typed_err)?;
        let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
        let scratch = std::env::temp_dir().join(format!("fid-knp1-lint-typed-{}-{seq}", std::process::id()));
        let typed_opts = typed::Options {
            cargo: PathBuf::from(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
            scratch: scratch.clone(),
        };
        let typed_res =
            typed::check(&graph_run.closure, &graph_run.config.workspace, &typed_cfg, &syn_cfg, &typed_opts)
                .map_err(map_typed_err);
        let _ = std::fs::remove_dir_all(&scratch);
        let typed_out = typed_res?;
        all_findings.extend(typed_out.findings);
        TypedStatus::Ran
    };

    // 4. FLAGS: build-configuration and emitted-code FMA check (always runs).
    let flags_out =
        flags::check_all(&flags_cfg).map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("flags: {e}")))?;
    all_findings.extend(flags_out.findings);

    // 5. DATA: reference-dataset denylist scan (always runs).
    let data_findings = data::scan_closure(&graph_run.closure, &denylist).map_err(map_data_err)?;
    all_findings.extend(data_findings);

    // Deduplicate identical findings (e.g. caught by both SYN and TYPED) deterministically.
    sort_findings(&mut all_findings);
    all_findings.dedup_by(|a, b| {
        (&a.crate_name, &a.crate_version, &a.file, a.line, a.column, a.rule.id(), &a.excerpt_digest)
            == (&b.crate_name, &b.crate_version, &b.file, b.line, b.column, b.rule.id(), &b.excerpt_digest)
    });

    // 6. WAIVERS & REPORT.
    let mut roots = CrateRoots::new();
    let mut linted: Vec<(String, String)> = Vec::with_capacity(graph_run.closure.crates.len());
    for c in &graph_run.closure.crates {
        roots.insert(&c.name, &c.version, &c.root);
        linted.push((c.name.clone(), c.version.clone()));
    }
    roots.insert(BUILD_CONFIG_CRATE, BUILD_CONFIG_VERSION, &graph_run.config.workspace);

    let applied = match apply_waivers(all_findings, &waivers, &roots) {
        Ok(a) => a,
        Err(errs) => {
            let any_schema = errs.iter().any(|e| e.code == ErrorCode::WaiverSchema);
            let msgs: Vec<String> = errs.iter().map(ToString::to_string).collect();
            if any_schema {
                return Err(CliError::new(CliErrorCode::ConfigInvalid, msgs.join("; ")));
            }
            // Stale waiver is a fatal rule failure (exit 1, no success report written).
            let _ = std::fs::remove_file(&args.out);
            return Ok(CheckOutcome { exit_code: EXIT_FINDINGS, report: None, messages: msgs });
        }
    };

    let inputs = InputDigests {
        config: input_digest(&config_bytes),
        lockfile: graph_run.lockfile_digest,
        waivers: input_digest(&waivers_bytes),
        denylist: denylist_digest,
    };
    let report = Report::new(TOOL_VERSION, inputs, applied)
        .map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("report: {e}")))?
        .with_typed(typed_status)
        .with_linted(linted);

    write_report_atomic(&report, &args.out)?;

    let exit_code = if report.passed() { EXIT_CLEAN } else { EXIT_FINDINGS };
    Ok(CheckOutcome { exit_code, report: Some(report), messages: Vec::new() })
}

fn write_report_atomic(report: &Report, out: &Path) -> Result<(), CliError> {
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent)
            .map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("create report dir: {e}")))?;
    }
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let file_name = out.file_name().and_then(|n| n.to_str()).unwrap_or("knp1-findings.json");
    let tmp_name = format!(".{file_name}.{}.{seq}.tmp", std::process::id());
    let tmp_path = match out.parent() {
        Some(p) if !p.as_os_str().is_empty() => p.join(tmp_name),
        _ => PathBuf::from(tmp_name),
    };
    if let Err(e) = report.write(&tmp_path) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(CliError::new(CliErrorCode::ConfigInvalid, format!("write report: {e}")));
    }
    if let Err(e) = std::fs::rename(&tmp_path, out) {
        let _ = std::fs::remove_file(&tmp_path);
        return Err(CliError::new(CliErrorCode::ConfigInvalid, format!("rename report: {e}")));
    }
    Ok(())
}

fn map_graph_err(e: graph::GraphError) -> CliError {
    let code = match e.code {
        GraphErrorCode::Unresolved => CliErrorCode::InputMissing,
        GraphErrorCode::ConfigInvalid | GraphErrorCode::ConfigExclude => CliErrorCode::ConfigInvalid,
    };
    CliError::new(code, e.message)
}

fn map_syn_err(e: syn_scan::SynError) -> CliError {
    let code = match e.code {
        SynErrorCode::Read | SynErrorCode::Graph(GraphErrorCode::Unresolved) => CliErrorCode::InputMissing,
        SynErrorCode::ConfigInvalid | SynErrorCode::Graph(_) => CliErrorCode::ConfigInvalid,
    };
    CliError::new(code, e.message)
}

fn map_typed_err(e: typed::TypedError) -> CliError {
    let code = match e.code {
        TypedErrorCode::Driver => CliErrorCode::DriverMissing,
        TypedErrorCode::Io | TypedErrorCode::Graph(GraphErrorCode::Unresolved) => CliErrorCode::InputMissing,
        TypedErrorCode::ConfigInvalid | TypedErrorCode::Graph(_) => CliErrorCode::ConfigInvalid,
    };
    CliError::new(code, e.message)
}

fn map_data_err(e: data::DataError) -> CliError {
    let code = match e.code {
        DataErrorCode::Read | DataErrorCode::Graph(GraphErrorCode::Unresolved) => CliErrorCode::InputMissing,
        DataErrorCode::ConfigInvalid | DataErrorCode::Denylist | DataErrorCode::Graph(_) => CliErrorCode::ConfigInvalid,
    };
    CliError::new(code, e.message)
}

fn knp1_fixtures_root(args: &SelftestArgs) -> PathBuf {
    if let Some(root) = args.data_ws_config.parent().and_then(Path::parent).and_then(Path::parent) {
        if root.join("flags").is_dir() && root.join("graph").is_dir() {
            return root.to_path_buf();
        }
    }
    args.seeded_dir.parent().unwrap_or_else(|| Path::new("fixtures/knp1")).to_path_buf()
}

fn validate_all(rule: &str, fs: &[Finding], failures: &mut Vec<String>) -> bool {
    for f in fs {
        if let Err(e) = f.validate() {
            failures.push(format!("{rule}: invalid finding emitted: {e}"));
            return false;
        }
    }
    true
}

fn check_seeded_syn_rule(
    rule: &str,
    seeded_dir: &Path,
    expected_map: &BTreeMap<String, String>,
    syn_cfg: &syn_scan::Config,
    failures: &mut Vec<String>,
) {
    let Some(exp) = expected_map.get(rule) else {
        failures.push(format!("{rule}: missing entry in expected-findings.txt"));
        return;
    };
    let dir = seeded_dir.join(rule);
    if !dir.is_dir() {
        failures.push(format!("{rule}: missing seeded fixture directory {}", dir.display()));
        return;
    }
    let files = match graph::enumerate_files(&dir, &[], &BTreeSet::new()) {
        Ok(f) if !f.is_empty() => f,
        Ok(_) => {
            failures.push(format!("{rule}: seeded fixture directory {} has no .rs files", dir.display()));
            return;
        }
        Err(e) => {
            failures.push(format!("{rule}: cannot enumerate seeded fixture: {e}"));
            return;
        }
    };
    let crate_name = format!("seeded-{}", rule.to_ascii_lowercase());
    let entry = CrateEntry {
        name: crate_name,
        version: "0.0.1".to_string(),
        source_kind: SourceKind::Vendored,
        root: dir,
        root_label: rule.to_string(),
        files,
        licence: None,
        path_from_root: Vec::new(),
    };
    let out = match syn_scan::scan_closure(&Closure { crates: vec![entry] }, syn_cfg) {
        Ok(o) => o,
        Err(e) => {
            failures.push(format!("{rule}: syn scan error: {e}"));
            return;
        }
    };
    if out.findings.len() != 1 {
        let got: Vec<String> = out.findings.iter().map(|f| f.rule.id()).collect();
        failures.push(format!("{rule}: expected exactly 1 finding, got {} ({got:?})", out.findings.len()));
        return;
    }
    let Some(f) = out.findings.first() else {
        failures.push(format!("{rule}: missing finding"));
        return;
    };
    if f.rule.id() != rule {
        failures.push(format!("{rule}: expected rule {rule}, got {}", f.rule.id()));
        return;
    }
    if !validate_all(rule, &out.findings, failures) {
        return;
    }
    let actual = format!(
        "{} {} {} {}:{}:{} {}",
        f.rule.id(),
        f.crate_name,
        f.crate_version,
        f.file,
        f.line,
        f.column,
        f.excerpt_digest
    );
    if actual != *exp {
        failures.push(format!("{rule}: expected `{exp}`, got `{actual}`"));
    }
}

fn check_meta_syn_rule(rule: &str, src: &str, syn_cfg: &syn_scan::Config, failures: &mut Vec<String>) {
    let files = vec!["src/lib.rs".to_string()];
    let fref = FileRef { crate_name: "selftest-meta", crate_version: "0.0.1", file: "src/lib.rs", crate_files: &files };
    let out = syn_scan::scan_file(fref, src.as_bytes(), syn_cfg);
    let matched = out.findings.len() == 1 && out.findings.first().is_some_and(|f| f.rule.id() == rule);
    if !matched {
        let got: Vec<String> = out.findings.iter().map(|f| f.rule.id()).collect();
        failures.push(format!("{rule}: meta-rule selftest expected [{rule}], got {got:?}"));
        return;
    }
    validate_all(rule, &out.findings, failures);
}

fn check_flag_rule(rule: &str, knp1_root: &Path, failures: &mut Vec<String>) {
    let flags_dir = knp1_root.join("flags");
    match rule {
        "KNP1-FLAG-NATIVE"
        | "KNP1-FLAG-CPU"
        | "KNP1-FLAG-FMA"
        | "KNP1-FLAG-CGU"
        | "KNP1-FLAG-LTO"
        | "KNP1-FLAG-TOOLCHAIN"
        | "KNP1-FLAG-KEYWORD"
        | "KNP1-FLAG-AMBIGUOUS"
        | "KNP1-FLAG-WRAPPER" => {
            let (subdir, exact_only) = match rule {
                "KNP1-FLAG-NATIVE" => ("native", true),
                "KNP1-FLAG-CPU" => ("cpu", false),
                "KNP1-FLAG-FMA" => ("fma", true),
                "KNP1-FLAG-CGU" => ("cgu", true),
                "KNP1-FLAG-LTO" => ("lto", true),
                "KNP1-FLAG-TOOLCHAIN" => ("toolchain", true),
                "KNP1-FLAG-KEYWORD" => ("cargo-tables", true),
                "KNP1-FLAG-AMBIGUOUS" => ("cargo-ambiguous", true),
                "KNP1-FLAG-WRAPPER" => ("wrapper", true),
                _ => return,
            };
            let cfg_path = flags_dir.join(subdir).join("knp1-lint.toml");
            match flags::run(&cfg_path) {
                Ok(fs) => {
                    let has_rule = fs.iter().any(|f| f.rule.id() == rule);
                    let only_rule = fs.iter().all(|f| f.rule.id() == rule);
                    if !has_rule || (exact_only && !only_rule) {
                        let got: Vec<String> = fs.iter().map(|f| f.rule.id()).collect();
                        failures.push(format!("{rule}: fixture {} emitted {got:?}", cfg_path.display()));
                        return;
                    }
                    validate_all(rule, &fs, failures);
                }
                Err(e) => failures.push(format!("{rule}: flags fixture {} failed: {e}", cfg_path.display())),
            }
        }
        "KNP1-FLAG-WASMFMA" | "KNP1-FLAG-WASMSIMD" | "KNP1-FLAG-WASMPARSE" => {
            let (file, policy) = match rule {
                "KNP1-FLAG-WASMFMA" => ("relaxed.wasm", SimdPolicy::Default),
                "KNP1-FLAG-WASMSIMD" => ("swizzle.wasm", SimdPolicy::Only(Vec::new())),
                "KNP1-FLAG-WASMPARSE" => ("truncated.wasm", SimdPolicy::Default),
                _ => return,
            };
            let path = flags_dir.join("wasm").join(file);
            match std::fs::read(&path) {
                Ok(bytes) => {
                    let fs = flags::check_wasm(&bytes, file, &policy);
                    let ok = !fs.is_empty() && fs.iter().all(|f| f.rule.id() == rule);
                    if !ok {
                        let got: Vec<String> = fs.iter().map(|f| f.rule.id()).collect();
                        failures.push(format!("{rule}: wasm fixture {} emitted {got:?}", path.display()));
                        return;
                    }
                    validate_all(rule, &fs, failures);
                }
                Err(e) => failures.push(format!("{rule}: cannot read {}: {e}", path.display())),
            }
        }
        "KNP1-FLAG-OPTOUT" => {
            let cfg_path = flags_dir.join("ok").join("knp1-lint.toml");
            let text = match std::fs::read_to_string(&cfg_path) {
                Ok(t) => t,
                Err(e) => {
                    failures.push(format!("{rule}: cannot read {}: {e}", cfg_path.display()));
                    return;
                }
            };
            let cfg_dir = cfg_path.parent().unwrap_or_else(|| Path::new("."));
            let cfg = match flags::parse_config(&text, cfg_dir) {
                Ok(c) => c,
                Err(e) => {
                    failures.push(format!("{rule}: parse_config failed: {e}"));
                    return;
                }
            };
            match flags::check_all(&cfg) {
                Ok(out) => {
                    let ok = out.findings.is_empty()
                        && !out.notes.is_empty()
                        && out.notes.iter().all(|f| f.rule.id() == rule);
                    if !ok {
                        let got: Vec<String> = out.notes.iter().map(|f| f.rule.id()).collect();
                        failures.push(format!(
                            "{rule}: expected clean findings and [{rule}] notes, got findings={} notes={got:?}",
                            out.findings.len()
                        ));
                        return;
                    }
                    validate_all(rule, &out.notes, failures);
                }
                Err(e) => failures.push(format!("{rule}: check_all failed: {e}")),
            }
        }
        _ => failures.push(format!("{rule}: no selftest fixture registered")),
    }
}

fn check_allow_rule(rule: &str, knp1_root: &Path, failures: &mut Vec<String>) {
    let graph_dir = knp1_root.join("graph");
    match rule {
        "KNP1-ALLOW-UNLISTED" | "KNP1-ALLOW-VERSION" => {
            let subdir = if rule == "KNP1-ALLOW-UNLISTED" { "unlisted" } else { "version" };
            let cfg_path = graph_dir.join(subdir).join("knp1-lint.toml");
            match graph::run(&cfg_path) {
                Ok(run) => {
                    let ok = !run.findings.is_empty() && run.findings.iter().all(|f| f.rule.id() == rule);
                    if !ok {
                        let got: Vec<String> = run.findings.iter().map(|f| f.rule.id()).collect();
                        failures.push(format!("{rule}: fixture {} emitted {got:?}", cfg_path.display()));
                        return;
                    }
                    validate_all(rule, &run.findings, failures);
                }
                Err(e) => failures.push(format!("{rule}: graph fixture {} failed: {e}", cfg_path.display())),
            }
        }
        "KNP1-ALLOW-LICENCE" => {
            let spdx_path = graph_dir.join("ok").join("spdx-allowlist.toml");
            let spdx_text = match std::fs::read_to_string(&spdx_path) {
                Ok(t) => t,
                Err(e) => {
                    failures.push(format!("{rule}: cannot read {}: {e}", spdx_path.display()));
                    return;
                }
            };
            let spdx = match graph::parse_spdx_allowlist(&spdx_text) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{rule}: parse_spdx_allowlist failed: {e}"));
                    return;
                }
            };
            let allow = match graph::parse_allowlist(
                "[[allow]]\nname = \"bad-licence-crate\"\nversion = \"=1.0.0\"\nreason = \"selftest\"\nreviewer = \"selftest\"\n",
            ) {
                Ok(a) => a,
                Err(e) => {
                    failures.push(format!("{rule}: parse_allowlist failed: {e}"));
                    return;
                }
            };
            let c = CrateEntry {
                name: "bad-licence-crate".to_string(),
                version: "1.0.0".to_string(),
                source_kind: SourceKind::Vendored,
                root: graph_dir.join("ok"),
                root_label: "bad-licence-crate".to_string(),
                files: vec!["src/lib.rs".to_string()],
                licence: Some("GPL-3.0-only".to_string()),
                path_from_root: vec!["root".to_string(), "bad-licence-crate".to_string()],
            };
            let fs = graph::check(&Closure { crates: vec![c] }, &allow, &spdx);
            let ok = fs.len() == 1 && fs.first().is_some_and(|f| f.rule.id() == rule);
            if !ok {
                let got: Vec<String> = fs.iter().map(|f| f.rule.id()).collect();
                failures.push(format!("{rule}: expected [{rule}], got {got:?}"));
                return;
            }
            validate_all(rule, &fs, failures);
        }
        _ => failures.push(format!("{rule}: no selftest fixture registered")),
    }
}

fn check_data_rule(
    rule: &str,
    data_ws_config: &Path,
    knp1_root: &Path,
    forms_findings: &mut Option<Result<Vec<Finding>, String>>,
    failures: &mut Vec<String>,
) {
    if rule == "KNP1-DATA-VALUE" {
        if !data_ws_config.is_file() {
            failures.push(format!("{rule}: missing data canary config {}", data_ws_config.display()));
            return;
        }
        match data::run(data_ws_config) {
            Ok(run) => {
                let ok = run.findings.len() == 1
                    && run.findings.first().is_some_and(|f| f.rule == Rule::Data("VALUE".to_string()));
                if !ok {
                    let got: Vec<String> = run.findings.iter().map(|f| f.rule.id()).collect();
                    failures.push(format!("{rule}: canary expected [{rule}], got {got:?}"));
                    return;
                }
                validate_all(rule, &run.findings, failures);
            }
            Err(e) => failures.push(format!("{rule}: data canary error: {e}")),
        }
        return;
    }

    let res = forms_findings.get_or_insert_with(|| {
        let data_dir = knp1_root.join("data");
        let dl = data::load_denylist_file(&data_dir.join("denylist.json")).map_err(|e| e.to_string())?;
        let forms_dir = data_dir.join("forms");
        let files = graph::enumerate_files(&forms_dir, &[], &BTreeSet::new()).map_err(|e| e.to_string())?;
        let entry = CrateEntry {
            name: "data-forms".to_string(),
            version: "0.0.1".to_string(),
            source_kind: SourceKind::Vendored,
            root: forms_dir,
            root_label: "data-forms".to_string(),
            files,
            licence: None,
            path_from_root: Vec::new(),
        };
        data::scan_closure(&Closure { crates: vec![entry] }, &dl).map_err(|e| e.to_string())
    });

    match res {
        Ok(fs) => {
            let matching: Vec<Finding> = fs.iter().filter(|f| f.rule.id() == rule).cloned().collect();
            if matching.is_empty() {
                failures.push(format!("{rule}: fixtures/knp1/data/forms emitted no {rule} finding"));
                return;
            }
            validate_all(rule, &matching, failures);
        }
        Err(e) => failures.push(format!("{rule}: data forms fixture error: {e}")),
    }
}

fn check_typed_rule(
    rule: &str,
    knp1_root: &Path,
    typed_findings: &mut Option<Result<Vec<Finding>, String>>,
    failures: &mut Vec<String>,
) {
    let res = typed_findings.get_or_insert_with(|| {
        let typed_dir = knp1_root.join("typed");
        let ws = std::fs::canonicalize(typed_dir.join("ws")).map_err(|e| format!("typed ws: {e}"))?;
        let ws_str = ws.to_string_lossy().replace('\\', "/");
        let ws_norm = ws_str.strip_prefix("//?/").unwrap_or(&ws_str);
        let raw =
            std::fs::read_to_string(typed_dir.join("messages.jsonl")).map_err(|e| format!("messages.jsonl: {e}"))?;
        let msgs = raw.replace("@WS@", ws_norm).into_bytes();
        let mut crates = Vec::new();
        for dir in ["clean", "macro", "minmax-var", "nowasm"] {
            let root = std::fs::canonicalize(ws.join(dir)).map_err(|e| format!("typed/{dir}: {e}"))?;
            crates.push(CrateRef { name: format!("typed-{dir}"), version: "0.0.1".to_string(), root });
        }
        let ctx = MapCtx { cwd: ws, crates, scratch: None };
        let parsed = typed::parse_messages(&msgs, "wasm32-unknown-unknown", &ctx).map_err(|e| e.to_string())?;
        Ok(typed::target_findings(&parsed, &ctx, "wasm32-unknown-unknown", None))
    });

    match res {
        Ok(fs) => {
            let matching: Vec<Finding> = fs.iter().filter(|f| f.rule.id() == rule).cloned().collect();
            if matching.is_empty() {
                failures.push(format!("{rule}: fixtures/knp1/typed/messages.jsonl emitted no {rule} finding"));
                return;
            }
            validate_all(rule, &matching, failures);
        }
        Err(e) => failures.push(format!("{rule}: typed fixture error: {e}")),
    }
}

/// Runs `fid-knp1-lint selftest` driven directly by [`catalogue`], verifying every fixed rule
/// and every concrete rule in every rule family in deterministic ID order
/// (REQ-KRN.NUM.LINT.CLI-004, AC-4).
pub fn run_selftest(args: &SelftestArgs) -> Result<SelftestOutcome, CliError> {
    if !args.seeded_dir.is_dir() {
        return Err(CliError::new(
            CliErrorCode::InputMissing,
            format!("seeded fixtures directory not found: {}", args.seeded_dir.display()),
        ));
    }

    let expected_map = match load_expected_seeded(&args.seeded_dir) {
        Ok(m) => m,
        Err(msg) => {
            return Ok(SelftestOutcome { passed: false, checked_rules: Vec::new(), failures: vec![msg] });
        }
    };
    let knp1_root = knp1_fixtures_root(args);
    let syn_cfg = syn_scan::Config::default();

    let mut checked_rules: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();
    let mut data_forms_cache: Option<Result<Vec<Finding>, String>> = None;
    let mut typed_cache: Option<Result<Vec<Finding>, String>> = None;

    for (entry, _) in catalogue() {
        if entry.ends_with('*') {
            let Some(family_rules) = concrete_family_rules(&entry) else {
                failures.push(format!("{entry}: unknown catalogue rule family with no selftest fixtures"));
                continue;
            };
            for &rule in family_rules {
                checked_rules.push(rule.to_string());
                if rule.starts_with("KNP1-FLAG-") {
                    check_flag_rule(rule, &knp1_root, &mut failures);
                } else if rule.starts_with("KNP1-ALLOW-") {
                    check_allow_rule(rule, &knp1_root, &mut failures);
                } else if rule.starts_with("KNP1-DATA-") {
                    check_data_rule(rule, &args.data_ws_config, &knp1_root, &mut data_forms_cache, &mut failures);
                } else if rule.starts_with("KNP1-TYPED-") {
                    check_typed_rule(rule, &knp1_root, &mut typed_cache, &mut failures);
                } else {
                    failures.push(format!("{rule}: no selftest fixture registered for family rule"));
                }
            }
            continue;
        }

        checked_rules.push(entry.clone());
        match entry.as_str() {
            "KNP1-PARSE" => {
                check_meta_syn_rule(&entry, "pub fn broken() { let s = \"unterminated; }", &syn_cfg, &mut failures)
            }
            "KNP1-PARSE-RESOLVE" => check_meta_syn_rule(
                &entry,
                "macro_rules! m { ($($x:ident),*) => { use a::{$($x),*}; } }\n",
                &syn_cfg,
                &mut failures,
            ),
            "KNP1-CFG" => {
                check_meta_syn_rule(&entry, "#[cfg(debug_assertions)] pub fn f() {}", &syn_cfg, &mut failures)
            }
            "KNP1-INCLUDE" => check_meta_syn_rule(&entry, "include!(\"outside.rs\");", &syn_cfg, &mut failures),
            r if SEEDED_SYN_RULES.contains(&r) => {
                check_seeded_syn_rule(r, &args.seeded_dir, &expected_map, &syn_cfg, &mut failures);
            }
            other => {
                failures.push(format!("{other}: no selftest fixture registered for catalogue rule"));
            }
        }
    }

    checked_rules.sort();
    Ok(SelftestOutcome { passed: failures.is_empty(), checked_rules, failures })
}

fn load_expected_seeded(seeded_dir: &Path) -> Result<BTreeMap<String, String>, String> {
    let path = seeded_dir.join("expected-findings.txt");
    let text = std::fs::read_to_string(&path)
        .map_err(|e| format!("expected-findings.txt missing or unreadable at {}: {e}", path.display()))?;
    let mut map = BTreeMap::new();
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rule) = trimmed.split_whitespace().next() {
            map.insert(rule.to_string(), trimmed.to_string());
        }
    }
    Ok(map)
}

/// Top-level CLI runner returning the process exit code (0, 1 or 2) and writing human-readable
/// diagnostics to `stdout`/`stderr` buffers.
pub fn run_cli(args: &[String]) -> (i32, String, String) {
    let cmd = match parse_args(args) {
        Ok(c) => c,
        Err(e) => return (EXIT_TOOL_ERROR, String::new(), format!("{e}\n")),
    };
    match cmd {
        Command::Check(cargs) => match run_check(&cargs) {
            Ok(out) => {
                let mut err = String::new();
                for m in &out.messages {
                    err.push_str(m);
                    err.push('\n');
                }
                let stdout = match &out.report {
                    Some(r) => format!(
                        "fid-knp1-lint: {} findings, {} waived, {} warnings\n",
                        r.findings.len(),
                        r.waived.len(),
                        r.warnings.len()
                    ),
                    None => String::new(),
                };
                (out.exit_code, stdout, err)
            }
            Err(e) => {
                let _ = std::fs::remove_file(&cargs.out);
                (EXIT_TOOL_ERROR, String::new(), format!("{e}\n"))
            }
        },
        Command::Selftest(sargs) => match run_selftest(&sargs) {
            Ok(out) => {
                if out.passed {
                    (
                        EXIT_CLEAN,
                        format!("fid-knp1-lint selftest: ok ({} rules verified)\n", out.checked_rules.len()),
                        String::new(),
                    )
                } else {
                    let mut err = String::new();
                    for f in &out.failures {
                        err.push_str("FAIL: ");
                        err.push_str(f);
                        err.push('\n');
                    }
                    (EXIT_FINDINGS, String::new(), err)
                }
            }
            Err(e) => (EXIT_TOOL_ERROR, String::new(), format!("{e}\n")),
        },
    }
}
