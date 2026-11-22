//! `cargo xtask <task>` (alias in `.cargo/config.toml`).

mod pins;

use fid_xtask::plat_deploy_netlist as netlist;
use fid_xtask::rel::{keys, runbook, sbom, sums, version};
use fid_xtask::{apps_policy, plat_gpu_invar, plat_matrix_tiers};
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask layers [--workspace <dir>] [--map <layers.toml>]\n       \
                     cargo xtask datasets-check --catalogue <toml> --domains <file>\n       \
                     cargo xtask data fetch (<id>... | --all | --owner <DOMAIN>) [--cache <dir>] [--offline] \
                     [--catalogue <toml>]\n       \
                     cargo xtask data path <id> [--cache <dir>] [--offline] [--catalogue <toml>]\n       \
                     cargo xtask data verify [--cache <dir>]\n       \
                     cargo xtask bots-check --register <toml> --restricted <txt> --today <YYYY-MM-DD> \
                     [--review-export <json>]\n       \
                     cargo xtask bot-blocking --threads <json> --register <bots.toml> --maintainers <txt>\n       \
                     cargo xtask plat-matrix validate <file>\n       \
                     cargo xtask plat-matrix page [--check] [--matrix <file>] --out <file>\n       \
                     cargo xtask plat-matrix lint-tiers [<file>]\n       \
                     cargo xtask plat-hw validate [--file <path>]\n       \
                     cargo xtask plat-hw validate-suite [--file <path>]\n       \
                     cargo xtask plat-hw result-schema\n       \
                     cargo xtask plat-deploy netlist --manifest <registry manifest json> \
                     --settings <settings-manifest.json> [--json <out>] [--md <out>] [--check]\n       \
                     cargo xtask plat-gpu invariance --backends <a,b,...> [--sequence <toml>] \
                     [--examples <dir>] [--work <dir>] [--fid <path>]\n       \
                     cargo xtask plat-gpu invariance --recorded <dir>\n       \
                     cargo xtask plat-gpu validate [--policy <file>] [--known-bad <file>] [--matrix <file>] \
                     [--phase <P>]\n       \
                     cargo xtask plat-deploy validate-layout [--file <file>] [--matrix <file>] [--phase <P>]\n       \
                     cargo xtask plat-deploy check-bundle --bundle <dir> --plan <package-plan.json>\n       \
                     cargo xtask plat-deploy check-bundles --plan <package-plan.json> --bundle <dir> \
                     [--bundle <dir> ...]\n       \
                     cargo xtask plat-deploy make-install-manifest --root <dir> --kind <per_user|machine> \
                     --release <v> --os <os> --arch <arch> [--out <file>]\n       \
                     cargo xtask plat-deploy bundle-schemas\n       \
                     cargo xtask personas-check [--personas <json>] [--apps <toml>]\n       \
                     cargo xtask study-tasks-check [--tasks <json>] [--maps <dir>] [--personas <json>] \
                     [--protocol <md>]\n       \
                     cargo xtask rel version-check --tag <tag> --channel nightly|beta|stable|lts \
                     [--workspace <dir>] [--binaries <dir>] [--json]\n       \
                     cargo xtask rel version-compare <a> <b>\n       \
                     cargo xtask rel sums --manifest <release-manifest.json> --dir <artefacts> --out <dir>
       \n                     cargo xtask rel runbook-check [--dir <release dir>] [--workflow <file>] [--json]
       \n                     cargo xtask rel runbook-render [--dir <release dir>] [--out <file>]
       \n                     cargo xtask rel sbom --plan <package-plan.json> [--workspace <dir>] --native <toml> \
                     --out <dir> [--artefacts <dir>] [--commit-time <YYYY-MM-DDThh:mm:ssZ>]
       \n                     cargo xtask rel keys-check [--keys <dir>] [--doc <KEYS.md>] [--date <YYYY-MM-DD>]";
const APPS_POLICY_USAGE: &str = "usage: cargo xtask apps-policy --workspace <dir> --registry-manifests <dir> \
                                 --apps-status <json> [--config apps-policy.toml]";
const KERNEL_ALLOW_USAGE: &str =
    "usage: cargo xtask kernel-allow [--workspace <dir>] [--map <layers.toml>] [--allowlist <kernel-allowlist.toml>]";
const MIG_LADDER_USAGE: &str =
    "usage: cargo xtask mig ladder-check --ladder <json> --manifest <registry manifest json>";
const MIG_SOURCES_USAGE: &str =
    "usage: cargo xtask mig sources-check --register <json> --manifest <registry manifest json> [--vocab-rules <toml>]";
const GEN_SURFACES_USAGE: &str =
    "usage: cargo xtask gen-surfaces --manifest <registry manifest json> --out <dir> [--test-emitter echo]...";
