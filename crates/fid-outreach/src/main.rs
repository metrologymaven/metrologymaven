//! `fid-outreach <command> ...`: Metrology Maven outreach tooling (domain OUTR).
//!
//! `copy-guard --policy <file> --checklist <file> --records <dir> --root <dir> [--json]` (spec node
//! `OUTR.BRAND.COPY`, D-013 option C): prints one line per finding (`<code> [<subject>]`), or the
//! canonical JSON report with `--json`. Exits 0 with no findings, 1 with any, and 2 when an input
//! cannot be read or parsed, or on bad usage.
//!
//! `review-gate --record <file> --release <id> --author <id> [--author <id>...] --roster <file>
//! --checklist <file> [--bots <file>] [--json]` (spec node `OUTR.BRAND.REVIEW`): checks one human
//! trade-dress review record. `--author` repeats for co-authors; `--bots` adds a bots register's
//! accounts to the denylist. Prints one `<release> <finding>` line per finding and a summary, or
//! the canonical JSON report with `--json`. Exits 0 when the review passes, 1 on any finding,
//! and 2 when a file is missing or does not parse, or on bad usage.
//!
//! `metric-def check --defs <file> --sources <file> [--json]` (spec node `OUTR.COMM.METRIC.DEF`):
//! text mode prints one line per finding, then one `metric <id>` line per valid metric and a
//! summary; `--json` prints the canonical JSON report instead. Exits 0 with no findings, 1 with
//! any, and 2 on unreadable or unparsable input or bad usage.
//!
//! `plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir> [--base <file>]
//! [--json]` (spec node `OUTR.LAUNCH.PLAN.FMT`): validates the milestone criteria file, the D-017
//! rule, plan-document dates and announced criteria. Exits 0 with no findings, 1 with any, and 2
//! when an input cannot be read or parsed, or on bad usage.
//!
//! `qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]` (spec node
//! `OUTR.SITE.QA.A11Y`): writes the report to `--out`, prints one line per unsuppressed
//! violation, and exits 0 when there is none, 1 when there is any, and 2 on unreadable or
//! unparsable input or bad usage.
//!
//! `doc-run --guide <file> --repo <dir> [--json]` (spec node `OUTR.DOCS.CONTRIB`): runs every
//! `sh run` block of the guide in a temporary copy of the revision it states. Exits 0 when all
//! blocks pass, 1 on `revision_mismatch` or any failing block, and 2 on an unreadable or
//! invalid guide, an unreadable repository, or bad usage.
//!
//! `guide-check --root <dir> --name <file> [--publish --out <dir>] [--json]` (spec node
//! `OUTR.BRAND.GUIDE.NAME`): checks that the guideline Markdown files write the family name only
//! as `{{FAMILY_NAME}}`; with `--publish`, writes copies with the accepted name substituted.
//! Exits 0 with no findings, 1 with any (nothing written), and 2 on unreadable input, an
//! unparsable name file, or bad usage.
//!
//! `asset-check --root <dir> --allow <file> --origin <url> [--exclude <glob>]... [--json]` (spec
//! node `OUTR.BRAND.ASSET.ATTR`): checks that every image, font and media file is original or
//! attributed under an allowlisted licence, and that no CSS or HTML loads a font or stylesheet
//! from another origin. Exits 0 with no findings, 1 with any, and 2 on unreadable or unparsable
//! input or bad usage.
//!
//! `mod-check --log <file> --roster <file> --today <YYYY-MM-DD> [--repo-root <dir>] [--json]` and
//! `mod-stats --log <file> --quarter <YYYYQn> --target-hours <n> --out <file> [--repo-root <dir>]`
//! (spec node `OUTR.COMM.MOD`): check the private moderation log, or write one quarter's counts.
//! Both exit 2 with `log_in_repo`, reading nothing, when the log lies inside the repository
//! checkout. `mod-check` exits 0 with no findings and 1 with any; `mod-stats` exits 0 after
//! writing `--out`, and 1 (writing nothing) when the log has field findings. Unreadable or
//! unparsable input and bad usage exit 2.
//!
//! `ref-render --records <dir> --manifest <file> --surfaces <file> --release <tag> --out <dir>`
//! and `ref-check --records <dir> --manifest <file> --surfaces <file> --ref <dir> [--json]` (spec
//! node `OUTR.DOCS.REF`): render the CLI, MCP and Python reference pages of every command major
//! from its `docs-ref-v1` record, or compare a published page tree with a fresh rendering.
//! `ref-render` exits 0 after writing, 1 (writing nothing) on `manifest_digest_mismatch` or
//! `record_missing`, and 2 on unreadable or unparsable input or bad usage. `ref-check` exits 0
//! with no findings, 1 with any, and 2 on unreadable or unparsable input (including
//! `manifest_digest_mismatch`) or bad usage.
//!
//! `site import verify --site <dir> --stage <dir> --revision <sha> --revision-date <YYYY-MM-DD>`
//! and `site import fetch --site <dir> --release <tag> --base-url <url>` (spec node
//! `OUTR.SITE.GEN.IMPORT`): verify checks the vendored inputs against `inputs.lock.toml` and each
//! release manifest, then stages them with `build-manifest.json` (no network); fetch adds one
//! release from an allowlisted host. Both print one line per finding and exit 1 on any, 0 on
//! success, and 2 on unreadable or unparsable input or bad usage.
//!
//! `search-index --edition <dir> --budget <file> [--json]` (spec node `OUTR.DOCS.SEARCH.INDEX`):
//! writes `search-index.js` at the edition root. Exits 0 after writing, 1 (writing nothing) with
//! an `over_budget` finding when the file would exceed `max_bytes` of the budget, and 2 on an
//! unreadable edition, an unparsable budget, an unwritable index, or bad usage.

