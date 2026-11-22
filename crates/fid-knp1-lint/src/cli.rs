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

use crate::data::{self, DataErrorCode};
use crate::findings::{
    apply_waivers, input_digest, load_waivers, sort_findings, CrateRoots, ErrorCode, Finding, InputDigests, Report,
    Rule, TypedStatus,
};
use crate::flags::{self, BUILD_CONFIG_CRATE, BUILD_CONFIG_VERSION};
use crate::graph::{self, Closure, CrateEntry, GraphErrorCode, SourceKind};
use crate::syn_scan::{self, FileRef, SynErrorCode};
use crate::typed::{self, TypedErrorCode};

/// Tool version embedded in reports (crate version).
pub const TOOL_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Exit code for a clean run (no unwaived findings, no errors).
pub const EXIT_CLEAN: i32 = 0;
/// Exit code when unwaived findings exist or a fatal rule/waiver/selftest failure occurred.
pub const EXIT_FINDINGS: i32 = 1;
/// Exit code on any tool or input error (fail closed, no success report written).
pub const EXIT_TOOL_ERROR: i32 = 2;

/// The 12 seeded syntax/API rules checked against `fixtures/knp1/seeded/<RULE>/` in rule-ID order.
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

    // Resolve optional denylist path (`--denylist` overrides `[data].denylist`).
    let denylist_path: Option<PathBuf> = if let Some(p) = &args.denylist {
        Some(p.clone())
    } else if let Some(toml::Value::Table(t)) = config_table.get("data") {
        if let Some(k) = t.keys().find(|k| k.as_str() != "denylist") {
            return Err(CliError::new(CliErrorCode::ConfigInvalid, format!("knp1-lint.toml: unknown key data.{k}")));
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
    } else {
        None
    };

    let (denylist_digest, denylist_opt) = match denylist_path {
        Some(p) => {
            if !p.exists() {
                return Err(CliError::new(CliErrorCode::InputMissing, format!("denylist not found: {}", p.display())));
            }
            let dl = data::load_denylist_file(&p).map_err(map_data_err)?;
            (dl.digest.clone(), Some(dl))
        }
        None => (input_digest(b""), None),
    };

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
        let typed_opts = typed::Options {
            cargo: PathBuf::from(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into())),
            scratch: std::env::temp_dir().join("fid-knp1-lint-typed"),
        };
        let typed_out =
            typed::check(&graph_run.closure, &graph_run.config.workspace, &typed_cfg, &syn_cfg, &typed_opts)
                .map_err(map_typed_err)?;
        all_findings.extend(typed_out.findings);
        TypedStatus::Ran
    };

    // 4. FLAGS: build-configuration and emitted-code FMA check (when `[flags]` table is present).
    if config_table.contains_key("flags") {
        let flags_cfg = flags::parse_config(config_text, config_dir)
            .map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("flags: {e}")))?;
        let flags_out = flags::check_all(&flags_cfg)
            .map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("flags: {e}")))?;
        all_findings.extend(flags_out.findings);
    }

    // 5. DATA: reference-dataset denylist scan (when denylist is provided).
    if let Some(dl) = &denylist_opt {
        let data_findings = data::scan_closure(&graph_run.closure, dl).map_err(map_data_err)?;
        all_findings.extend(data_findings);
    }

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

    report.write(&args.out).map_err(|e| CliError::new(CliErrorCode::ConfigInvalid, format!("write report: {e}")))?;

    let exit_code = if report.passed() { EXIT_CLEAN } else { EXIT_FINDINGS };
    Ok(CheckOutcome { exit_code, report: Some(report), messages: Vec::new() })
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

