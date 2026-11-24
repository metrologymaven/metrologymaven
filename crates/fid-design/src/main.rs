//! `fid-design pkg-check --root <dir> [--json]`,
//! `fid-design pkg-diff --from <dir> --to <dir> [--json]`,
//! `fid-design tokens-check --root <dir> [--json]`,
//! `fid-design status-check --root <dir> [--json]`,
//! `fid-design space-check --root <dir> [--json]`,
//! `fid-design palettes-check --root <dir> [--baseline <digests>] [--json]`,
//! `fid-design color-check --root <dir> [--baseline <digests>] [--out <file>] [--json]`,
//! `fid-design princ-check [--file <md>] [--json]` and
//! `fid-design words-check --copy <jsonl> [--words <json>] [--json]` and
//! `fid-design review-check --changed <file> --root <design dir> [--base <repo>] [--pr <N>]
//! [--deny-names <file>] [--json]` or `review-check --all --root <design dir> [--base <repo>] ..`,
//! `fid-design icons-prov --root <design dir> --attribution <file> [--json]`,
//! `fid-design icons-registry --manifest <file> --root <design dir> [--json]`,
//! `fid-design screens-check --root <design dir> --journeys <dir> [--ga <app>] [--json]`,
//! `fid-design screens-gate --spec <spec dir> --root <design dir> [--json]`,
//! `fid-design icons-lint --root <icons dir> [--json]`,
//! `fid-design glyph-distinct --root <icons dir> [--threshold <0..1>] [--json]`,
//! `fid-design fonts-check --root <design dir> --allowlist <file> [--json]`,
//! `fid-design page-check (--root <design dir> | --file <pages.json>) [--json]`,
//! `fid-design lint-literals --root <dir> [--rules <toml>] [--package <dir>] [--allowlist <file>] [--json]`,
//! `fid-design research-check --root <dir> [--key <private csv>] [--json]` and
//! `fid-design sources-check --root <design dir> [--spec <spec dir>] [--json]`.
//! Exit codes: 0 no findings, 1 findings, 2 unreadable package or bad usage (`icons-lint` also
//! exits 2 for a root holding no SVG).

use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str =
    "usage: fid-design pkg-check --root <dir> [--json]
       fid-design pkg-diff --from <dir> --to <dir> [--json]
       fid-design tokens-check --root <design dir> [--json]
       fid-design status-check --root <design dir> [--json]
       fid-design space-check --root <design dir> [--json]
       fid-design palettes-check --root <design dir> [--baseline <digests>] [--json]
       fid-design color-check --root <design dir> [--baseline <digests>] [--out <file>] [--json]
       fid-design princ-check [--file <md>] [--json]
       fid-design words-check --copy <jsonl> [--words <json>] [--json]
       fid-design review-check --changed <file> --root <design dir> [--base <repo>] [--pr <N>] [--deny-names <file>] [--json]
       fid-design review-check --all --root <design dir> [--base <repo>] [--deny-names <file>] [--json]
       fid-design icons-prov --root <design dir> --attribution <file> [--json]
       fid-design icons-registry --manifest <file> --root <design dir> [--json]
       fid-design screens-check --root <design dir> --journeys <dir> [--ga <app>] [--json]
       fid-design screens-gate --spec <spec dir> --root <design dir> [--json]
       fid-design icons-lint --root <icons dir> [--json]
       fid-design glyph-distinct --root <icons dir> [--threshold <0..1>] [--json]
       fid-design fonts-check --root <design dir> --allowlist <file> [--json]
       fid-design page-check (--root <design dir> | --file <pages.json>) [--json]
       fid-design lint-literals --root <dir> [--rules <toml>] [--package <dir>] [--allowlist <file>] [--json]
       fid-design research-check --root <dir> [--key <private csv>] [--json]
       fid-design sources-check --root <design dir> [--spec <spec dir>] [--json]";