use fid_outreach::asset_check;
use fid_outreach::copy_guard;
use fid_outreach::doc_run;
use fid_outreach::guide_check;
use fid_outreach::launch_plan;
use fid_outreach::metrics::def;
use fid_outreach::moderation;
use fid_outreach::qa::a11y::{self, Options};
use fid_outreach::ref_pages;
use fid_outreach::review_gate;
use fid_outreach::search_index;
use fid_outreach::site::import;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "usage: fid-outreach <command> [args]
commands:
  copy-guard --policy <file> --checklist <file> --records <dir> --root <dir> [--json]
  review-gate --record <file> --release <id> --author <id>... --roster <file> --checklist <file> [--bots <file>] [--json]
  metric-def check --defs <file> --sources <file> [--json]
  plan-check --criteria <file> --apps <apps.toml> --plan <file> --reviews <dir> [--base <file>] [--json]
  qa a11y --root <dir> --tokens <file> --out <file> [--config <file>]
  doc-run --guide <file> --repo <dir> [--json]
  guide-check --root <dir> --name <file> [--publish --out <dir>] [--json]
  asset-check --root <dir> --allow <file> --origin <url> [--exclude <glob>]... [--json]
  mod-check --log <file> --roster <file> --today <YYYY-MM-DD> [--repo-root <dir>] [--json]
  mod-stats --log <file> --quarter <YYYYQn> --target-hours <n> --out <file> [--repo-root <dir>]
  ref-render --records <dir> --manifest <file> --surfaces <file> --release <tag> --out <dir>
  ref-check --records <dir> --manifest <file> --surfaces <file> --ref <dir> [--json]
  site import verify --site <dir> --stage <dir> --revision <sha> --revision-date <YYYY-MM-DD>
  site import fetch --site <dir> --release <tag> --base-url <url>
  search-index --edition <dir> --budget <file> [--json]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("copy-guard"), _) => copy_guard_cmd(args.get(1..).unwrap_or(&[])),
        (Some("review-gate"), _) => review_gate_cmd(args.get(1..).unwrap_or(&[])),
        (Some("metric-def"), Some("check")) => metric_def_check(args.get(2..).unwrap_or(&[])),
        (Some("plan-check"), _) => plan_check(args.get(1..).unwrap_or(&[])),
        (Some("qa"), Some("a11y")) => qa_a11y(args.get(2..).unwrap_or(&[])),
        (Some("doc-run"), _) => doc_run_cmd(args.get(1..).unwrap_or(&[])),
        (Some("guide-check"), _) => guide_check_cmd(args.get(1..).unwrap_or(&[])),
        (Some("asset-check"), _) => asset_check_cmd(args.get(1..).unwrap_or(&[])),
        (Some(cmd @ ("mod-check" | "mod-stats")), _) => moderation_cmd(cmd, args.get(1..).unwrap_or(&[])),
        (Some(cmd @ ("ref-render" | "ref-check")), _) => ref_pages_cmd(cmd, args.get(1..).unwrap_or(&[])),
        (Some("site"), Some("import")) => site_import_cmd(args.get(2..).unwrap_or(&[])),
        (Some("search-index"), _) => search_index_cmd(args.get(1..).unwrap_or(&[])),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn emit(bytes: &[u8]) {
    let mut out = std::io::stdout().lock();
    // A closed stdout is not worth a different exit code; the exit status carries the result.
    let _ = out.write_all(bytes).and_then(|()| out.write_all(b"\n"));
}