/// Runs `fid-knp1-lint selftest` against every catalogue rule in deterministic ID order
/// (REQ-KRN.NUM.LINT.CLI-004, AC-4).
pub fn run_selftest(args: &SelftestArgs) -> Result<SelftestOutcome, CliError> {
    if !args.seeded_dir.is_dir() {
        return Err(CliError::new(
            CliErrorCode::InputMissing,
            format!("seeded fixtures directory not found: {}", args.seeded_dir.display()),
        ));
    }

    // Load optional expected-findings.txt (maps rule_id -> expected line).
    let expected_map = load_expected_seeded(&args.seeded_dir);
    let syn_cfg = syn_scan::Config::default();

    let mut checked_rules: Vec<String> = Vec::new();
    let mut failures: Vec<String> = Vec::new();

    // 1. All 12 seeded syntax/API rules under `<seeded>/<RULE>/`.
    for rule in SEEDED_SYN_RULES {
        checked_rules.push(rule.to_string());
        let dir = args.seeded_dir.join(rule);
        if !dir.is_dir() {
            failures.push(format!("{rule}: missing seeded fixture directory {}", dir.display()));
            continue;
        }
        let files = match graph::enumerate_files(&dir, &[], &BTreeSet::new()) {
            Ok(f) if !f.is_empty() => f,
            Ok(_) => {
                failures.push(format!("{rule}: seeded fixture directory {} has no .rs files", dir.display()));
                continue;
            }
            Err(e) => {
                failures.push(format!("{rule}: cannot enumerate seeded fixture: {e}"));
                continue;
            }
        };
        let crate_name = format!("seeded-{}", rule.to_ascii_lowercase());
        let entry = CrateEntry {
            name: crate_name.clone(),
            version: "0.0.1".to_string(),
            source_kind: SourceKind::Vendored,
            root: dir,
            root_label: rule.to_string(),
            files,
            licence: None,
            path_from_root: Vec::new(),
        };
        let closure = Closure { crates: vec![entry] };
        let out = match syn_scan::scan_closure(&closure, &syn_cfg) {
            Ok(o) => o,
            Err(e) => {
                failures.push(format!("{rule}: syn scan error: {e}"));
                continue;
            }
        };
        if out.findings.len() != 1 {
            let got: Vec<String> = out.findings.iter().map(|f| f.rule.id()).collect();
            failures.push(format!("{rule}: expected exactly 1 finding, got {} ({got:?})", out.findings.len()));
            continue;
        }
        let Some(f) = out.findings.first() else {
            failures.push(format!("{rule}: missing finding"));
            continue;
        };
        if f.rule.id() != rule {
            failures.push(format!("{rule}: expected rule {rule}, got {}", f.rule.id()));
            continue;
        }
        if let Err(e) = f.validate() {
            failures.push(format!("{rule}: invalid finding emitted: {e}"));
            continue;
        }
        if let Some(exp) = expected_map.get(rule) {
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
    }

    // 2. The 4 syntax meta-rules (KNP1-PARSE, KNP1-PARSE-RESOLVE, KNP1-CFG, KNP1-INCLUDE).
    let meta_cases: [(&str, &str); 4] = [
        ("KNP1-PARSE", "pub fn broken() { let s = \"unterminated; }"),
        ("KNP1-PARSE-RESOLVE", "macro_rules! m { ($($x:ident),*) => { use a::{$($x),*}; } }\n"),
        ("KNP1-CFG", "#[cfg(debug_assertions)] pub fn f() {}"),
        ("KNP1-INCLUDE", "include!(\"outside.rs\");"),
    ];
    for (rule, src) in meta_cases {
        checked_rules.push(rule.to_string());
        let files = vec!["src/lib.rs".to_string()];
        let fref =
            FileRef { crate_name: "selftest-meta", crate_version: "0.0.1", file: "src/lib.rs", crate_files: &files };
        let out = syn_scan::scan_file(fref, src.as_bytes(), &syn_cfg);
        let matched = out.findings.len() == 1 && out.findings.first().is_some_and(|f| f.rule.id() == rule);
        if !matched {
            let got: Vec<String> = out.findings.iter().map(|f| f.rule.id()).collect();
            failures.push(format!("{rule}: meta-rule selftest expected [{rule}], got {got:?}"));
        }
    }

    // 3. DATA canary (`KNP1-DATA-VALUE`) via `--data-ws`.
    checked_rules.push("KNP1-DATA-VALUE".to_string());
    if !args.data_ws_config.is_file() {
        failures.push(format!("KNP1-DATA-VALUE: missing data canary config {}", args.data_ws_config.display()));
    } else {
        match data::run(&args.data_ws_config) {
            Ok(run) => {
                let ok = run.findings.len() == 1
                    && run.findings.first().is_some_and(|f| f.rule == Rule::Data("VALUE".to_string()));
                if !ok {
                    let got: Vec<String> = run.findings.iter().map(|f| f.rule.id()).collect();
                    failures.push(format!("KNP1-DATA-VALUE: canary expected [KNP1-DATA-VALUE], got {got:?}"));
                }
            }
            Err(e) => failures.push(format!("KNP1-DATA-VALUE: data canary error: {e}")),
        }
    }

    // 4. GRAPH allowlist rule (`KNP1-ALLOW-UNLISTED`) self-check.
    checked_rules.push("KNP1-ALLOW-UNLISTED".to_string());
    {
        let c = CrateEntry {
            name: "unlisted-crate".to_string(),
            version: "1.0.0".to_string(),
            source_kind: SourceKind::Vendored,
            root: PathBuf::from("."),
            root_label: "unlisted-crate".to_string(),
            files: vec!["src/lib.rs".to_string()],
            licence: Some("MIT".to_string()),
            path_from_root: vec!["root".to_string(), "unlisted-crate".to_string()],
        };
        let spdx = graph::parse_spdx_allowlist("licences = [\"MIT\"]\nexceptions = []\n");
        match spdx {
            Ok(spdx) => {
                let fs = graph::check(&Closure { crates: vec![c] }, &[], &spdx);
                let ok = fs.len() == 1 && fs.first().is_some_and(|f| f.rule == Rule::Allow("UNLISTED".to_string()));
                if !ok {
                    let got: Vec<String> = fs.iter().map(|f| f.rule.id()).collect();
                    failures.push(format!("KNP1-ALLOW-UNLISTED: expected [KNP1-ALLOW-UNLISTED], got {got:?}"));
                }
            }
            Err(e) => failures.push(format!("KNP1-ALLOW-UNLISTED: spdx setup error: {e}")),
        }
    }

    // 5. FLAGS rule (`KNP1-FLAG-NATIVE`) self-check.
    checked_rules.push("KNP1-FLAG-NATIVE".to_string());
    {
        let toks = flags::tokens("-C target-cpu=native");
        let opts = flags::codegen_options(&toks);
        let saw_native = opts.iter().any(|o| o.key == "target-cpu" && o.value == "native");
        if !saw_native {
            failures.push("KNP1-FLAG-NATIVE: flags parser did not detect target-cpu=native".to_string());
        }
    }

    checked_rules.sort();
    Ok(SelftestOutcome { passed: failures.is_empty(), checked_rules, failures })
}

fn load_expected_seeded(seeded_dir: &Path) -> BTreeMap<String, String> {
    let mut map = BTreeMap::new();
    let path = seeded_dir.join("expected-findings.txt");
    let Ok(text) = std::fs::read_to_string(&path) else {
        return map;
    };
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        if let Some(rule) = trimmed.split_whitespace().next() {
            map.insert(rule.to_string(), trimmed.to_string());
        }
    }
    map
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
            Err(e) => (EXIT_TOOL_ERROR, String::new(), format!("{e}\n")),
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
