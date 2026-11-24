//! Embeds the generated `cli-tree.json` and matching `cli-manifest.json` into `fid` at compile
//! time (`AUTO.CLI.TREE`, `AUTO.CLI.GATE`).
//!
//! Override the source tree by setting `FID_CLI_TREE=/path/to/cli-tree.json` and optionally
//! `FID_CLI_MANIFEST=/path/to/manifest.json`. When `FID_CLI_TREE` is unset, embeds the default
//! fixture tree `fixtures/cli/min-classes.tree.json` and manifest
//! `fixtures/manifest/min-classes.json`.

use std::env;
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("cargo:rerun-if-env-changed=FID_CLI_TREE");
    println!("cargo:rerun-if-env-changed=FID_CLI_MANIFEST");

    let manifest_dir = match env::var_os("CARGO_MANIFEST_DIR") {
        Some(v) => PathBuf::from(v),
        None => std::process::exit(1),
    };
    let out_dir = match env::var_os("OUT_DIR") {
        Some(v) => PathBuf::from(v),
        None => std::process::exit(1),
    };

    let custom_tree = env::var_os("FID_CLI_TREE").is_some_and(|p| !p.is_empty());
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

    let manifest_bytes = match env::var_os("FID_CLI_MANIFEST") {
        Some(p) if !p.is_empty() => {
            let p = PathBuf::from(p);
            println!("cargo:rerun-if-changed={}", p.display());
            match fs::read(&p) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("fid-cli build.rs: cannot read manifest at {}: {e}", p.display());
                    std::process::exit(1);
                }
            }
        }
        _ if !custom_tree => {
            let p = manifest_dir.join("../../fixtures/manifest/min-classes.json");
            println!("cargo:rerun-if-changed={}", p.display());
            match fs::read(&p) {
                Ok(b) => b,
                Err(e) => {
                    eprintln!("fid-cli build.rs: cannot read default manifest at {}: {e}", p.display());
                    std::process::exit(1);
                }
            }
        }
        _ => Vec::new(),
    };
    let manifest_dst = out_dir.join("cli-manifest.json");
    if let Err(e) = fs::write(&manifest_dst, manifest_bytes) {
        eprintln!("fid-cli build.rs: cannot write {}: {e}", manifest_dst.display());
        std::process::exit(1);
    }
}
