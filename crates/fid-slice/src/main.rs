//! `fid-slice`: runnable CLI for Metrology Maven Vertical Slice 0.
//!
//! Runs the end-to-end deterministic Inspect + Report + Verify pipeline on either the
//! built-in synthetic aerospace bracket (`--demo`) or a user-supplied JSON job (`--input`),
//! or verifies an existing `.zip` Result Package (`--verify`).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::ExitCode;

use fid_canon::{to_canonical_bytes, CValue};
use fid_slice::{demo_job, outcome_str, run_slice, verify_package_bytes, JobSpec, SliceOutput};
use fid_surface::{Envelope, ErrorCode};

const CMD_RUN: &str = "slice.run_vertical@1";
const CMD_VERIFY: &str = "slice.verify_package@1";

const USAGE: &str = "\
fid-slice: Metrology Maven Vertical Slice 0 (Inspect + Report + Verify)

USAGE:
    fid-slice --demo [--out <dir>] [--json]
    fid-slice --input <job.json> [--out <dir>] [--json]
    fid-slice --verify <package.zip> [--json]
    fid-slice --help";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let want_json = args.iter().any(|a| a == "--json");
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        println!("{USAGE}");
        return ExitCode::SUCCESS;
    }

    let mut demo = false;
    let mut input: Option<PathBuf> = None;
    let mut verify: Option<PathBuf> = None;
    let mut out_dir: Option<PathBuf> = None;

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--demo" => {
                demo = true;
                i += 1;
            }
            "--json" => {
                i += 1;
            }
            "--input" => {
                let Some(p) = args.get(i + 1) else {
                    return fail(want_json, CMD_RUN, ErrorCode::UsageError, "--input needs a path");
                };
                input = Some(PathBuf::from(p));
                i += 2;
            }
            "--out" => {
                let Some(p) = args.get(i + 1) else {
                    return fail(want_json, CMD_RUN, ErrorCode::UsageError, "--out needs a directory");
                };
                out_dir = Some(PathBuf::from(p));
                i += 2;
            }
            "--verify" => {
                let Some(p) = args.get(i + 1) else {
                    return fail(want_json, CMD_VERIFY, ErrorCode::UsageError, "--verify needs a package path");
                };
                verify = Some(PathBuf::from(p));
                i += 2;
            }
            other => {
                return fail(want_json, CMD_RUN, ErrorCode::UsageError, &format!("unknown argument `{other}`"));
            }
        }
    }

    if let Some(pkg_path) = verify {
        return run_verify(&pkg_path, want_json);
    }

    let job = if let Some(path) = input {
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                return fail(want_json, CMD_RUN, ErrorCode::NotFound, &format!("reading {}: {e}", path.display()));
            }
        };
        match JobSpec::from_json(&bytes) {
            Ok(j) => j,
            Err(e) => return fail(want_json, CMD_RUN, ErrorCode::ValidationError, &e.to_string()),
        }
    } else if demo {
        demo_job()
    } else {
        return fail(want_json, CMD_RUN, ErrorCode::UsageError, "pass --demo, --input, or --verify");
    };

    let output = match run_slice(&job) {
        Ok(o) => o,
        Err(e) => return fail(want_json, CMD_RUN, ErrorCode::ValidationError, &e.to_string()),
    };

    if let Some(dir) = &out_dir {
        if let Err(msg) = write_outputs(dir, &output) {
            return fail(want_json, CMD_RUN, ErrorCode::PreconditionFailed, &msg);
        }
    }

    if want_json {
        let env = Envelope::computed(CMD_RUN, summary_cvalue(&output, out_dir.as_deref()));
        emit_envelope(&env);
    } else {
        print_human(&output, out_dir.as_deref());
    }
    ExitCode::SUCCESS
}

fn run_verify(path: &PathBuf, want_json: bool) -> ExitCode {
    let bytes = match std::fs::read(path) {
        Ok(b) => b,
        Err(e) => {
            return fail(want_json, CMD_VERIFY, ErrorCode::NotFound, &format!("reading {}: {e}", path.display()));
        }
    };
    match verify_package_bytes(&bytes) {
        Ok(members) => {
            let digest = fid_canon::digest_bytes(&bytes).to_string();
            if want_json {
                let mut obj = BTreeMap::new();
                obj.insert("members_verified".into(), CValue::Num(members as f64));
                obj.insert("package_digest".into(), CValue::Str(digest));
                obj.insert("status".into(), CValue::Str("VERIFIED".into()));
                emit_envelope(&Envelope::computed(CMD_VERIFY, CValue::Obj(obj)));
            } else {
                println!("VERIFIED  {} ({members} members, {digest})", path.display());
            }
            ExitCode::SUCCESS
        }
        Err(e) => fail(want_json, CMD_VERIFY, ErrorCode::ValidationError, &e.to_string()),
    }
}