fn copy_guard_cmd(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            flag @ ("--policy" | "--checklist" | "--records" | "--root") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(policy), Some(checklist), Some(records), Some(root)) =
        (opts.get("--policy"), opts.get("--checklist"), opts.get("--records"), opts.get("--root"))
    else {
        return usage();
    };
    let inputs = copy_guard::Inputs {
        policy: Path::new(policy),
        checklist: Path::new(checklist),
        records: Path::new(records),
        root: Path::new(root),
    };
    match copy_guard::run(&inputs) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                for f in &report.findings {
                    println!("{f}");
                }
                println!("copy-guard: {} finding(s)", report.findings.len());
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                emit(&e.to_json());
            }
            eprintln!("copy-guard: {e}");
            ExitCode::from(2)
        }
    }
}

fn review_gate_cmd(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut authors: Vec<String> = Vec::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--author" => {
                let Some(v) = it.next() else { return usage() };
                authors.push(v.clone());
            }
            flag @ ("--record" | "--release" | "--roster" | "--checklist" | "--bots") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(record), Some(release), Some(roster), Some(checklist)) =
        (opts.get("--record"), opts.get("--release"), opts.get("--roster"), opts.get("--checklist"))
    else {
        return usage();
    };
    if authors.is_empty() {
        return usage();
    }
    let inputs = review_gate::Inputs {
        record: Path::new(record),
        release,
        authors: &authors,
        roster: Path::new(roster),
        checklist: Path::new(checklist),
        bots: opts.get("--bots").map(Path::new),
    };
    match review_gate::run(&inputs) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                print!("{}", report.render());
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                emit(&e.to_json());
            }
            eprintln!("review-gate: {e}");
            ExitCode::from(2)
        }
    }
}

fn metric_def_check(args: &[String]) -> ExitCode {
    let mut defs: Option<PathBuf> = None;
    let mut sources: Option<PathBuf> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--defs" => match it.next() {
                Some(v) => defs = Some(PathBuf::from(v)),
                None => return usage(),
            },
            "--sources" => match it.next() {
                Some(v) => sources = Some(PathBuf::from(v)),
                None => return usage(),
            },
            _ => return usage(),
        }
    }
    let (Some(defs), Some(sources)) = (defs, sources) else {
        return usage();
    };
    match def::check_files(&defs, &sources) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                for f in &report.findings {
                    println!("{f}");
                }
                for m in &report.metrics {
                    println!("metric {}", m.id);
                }
                println!(
                    "metric-def check: {} metric(s), {} source(s), {} finding(s)",
                    report.metrics.len(),
                    report.sources.len(),
                    report.findings.len()
                );
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                emit(&def::error_json(&e));
            }
            eprintln!("{e}");
            ExitCode::from(2)
        }
    }
}

