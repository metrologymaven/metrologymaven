//! `fid-crash`: the crash harness command (`cargo xtask crash-harness` runs `fid-crash run`).
//!
//! ```text
//! fid-crash run [--seed N] [--kills N] [--max-delay-us N] [--rotate N] [--dir DIR]
//!               [--mode correct|ack-before-commit|row-before-blob-flush]
//!               [--instant KILL:DELAY_US [--attempts N]]
//!               [--power-loss --dm-name NAME --dm-dev DEV --dm-sectors N --mount DIR]
//! fid-crash writer --mode MODE --root DIR [--max-commits N] [--hang-at STEP] [--wal-sizes]
//! ```
//!
//! `run` exits 0 when every check passed, 1 on a failed check (printing the seed, the kill
//! instant and the bus error) and 2 on a usage error. The broken modes and `--hang-at` exist
//! only in builds with the `fault-inject` feature. `writer` is the child `run` spawns.

use fid_crash::adapter::{CrashAdapter, StoreAdapter, WriterMode};
use fid_crash::{run, run_writer, Config, KillInstant, PowerCut};
use std::ffi::OsString;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-crash run [--seed N] [--kills N] [--max-delay-us N] [--rotate N] [--dir DIR] \
                     [--mode MODE] [--instant KILL:DELAY_US [--attempts N]] \
                     [--power-loss --dm-name NAME --dm-dev DEV --dm-sectors N --mount DIR]\n       \
                     fid-crash writer --mode MODE --root DIR [--max-commits N] [--hang-at STEP] [--wal-sizes]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("run") => cmd_run(&args[1..]),
        Some("writer") => cmd_writer(&args[1..]),
        _ => Err(USAGE.to_string()),
    };
    match result {
        Ok(code) => code,
        Err(msg) => {
            eprintln!("fid-crash: {msg}");
            ExitCode::from(2)
        }
    }
}

/// Flag parser: `--name value` pairs and bare switches.
struct Flags<'a> {
    args: &'a [String],
    switches: &'a [&'a str],
}

impl<'a> Flags<'a> {
    fn check(&self, known: &[&str]) -> Result<(), String> {
        let mut i = 0;
        while let Some(a) = self.args.get(i) {
            if self.switches.contains(&a.as_str()) {
                i += 1;
            } else if known.contains(&a.as_str()) && i + 1 < self.args.len() {
                i += 2;
            } else {
                return Err(format!("unexpected argument {a:?}\n{USAGE}"));
            }
        }
        Ok(())
    }

    fn value(&self, name: &str) -> Option<&'a str> {
        let mut i = 0;
        let mut found = None;
        while let Some(a) = self.args.get(i) {
            if self.switches.contains(&a.as_str()) {
                i += 1;
                continue;
            }
            if a == name {
                found = self.args.get(i + 1).map(String::as_str);
            }
            i += 2;
        }
        found
    }

    fn has(&self, switch: &str) -> bool {
        self.args.iter().any(|a| a == switch)
    }

    fn number(&self, name: &str, default: u64) -> Result<u64, String> {
        match self.value(name) {
            None => Ok(default),
            Some(v) => v.parse().map_err(|_| format!("{name} must be a non-negative integer, got {v:?}")),
        }
    }
}

fn mode(flags: &Flags<'_>) -> Result<WriterMode, String> {
    let name = flags.value("--mode").unwrap_or("correct");
    WriterMode::parse(name).ok_or_else(|| {
        format!("unknown mode {name:?} (the broken self-test modes need a build with the fault-inject feature)")
    })
}

