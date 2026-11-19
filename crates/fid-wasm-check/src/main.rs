//! `fid-wasm-check`: CI tool for the kernel WASM reference artifact (spec node KRN.MAN.WASM).
//!
//! ```text
//! fid-wasm-check imports <module.wasm> <allowlist>
//! fid-wasm-check runtime-digest <wasm-runtime.toml>
//! fid-wasm-check manifest <manifest.json> <module.wasm> <allowlist> <wasm-runtime.toml>
//! fid-wasm-check run <wasm-runtime.toml> <allowlist> <module.wasm>        (call on stdin)
//! fid-wasm-check golden <wasm-runtime.toml> <allowlist> <module.wasm> <calls file>
//! ```
//!
//! Exit codes: `imports`, `runtime-digest`, `manifest` and `golden` exit 0 when the check
//! passes, 1 when it fails and 2 on a usage or read error. `run` exits with the runner's code
//! (0, 1 or 2 from the shim; 3 `NOT_REPLAYABLE(resource)`; 4 `NOT_REPLAYABLE(trap)`) and 5
//! when it cannot run at all (bad settings, module refused, wrong or missing wasmtime).

use std::io::{Read, Write};
use std::process::ExitCode;

use fid_wasm_check::allow::{self, Allowlist};
use fid_wasm_check::manifest::cross_check;
use fid_wasm_check::module;
use fid_wasm_check::runner::{self, Outcome};
use fid_wasm_check::runtime::RuntimeConfig;

const USAGE: &str = "usage: fid-wasm-check imports <module.wasm> <allowlist>\n\
       fid-wasm-check runtime-digest <wasm-runtime.toml>\n\
       fid-wasm-check manifest <manifest.json> <module.wasm> <allowlist> <wasm-runtime.toml>\n\
       fid-wasm-check run <wasm-runtime.toml> <allowlist> <module.wasm>\n\
       fid-wasm-check golden <wasm-runtime.toml> <allowlist> <module.wasm> <calls file>";

/// Largest file this tool reads (modules are capped lower by the reader itself).
const MAX_FILE_BYTES: u64 = 128 * 1024 * 1024;

fn read(path: &str) -> Result<Vec<u8>, String> {
    let f = std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?;
    let mut buf = Vec::new();
    f.take(MAX_FILE_BYTES + 1).read_to_end(&mut buf).map_err(|e| format!("{path}: {e}"))?;
    if buf.len() as u64 > MAX_FILE_BYTES {
        return Err(format!("{path}: larger than {MAX_FILE_BYTES} bytes"));
    }
    Ok(buf)
}

fn load_allow(path: &str) -> Result<Allowlist, String> {
    Allowlist::parse(&read(path)?).map_err(|e| format!("{path}: {e}"))
}

fn load_runtime(path: &str) -> Result<RuntimeConfig, String> {
    RuntimeConfig::parse_canonical(&read(path)?).map_err(|e| format!("{path}: {e}"))
}

/// Parses the module and checks its imports; `Ok(lines, passed)`.
fn imports(module_path: &str, allow_path: &str) -> Result<(Vec<String>, bool), String> {
    let allow = load_allow(allow_path)?;
    let m = module::parse(&read(module_path)?).map_err(|e| format!("{module_path}: module refused: {e}"))?;
    let report = allow::check(&m, &allow);
    Ok((report.lines(), report.passed()))
}

/// Loads settings, verifies the module and wasmtime: everything `run` and `golden` need.
fn prepare(runtime_path: &str, allow_path: &str, module_path: &str) -> Result<RuntimeConfig, String> {
    let cfg = load_runtime(runtime_path)?;
    let (lines, passed) = imports(module_path, allow_path)?;
    if !passed {
        return Err(format!("{module_path}: imports not allowed:\n{}", lines.join("\n")));
    }
    runner::check_wasmtime(&cfg).map_err(|e| e.to_string())?;
    Ok(cfg)
}

fn manifest(manifest_path: &str, module_path: &str, allow_path: &str, runtime_path: &str) -> Result<bool, String> {
    let manifest = read(manifest_path)?;
    let module_bytes = read(module_path)?;
    let allow = load_allow(allow_path)?;
    let cfg = load_runtime(runtime_path)?;
    match cross_check(&manifest, &module_bytes, &allow, &cfg) {
        Ok(d) => {
            println!("manifest ok: {d}");
            Ok(true)
        }
        Err(e) => {
            println!("{}: {e}", e.code());
            Ok(false)
        }
    }
}

fn golden(cfg: &RuntimeConfig, module_path: &str, calls_path: &str) -> Result<bool, String> {
    let text = String::from_utf8(read(calls_path)?).map_err(|_| format!("{calls_path}: not UTF-8"))?;
    let cases = runner::parse_cases(&text).map_err(|line| format!("{calls_path}: bad line {line}"))?;
    if cases.is_empty() {
        return Err(format!("{calls_path}: no cases"));
    }
    let mut failed = 0usize;
    for case in &cases {
        let got = runner::run(cfg, module_path, &case.call).map_err(|e| e.to_string())?;
        if got.exit_code() == case.exit && got.stdout() == case.stdout.as_slice() {
            println!("ok {}", case.name);
        } else {
            failed += 1;
            println!(
                "FAIL {}: exit {} (expected {}), stdout {}",
                case.name,
                got.exit_code(),
                case.exit,
                String::from_utf8_lossy(got.stdout())
            );
        }
    }
    println!("{} cases, {failed} failed", cases.len());
    Ok(failed == 0)
}

fn verdict(r: Result<bool, String>) -> ExitCode {
    match r {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let a: Vec<&str> = args.iter().map(String::as_str).collect();
    match a.as_slice() {
        ["imports", module_path, allow_path] => verdict(imports(module_path, allow_path).map(|(lines, passed)| {
            for l in lines {
                println!("{l}");
            }
            passed
        })),
        ["runtime-digest", path] => verdict(load_runtime(path).map(|cfg| {
            println!("{}", cfg.config_digest());
            true
        })),
        ["manifest", manifest_path, module_path, allow_path, runtime_path] => {
            verdict(manifest(manifest_path, module_path, allow_path, runtime_path))
        }
        ["run", runtime_path, allow_path, module_path] => {
            let cfg = match prepare(runtime_path, allow_path, module_path) {
                Ok(c) => c,
                Err(e) => {
                    eprintln!("error: {e}");
                    return ExitCode::from(5);
                }
            };
            let mut call = Vec::new();
            let limit = MAX_FILE_BYTES + 1;
            if std::io::stdin().lock().take(limit).read_to_end(&mut call).is_err() || call.len() as u64 >= limit {
                eprintln!("error: cannot read the call from stdin");
                return ExitCode::from(5);
            }
            match runner::run(&cfg, module_path, &call) {
                Ok(outcome) => {
                    let mut out = std::io::stdout().lock();
                    if out.write_all(outcome.stdout()).and_then(|()| out.flush()).is_err() {
                        return ExitCode::from(5);
                    }
                    if !matches!(outcome, Outcome::Shim { .. }) {
                        eprintln!("runner: {}", String::from_utf8_lossy(outcome.stdout()));
                    }
                    ExitCode::from(outcome.exit_code())
                }
                Err(e) => {
                    eprintln!("error: {e}");
                    ExitCode::from(5)
                }
            }
        }
        ["golden", runtime_path, allow_path, module_path, calls_path] => {
            match prepare(runtime_path, allow_path, module_path) {
                Ok(cfg) => verdict(golden(&cfg, module_path, calls_path)),
                Err(e) => verdict(Err(e)),
            }
        }
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}
