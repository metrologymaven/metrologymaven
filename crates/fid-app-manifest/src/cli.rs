//! `fid-app-manifest check --manifest <file> --catalogues <dir> [--seeds <dir>]`.
//!
//! Exit 0 with no output when the manifest passes; exit 1 with one finding per line on stdout
//! (sorted, REQ-009) when it or a catalogue file does not; exit 2 with a usage message on
//! stderr for bad arguments. Output never contains a path.

use crate::{load_manifest, Catalogues};
use std::path::PathBuf;

/// Usage text.
pub const USAGE: &str = "usage: fid-app-manifest check --manifest <file> --catalogues <dir> [--seeds <dir>]\n";
const MAX_ARGS: usize = 7;

/// Runs the command line `args` (without the program name) and returns the exit code,
/// stdout and stderr.
pub fn run_cli(args: &[String]) -> (i32, String, String) {
    let usage = |why: &str| (2, String::new(), format!("fid-app-manifest: {why}\n{USAGE}"));
    if args.len() > MAX_ARGS {
        return usage("too many arguments");
    }
    let Some((sub, rest)) = args.split_first() else { return usage("missing subcommand") };
    if sub != "check" {
        return usage("unknown subcommand");
    }
    let (mut manifest, mut catalogues, mut seeds): (Option<PathBuf>, Option<PathBuf>, Option<PathBuf>) =
        (None, None, None);
    let mut it = rest.iter();
    while let Some(flag) = it.next() {
        let slot = match flag.as_str() {
            "--manifest" => &mut manifest,
            "--catalogues" => &mut catalogues,
            "--seeds" => &mut seeds,
            _ => return usage("unknown option"),
        };
        if slot.is_some() {
            return usage("option given twice");
        }
        let Some(value) = it.next() else { return usage("option without a value") };
        *slot = Some(PathBuf::from(value));
    }
    let (Some(manifest), Some(catalogues)) = (manifest, catalogues) else {
        return usage("--manifest and --catalogues are required");
    };
    let findings = match Catalogues::from_dir(&catalogues, seeds.as_deref()) {
        Ok(c) => match load_manifest(&manifest, &c) {
            Ok(_) => return (0, String::new(), String::new()),
            Err(f) => f,
        },
        Err(f) => f,
    };
    let mut out = String::new();
    for f in &findings {
        out.push_str(&f.to_string());
        out.push('\n');
    }
    (1, out, String::new())
}
