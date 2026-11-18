//! `cargo xtask <task>` (alias in `.cargo/config.toml`).

mod pins;

use fid_xtask::rel::{runbook, sbom, sums, version};
use fid_xtask::{apps_policy, plat_matrix_tiers};
use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, ExitCode};

const USAGE: &str = "usage: cargo xtask layers [--workspace <dir>] [--map <layers.toml>]\n       \
                     cargo xtask datasets-check --catalogue <toml> --domains <file>\n       \
                     cargo xtask plat-matrix validate <file>\n       \
                     cargo xtask plat-matrix lint-tiers [<file>]\n       \
                     cargo xtask plat-hw validate [--file <path>]\n       \
                     cargo xtask plat-hw validate-suite [--file <path>]\n       \
                     cargo xtask plat-hw result-schema\n       \
                     cargo xtask personas-check [--personas <json>] [--apps <toml>]\n       \
                     cargo xtask rel version-check --tag <tag> --channel nightly|beta|stable|lts \
                     [--workspace <dir>] [--binaries <dir>] [--json]\n       \
                     cargo xtask rel version-compare <a> <b>\n       \
                     cargo xtask rel sums --manifest <release-manifest.json> --dir <artefacts> --out <dir>
       \n                     cargo xtask rel runbook-check [--dir <release dir>] [--workflow <file>] [--json]
       \n                     cargo xtask rel runbook-render [--dir <release dir>] [--out <file>]
       \n                     cargo xtask rel sbom --plan <package-plan.json> [--workspace <dir>] --native <toml> \
                     --out <dir> [--artefacts <dir>] [--commit-time <YYYY-MM-DDThh:mm:ssZ>]";
const APPS_POLICY_USAGE: &str = "usage: cargo xtask apps-policy --workspace <dir> --registry-manifests <dir> \
                                 --apps-status <json> [--config apps-policy.toml]";
const LINT_USAGE: &str =
    "usage: cargo xtask lint-crate-graph [--workspace <dir>] [--allowlist <file>] [--writers <file>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("layers") => layers(&args[1..]),
        Some("lint-crate-graph") => lint_crate_graph(&args[1..]),
        Some("datasets-check") => datasets_check(&args),
        Some("plat-matrix") if args.get(1).map(String::as_str) == Some("lint-tiers") => lint_tiers(&args[2..]),
        Some("plat-matrix") => fid_platform(&args, "plat-matrix"),
        Some("plat-hw") => fid_platform(&args, "plat-hw"),
        Some("pins") => pins::cli(&args[1..]),
        Some("personas-check") => personas_check(&args[1..]),
        Some("apps-policy") => apps_policy_check(&args[1..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("version-check") => version_check(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("version-compare") => version_compare(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("sums") => rel_sums(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("runbook-check") => runbook_check(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("runbook-render") => runbook_render(&args[2..]),
        Some("rel") if args.get(1).map(String::as_str) == Some("sbom") => rel_sbom(&args[2..]),
        _ => {
            eprintln!("{USAGE}");
            eprintln!("{LINT_USAGE}");
            eprintln!("{APPS_POLICY_USAGE}");
            ExitCode::from(2)
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

/// ECO.DATA.CAT: runs the `fid-data` binary. `fid-data` is a `tool` crate, and the layer
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

/// PLAT.MATRIX.FILE and PLAT.HW.PROFILE: runs `fid-platform plat-matrix ...` or
/// `fid-platform plat-hw ...` through Cargo, for the same layer reason as `datasets-check`
/// (`fid-platform` is a `tool` crate).
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
