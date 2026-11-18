//! The `fid-data` tool; `cargo xtask datasets-check ...` and `cargo xtask data ...` run it.
//!
//! - `fid-data datasets-check --catalogue <toml> --domains <file>` (spec node
//!   `ECO.DATA.CAT`): prints one finding per line, sorted by entry ID and code, and exits
//!   1 if there are any; prints `datasets-check: ok` and exits 0 otherwise.
//! - `fid-data data fetch (<id>... | --all | --owner <DOMAIN>) [options]`,
//!   `fid-data data path <id> [options]` and `fid-data data verify [--cache <dir>]` (spec
//!   node `ECO.DATA.FETCH`). Options: `--cache <dir>` (else `FID_DATA_CACHE`, else the
//!   per-user cache), `--offline` (or `FID_DATA_OFFLINE=1`), `--catalogue <toml>` (else
//!   `FID_DATA_CATALOGUE`, else the repository's `datasets.toml`). Paths go to stdout;
//!   progress and errors (`<CODE> <id>: <detail>`) to stderr. Exit 1 on any error.
//!
//! Exit 2 on bad usage.

use fid_data::cat::check_files;
use fid_data::fetch::{self, DataError, SystemTransport};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-data datasets-check --catalogue <toml> --domains <file>\n       \
                     fid-data data fetch (<id>... | --all | --owner <DOMAIN>) [--cache <dir>] [--offline] \
                     [--catalogue <toml>]\n       \
                     fid-data data path <id> [--cache <dir>] [--offline] [--catalogue <toml>]\n       \
                     fid-data data verify [--cache <dir>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("datasets-check"), _) => datasets_check(&args[1..]),
        (Some("data"), Some("fetch")) => data_cmd(Cmd::Fetch, &args[2..]),
        (Some("data"), Some("path")) => data_cmd(Cmd::Path, &args[2..]),
        (Some("data"), Some("verify")) => data_cmd(Cmd::Verify, &args[2..]),
        _ => usage(),
    }
}

fn usage() -> ExitCode {
    eprintln!("{USAGE}");
    ExitCode::from(2)
}

fn datasets_check(args: &[String]) -> ExitCode {
    let mut catalogue: Option<PathBuf> = None;
    let mut domains: Option<PathBuf> = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match (a.as_str(), it.next()) {
            ("--catalogue", Some(v)) => catalogue = Some(PathBuf::from(v)),
            ("--domains", Some(v)) => domains = Some(PathBuf::from(v)),
            _ => return usage(),
        }
    }
    let (Some(catalogue), Some(domains)) = (catalogue, domains) else {
        return usage();
    };
    let findings = check_files(&catalogue, &domains);
    if findings.is_empty() {
        println!("datasets-check: ok");
        return ExitCode::SUCCESS;
    }
    for f in &findings {
        println!("{f}");
    }
    ExitCode::from(1)
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Cmd {
    Fetch,
    Path,
    Verify,
}

#[derive(Default)]
struct Opts {
    ids: Vec<String>,
    all: bool,
    owner: Option<String>,
    cache: Option<PathBuf>,
    catalogue: Option<PathBuf>,
    offline: bool,
}

fn parse_opts(cmd: Cmd, args: &[String]) -> Option<Opts> {
    let mut o = Opts::default();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--cache" if o.cache.is_none() => o.cache = Some(PathBuf::from(it.next()?)),
            "--catalogue" if cmd != Cmd::Verify && o.catalogue.is_none() => {
                o.catalogue = Some(PathBuf::from(it.next()?));
            }
            "--offline" if cmd != Cmd::Verify => o.offline = true,
            "--all" if cmd == Cmd::Fetch => o.all = true,
            "--owner" if cmd == Cmd::Fetch && o.owner.is_none() => o.owner = Some(it.next()?.clone()),
            s if !s.starts_with('-') && cmd != Cmd::Verify => o.ids.push(s.to_string()),
            _ => return None,
        }
    }
    let selectors = usize::from(!o.ids.is_empty()) + usize::from(o.all) + usize::from(o.owner.is_some());
    let ok = match cmd {
        Cmd::Fetch => selectors == 1,
        Cmd::Path => o.ids.len() == 1,
        Cmd::Verify => true,
    };
    ok.then_some(o)
}

fn data_cmd(cmd: Cmd, args: &[String]) -> ExitCode {
    let Some(o) = parse_opts(cmd, args) else {
        return usage();
    };
    match run(cmd, o) {
        Ok(()) => ExitCode::SUCCESS,
        Err(errors) => {
            for e in &errors {
                eprintln!("{e}");
            }
            ExitCode::from(1)
        }
    }
}

fn run(cmd: Cmd, o: Opts) -> Result<(), Vec<DataError>> {
    let root = match o.cache {
        Some(c) => c,
        None => fetch::default_cache().map_err(|e| vec![e])?,
    };
    if cmd == Cmd::Verify {
        let (ok, errors) = fetch::verify_cache(&root);
        if !errors.is_empty() {
            return Err(errors);
        }
        println!("data verify: ok ({ok} files in {})", root.display());
        return Ok(());
    }
    let offline = o.offline || fetch::env_offline();
    let catalogue = o.catalogue.unwrap_or_else(fetch::default_catalogue);
    let entries = fetch::load_catalogue(&catalogue).map_err(|e| vec![e])?;
    let mut selected = Vec::new();
    let mut errors = Vec::new();
    if o.all || o.owner.is_some() {
        selected.extend(entries.iter().filter(|e| o.all || o.owner.as_deref() == Some(e.owner.as_str())));
    }
    for id in &o.ids {
        match fetch::find(&entries, id) {
            Ok(e) => selected.push(e),
            Err(e) => errors.push(e),
        }
    }
    let mut log = std::io::stderr();
    for entry in selected {
        match fetch::fetch_entry(entry, &root, offline, &SystemTransport, &mut log) {
            Ok(path) => println!("{}", path.display()),
            Err(e) => errors.push(e),
        }
    }
    if errors.is_empty() {
        Ok(())
    } else {
        Err(errors)
    }
}
