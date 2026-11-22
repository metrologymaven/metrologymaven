//! Embeds the generated `cli-tree.json` into `fid` at compile time (`AUTO.CLI.TREE`).
//!
//! Override the source tree by setting `FID_CLI_TREE=/path/to/cli-tree.json`. When unset,
//! embeds the default fixture tree `fixtures/cli/min-classes.tree.json`.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=FID_CLI_TREE");

    let manifest_dir = match env::var_os("CARGO_MANIFEST_DIR") {
        Some(v) => PathBuf::from(v),
        None => std::process::exit(1),
    };
    let out_dir = match env::var_os("OUT_DIR") {
        Some(v) => PathBuf::from(v),
        None => std::process::exit(1),
    };

    let src = match env::var_os("FID_CLI_TREE") {
        Some(p) if !p.is_empty() => PathBuf::from(p),
        _ => manifest_dir.join("../../fixtures/cli/min-classes.tree.json"),
    };
    println!("cargo:rerun-if-changed={}", src.display());

    let bytes = match fs::read(&src) {
        Ok(b) => b,
        Err(e) => {
            eprintln!("fid-cli build.rs: cannot read cli-tree at {}: {e}", src.display());
            std::process::exit(1);
        }
    };
    let dst = out_dir.join("cli-tree.json");
    if let Err(e) = fs::write(&dst, bytes) {
        eprintln!("fid-cli build.rs: cannot write {}: {e}", dst.display());
        std::process::exit(1);
    }
}
