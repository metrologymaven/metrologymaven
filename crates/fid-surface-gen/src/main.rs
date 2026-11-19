//! `fid-surface-gen --manifest <json> --out <dir> [--test-emitter echo]...`, run by
//! `cargo xtask gen-surfaces` (AUTO.SURF.GEN).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    fid_surface_gen::cli(&args)
}