fn plan_check(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            flag @ ("--criteria" | "--apps" | "--plan" | "--reviews" | "--base") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(criteria), Some(apps), Some(plan), Some(reviews)) =
        (opts.get("--criteria"), opts.get("--apps"), opts.get("--plan"), opts.get("--reviews"))
    else {
        return usage();
    };
    let inputs = launch_plan::Inputs {
        criteria: Path::new(criteria),
        apps: Path::new(apps),
        plan: Path::new(plan),
        reviews: Path::new(reviews),
        base: opts.get("--base").map(Path::new),
    };
    match launch_plan::check(&inputs) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                print!("{}", report.render());
            }
            if report.findings.is_empty() {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(1)
            }
        }
        Err(e) => {
            if json {
                emit(&fid_canon::to_canonical_bytes(&e.to_cvalue()).unwrap_or_default());
            }
            eprintln!("plan-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn qa_a11y(args: &[String]) -> ExitCode {
    let mut root: Option<PathBuf> = None;
    let mut tokens: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut config: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--root", Some(v)) => root = Some(PathBuf::from(v)),
            ("--tokens", Some(v)) => tokens = Some(PathBuf::from(v)),
            ("--out", Some(v)) => out = Some(PathBuf::from(v)),
            ("--config", Some(v)) => config = Some(PathBuf::from(v)),
            _ => return usage(),
        }
    }
    let (Some(root), Some(tokens), Some(out)) = (root, tokens, out) else {
        return usage();
    };
    match a11y::run(&Options { root, tokens, config }) {
        Ok(report) => {
            let bytes = match report.to_json() {
                Ok(b) => b,
                Err(m) => {
                    eprintln!("A11Y-IO({}): {m}", out.display());
                    return ExitCode::from(2);
                }
            };
            if let Err(e) = std::fs::write(&out, &bytes) {
                eprintln!("A11Y-IO({}): {e}", out.display());
                return ExitCode::from(2);
            }
            for v in &report.violations {
                println!("{v}");
            }
            println!(
                "qa a11y: {} page(s), {} violation(s), {} suppressed, {} unmatched suppression(s)",
                report.pages,
                report.violations.len(),
                report.suppressed.len(),
                report.unmatched.len()
            );
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            eprintln!("{e}");
            if let Err(w) = std::fs::write(&out, a11y::error_json(&e)) {
                eprintln!("A11Y-IO({}): {w}", out.display());
            }
            ExitCode::from(2)
        }
    }
}

fn doc_run_cmd(args: &[String]) -> ExitCode {
    let mut guide: Option<PathBuf> = None;
    let mut repo: Option<PathBuf> = None;
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--guide" | "--repo" => {
                let Some(v) = it.next() else { return usage() };
                let slot = if a == "--guide" { &mut guide } else { &mut repo };
                if slot.replace(PathBuf::from(v)).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(guide), Some(repo)) = (guide, repo) else {
        return usage();
    };
    match doc_run::run(&doc_run::Options { guide, repo, timeout: doc_run::DEFAULT_TIMEOUT }) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                print!("{}", report.render());
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                emit(&doc_run::error_json(&e));
            }
            eprintln!("doc-run: {e}");
            ExitCode::from(2)
        }
    }
}

