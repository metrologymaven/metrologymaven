//! Repository check `fid-app-manifest check` (spec node ECO.APPS.MANIFEST.SCHEMA).

use std::io::Write as _;

fn main() {
    // `args_os`: a non-UTF-8 argument is a usage error, not a panic in `std::env::args`.
    let args: Option<Vec<String>> = std::env::args_os().skip(1).map(|a| a.into_string().ok()).collect();
    let (code, stdout, stderr) = match args {
        Some(args) => fid_app_manifest::cli::run_cli(&args),
        None => {
            (2, String::new(), format!("fid-app-manifest: argument is not UTF-8\n{}", fid_app_manifest::cli::USAGE))
        }
    };
    let _ = std::io::stdout().write_all(stdout.as_bytes());
    let _ = std::io::stderr().write_all(stderr.as_bytes());
    std::process::exit(code);
}