fn write_outputs(dir: &PathBuf, output: &SliceOutput) -> Result<(), String> {
    std::fs::create_dir_all(dir).map_err(|e| format!("creating {}: {e}", dir.display()))?;
    std::fs::write(dir.join("result-package.zip"), &output.package_bytes)
        .map_err(|e| format!("writing result-package.zip: {e}"))?;
    std::fs::write(dir.join("form3.csv"), output.form3_csv.as_bytes())
        .map_err(|e| format!("writing form3.csv: {e}"))?;
    std::fs::write(dir.join("records.json"), &output.records_json).map_err(|e| format!("writing records.json: {e}"))?;
    Ok(())
}

fn summary_cvalue(o: &SliceOutput, out_dir: Option<&std::path::Path>) -> CValue {
    let chars = o
        .characteristics
        .iter()
        .map(|c| {
            let mut m = BTreeMap::new();
            m.insert("algorithm_id".into(), CValue::Str(c.algorithm_id.clone()));
            m.insert("balloon".into(), CValue::Num(f64::from(c.balloon)));
            m.insert("formatted_value".into(), CValue::Str(c.formatted_mm.clone()));
            m.insert("id".into(), CValue::Str(c.id.clone()));
            m.insert("label".into(), CValue::Str(c.label.clone()));
            m.insert("outcome".into(), CValue::Str(outcome_str(c.outcome).to_owned()));
            m.insert("requirement".into(), CValue::Str(c.requirement.clone()));
            m.insert("uec".into(), CValue::Str(c.uec.clone()));
            CValue::Obj(m)
        })
        .collect();
    let mut obj = BTreeMap::new();
    obj.insert("characteristics".into(), CValue::Arr(chars));
    obj.insert("journal_entries".into(), CValue::Num(o.journal.len() as f64));
    obj.insert("journal_head".into(), CValue::Str(o.journal_head.to_string()));
    obj.insert("package_bytes".into(), CValue::Num(o.package_bytes.len() as f64));
    obj.insert("package_digest".into(), CValue::Str(o.package_digest.to_string()));
    obj.insert("part_number".into(), CValue::Str(o.part_number.clone()));
    obj.insert("revision".into(), CValue::Str(o.revision.clone()));
    obj.insert("serial_number".into(), CValue::Str(o.serial_number.clone()));
    obj.insert("verified_members".into(), CValue::Num(o.verified_members as f64));
    if let Some(d) = out_dir {
        obj.insert("out_dir".into(), CValue::Str(d.display().to_string()));
    }
    CValue::Obj(obj)
}

fn print_human(o: &SliceOutput, out_dir: Option<&std::path::Path>) {
    println!("Metrology Maven Vertical Slice 0 — {} Rev {} ({})", o.part_number, o.revision, o.serial_number);
    println!(
        "3-2-1 DRF [A|B|C] origin (m): [{:.6}, {:.6}, {:.6}]",
        o.drf_origin_m[0], o.drf_origin_m[1], o.drf_origin_m[2]
    );
    println!();
    println!("{:<4} {:<28} {:<26} {:>12}  {:<12} UEC", "#", "Characteristic", "Requirement", "Measured", "Outcome");
    println!("{}", "-".repeat(94));
    for c in &o.characteristics {
        println!(
            "{:<4} {:<28} {:<26} {:>12}  {:<12} {}",
            c.balloon,
            c.label,
            c.requirement,
            c.formatted_mm,
            outcome_str(c.outcome),
            c.uec
        );
    }
    println!();
    println!("Journal head ({} entries): {}", o.journal.len(), o.journal_head);
    println!(
        "Result Package VERIFIED ({} members, {} bytes): {}",
        o.verified_members,
        o.package_bytes.len(),
        o.package_digest
    );
    if let Some(dir) = out_dir {
        println!("Wrote artifacts to {}", dir.display());
    }
}

fn emit_envelope(env: &Envelope) {
    if let Ok(buf) = to_canonical_bytes(&env.to_value()) {
        if let Ok(s) = String::from_utf8(buf) {
            println!("{s}");
        }
    }
}

fn fail(want_json: bool, cmd: &str, code: ErrorCode, msg: &str) -> ExitCode {
    if want_json {
        emit_envelope(&Envelope::failed(cmd, code, msg, None));
    } else {
        eprintln!("error: {msg}");
    }
    ExitCode::from(1)
}