fn guide_check_cmd(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut publish = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--publish" => publish = true,
            flag @ ("--root" | "--name" | "--out") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(root), Some(name)) = (opts.get("--root"), opts.get("--name")) else {
        return usage();
    };
    let publish_to = match (publish, opts.get("--out")) {
        (true, Some(out)) => Some(PathBuf::from(out)),
        (false, None) => None,
        _ => return usage(),
    };
    let options = guide_check::Options { root: PathBuf::from(root), name_file: PathBuf::from(name), publish_to };
    match guide_check::run(&options) {
        Ok(report) => {
            if json {
                match report.to_json() {
                    Ok(bytes) => emit(&bytes),
                    Err(m) => {
                        eprintln!("guide-check: report serialisation failed: {m}");
                        return ExitCode::from(2);
                    }
                }
            } else {
                print!("{}", report.render());
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                match e.to_json() {
                    Ok(bytes) => emit(&bytes),
                    Err(m) => eprintln!("guide-check: error serialisation failed: {m}"),
                }
            }
            eprintln!("guide-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn asset_check_cmd(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut excludes: Vec<String> = Vec::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            "--exclude" => {
                let Some(v) = it.next() else { return usage() };
                excludes.push(v.clone());
            }
            flag @ ("--root" | "--allow" | "--origin") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(root), Some(allow), Some(origin)) = (opts.get("--root"), opts.get("--allow"), opts.get("--origin"))
    else {
        return usage();
    };
    let options = asset_check::Options {
        root: PathBuf::from(root),
        allow: PathBuf::from(allow),
        origin: origin.to_string(),
        excludes,
    };
    match asset_check::run(&options) {
        Ok(report) => {
            if json {
                match report.to_json() {
                    Ok(bytes) => emit(&bytes),
                    Err(m) => {
                        eprintln!("asset-check: report serialisation failed: {m}");
                        return ExitCode::from(2);
                    }
                }
            } else {
                print!("{}", report.render());
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                match e.to_json() {
                    Ok(bytes) => emit(&bytes),
                    Err(m) => eprintln!("asset-check: error serialisation failed: {m}"),
                }
            }
            eprintln!("asset-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn moderation_cmd(cmd: &str, args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (cmd, a.as_str()) {
            ("mod-check", "--json") => json = true,
            ("mod-check", flag @ ("--log" | "--roster" | "--today" | "--repo-root"))
            | ("mod-stats", flag @ ("--log" | "--quarter" | "--target-hours" | "--out" | "--repo-root")) => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let Some(log) = opts.get("--log").map(PathBuf::from) else { return usage() };
    let repo_root = opts.get("--repo-root").map(PathBuf::from);
    if cmd == "mod-check" {
        let (Some(roster), Some(today)) =
            (opts.get("--roster"), opts.get("--today").and_then(|d| moderation::Date::parse(d)))
        else {
            return usage();
        };
        let options = moderation::CheckOptions { log, roster: PathBuf::from(roster), today, repo_root };
        return match moderation::run_check(&options) {
            Ok(report) => {
                if json {
                    emit(&report.to_json());
                } else {
                    print!("{}", report.render());
                }
                ExitCode::from(report.exit_code())
            }
            Err(e) => {
                if json {
                    emit(&e.to_json());
                }
                eprintln!("mod-check: {e}");
                ExitCode::from(2)
            }
        };
    }
    let quarter = opts.get("--quarter").and_then(|q| moderation::Quarter::parse(q));
    let hours = opts.get("--target-hours").and_then(|h| h.parse::<u32>().ok());
    let (Some(quarter), Some(target_hours), Some(out)) = (quarter, hours, opts.get("--out")) else {
        return usage();
    };
    if !(1..=moderation::MAX_TARGET_HOURS).contains(&target_hours) {
        return usage();
    }
    match moderation::run_stats(&moderation::StatsOptions { log, quarter, target_hours, repo_root }) {
        Ok(moderation::StatsResult::Counts(bytes)) => match std::fs::write(out, &bytes) {
            Ok(()) => {
                println!("mod-stats: wrote {out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("mod-stats: output_unwritable {out}: {e}");
                ExitCode::from(2)
            }
        },
        Ok(moderation::StatsResult::Invalid(findings)) => {
            for f in &findings {
                println!("{f}");
            }
            println!("mod-stats: {} finding(s), nothing written", findings.len());
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("mod-stats: {e}");
            ExitCode::from(2)
        }
    }
}

fn site_import_cmd(args: &[String]) -> ExitCode {
    let Some(sub) = args.first().map(String::as_str) else { return usage() };
    let flags: &[&str] = match sub {
        "verify" => &["--site", "--stage", "--revision", "--revision-date"],
        "fetch" => &["--site", "--release", "--base-url"],
        _ => return usage(),
    };
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut it = args.iter().skip(1);
    while let Some(a) = it.next() {
        let Some(flag) = flags.iter().find(|f| **f == a.as_str()) else { return usage() };
        let Some(v) = it.next() else { return usage() };
        if opts.insert(*flag, v.as_str()).is_some() {
            return usage();
        }
    }
    if opts.len() != flags.len() {
        return usage();
    }
    let get = |k: &str| opts.get(k).copied().unwrap_or("");
    let site = Path::new(get("--site"));
    let result = if sub == "verify" {
        let stage = Path::new(get("--stage"));
        let a = import::VerifyArgs { site, stage, revision: get("--revision"), revision_date: get("--revision-date") };
        import::verify_and_stage(&a).map(|findings| (findings, format!("verified; staged to {}", stage.display())))
    } else {
        let a = import::fetch::FetchArgs { site, release: get("--release"), base_url: get("--base-url") };
        import::fetch::fetch(&a).map(|r| match r {
            Ok(f) => (Vec::new(), format!("fetched {} file(s) into inputs/{}/", f.entries.len(), get("--release"))),
            Err(findings) => (findings, String::new()),
        })
    };
    match result {
        Ok((findings, summary)) if findings.is_empty() => {
            println!("site import {sub}: {summary}");
            ExitCode::SUCCESS
        }
        Ok((findings, _)) => {
            for f in &findings {
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

fn ref_pages_cmd(cmd: &str, args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (cmd, a.as_str()) {
            ("ref-check", "--json") => json = true,
            (_, flag @ ("--records" | "--manifest" | "--surfaces"))
            | ("ref-render", flag @ ("--release" | "--out"))
            | ("ref-check", flag @ "--ref") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(records), Some(manifest), Some(surfaces)) =
        (opts.get("--records"), opts.get("--manifest"), opts.get("--surfaces"))
    else {
        return usage();
    };
    let inputs =
        ref_pages::Inputs { records: Path::new(records), manifest: Path::new(manifest), surfaces: Path::new(surfaces) };
    if cmd == "ref-render" {
        let (Some(release), Some(out)) = (opts.get("--release"), opts.get("--out")) else {
            return usage();
        };
        if !ref_pages::valid_release(release) {
            return usage();
        }
        return match ref_pages::render_tree(&inputs, release, Path::new(out)) {
            Ok(ref_pages::RenderOutcome::Written(n)) => {
                println!("ref-render: wrote {n} page(s) under {}", Path::new(out).join(release).join("ref").display());
                ExitCode::SUCCESS
            }
            Ok(ref_pages::RenderOutcome::Refused(findings)) => {
                for f in &findings {
                    println!("{f}");
                }
                println!("ref-render: {} finding(s), nothing written", findings.len());
                ExitCode::from(1)
            }
            Err(e) => {
                eprintln!("ref-render: {e}");
                ExitCode::from(2)
            }
        };
    }
    let Some(ref_dir) = opts.get("--ref") else { return usage() };
    match ref_pages::check(&inputs, Path::new(ref_dir)) {
        Ok(report) => {
            if json {
                emit(&report.to_json());
            } else {
                print!("{}", report.render());
            }
            ExitCode::from(report.exit_code())
        }
        Err(e) => {
            if json {
                emit(&e.to_json());
            }
            eprintln!("ref-check: {e}");
            ExitCode::from(2)
        }
    }
}

fn search_index_cmd(args: &[String]) -> ExitCode {
    let mut opts = BTreeMap::<&str, &str>::new();
    let mut json = false;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => json = true,
            flag @ ("--edition" | "--budget") => {
                let Some(v) = it.next() else { return usage() };
                if opts.insert(flag, v.as_str()).is_some() {
                    return usage();
                }
            }
            _ => return usage(),
        }
    }
    let (Some(edition), Some(budget)) = (opts.get("--edition"), opts.get("--budget")) else {
        return usage();
    };
    match search_index::run(Path::new(edition), Path::new(budget)) {
        Ok(outcome) => {
            if json {
                emit(&outcome.to_json());
            } else {
                print!("{}", outcome.render());
            }
            ExitCode::from(outcome.exit_code())
        }
        Err(e) => {
            if json {
                emit(&e.to_json());
            }
            eprintln!("search-index: {e}");
            ExitCode::from(2)
        }
    }
}