const LINT_USAGE: &str =
    "usage: cargo xtask lint-crate-graph [--workspace <dir>] [--allowlist <file>] [--writers <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layers") => layers(&args[1..]),
        Some("lint-crate-graph") => lint_crate_graph(&args[1..]),
        Some("kernel-allow") => kernel_allow(&args[1..]),
        Some("datasets-check" | "data") => datasets_check(&args),
        Some("bots-check") => bots_check(&args[1..]),
        Some("bot-blocking") => bot_blocking(&args[1..]),
        Some("plat-matrix") if args.get(1).map(String::as_str) == Some("lint-tiers") => lint_tiers(&args[2..]),
        Some("plat-matrix") => fid_platform(&args, "plat-matrix"),
        Some("plat-hw") => fid_platform(&args, "plat-hw"),
        Some("plat-deploy") if args.get(1).map(String::as_str) == Some("netlist") => plat_deploy_netlist(&args[2..]),
        Some("plat-gpu") if args.get(1).map(String::as_str) == Some("invariance") => plat_gpu_invariance(&args[2..]),
        Some("plat-gpu") if args.get(1).map(String::as_str) == Some("validate") => fid_platform(&args, "plat-gpu"),
        Some("plat-deploy")
            if matches!(
                args.get(1).map(String::as_str),
                Some("validate-layout" | "check-bundle" | "check-bundles" | "make-install-manifest" | "bundle-schemas")
            ) =>
        {
            fid_platform(&args, "plat-deploy")
        }
        Some("pins") => pins::cli(&args[1..]),
        Some("mig") if args.get(1).map(String::as_str) == Some("ladder-check") => mig_ladder_check(&args[2..]),
        Some("mig") if args.get(1).map(String::as_str) == Some("sources-check") => mig_sources_check(&args[2..]),
        Some("conic-audit") => fid_xtask::conic_audit::cli(&args[1..]),
        Some("registry-gate") => fid_xtask::registry_gate::cli(&args[1..]),
        Some("convergence-check") => fid_xtask::convergence::check_cli(&args[1..]),
        Some("convergence-doc") => fid_xtask::convergence::doc_cli(&args[1..]),
        Some("gen-surfaces") => gen_surfaces(&args[1..]),
        Some("surface-check") => surface_check(&args[1..]),
        Some("crash-harness") => crash_harness(&args[1..]),
        Some("personas-check") => personas_check(&args[1..]),
        Some("apps-policy") => apps_policy_check(&args[1..]),
        Some("study-tasks-check") => study_tasks_check(&args[1..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("version-check") => version_check(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("version-compare") => version_compare(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("sums") => rel_sums(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("runbook-check") => runbook_check(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("runbook-render") => runbook_render(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("sbom") => rel_sbom(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("keys-check") => keys_check(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("notes") => fid_xtask::rel::notes::cli(&args[2..]),
        _ => {
            eprintln!("{USAGE}");
            eprintln!("{LINT_USAGE}");
            eprintln!("{KERNEL_ALLOW_USAGE}");
            eprintln!("{APPS_POLICY_USAGE}");
            eprintln!("{MIG_LADDER_USAGE}");
            eprintln!("{MIG_SOURCES_USAGE}");
            eprintln!("{}", fid_xtask::conic_audit::USAGE);
            eprintln!("{}", fid_xtask::registry_gate::USAGE);
            eprintln!("{}", fid_xtask::convergence::CHECK_USAGE);
            eprintln!("{}", fid_xtask::convergence::DOC_USAGE);
            eprintln!("{GEN_SURFACES_USAGE}");
            eprintln!("{}", fid_xtask::rel::notes::USAGE);
            ExitCode::from(2)
        }
    }
}

/// MIG.LADDER.DEF: `mig ladder-check`. Prints the schema, version and digest of the ladder
/// when it passes, otherwise one line per finding (`LADDER-SCHEMA`, `LADDER-FORWARD`,
/// `LADDER-MISSING-CMD`, `LADDER-BASIS`). Exit 0: pass; 1: findings; 2: usage error or an
/// unreadable file. Offline; reads only.
fn mig_ladder_check(args: &[String]) -> ExitCode {
    let (mut ladder, mut manifest) = (None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--ladder", Some(v)) => ladder = Some(PathBuf::from(v)),
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{MIG_LADDER_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(ladder), Some(manifest)) = (ladder, manifest) else {
        eprintln!("mig ladder-check: --ladder and --manifest are required\n{MIG_LADDER_USAGE}");
        return ExitCode::from(2);
    };
    let read = |p: &Path, limit: usize| -> Result<Vec<u8>, String> {
        let file = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut bytes = Vec::new();
        // One byte over the limit is enough for the parser to report the input as too large.
        let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
        file.take(cap).read_to_end(&mut bytes).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(bytes)
    };
    let inputs = read(&ladder, fid_mig::ladder::def::MAX_LADDER_BYTES)
        .and_then(|l| read(&manifest, fid_mig::ladder::def::MAX_MANIFEST_BYTES).map(|m| (l, m)));
    let (ladder_bytes, manifest_bytes) = match inputs {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mig ladder-check: cannot read {e}");
            return ExitCode::from(2);
        }
    };
    let report = fid_mig::ladder::def::check(&ladder_bytes, &manifest_bytes);
    for f in &report.findings {
        println!("{f}");
    }
    match &report.ladder {
        Some(l) if report.findings.is_empty() => {
            println!("ladder-check: ok {} version {} digest {}", l.schema(), l.version(), l.digest());
            ExitCode::SUCCESS
        }
        _ => {
            println!("ladder-check: {} findings", report.findings.len());
            ExitCode::from(1)
        }
    }
}

/// MIG.SRC.REG: `mig sources-check`. Checks a `mig-sources@1` register: schema, every open
/// format registered in the manifest's format catalogue (`SRC-SCHEMA`), `SRC-DUP`,
/// `SRC-REVIEW`, `SRC-ATTEST`, `SRC-POLICY`, then, with `--vocab-rules`, the vocabulary lint
/// of every entry text (`SRC-VOCAB` for each error-severity finding). Prints one line per
/// finding, or the register's schema, version, digest and entry count when it passes; without
/// `--vocab-rules` it says that the vocabulary check was skipped. Exit 0: pass; 1: findings;
/// 2: usage error or an unreadable file. Offline; reads only.
fn mig_sources_check(args: &[String]) -> ExitCode {
    use fid_mig::src::reg;
    let (mut register, mut manifest, mut vocab) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--register", Some(v)) if register.is_none() => register = Some(PathBuf::from(v)),
            ("--manifest", Some(v)) if manifest.is_none() => manifest = Some(PathBuf::from(v)),
            ("--vocab-rules", Some(v)) if vocab.is_none() => vocab = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{MIG_SOURCES_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(register), Some(manifest)) = (register, manifest) else {
        eprintln!("mig sources-check: --register and --manifest are required");
        eprintln!("{MIG_SOURCES_USAGE}");
        return ExitCode::from(2);
    };
    let read = |p: &Path, limit: usize| -> Result<Vec<u8>, String> {
        let f = std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))?;
        let mut b = Vec::new();
        // One byte over the limit is enough for the parser to report the input as too large.
        let cap = u64::try_from(limit).unwrap_or(u64::MAX).saturating_add(1);
        f.take(cap).read_to_end(&mut b).map_err(|e| format!("{}: {e}", p.display()))?;
        Ok(b)
    };
    let inputs =
        read(&register, reg::MAX_REGISTER_BYTES).and_then(|r| read(&manifest, reg::MAX_MANIFEST_BYTES).map(|m| (r, m)));
    let (bytes, manifest_bytes) = match inputs {
        Ok(v) => v,
        Err(e) => {
            eprintln!("mig sources-check: cannot read {e}");
            return ExitCode::from(2);
        }
    };
    let report = reg::check(&bytes, &manifest_bytes);
    let mut findings = report.findings.clone();
    if let (Some(r), Some(rules_path)) = (&report.register, &vocab) {
        let rules = match fid_vocab::RuleSet::load(rules_path) {
            Ok(r) => r,
            Err(e) => {
                eprintln!("mig sources-check: cannot load {}: {e}", rules_path.display());
                return ExitCode::from(2);
            }
        };
        let units: Vec<fid_vocab::Unit> = reg::texts(r)
            .into_iter()
            .map(|(id, field, text)| fid_vocab::Unit::new(format!("{id}#{field}"), text))
            .collect();
        for f in fid_vocab::lint(&rules, &units).findings {
            if f.severity == fid_vocab::Severity::Error {
                let id = f.unit.split('#').next().unwrap_or_default().to_string();
                findings.push(reg::Finding::Vocab { id, rule: f.rule, message: f.message });
            }
        }
    }
    for f in &findings {
        println!("{f}");
    }
    if vocab.is_none() {
        println!("sources-check: vocabulary check skipped (no --vocab-rules)");
    }
    match &report.register {
        Some(r) if findings.is_empty() => {
            println!(
                "sources-check: ok {} version {} digest {} entries {}",
                reg::SCHEMA,
                r.version(),
                r.digest(),
                r.entries().len()
            );
            ExitCode::SUCCESS
        }
        _ => {
            println!("sources-check: {} findings", findings.len());
            ExitCode::from(1)
        }
    }
}

/// ECO.APPS.POLICY: `apps-policy`. Prints findings sorted by code, crate and location. Exit
/// 0: no findings; 1: findings (including POLICY-INPUT); 2: bad usage. Offline; reads only.
fn apps_policy_check(args: &[String]) -> ExitCode {
    let (mut workspace, mut manifests, mut status, mut config) = (None, None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = Some(PathBuf::from(v)),
            ("--registry-manifests", Some(v)) => manifests = Some(PathBuf::from(v)),
            ("--apps-status", Some(v)) => status = Some(PathBuf::from(v)),
            ("--config", Some(v)) => config = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{APPS_POLICY_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(workspace), Some(registry_manifests), Some(apps_status)) = (workspace, manifests, status) else {
        eprintln!("apps-policy: --workspace, --registry-manifests and --apps-status are required\n{APPS_POLICY_USAGE}");
        return ExitCode::from(2);
    };
    let findings = apps_policy::run(&apps_policy::Inputs { workspace, registry_manifests, apps_status, config });
    if findings.is_empty() {
        println!("apps-policy: ok");
        return ExitCode::SUCCESS;
    }
    for f in &findings {
        println!("{f}");
    }
    ExitCode::from(1)
}

/// ECO.DATA.CAT (`datasets-check`) and ECO.DATA.FETCH (`data fetch|path|verify`): runs the
/// `fid-data` binary. `fid-data` is a `tool` crate, and the layer
/// rule forbids depending on one, so the wrapper starts it through Cargo instead.
fn datasets_check(args: &[String]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args(["run", "-q", "--manifest-path", manifest, "-p", "fid-data", "--bin", "fid-data", "--"])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(1)),
        Err(e) => {
            println!("DATA-IO -: cannot run cargo: {e}");
            ExitCode::from(1)
        }
    }
}

/// ECO.CI.BOTS.REG: exit 0 when the register passes, 1 on any finding, 2 on a usage error.
fn bots_check(args: &[String]) -> ExitCode {
    let mut opts: BTreeMap<&str, &str> = BTreeMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let known = matches!(a.as_str(), "--register" | "--restricted" | "--today" | "--review-export");
        match it.next() {
            Some(v) if known && !opts.contains_key(a.as_str()) => {
                opts.insert(a.as_str(), v.as_str());
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(register), Some(restricted), Some(today)) =
        (opts.get("--register"), opts.get("--restricted"), opts.get("--today"))
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    let export = opts.get("--review-export").map(Path::new);
    match fid_xtask::bots::check(Path::new(register), Path::new(restricted), today, export) {
        Ok(findings) if findings.is_empty() => {
            println!("bots-check: ok");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// ECO.CI.BOTS.BLOCK: exit 0 when no blocking bot thread is unresolved, 1 when one is or an
/// input is missing or malformed (`BLOCK-INPUT`, fail closed), 2 on a usage error.
fn bot_blocking(args: &[String]) -> ExitCode {
    let mut opts: BTreeMap<&str, &str> = BTreeMap::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let known = matches!(a.as_str(), "--threads" | "--register" | "--maintainers");
        match it.next() {
            Some(v) if known && !opts.contains_key(a.as_str()) => {
                opts.insert(a.as_str(), v.as_str());
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(threads), Some(register), Some(maintainers)) =
        (opts.get("--threads"), opts.get("--register"), opts.get("--maintainers"))
    else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    use fid_xtask::bots::block;
    match block::check(Path::new(threads), Path::new(register), Path::new(maintainers)) {
        Ok(report) => {
            for line in report.lines() {
                println!("{line}");
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            let e: String = e.chars().map(|c| if c.is_control() { ' ' } else { c }).collect();
            println!("{} {e}", block::INPUT);
            ExitCode::from(1)
        }
    }
}

/// PLAT.MATRIX.FILE, PLAT.HW.PROFILE, PLAT.GPU.POLICY, PLAT.DEPLOY.NOADMIN.LAYOUT and
/// PLAT.DEPLOY.OFFLINE.BUNDLE: runs `fid-platform plat-matrix ...`,
/// `fid-platform plat-deploy validate-layout|check-bundle|make-install-manifest|bundle-schemas ...`,
/// `fid-platform plat-hw ...` or `fid-platform plat-gpu validate ...` through Cargo, for the same layer reason as `datasets-check`
/// (`fid-platform` is a `tool` crate).
/// AUTO.SURF.GEN: `gen-surfaces`. Runs the `fid-surface-gen` binary, a `tool` crate that no
/// other crate may link (ECO.REPO.LAYERS), with the same arguments and exit code.
fn gen_surfaces(args: &[String]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args(["run", "-q", "--manifest-path", manifest, "-p", "fid-surface-gen", "--bin", "fid-surface-gen", "--"])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(2)),
        Err(e) => {
            eprintln!("gen-surfaces: cannot run cargo: {e}");
            ExitCode::from(2)
        }
    }
}

fn surface_check(args: &[String]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args(["run", "-q", "--manifest-path", manifest, "-p", "fid-surface-gen", "--bin", "fid-surface-check", "--"])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(2)),
        Err(e) => {
            eprintln!("surface-check: cannot run cargo: {e}");
            ExitCode::from(2)
        }
    }
}

/// CORE.STO.SQL.CRASH: `crash-harness [--seed N] [--kills N] [--power-loss ...]`. Runs
/// `fid-crash run` (a `test-support` crate that builds SQLite, so it is not linked into xtask),
/// in release with its `bus-errors` feature, with the same arguments and exit code.
fn crash_harness(args: &[String]) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args([
            "run",
            "-q",
            "--release",
            "--manifest-path",
            manifest,
            "-p",
            "fid-crash",
            "--features",
            "bus-errors",
            "--bin",
            "fid-crash",
            "--",
            "run",
        ])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(2)),
        Err(e) => {
            eprintln!("crash-harness: cannot run cargo: {e}");
            ExitCode::from(2)
        }
    }
}

fn fid_platform(args: &[String], group: &str) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let manifest = concat!(env!("CARGO_MANIFEST_DIR"), "/../../Cargo.toml");
    let status = Command::new(cargo)
        .args(["run", "-q", "--manifest-path", manifest, "-p", "fid-platform", "--bin", "fid-platform", "--"])
        .args(args)
        .status();
    match status {
        Ok(s) => ExitCode::from(s.code().and_then(|c| u8::try_from(c).ok()).unwrap_or(2)),
        Err(e) => {
            eprintln!("{group}: cannot run cargo: {e}");
            ExitCode::from(2)
        }
    }
}

/// PLAT.GPU.FALLBK.INVAR: `plat-gpu invariance`. With `--backends`, runs the invariance
/// sequence once per backend through `fid --gpu-backend` and compares the digest lists; with
/// `--recorded <dir>`, compares recorded lists only. Prints the report (or the error). Exit 0:
/// all lists equal; 1: Diverged, BackendUnavailable or SequenceFailed; 2: bad usage or input.
fn plat_gpu_invariance(args: &[String]) -> ExitCode {
    let mut backends: Option<&str> = None;
    let mut recorded: Option<PathBuf> = None;
    let mut sequence = PathBuf::from(plat_gpu_invar::DEFAULT_SEQUENCE);
    let mut examples = PathBuf::from(plat_gpu_invar::DEFAULT_EXAMPLES);
    let mut work: Option<PathBuf> = None;
    let mut fid = PathBuf::from("fid");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--backends", Some(v)) => backends = Some(v),
            ("--recorded", Some(v)) => recorded = Some(PathBuf::from(v)),
            ("--sequence", Some(v)) => sequence = PathBuf::from(v),
            ("--examples", Some(v)) => examples = PathBuf::from(v),
            ("--work", Some(v)) => work = Some(PathBuf::from(v)),
            ("--fid", Some(v)) => fid = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let result = match (backends, recorded) {
        (None, Some(dir)) => plat_gpu_invar::read_recorded(&dir).map(|lists| {
            let mut report = plat_gpu_invar::compare(&lists);
            report.recorded = true;
            report
        }),
        (Some(list), None) => match plat_gpu_invar::parse_backends(list) {
            Ok(backends) => {
                // A default work folder is ours alone (named by process ID) and removed on exit,
                // whatever the outcome; a `--work` folder given by the caller is left in place.
                let temporary = work.is_none();
                let default_work = format!("fid-plat-gpu-invariance-{}", std::process::id());
                let work = work.unwrap_or_else(|| std::env::temp_dir().join(default_work));
                let opts = plat_gpu_invar::RunOptions {
                    sequence: &sequence,
                    examples: &examples,
                    work: &work,
                    backends: &backends,
                    host_os: std::env::consts::OS,
                };
                let result = plat_gpu_invar::run(&opts, &mut plat_gpu_invar::FidDriver::new(fid));
                if temporary && work.exists() {
                    if let Err(e) = std::fs::remove_dir_all(&work) {
                        eprintln!("plat-gpu invariance: cannot remove {}: {e}", work.display());
                    }
                }
                result
            }
            Err(e) => Err(e),
        },
        _ => {
            eprintln!("plat-gpu invariance: give exactly one of --backends and --recorded\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match result {
        Ok(report) => {
            print!("{}", report.render());
            if report.equal {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(e.exit_code())
        }
    }
}

/// FLOW.PERSONA.PERS: checks the persona definitions file against the app list and the
/// built-in governance profile names. Exit 0: clean; 1: findings; 2: usage error.
fn personas_check(args: &[String]) -> ExitCode {
    let mut personas = PathBuf::from("acceptance/personas/personas.json");
    let mut apps = PathBuf::from("apps.toml");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--personas", Some(v)) => personas = PathBuf::from(v),
            ("--apps", Some(v)) => apps = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let report = fid_accept::persona::check_files(&personas, &apps);
    for f in &report.findings {
        println!("{f}");
    }
    if report.is_ok() {
        println!("personas-check: {} personas", report.set.personas.len());
        ExitCode::SUCCESS
    } else {
        println!("personas-check: {} findings", report.findings.len());
        ExitCode::from(1)
    }
}

/// FLOW.STUDY.PROTO: checks the study task list against the journey maps and the persona
/// file, and the protocol document's citations. Exit 0: clean; 1: findings; 2: usage error.
fn study_tasks_check(args: &[String]) -> ExitCode {
    let mut tasks = PathBuf::from("acceptance/study/tasks.json");
    let mut maps = PathBuf::from("acceptance/journeys/persona");
    let mut personas = PathBuf::from("acceptance/personas/personas.json");
    let mut protocol = PathBuf::from("docs/study/protocol.md");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--tasks", Some(v)) => tasks = PathBuf::from(v),
            ("--maps", Some(v)) => maps = PathBuf::from(v),
            ("--personas", Some(v)) => personas = PathBuf::from(v),
            ("--protocol", Some(v)) => protocol = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let report = fid_accept::study::check_files(&tasks, &maps, &personas, Some(&protocol));
    for f in &report.findings {
        println!("{f}");
    }
    if report.is_ok() {
        println!("study-tasks-check: {} tasks", report.tasks.len());
        ExitCode::SUCCESS
    } else {
        println!("study-tasks-check: {} findings", report.findings.len());
        ExitCode::from(1)
    }
}

/// PLAT.MATRIX.POLICY.DEFS: `plat-matrix lint-tiers [<file>]` (default
/// `docs/platform/tiers.md`). Prints one finding per line; exits 0 when the tier table is
/// complete, 1 on findings and 2 on bad usage or an unreadable file.
fn lint_tiers(args: &[String]) -> ExitCode {
    let path = match args {
        [] => plat_matrix_tiers::DEFAULT_PATH,
        [file] => file.as_str(),
        _ => {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        }
    };
    // Read at most one byte past the limit so an oversized file is refused, not loaded.
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(plat_matrix_tiers::MAX_BYTES + 1).read_to_end(&mut bytes));
    if let Err(e) = read {
        eprintln!("plat-matrix lint-tiers: cannot read {path}: {e}");
        return ExitCode::from(2);
    }
    if bytes.len() as u64 > plat_matrix_tiers::MAX_BYTES {
        eprintln!("plat-matrix lint-tiers: {path} is larger than {} bytes", plat_matrix_tiers::MAX_BYTES);
        return ExitCode::from(2);
    }
    let Ok(text) = String::from_utf8(bytes) else {
        eprintln!("plat-matrix lint-tiers: {path} is not UTF-8");
        return ExitCode::from(2);
    };
    let findings = plat_matrix_tiers::lint(&text);
    if findings.is_empty() {
        println!("lint-tiers: ok");
        return ExitCode::SUCCESS;
    }
    for f in findings {
        println!("{f}");
    }
    ExitCode::from(1)
}

/// ECO.REL.PROC.VERSION: `rel version-check`. Exit 0: the tag, workspace and binaries agree;
/// 1: findings (one per line, or one JSON report with `--json`); 2: bad usage. Offline: the
/// tag comes from `--tag`, never from git or the network, and nothing is written.
fn version_check(args: &[String]) -> ExitCode {
    let mut tag: Option<&str> = None;
    let mut channel: Option<version::Channel> = None;
    let mut workspace = PathBuf::from(".");
    let mut binaries: Option<PathBuf> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            json = true;
            continue;
        }
        match (a.as_str(), it.next()) {
            ("--tag", Some(v)) => tag = Some(v),
            ("--channel", Some(v)) => match version::Channel::parse(v) {
                Some(c) => channel = Some(c),
                None => {
                    eprintln!("rel version-check: unknown channel {v:?}\n{USAGE}");
                    return ExitCode::from(2);
                }
            },
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--binaries", Some(v)) => binaries = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(tag), Some(channel)) = (tag, channel) else {
        eprintln!("rel version-check: --tag and --channel are required\n{USAGE}");
        return ExitCode::from(2);
    };
    let result = version::version_check(tag, channel, &workspace, binaries.as_deref());
    if json {
        println!("{}", version::report_json(tag, channel, &result));
    }
    match result {
        Ok(v) => {
            if !json {
                println!("version-check: ok {v} ({})", channel.name());
            }
            ExitCode::SUCCESS
        }
        Err(findings) => {
            if !json {
                for f in findings {
                    println!("{f}");
                }
            }
            ExitCode::from(1)
        }
    }
}

/// ECO.REL.SIGN.SUMS: `rel sums --manifest <file> --dir <dir> --out <dir>` writes `SHA256SUMS`
/// and `sums-statement.json` into the output directory. Exit 0: written; 1: findings
/// (SUMS-MISSING, SUMS-UNLISTED, SUMS-NAME, SUMS-IO), one per line, nothing written;
/// 2: bad usage or an unparseable manifest (INPUT-PARSE). Offline; signs nothing.
fn rel_sums(args: &[String]) -> ExitCode {
    let (mut manifest, mut dir, mut out): (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) = (None, None, None);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            ("--dir", Some(v)) => dir = Some(PathBuf::from(v)),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(manifest), Some(dir), Some(out)) = (manifest, dir, out) else {
        eprintln!("rel sums: --manifest, --dir and --out are required\n{USAGE}");
        return ExitCode::from(2);
    };
    match sums::run(&manifest, &dir, &out) {
        Ok(s) => {
            println!("sums: ok {} artefacts ({})", s.entries.len(), s.release);
            ExitCode::SUCCESS
        }
        Err(findings) => {
            let code = findings.iter().map(sums::SumsError::exit_code).max().unwrap_or(1);
            for f in &findings {
                println!("{f}");
            }
            ExitCode::from(code)
        }
    }
}

/// ECO.REL.SIGN.SBOM: `rel sbom --plan <file> [--workspace <dir>] --native <toml> --out <dir>
/// [--artefacts <dir>] [--commit-time <t>]` writes `<artefact>.spdx.json` per planned artefact.
/// Exit 0: written; 1: findings (SBOM-LICENCE, SBOM-NATIVE-DIGEST, SBOM-IO), one per line,
/// nothing written; 2: bad usage or an unusable input (SBOM-PLAN-PARSE, SBOM-NATIVE-PARSE,
/// SBOM-METADATA). Offline: `cargo metadata` runs with `--locked --offline`.
fn rel_sbom(args: &[String]) -> ExitCode {
    let (mut plan, mut native, mut out, mut artefacts): (
        Option<PathBuf>,
        Option<PathBuf>,
        Option<PathBuf>,
        Option<PathBuf>,
    ) = (None, None, None, None);
    let mut commit_time: Option<String> = None;
    let mut workspace = PathBuf::from(".");
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--plan", Some(v)) => plan = Some(PathBuf::from(v)),
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--native", Some(v)) => native = Some(PathBuf::from(v)),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            ("--artefacts", Some(v)) => artefacts = Some(PathBuf::from(v)),
            ("--commit-time", Some(v)) => commit_time = Some(v.clone()),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(plan), Some(native), Some(out)) = (plan, native, out) else {
        eprintln!("rel sbom: --plan, --native and --out are required\n{USAGE}");
        return ExitCode::from(2);
    };
    let opts = sbom::Options {
        plan: &plan,
        workspace: &workspace,
        native: &native,
        artefacts: artefacts.as_deref(),
        commit_time: commit_time.as_deref(),
    };
    match sbom::run(&opts, &out) {
        Ok((release, docs)) => {
            println!("sbom: ok {} documents ({release})", docs.len());
            ExitCode::SUCCESS
        }
        Err(findings) => {
            let code = findings.iter().map(sbom::SbomError::exit_code).max().unwrap_or(1);
            for f in &findings {
                println!("{f}");
            }
            ExitCode::from(code)
        }
    }
}

/// ECO.REL.SIGN.GUIDE: `rel keys-check [--keys <dir>] [--doc <KEYS.md>] [--date <YYYY-MM-DD>]`
/// checks that every key file in the keys directory (default `keys`) is listed in `KEYS.md`
/// (default `KEYS.md`) with its SPKI SHA-256 fingerprint and that no active key is past its
/// validity on the check date (default: today, UTC). Exit 0: consistent; 1: findings, one per
/// line; 2: bad usage, unreadable inputs or a malformed key table. Reads files only.
fn keys_check(args: &[String]) -> ExitCode {
    let mut dir = PathBuf::from("keys");
    let mut doc = PathBuf::from("KEYS.md");
    let mut date: Option<String> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--keys", Some(v)) => dir = PathBuf::from(v),
            ("--doc", Some(v)) => doc = PathBuf::from(v),
            ("--date", Some(v)) => date = Some(v.clone()),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(date) = date.or_else(keys::today_utc) else {
        eprintln!("rel keys-check: the system clock is before 1970; pass --date");
        return ExitCode::from(2);
    };
    match keys::check(&dir, &doc, &date) {
        Ok(n) => {
            println!("keys-check: ok {n} keys ({date})");
            ExitCode::SUCCESS
        }
        Err(findings) => {
            for f in &findings {
                println!("{f}");
            }
            ExitCode::from(keys::exit_code(&findings))
        }
    }
}

/// ECO.REL.PROC.RUNBOOK: `rel runbook-check [--dir <dir>] [--workflow <file>] [--json]`.
/// Exit 0: consistent (`{"ok":true,"steps":<n>}` with `--json`); 1: findings, one per line
/// (or one JSON report); 2: bad usage or RUNBOOK-PARSE. Reads files only: it never writes,
/// never touches git or the network, and never creates tags, releases or signatures.
fn runbook_check(args: &[String]) -> ExitCode {
    let mut dir = PathBuf::from("release");
    let mut workflow: Option<PathBuf> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--json" {
            json = true;
            continue;
        }
        match (a.as_str(), it.next()) {
            ("--dir", Some(v)) => dir = PathBuf::from(v),
            ("--workflow", Some(v)) => workflow = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let result = runbook::check(&dir, workflow.as_deref());
    if json {
        println!("{}", runbook::report_json(&result));
    }
    match result {
        Ok(n) => {
            if !json {
                println!("runbook-check: ok {n} steps");
            }
            ExitCode::SUCCESS
        }
        Err(findings) => {
            if !json {
                for f in &findings {
                    println!("{f}");
                }
            }
            ExitCode::from(runbook::exit_code(&findings))
        }
    }
}

/// ECO.REL.PROC.RUNBOOK: `rel runbook-render [--dir <dir>] [--out <file>]` renders
/// `RUNBOOK.md` from `<dir>/runbook.toml` and `<dir>/RUNBOOK.preamble.md` to stdout, or to
/// `--out` (LF line endings on every OS). Exit 2 on bad usage or RUNBOOK-PARSE.
fn runbook_render(args: &[String]) -> ExitCode {
    let mut dir = PathBuf::from("release");
    let mut out: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--dir", Some(v)) => dir = PathBuf::from(v),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let text = match runbook::render_dir(&dir) {
        Ok((_, text)) => text,
        Err(f) => {
            println!("{f}");
            return ExitCode::from(2);
        }
    };
    match out {
        None => {
            print!("{text}");
            ExitCode::SUCCESS
        }
        Some(path) => match std::fs::write(&path, text.as_bytes()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                println!("RUNBOOK-PARSE {}: cannot write: {e}", path.display());
                ExitCode::from(2)
            }
        },
    }
}

