//! `fid-manifest-gen`: build-time kernel manifest tool (spec node `KRN.MAN.MANI`).
//!
//! ```text
//! fid-manifest-gen toolchain-check  [--workspace <dir>]
//! fid-manifest-gen toolchain-digest [--workspace <dir>] [--rustc <program>] [--record <file>]
//! ```
//!
//! `toolchain-check` checks the kernel build settings in `<dir>/kernel-build` (KRN.MAN.REPRO
//! AC-4) without running a toolchain. `toolchain-digest` also runs `<program> -vV` and
//! `<program> --print cfg ..` (default `rustc`) from `<dir>/kernel-build`, with
//! `RUSTUP_TOOLCHAIN` removed so rustup picks the pinned toolchain exactly as the rebuild script
//! does; it fails on any drift from the pinned toolchain, prints the toolchain-and-flags digest
//! (`sha256:<hex>`) and, with `--record`, writes the canonical record. The workspace defaults
//! to the current directory. Exit codes: 0 pass, 1 the settings or the toolchain were refused,
//! 2 usage or I/O error.

use std::path::{Path, PathBuf};
use std::process::{Command, ExitCode};

use fid_manifest_gen::toolchain::{
    self, Inputs, ToolchainError, CARGO_CONFIG, KERNEL_BUILD, LOCKFILE, MAX_LOCKFILE_BYTES, MAX_RUSTC_OUTPUT_BYTES,
    MAX_SETTINGS_BYTES, RUST_TOOLCHAIN, SETTINGS_DIR, WORKSPACE_CONFIG, WORKSPACE_MANIFEST,
};

const USAGE: &str = "usage: fid-manifest-gen toolchain-check [--workspace <dir>]\n\
       fid-manifest-gen toolchain-digest [--workspace <dir>] [--rustc <program>] [--record <file>]";

enum Failure {
    Refused(ToolchainError),
    Usage(String),
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

fn run(args: &[String]) -> Result<(), Failure> {
    let (cmd, rest) = args.split_first().ok_or_else(|| Failure::Usage(String::new()))?;
    match cmd.as_str() {
        "toolchain-check" => {
            let o = opts(rest, false)?;
            let files = Files::load(&o.workspace)?;
            let st = toolchain::settings(&files.inputs())?;
            println!("kernel build settings ok: {} {} {}", st.channel, st.target, st.package);
            Ok(())
        }
        "toolchain-digest" => {
            let o = opts(rest, true)?;
            let files = Files::load(&o.workspace)?;
            let st = toolchain::settings(&files.inputs())?;
            let dir = o.workspace.join(SETTINGS_DIR);
            let vv = rustc(&o.rustc, &dir, &["-vV".to_string()])?;
            let cfg = rustc(&o.rustc, &dir, &st.rustc_cfg_args())?;
            let rec = toolchain::record(&files.inputs(), &vv, &cfg)?;
            if let Some(path) = &o.record {
                std::fs::write(path, &rec.canonical).map_err(|e| Failure::Usage(format!("{}: {e}", path.display())))?;
            }
            println!("{}", rec.digest);
            Ok(())
        }
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
        Err(Failure::Usage(m)) => {
            if !m.is_empty() {
                eprintln!("error: {m}");
            }
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
