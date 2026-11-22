//! `fid-surface-check --manifest <json> --generated <dir>`, run by
//! `cargo xtask surface-check` (AUTO.SURF.CHECK).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    fid_surface_gen::check::check_cli(&args)
}