fn cmd_run(args: &[String]) -> Result<ExitCode, String> {
    let flags = Flags { args, switches: &["--power-loss"] };
    flags.check(&[
        "--seed",
        "--kills",
        "--max-delay-us",
        "--rotate",
        "--dir",
        "--mode",
        "--instant",
        "--attempts",
        "--dm-name",
        "--dm-dev",
        "--dm-sectors",
        "--mount",
    ])?;
    let mode = mode(&flags)?;
    let exe = std::env::current_exe().map_err(|e| format!("cannot find own executable: {e}"))?;
    let writer: Vec<OsString> = vec![exe.into_os_string(), "writer".into(), "--mode".into(), mode.name().into()];
    let power = if flags.has("--power-loss") {
        if !cfg!(target_os = "linux") {
            return Err("--power-loss is available on Linux only".to_string());
        }
        let need = |n: &str| flags.value(n).ok_or_else(|| format!("--power-loss needs {n}"));
        Some(PowerCut {
            dm_name: need("--dm-name")?.to_string(),
            backing: PathBuf::from(need("--dm-dev")?),
            sectors: flags.number("--dm-sectors", 0)?,
            mount: PathBuf::from(need("--mount")?),
        })
    } else {
        None
    };
    let default_dir = match &power {
        Some(p) => p.mount.join("fid-crash"),
        None => std::env::temp_dir().join(format!("fid-crash-{}", std::process::id())),
    };
    let dir = flags.value("--dir").map_or(default_dir, PathBuf::from);
    let mut config = Config::new(flags.number("--seed", 1)?, flags.number("--kills", 1000)?, dir, writer);
    config.max_delay_us = flags.number("--max-delay-us", config.max_delay_us)?;
    config.rotate_every = flags.number("--rotate", config.rotate_every)?;
    config.attempts = flags.number("--attempts", 20)?;
    config.power = power;
    if let Some(s) = flags.value("--instant") {
        config.only = Some(KillInstant::parse(s).ok_or_else(|| format!("--instant must be KILL:DELAY_US, got {s:?}"))?);
    }
    let adapter = StoreAdapter::new(mode);
    let mut out = std::io::stdout();
    let _ = writeln!(
        out,
        "crash-harness: mode {}, seed {}, {} in {}",
        mode.name(),
        config.seed,
        match config.only {
            Some(i) => format!("instant {i} x{}", config.attempts),
            None => format!("{} kills", config.kills),
        },
        config.work_dir.display()
    );
    match run(&config, &adapter, &mut out) {
        Ok(_) => {
            if config.only.is_some() {
                let _ = writeln!(out, "did not reproduce in {} attempts", config.attempts);
            }
            Ok(ExitCode::SUCCESS)
        }
        Err(_) => Ok(ExitCode::from(1)),
    }
}

fn cmd_writer(args: &[String]) -> Result<ExitCode, String> {
    let flags = Flags { args, switches: &["--wal-sizes"] };
    flags.check(&["--mode", "--root", "--max-commits", "--hang-at"])?;
    let mode = mode(&flags)?;
    let root = PathBuf::from(flags.value("--root").ok_or("writer needs --root")?);
    #[allow(unused_mut)]
    let mut adapter = StoreAdapter::new(mode);
    if let Some(step) = flags.value("--hang-at") {
        #[cfg(feature = "fault-inject")]
        {
            let step: u64 = step.parse().map_err(|_| format!("--hang-at must be a step number, got {step:?}"))?;
            adapter.set_probe(std::sync::Arc::new(fid_crash::adapter::StepProbe::new(Some(step), None)));
        }
        #[cfg(not(feature = "fault-inject"))]
        return Err(format!("--hang-at {step} needs a build with the fault-inject feature"));
    }
    let wal = root.join(format!("{}-wal", fid_store::layout::DB_FILE));
    let wal_sizes = flags.has("--wal-sizes");
    let mut after = |seq: u64, out: &mut dyn Write| -> std::io::Result<()> {
        if wal_sizes {
            let len = std::fs::metadata(&wal).map_or(0, |m| m.len());
            writeln!(out, "WAL {seq} {len}")?;
            out.flush()?;
        }
        Ok(())
    };
    let max = flags.number("--max-commits", 1_000_000)?;
    let mut out = std::io::stdout();
    match run_writer(&mut adapter as &mut dyn CrashAdapter, &root, max, &mut out, &mut after) {
        Ok(()) => Ok(ExitCode::SUCCESS),
        Err(e) => {
            eprintln!("fid-crash writer: {e}");
            Ok(ExitCode::from(3))
        }
    }
}