#[derive(Default)]
struct Args {
    root: Option<PathBuf>,
    key: Option<PathBuf>,
    rules: Option<PathBuf>,
    package: Option<PathBuf>,
    allowlist: Option<PathBuf>,
    from: Option<PathBuf>,
    to: Option<PathBuf>,
    file: Option<PathBuf>,
    copy: Option<PathBuf>,
    words: Option<PathBuf>,
    changed: Option<PathBuf>,
    deny_names: Option<PathBuf>,
    base: Option<PathBuf>,
    baseline: Option<PathBuf>,
    out: Option<PathBuf>,
    attribution: Option<PathBuf>,
    manifest: Option<PathBuf>,
    journeys: Option<PathBuf>,
    ga: Option<String>,
    spec: Option<PathBuf>,
    pr: Option<u64>,
    threshold: Option<String>,
    all: bool,
    json: bool,
}

fn parse_args(args: &[String]) -> Option<Args> {
    let mut out = Args::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--json" => out.json = true,
            "--root" => out.root = Some(PathBuf::from(it.next()?)),
            "--key" => out.key = Some(PathBuf::from(it.next()?)),
            "--rules" => out.rules = Some(PathBuf::from(it.next()?)),
            "--package" => out.package = Some(PathBuf::from(it.next()?)),
            "--allowlist" => out.allowlist = Some(PathBuf::from(it.next()?)),
            "--from" => out.from = Some(PathBuf::from(it.next()?)),
            "--to" => out.to = Some(PathBuf::from(it.next()?)),
            "--file" => out.file = Some(PathBuf::from(it.next()?)),
            "--copy" => out.copy = Some(PathBuf::from(it.next()?)),
            "--words" => out.words = Some(PathBuf::from(it.next()?)),
            "--changed" => out.changed = Some(PathBuf::from(it.next()?)),
            "--deny-names" => out.deny_names = Some(PathBuf::from(it.next()?)),
            "--base" => out.base = Some(PathBuf::from(it.next()?)),
            "--baseline" => out.baseline = Some(PathBuf::from(it.next()?)),
            "--out" => out.out = Some(PathBuf::from(it.next()?)),
            "--attribution" => out.attribution = Some(PathBuf::from(it.next()?)),
            "--manifest" => out.manifest = Some(PathBuf::from(it.next()?)),
            "--journeys" => out.journeys = Some(PathBuf::from(it.next()?)),
            "--ga" => out.ga = Some(it.next()?.clone()),
            "--spec" => out.spec = Some(PathBuf::from(it.next()?)),
            "--pr" => {
                let n: u64 = it.next()?.parse().ok()?;
                if n == 0 {
                    return None;
                }
                out.pr = Some(n);
            }
            "--threshold" => out.threshold = Some(it.next()?.clone()),
            "--all" => out.all = true,
            _ => return None,
        }
    }
    Some(out)
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn fail(e: impl std::fmt::Display) -> ExitCode {
    eprintln!("fid-design: error: {e}");
    ExitCode::from(2)
}

fn emit(bytes: &[u8]) -> Result<(), String> {
    let mut out = std::io::stdout().lock();
    out.write_all(bytes).and_then(|()| out.flush()).map_err(|e| format!("cannot write the report: {e}"))
}

fn finish(json: Result<Vec<u8>, fid_canon::CanonError>, text: String, as_json: bool, code: u8) -> ExitCode {
    let body = if as_json {
        match json {
            Ok(mut b) => {
                b.push(b'\n');
                b
            }
            Err(e) => return fail(format!("report encoding: {}", e.code())),
        }
    } else {
        text.into_bytes()
    };
    match emit(&body) {
        Ok(()) => ExitCode::from(code),
        Err(e) => fail(e),
    }
}

