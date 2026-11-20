//! `fid-manifest-gen`: build-time kernel manifest tool (spec node `KRN.MAN.MANI`).
//!
//! ```text
//! fid-manifest-gen toolchain-check  [--workspace <dir>]
//! fid-manifest-gen toolchain-digest [--workspace <dir>] [--rustc <program>] [--record <file>]
//! fid-manifest-gen toolchain-flags  [--workspace <dir>]
//! fid-manifest-gen core   --config <manifest-gen.toml> --out <core.json> [--constant <file>]
//! fid-manifest-gen finish --core <core.json> --artifacts <artifacts.toml> --out <kernel-manifest.json>
//!                         [--xart <program>] [--wasm-check <program>] [--timeout-ms <ms>] [--work-dir <dir>]
//! ```
//!
//! `core` (stage one, KRN.MAN.MANI.GEN) gathers the manifest core from the sources the
//! configuration names, writes the stage-one manifest (canonical, no artifacts) to `--out`,
//! writes the generated `manifest_core_digest` constant when `--constant` names the configured
//! constant file (`<workspace>/<constant>`; any other path is refused), and prints the core
//! digest. `finish` (stage two) hashes every artifact of the list, asks each for its
//! core digest through `fid-xart run` (default program `fid-xart`, deadline 60 000 ms) with a
//! command derived from the artifact's kind (native: the file itself; wasm: `fid-wasm-check run
//! <runtime> <allow> <file>`), and
//! writes the full manifest to `--out` and prints its digest. Both remove `--out` first, so a
//! failed run leaves no stale file. Exit codes: 0 written, 1 refused (a typed error such as
//! `missing_source: <field>` or `core_digest_mismatch: <artifact>`, printed on stdout), 2 usage
//! or I/O error.
//!
//! `toolchain-check` checks the kernel build settings in `<dir>/kernel-build` (KRN.MAN.REPRO
//! AC-4) without running a toolchain. `toolchain-digest` also runs `<program> -vV` and
//! `<program> --print cfg ..` (default `rustc`) from `<dir>/kernel-build`, with
//! `RUSTUP_TOOLCHAIN` removed so rustup picks the pinned toolchain exactly as the rebuild script
//! does; it fails on any drift from the pinned toolchain, prints the toolchain-and-flags digest
//! (`sha256:<hex>`) and, with `--record`, writes the canonical record. The workspace defaults
//! to the current directory. `toolchain-flags` prints the checked static rustflags of the kernel
//! target, one per line, for the rebuild script to pass explicitly (CARGO_ENCODED_RUSTFLAGS).
//! Every command first refuses any Cargo configuration file the kernel build would read besides
//! the two it checks (`stray_cargo_config`). Exit codes: 0 pass, 1 the settings or the toolchain were refused,
//! 2 usage or I/O error.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fid_manifest_gen::gen::{self, Config, GenError, XartReporter};
use fid_manifest_gen::toolchain::{
    self, Inputs, ToolchainError, CARGO_CONFIG, KERNEL_BUILD, LOCKFILE, MAX_LOCKFILE_BYTES, MAX_RUSTC_OUTPUT_BYTES,
    MAX_SETTINGS_BYTES, RUST_TOOLCHAIN, SETTINGS_DIR, WORKSPACE_CONFIG, WORKSPACE_MANIFEST,
};

const USAGE: &str = "usage: fid-manifest-gen toolchain-check [--workspace <dir>]\n\
       fid-manifest-gen toolchain-digest [--workspace <dir>] [--rustc <program>] [--record <file>]\n\
       fid-manifest-gen toolchain-flags [--workspace <dir>]\n\
       fid-manifest-gen core --config <manifest-gen.toml> --out <core.json> [--constant <file>]\n\
       fid-manifest-gen finish --core <core.json> --artifacts <artifacts.toml> --out <kernel-manifest.json> \
[--xart <program>] [--wasm-check <program>] [--timeout-ms <ms>] [--work-dir <dir>]";

enum Failure {
    Refused(ToolchainError),
    Gen(GenError),
    Usage(String),
}

impl From<GenError> for Failure {
    fn from(e: GenError) -> Self {
        Failure::Gen(e)
    }
}

/// `--flag value` pairs, each flag at most once and in `allowed`.
fn flags<'a>(args: &'a [String], allowed: &[&str]) -> Result<Vec<(&'a str, &'a str)>, Failure> {
    let mut out: Vec<(&str, &str)> = Vec::new();
    let mut it = args.iter();
    while let Some(a) = it.next() {
        if !allowed.contains(&a.as_str()) || out.iter().any(|(k, _)| *k == a.as_str()) {
            return Err(Failure::Usage(format!("unknown or repeated option {a}")));
        }
        let v = it.next().ok_or_else(|| Failure::Usage(format!("{a}: missing value")))?;
        out.push((a.as_str(), v.as_str()));
    }
    Ok(out)
}