/// ECO.REL.PROC.VERSION: `rel version-compare <a> <b>` prints `lt`, `eq` or `gt` (SemVer
/// 2.0.0 precedence of a against b). Either may carry the tag's `v`. Only the tag grammar is
/// accepted: build metadata (`+...`) is rejected with VERSION-TAG-FORMAT. Exit 1 on an
/// invalid version, 2 on bad usage.
fn version_compare(args: &[String]) -> ExitCode {
    let [a, b] = args else {
        eprintln!("{USAGE}");
        return ExitCode::from(2);
    };
    match (version::parse_any(a), version::parse_any(b)) {
        (Ok(a), Ok(b)) => {
            println!("{}", version::ordering_word(a.precedence(&b)));
            ExitCode::SUCCESS
        }
        (Err(e), _) | (_, Err(e)) => {
            println!("{e}");
            ExitCode::from(1)
        }
    }
}

fn layers(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut map: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--map", Some(v)) => map = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let map = map.unwrap_or_else(|| workspace.join("layers.toml"));
    match fid_xtask::run(&workspace, &map) {
        Ok(findings) if findings.is_empty() => {
            println!("layers: ok");
            ExitCode::SUCCESS
        }
        Ok(findings) => {
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(1)
        }
    }
}

/// `kernel-allow` (ECO.REPO.ALLOW). Exit 0: every third-party crate in the kernel graph is
/// allowlisted (warnings may be printed); 1: findings, or a file or metadata error (fail
/// closed); 2: usage.
fn kernel_allow(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut map: Option<PathBuf> = None;
    let mut allowlist: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--map", Some(v)) => map = Some(PathBuf::from(v)),
            ("--allowlist", Some(v)) => allowlist = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{KERNEL_ALLOW_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let map = map.unwrap_or_else(|| workspace.join("layers.toml"));
    let allowlist = allowlist.unwrap_or_else(|| workspace.join(fid_xtask::kernel_allow::ALLOWLIST_FILE));
    match fid_xtask::kernel_allow::run(&workspace, &map, &allowlist) {
        Ok(report) => {
            for w in &report.warnings {
                println!("{w}");
            }
            if report.findings.is_empty() {
                println!("kernel-allow: ok");
                return ExitCode::SUCCESS;
            }
            for f in &report.findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            println!("{e}");
            ExitCode::from(1)
        }
    }
}

/// `lint-crate-graph` (CORE.BUS.LINT). Exit 0: clean; 1: findings; 2: usage, allowlist or
/// metadata error.
fn lint_crate_graph(args: &[String]) -> ExitCode {
    let mut workspace = PathBuf::from(".");
    let mut allowlist: Option<PathBuf> = None;
    let mut writers: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--workspace", Some(v)) => workspace = PathBuf::from(v),
            ("--allowlist", Some(v)) => allowlist = Some(PathBuf::from(v)),
            ("--writers", Some(v)) => writers = Some(PathBuf::from(v)),
            _ => {
                eprintln!("{LINT_USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let allowlist = allowlist.unwrap_or_else(|| workspace.join(fid_xtask::crate_graph::ALLOWLIST_FILE));
    let writers = writers.unwrap_or_else(|| workspace.join(fid_xtask::crate_graph::WRITERS_FILE));
    match fid_xtask::crate_graph::run_with_warnings(&workspace, &allowlist, &writers) {
        Ok((findings, warnings)) => {
            for w in warnings {
                eprintln!("{w}");
            }
            if findings.is_empty() {
                println!("lint-crate-graph: ok");
                return ExitCode::SUCCESS;
            }
            for f in findings {
                println!("{f}");
            }
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

/// PLAT.DEPLOY.OFFLINE.NETLIST: `plat-deploy netlist`. Writes `network-features.json` and
/// `network-features.md` (default under `docs/deploy/`), or with `--check` compares them with a
/// fresh generation. Exit 0: ok; 1: X1 findings, unreadable manifest or drift (one per line);
/// 2: bad usage.
fn plat_deploy_netlist(args: &[String]) -> ExitCode {
    let mut manifest: Option<PathBuf> = None;
    let mut settings: Option<PathBuf> = None;
    let mut json = PathBuf::from(netlist::DEFAULT_JSON);
    let mut md = PathBuf::from(netlist::DEFAULT_MD);
    let mut check = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if a == "--check" {
            check = true;
            continue;
        }
        match (a.as_str(), it.next()) {
            ("--manifest", Some(v)) => manifest = Some(PathBuf::from(v)),
            ("--settings", Some(v)) => settings = Some(PathBuf::from(v)),
            ("--json", Some(v)) => json = PathBuf::from(v),
            ("--md", Some(v)) => md = PathBuf::from(v),
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(manifest), Some(settings)) = (manifest, settings) else {
        eprintln!("plat-deploy netlist: --manifest and --settings are required\n{USAGE}");
        return ExitCode::from(2);
    };
    match netlist::run(&manifest, &settings, &json, &md, check) {
        Ok(n) => {
            let verb = if check { "up to date" } else { "written" };
            println!("netlist: {n} commands, {verb}");
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in errors {
                println!("{e:?}");
            }
            ExitCode::from(1)
        }
    }
}