fn main() -> ExitCode {
    let all: Vec<String> = std::env::args().skip(1).collect();
    let Some(cmd) = all.first() else {
        return usage();
    };
    let Some(args) = parse_args(&all[1..]) else {
        return usage();
    };
    if cmd == "glyph-distinct" {
        let others = args.key.is_some()
            || args.rules.is_some()
            || args.package.is_some()
            || args.allowlist.is_some()
            || args.from.is_some()
            || args.to.is_some()
            || args.file.is_some()
            || args.copy.is_some()
            || args.words.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.attribution.is_some()
            || args.manifest.is_some()
            || args.journeys.is_some()
            || args.ga.is_some()
            || args.spec.is_some()
            || args.pr.is_some()
            || args.all;
        let Some(root) = &args.root else {
            return usage();
        };
        if others {
            return usage();
        }
        let threshold = match &args.threshold {
            None => fid_design::glyph::Threshold::default_value(),
            Some(t) => match fid_design::glyph::Threshold::parse(t) {
                Ok(t) => t,
                Err(e) => return fail(e),
            },
        };
        return match fid_design::glyph::check(root, &threshold) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        };
    }
    if args.threshold.is_some() {
        return usage();
    }
    let pkg_flags = args.root.is_some() || args.from.is_some() || args.to.is_some();
    let princ_flags = args.file.is_some() || args.copy.is_some() || args.words.is_some();
    let screen_flags = args.journeys.is_some() || args.ga.is_some() || args.spec.is_some();
    if cmd == "fonts-check" {
        let others = princ_flags
            || screen_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.pr.is_some()
            || args.all
            || args.attribution.is_some()
            || args.manifest.is_some()
            || args.rules.is_some()
            || args.package.is_some()
            || args.key.is_some();
        let (Some(root), Some(allowlist)) = (&args.root, &args.allowlist) else {
            return usage();
        };
        if others {
            return usage();
        }
        return match fid_design::fonts::check(root, allowlist) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        };
    }
    if cmd == "lint-literals" {
        let others = princ_flags
            || screen_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.pr.is_some()
            || args.all
            || args.attribution.is_some()
            || args.manifest.is_some()
            || args.key.is_some();
        let Some(root) = &args.root else {
            return usage();
        };
        if others {
            return usage();
        }
        let o = fid_design::lint::Options {
            root,
            rules: args.rules.as_deref(),
            package: args.package.as_deref(),
            allowlist: args.allowlist.as_deref(),
        };
        return match fid_design::lint::check(&o) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        };
    }
    if cmd == "research-check" {
        let others = princ_flags
            || screen_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.pr.is_some()
            || args.all
            || args.attribution.is_some()
            || args.manifest.is_some()
            || args.rules.is_some()
            || args.package.is_some()
            || args.allowlist.is_some();
        let Some(root) = &args.root else {
            return usage();
        };
        if others {
            return usage();
        }
        return match fid_design::research::check(root, args.key.as_deref()) {
            Ok(r) => {
                // Without `--key` names cannot be checked; say so even when stdout is JSON (the
                // text report carries the same notice, and the JSON has `names_checked: false`).
                if !r.names_checked && args.json {
                    let _ = writeln!(std::io::stderr(), "{}", fid_design::research::NO_KEY_NOTICE);
                }
                finish(r.to_json(), r.to_text(), args.json, r.exit_code())
            }
            Err(e) => fail(e),
        };
    }
    if args.rules.is_some() || args.package.is_some() || args.allowlist.is_some() || args.key.is_some() {
        return usage();
    }
    if cmd == "sources-check" {
        let others = princ_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.pr.is_some()
            || args.all
            || args.attribution.is_some()
            || args.manifest.is_some()
            || args.journeys.is_some()
            || args.ga.is_some();
        let Some(root) = args.root.clone() else {
            return usage();
        };
        if others {
            return usage();
        }
        return match fid_design::sources::check(&root, args.spec.as_deref()) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        };
    }
    if cmd == "screens-check" || cmd == "screens-gate" {
        let others = princ_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.pr.is_some()
            || args.all
            || args.attribution.is_some()
            || args.manifest.is_some();
        let Some(root) = args.root.clone() else {
            return usage();
        };
        if others {
            return usage();
        }
        return match (cmd.as_str(), &args.journeys, &args.spec, &args.ga) {
            ("screens-check", Some(journeys), None, ga) => {
                match fid_design::screens::check_screens(&root, journeys, ga.as_deref()) {
                    Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                    Err(e) => fail(e),
                }
            }
            ("screens-gate", None, Some(spec), None) => match fid_design::screens::check_gate(spec, &root) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            },
            _ => usage(),
        };
    }
    if screen_flags {
        return usage();
    }
    if cmd == "icons-prov" || cmd == "icons-registry" {
        let others = princ_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
            || args.changed.is_some()
            || args.deny_names.is_some()
            || args.base.is_some()
            || args.pr.is_some()
            || args.all;
        let Some(root) = args.root.clone() else {
            return usage();
        };
        if others {
            return usage();
        }
        return match (cmd.as_str(), &args.attribution, &args.manifest) {
            ("icons-prov", Some(attr), None) => match fid_design::icons_prov::check_prov(&root, attr) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            },
            ("icons-registry", None, Some(man)) => match fid_design::icons_prov::check_registry(man, &root) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            },
            _ => usage(),
        };
    }
    if args.attribution.is_some() || args.manifest.is_some() {
        return usage();
    }
    if cmd == "review-check" {
        let mode_ok = if args.all { args.changed.is_none() && args.pr.is_none() } else { args.changed.is_some() };
        let Some(root) = args.root.clone() else {
            return usage();
        };
        if !mode_ok
            || princ_flags
            || args.from.is_some()
            || args.to.is_some()
            || args.baseline.is_some()
            || args.out.is_some()
        {
            return usage();
        }
        let o = fid_design::review::Options {
            changed: args.changed.clone(),
            root,
            base: args.base.clone(),
            deny: args.deny_names.clone(),
            pr: args.pr,
            all: args.all,
        };
        return match fid_design::review::check(&o) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        };
    }
    if args.changed.is_some()
        || args.deny_names.is_some()
        || args.base.is_some()
        || args.pr.is_some()
        || args.all
        || (args.baseline.is_some() && cmd != "palettes-check" && cmd != "color-check")
        || (args.out.is_some() && cmd != "color-check")
    {
        return usage();
    }
    match cmd.as_str() {
        "page-check" if args.from.is_none() && args.to.is_none() && args.copy.is_none() && args.words.is_none() => {
            return match (&args.root, &args.file) {
                (Some(root), None) => match fid_design::page::check(root) {
                    Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                    Err(e) => fail(e),
                },
                (None, Some(file)) => match fid_design::page::check_file(file) {
                    Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                    Err(e) => fail(e),
                },
                _ => usage(),
            };
        }
        "princ-check" if !pkg_flags && args.copy.is_none() && args.words.is_none() => {
            let file = args.file.clone().unwrap_or_else(|| PathBuf::from(fid_design::princ::DEFAULT_FILE));
            return match fid_design::princ::check(&file) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            };
        }
        "words-check" if !pkg_flags && args.file.is_none() => {
            let Some(copy) = &args.copy else {
                return usage();
            };
            let words = args.words.clone().unwrap_or_else(|| PathBuf::from(fid_design::words::DEFAULT_WORDS));
            return match fid_design::words::check(copy, &words) {
                Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
                Err(e) => fail(e),
            };
        }
        _ if princ_flags => return usage(),
        _ => {}
    }
    match (cmd.as_str(), &args.root, &args.from, &args.to) {
        ("pkg-check", Some(root), None, None) => match fid_design::pkg::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("pkg-diff", None, Some(from), Some(to)) => match fid_design::pkg::diff(from, to) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("tokens-check", Some(root), None, None) => match fid_design::tokens::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("status-check", Some(root), None, None) => match fid_design::status::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("space-check", Some(root), None, None) => match fid_design::space::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("color-check", Some(root), None, None) => match fid_design::color::check(root, args.baseline.as_deref()) {
            Ok(r) => {
                if let Some(out) = &args.out {
                    let written =
                        r.to_json().map_err(|e| format!("report encoding: {}", e.code())).and_then(|mut b| {
                            b.push(b'\n');
                            std::fs::write(out, b).map_err(|e| format!("cannot write {}: {e}", out.display()))
                        });
                    if let Err(e) = written {
                        return fail(e);
                    }
                }
                finish(r.to_json(), r.to_text(), args.json, r.exit_code())
            }
            Err(e) => fail(e),
        },
        ("palettes-check", Some(root), None, None) => match fid_design::palettes::check(root, args.baseline.as_deref())
        {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        ("icons-lint", Some(root), None, None) => match fid_design::icons_lint::check(root) {
            Ok(r) => finish(r.to_json(), r.to_text(), args.json, r.exit_code()),
            Err(e) => fail(e),
        },
        _ => usage(),
    }
}