fn flag<'a>(f: &[(&str, &'a str)], key: &str) -> Option<&'a str> {
    f.iter().find(|(k, _)| *k == key).map(|(_, v)| *v)
}

fn required<'a>(f: &[(&str, &'a str)], key: &str) -> Result<&'a str, Failure> {
    flag(f, key).ok_or_else(|| Failure::Usage(format!("missing {key}")))
}

/// Removes a stale output file; a missing one is fine.
fn remove_stale(out: &Path) -> Result<(), Failure> {
    match std::fs::remove_file(out) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(Failure::Usage(format!("{}: cannot remove the stale file: {e}", out.display()))),
    }
}

fn write(path: &Path, bytes: &[u8]) -> Result<(), Failure> {
    std::fs::write(path, bytes).map_err(|e| Failure::Usage(format!("{}: {e}", path.display())))
}

fn core_command(rest: &[String]) -> Result<(), Failure> {
    let f = flags(rest, &["--config", "--out", "--constant"])?;
    let out = Path::new(required(&f, "--out")?);
    remove_stale(out)?;
    let config = Config::load(Path::new(required(&f, "--config")?))?;
    let stage = gen::core(&config)?;
    if let Some(c) = flag(&f, "--constant") {
        // Only the configured constant may be written: `--constant` must name
        // <workspace>/<constant> (core has checked that the file exists).
        let ws = config.workspace.clone().unwrap_or_default();
        let target = ws.join(&stage.constant_path);
        let same = match (std::fs::canonicalize(c), std::fs::canonicalize(&target)) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        if !same {
            return Err(Failure::Gen(GenError::SourceInvalid {
                field: gen::field::CONFIG,
                reason: format!("--constant must name {}", target.display()),
            }));
        }
        write(&target, &stage.constant)?;
    }
    write(out, stage.manifest.canonical_json())?;
    println!("{}", stage.manifest.core_digest());
    Ok(())
}

fn finish_command(rest: &[String]) -> Result<(), Failure> {
    let f = flags(rest, &["--core", "--artifacts", "--out", "--xart", "--wasm-check", "--timeout-ms", "--work-dir"])?;
    let out = Path::new(required(&f, "--out")?);
    remove_stale(out)?;
    let read_all = |key: &str| -> Result<Vec<u8>, Failure> {
        let p = required(&f, key)?;
        std::fs::read(p).map_err(|e| Failure::Usage(format!("{p}: {e}")))
    };
    let stage_one = read_all("--core")?;
    let mut artifacts = gen::parse_artifacts(&read_all("--artifacts")?)?;
    if let Some(p) = flag(&f, "--wasm-check") {
        artifacts.wasm_check = p.to_string();
    }
    let timeout_ms = match flag(&f, "--timeout-ms") {
        None => 60_000,
        Some(v) => v
            .parse::<u64>()
            .ok()
            .filter(|ms| (1..=86_400_000).contains(ms))
            .ok_or_else(|| Failure::Usage("bad --timeout-ms".to_string()))?,
    };
    let work_dir = match flag(&f, "--work-dir") {
        Some(d) => PathBuf::from(d),
        None => match out.parent() {
            Some(p) if !p.as_os_str().is_empty() => p.to_path_buf(),
            _ => PathBuf::from("."),
        },
    };
    let mut reporter =
        XartReporter { program: flag(&f, "--xart").unwrap_or("fid-xart").to_string(), work_dir, timeout_ms };
    let manifest = gen::finish(&stage_one, &artifacts, &mut reporter)?;
    write(out, manifest.canonical_json())?;
    println!("{}", manifest.manifest_digest());
    Ok(())
}

impl From<ToolchainError> for Failure {
    fn from(e: ToolchainError) -> Self {
        Failure::Refused(e)
    }
}

fn read(ws: &Path, rel: &'static str, max: usize) -> Result<Vec<u8>, Failure> {
    let path = ws.join(rel);
    let meta = std::fs::metadata(&path).map_err(|e| Failure::Usage(format!("{rel}: {e}")))?;
    if meta.len() > max as u64 {
        return Err(Failure::Refused(ToolchainError::BadFile { file: rel }));
    }
    std::fs::read(&path).map_err(|e| Failure::Usage(format!("{rel}: {e}")))
}

struct Files {
    rust_toolchain: Vec<u8>,
    cargo_config: Vec<u8>,
    kernel_build: Vec<u8>,
    workspace_manifest: Vec<u8>,
    workspace_config: Option<Vec<u8>>,
    lockfile: Vec<u8>,
}

impl Files {
    fn load(ws: &Path) -> Result<Files, Failure> {
        let workspace_config = if ws.join(WORKSPACE_CONFIG).exists() {
            Some(read(ws, WORKSPACE_CONFIG, MAX_SETTINGS_BYTES)?)
        } else {
            None
        };
        Ok(Files {
            rust_toolchain: read(ws, RUST_TOOLCHAIN, MAX_SETTINGS_BYTES)?,
            cargo_config: read(ws, CARGO_CONFIG, MAX_SETTINGS_BYTES)?,
            kernel_build: read(ws, KERNEL_BUILD, MAX_SETTINGS_BYTES)?,
            workspace_manifest: read(ws, WORKSPACE_MANIFEST, MAX_SETTINGS_BYTES)?,
            workspace_config,
            lockfile: read(ws, LOCKFILE, MAX_LOCKFILE_BYTES)?,
        })
    }

    fn inputs(&self) -> Inputs<'_> {
        Inputs {
            rust_toolchain: &self.rust_toolchain,
            cargo_config: &self.cargo_config,
            kernel_build: &self.kernel_build,
            workspace_manifest: &self.workspace_manifest,
            workspace_config: self.workspace_config.as_deref(),
            lockfile: &self.lockfile,
        }
    }
}

/// Runs the toolchain's `rustc` from the settings directory and returns its stdout.
fn rustc(program: &str, dir: &Path, args: &[String]) -> Result<String, Failure> {
    let out = Command::new(program)
        .args(args)
        .current_dir(dir)
        .env_remove("RUSTUP_TOOLCHAIN")
        .output()
        .map_err(|e| Failure::Usage(format!("{program}: {e}")))?;
    if !out.status.success() {
        return Err(Failure::Usage(format!(
            "{program} {}: {}\n{}",
            args.join(" "),
            out.status,
            String::from_utf8_lossy(&out.stderr)
        )));
    }
    if out.stdout.len() > MAX_RUSTC_OUTPUT_BYTES {
        return Err(Failure::Refused(ToolchainError::BadRustcOutput { reason: "too_large" }));
    }
    String::from_utf8(out.stdout).map_err(|_| Failure::Refused(ToolchainError::BadRustcOutput { reason: "utf8" }))
}

struct Opts {
    workspace: PathBuf,
    rustc: String,
    record: Option<PathBuf>,
}

fn opts(args: &[String], allow_toolchain: bool) -> Result<Opts, Failure> {
    let mut o = Opts { workspace: PathBuf::from("."), rustc: "rustc".to_string(), record: None };
    let mut it = args.iter();
    while let Some(a) = it.next() {
        let value = it.next().ok_or_else(|| Failure::Usage(format!("{a}: missing value")))?;
        match a.as_str() {
            "--workspace" => o.workspace = PathBuf::from(value),
            "--rustc" if allow_toolchain => o.rustc = value.clone(),
            "--record" if allow_toolchain => o.record = Some(PathBuf::from(value)),
            _ => return Err(Failure::Usage(format!("unknown option {a}"))),
        }
    }
    Ok(o)
}

/// Loads and checks the settings of `ws` (made absolute so every directory above it is seen).
fn checked(ws: &Path) -> Result<(Files, toolchain::Settings, PathBuf), Failure> {
    let ws = std::fs::canonicalize(ws).map_err(|e| Failure::Usage(format!("{}: {e}", ws.display())))?;
    toolchain::stray_cargo_config(&ws)?;
    let files = Files::load(&ws)?;
    let st = toolchain::settings(&files.inputs())?;
    Ok((files, st, ws))
}

fn run(args: &[String]) -> Result<(), Failure> {
    let (cmd, rest) = args.split_first().ok_or_else(|| Failure::Usage(String::new()))?;
    match cmd.as_str() {
        "toolchain-flags" => {
            let o = opts(rest, false)?;
            let (_, st, _) = checked(&o.workspace)?;
            for f in &st.rustflags {
                println!("{f}");
            }
            Ok(())
        }
        "toolchain-check" => {
            let o = opts(rest, false)?;
            let (_, st, _) = checked(&o.workspace)?;
            println!("kernel build settings ok: {} {} {}", st.channel, st.target, st.package);
            Ok(())
        }
        "toolchain-digest" => {
            let o = opts(rest, true)?;
            let (files, st, ws) = checked(&o.workspace)?;
            let dir = ws.join(SETTINGS_DIR);
            let vv = rustc(&o.rustc, &dir, &["-vV".to_string()])?;
            let cfg = rustc(&o.rustc, &dir, &st.rustc_cfg_args())?;
            let rec = toolchain::record(&files.inputs(), &vv, &cfg)?;
            if let Some(path) = &o.record {
                std::fs::write(path, &rec.canonical).map_err(|e| Failure::Usage(format!("{}: {e}", path.display())))?;
            }
            println!("{}", rec.digest);
            Ok(())
        }
        "core" => core_command(rest),
        "finish" => finish_command(rest),
        _ => Err(Failure::Usage(format!("unknown command {cmd}"))),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match run(&args) {
        Ok(()) => ExitCode::SUCCESS,
        Err(Failure::Refused(e)) => {
            println!("{e}");
            ExitCode::from(1)
        }
        Err(Failure::Gen(e @ GenError::Io { .. })) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
        Err(Failure::Gen(e)) => {
            println!("{e}");
            ExitCode::from(1)
        }
        Err(Failure::Usage(m)) => {
            if !m.is_empty() {
                eprintln!("error: {m}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
