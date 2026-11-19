//! `fid-platform plat-matrix validate <file>` (spec node `PLAT.MATRIX.FILE`) and
//! `fid-platform plat-hw validate [--file <path>]` (spec node `PLAT.HW.PROFILE`),
//! `fid-platform plat-hw validate-suite [--file <path>]` and
//! `fid-platform plat-hw result-schema` (spec node `PLAT.HW.SCHEMA`) and
//! `fid-platform plat-kiosk display [--file <path>]` (spec node `PLAT.DEPLOY.KIOSK.DISPLAY`) and
//! `fid-platform plat-gpu validate [--policy <file>] [--known-bad <file>] [--matrix <file>]
//! [--phase <P>]` (spec node `PLAT.GPU.POLICY`) and
//! `fid-platform plat-deploy validate-layout [--file <file>] [--matrix <file>] [--phase <P>]`
//! (spec node `PLAT.DEPLOY.NOADMIN.LAYOUT`) and
//! `fid-platform plat-matrix page [--check] [--matrix <file>] --out <file>` (spec node `PLAT.MATRIX.PAGE`).
//! `cargo xtask plat-matrix ...`, `cargo xtask plat-hw ...`, `cargo xtask plat-gpu validate` and
//! `cargo xtask plat-deploy validate-layout` run this binary.
//!
//! `plat-matrix validate` prints one diagnostic per line and then `<rows> rows, <errors> errors`.
//! `plat-hw validate` prints the profile names, one per line, when the file is valid, and
//! one diagnostic per line otherwise. `plat-hw validate-suite` does the same for a benchmark
//! suite manifest, printing workload IDs. All three exit 0 when there are no errors, 1 when
//! there are, and 2 on bad usage or an unreadable file. `plat-hw result-schema` writes the
//! result JSON Schema to `platform/bench-result.schema.json` (exit 0, or 2 if it cannot).
//! `plat-kiosk display` prints `px_per_mm = <value to 3 decimals>` when the reference kiosk
//! display file is valid (exit 0) and one diagnostic per line otherwise (exit 1); 2 on bad
//! usage or an unreadable file.
//! `plat-gpu validate` checks the policy file (including the support-matrix cross-check at the
//! given phase, default P1a) and the known-bad list. It prints any support-matrix diagnostics
//! prefixed `matrix: `, then the policy diagnostics and `policy: <rows> rows, <errors> errors`,
//! then the known-bad diagnostics and `known-bad: <entries> entries, <errors> errors`. Exit 0
//! when there are no errors, 1 when there are, 2 on bad usage or an unreadable file.
//! `plat-deploy validate-layout` checks the per-user layout file (default
//! `platform/peruser-layout.toml`) and that every OS family with a Tier 1 row effective at the
//! given phase (default P1a) of the support matrix (default `platform/support-matrix.toml`) has
//! a layout. It prints any support-matrix diagnostics prefixed `matrix: `, then the layout
//! diagnostics and `layout: <families> os, <errors> errors`. Exit 0 when there are no errors, 1
//! when there are, 2 on bad usage or an unreadable file.
//! `plat-matrix page` validates the matrix (default `platform/support-matrix.toml`) and writes
//! the generated page to `--out` (exit 0). With `--check` it writes nothing: it prints a unified
//! diff and exits 1 when the file at `--out` differs from the generated page, and exits 0 when
//! they are equal. A matrix with validation errors prints them to stderr, writes nothing and
//! exits 1; bad usage or an unreadable or unwritable file exits 2.

use fid_platform::{bench_schema, gpu_policy, hw_profile, kiosk_display, matrix, page, peruser_layout};
use std::io::Read;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-platform plat-matrix validate <file>\n       \
                     fid-platform plat-matrix page [--check] [--matrix <file>] --out <file>\n       \
                     fid-platform plat-hw validate [--file <path>]\n       \
                     fid-platform plat-hw validate-suite [--file <path>]\n       \
                     fid-platform plat-hw result-schema\n       \
                     fid-platform plat-kiosk display [--file <path>]\n       \
                     fid-platform plat-gpu validate [--policy <file>] [--known-bad <file>] [--matrix <file>] \
                     [--phase <P>]\n       \
                     fid-platform plat-deploy validate-layout [--file <file>] [--matrix <file>] [--phase <P>]";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.as_slice() {
        [group, cmd, file] if group == "plat-matrix" && cmd == "validate" => validate(file),
        [group, cmd, rest @ ..] if group == "plat-matrix" && cmd == "page" => page_cmd(rest),
        [group, cmd] if group == "plat-hw" && cmd == "validate" => validate_hw(hw_profile::DEFAULT_PATH),
        [group, cmd, flag, file] if group == "plat-hw" && cmd == "validate" && flag == "--file" => validate_hw(file),
        [group, cmd] if group == "plat-hw" && cmd == "validate-suite" => {
            validate_suite(bench_schema::SUITE_DEFAULT_PATH)
        }
        [group, cmd, flag, file] if group == "plat-hw" && cmd == "validate-suite" && flag == "--file" => {
            validate_suite(file)
        }
        [group, cmd] if group == "plat-hw" && cmd == "result-schema" => result_schema(),
        [group, cmd] if group == "plat-kiosk" && cmd == "display" => kiosk(kiosk_display::DEFAULT_PATH),
        [group, cmd, flag, file] if group == "plat-kiosk" && cmd == "display" && flag == "--file" => kiosk(file),
        [group, cmd, rest @ ..] if group == "plat-gpu" && cmd == "validate" => gpu_validate(rest),
        [group, cmd, rest @ ..] if group == "plat-deploy" && cmd == "validate-layout" => layout_validate(rest),
        _ => {
            eprintln!("{USAGE}");
            ExitCode::from(2)
        }
    }
}

/// Reads at most `max + 1` bytes so an oversized file is reported, not loaded.
fn read_capped(path: &str, max: usize, cmd: &str) -> Option<Vec<u8>> {
    let mut bytes = Vec::new();
    let read = std::fs::File::open(path).and_then(|f| f.take(max as u64 + 1).read_to_end(&mut bytes));
    match read {
        Ok(_) => Some(bytes),
        Err(e) => {
            eprintln!("{cmd}: cannot read {path}: {e}");
            None
        }
    }
}

fn validate(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, matrix::MAX_BYTES, "plat-matrix validate") else {
        return ExitCode::from(2);
    };
    let v = matrix::validate(&bytes);
    print!("{}", v.render());
    if v.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

fn validate_hw(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, hw_profile::MAX_BYTES, "plat-hw validate") else {
        return ExitCode::from(2);
    };
    match hw_profile::validate(&bytes) {
        Ok(p) => {
            for name in p.profiles.keys() {
                println!("{name}");
            }
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in errors {
                println!("{e}");
            }
            ExitCode::from(1)
        }
    }
}

fn validate_suite(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, bench_schema::MAX_BYTES, "plat-hw validate-suite") else {
        return ExitCode::from(2);
    };
    match bench_schema::validate_suite(&bytes) {
        Ok(suite) => {
            for w in &suite.workloads {
                println!("{}", w.id);
            }
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in errors {
                println!("{e}");
            }
            ExitCode::from(1)
        }
    }
}

fn kiosk(path: &str) -> ExitCode {
    let Some(bytes) = read_capped(path, kiosk_display::MAX_BYTES, "plat-kiosk display") else {
        return ExitCode::from(2);
    };
    match kiosk_display::validate(&bytes) {
        Ok(d) => {
            println!("px_per_mm = {}", d.px_per_mm_text());
            ExitCode::SUCCESS
        }
        Err(errors) => {
            for e in errors {
                println!("{e}");
            }
            ExitCode::from(1)
        }
    }
}

fn result_schema() -> ExitCode {
    let path = bench_schema::RESULT_SCHEMA_PATH;
    let written = bench_schema::result_json_schema_bytes()
        .map_err(|e| e.code().to_string())
        .and_then(|bytes| std::fs::write(path, bytes).map_err(|e| e.to_string()));
    match written {
        Ok(()) => {
            println!("wrote {path}");
            ExitCode::SUCCESS
        }
        Err(e) => {
            eprintln!("plat-hw result-schema: cannot write {path}: {e}");
            ExitCode::from(2)
        }
    }
}

/// `plat-gpu validate`: see the module documentation for the output and exit codes.
fn gpu_validate(args: &[String]) -> ExitCode {
    let mut policy = gpu_policy::POLICY_PATH.to_string();
    let mut known_bad = gpu_policy::KNOWN_BAD_PATH.to_string();
    let mut matrix_path = gpu_policy::MATRIX_PATH.to_string();
    let mut phase = gpu_policy::DEFAULT_PHASE;
    let mut seen: Vec<&str> = Vec::new();
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        if seen.contains(&flag.as_str()) {
            eprintln!("plat-gpu validate: {flag} given twice\n{USAGE}");
            return ExitCode::from(2);
        }
        seen.push(flag.as_str());
        match flag.as_str() {
            "--policy" => policy.clone_from(value),
            "--known-bad" => known_bad.clone_from(value),
            "--matrix" => matrix_path.clone_from(value),
            "--phase" => match matrix::Phase::from_name(value) {
                Some(p) => phase = p,
                None => {
                    eprintln!("plat-gpu validate: {value} is not a phase code (P0, P1a, P1b, P2, P3, P4, later)");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let cmd = "plat-gpu validate";
    let (Some(policy_bytes), Some(bad_bytes), Some(matrix_bytes)) = (
        read_capped(&policy, gpu_policy::MAX_BYTES, cmd),
        read_capped(&known_bad, gpu_policy::MAX_BYTES, cmd),
        read_capped(&matrix_path, matrix::MAX_BYTES, cmd),
    ) else {
        return ExitCode::from(2);
    };

    let m = matrix::validate(&matrix_bytes);
    for e in &m.errors {
        println!("matrix: {e}");
    }
    let mut p = gpu_policy::validate_policy(&policy_bytes);
    if let (Some(pol), Some(mat)) = (&p.value, &m.matrix) {
        let missing = gpu_policy::check_coverage(pol, mat, phase);
        if !missing.is_empty() {
            p.errors.extend(missing);
            p.value = None;
        }
    }
    print!("{}", p.render("policy", "rows"));
    let k = gpu_policy::validate_known_bad(&bad_bytes);
    print!("{}", k.render("known-bad", "entries"));
    if m.errors.is_empty() && p.errors.is_empty() && k.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `plat-deploy validate-layout`: see the module documentation for the output and exit codes.
fn layout_validate(args: &[String]) -> ExitCode {
    let cmd = "plat-deploy validate-layout";
    let mut file = peruser_layout::DEFAULT_PATH.to_string();
    let mut matrix_path = peruser_layout::MATRIX_PATH.to_string();
    let mut phase = peruser_layout::DEFAULT_PHASE;
    let mut seen: Vec<&str> = Vec::new();
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("{USAGE}");
            return ExitCode::from(2);
        };
        if seen.contains(&flag.as_str()) {
            eprintln!("{cmd}: {flag} given twice\n{USAGE}");
            return ExitCode::from(2);
        }
        seen.push(flag.as_str());
        match flag.as_str() {
            "--file" => file.clone_from(value),
            "--matrix" => matrix_path.clone_from(value),
            "--phase" => match matrix::Phase::from_name(value) {
                Some(p) => phase = p,
                None => {
                    eprintln!("{cmd}: {value} is not a phase code (P0, P1a, P1b, P2, P3, P4, later)");
                    return ExitCode::from(2);
                }
            },
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let (Some(layout_bytes), Some(matrix_bytes)) =
        (read_capped(&file, peruser_layout::MAX_BYTES, cmd), read_capped(&matrix_path, matrix::MAX_BYTES, cmd))
    else {
        return ExitCode::from(2);
    };
    let m = matrix::validate(&matrix_bytes);
    for e in &m.errors {
        println!("matrix: {e}");
    }
    let mut v = peruser_layout::validate(&layout_bytes);
    if let (Some(layout), Some(mat)) = (&v.layout, &m.matrix) {
        let missing = peruser_layout::check_coverage(layout, mat, phase);
        if !missing.is_empty() {
            v.errors.extend(missing);
            v.layout = None;
        }
    }
    print!("{}", v.render());
    if m.errors.is_empty() && v.errors.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::from(1)
    }
}

/// `plat-matrix page`: see the module documentation for the output and exit codes.
fn page_cmd(args: &[String]) -> ExitCode {
    let cmd = "plat-matrix page";
    let mut check = false;
    let mut matrix_path = page::DEFAULT_MATRIX.to_string();
    let mut out: Option<String> = None;
    let mut seen: Vec<&str> = Vec::new();
    let mut it = args.iter();
    while let Some(flag) = it.next() {
        if seen.contains(&flag.as_str()) {
            eprintln!("{cmd}: {flag} given twice\n{USAGE}");
            return ExitCode::from(2);
        }
        seen.push(flag.as_str());
        match flag.as_str() {
            "--check" => check = true,
            "--matrix" | "--out" => {
                let Some(value) = it.next() else {
                    eprintln!("{cmd}: {flag} needs a value\n{USAGE}");
                    return ExitCode::from(2);
                };
                if flag == "--matrix" {
                    matrix_path.clone_from(value);
                } else {
                    out = Some(value.clone());
                }
            }
            _ => {
                eprintln!("{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(out) = out else {
        eprintln!("{cmd}: --out is required\n{USAGE}");
        return ExitCode::from(2);
    };
    let Some(bytes) = read_capped(&matrix_path, matrix::MAX_BYTES, cmd) else {
        return ExitCode::from(2);
    };
    let generated = match page::render(&bytes) {
        Ok(p) => p,
        Err(v) => {
            eprint!("{}", v.render());
            return ExitCode::from(1);
        }
    };
    if !check {
        return match std::fs::write(&out, generated.as_bytes()) {
            Ok(()) => {
                println!("wrote {out}");
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("{cmd}: cannot write {out}: {e}");
                ExitCode::from(2)
            }
        };
    }
    let Some(committed) = read_capped(&out, page::MAX_PAGE_BYTES, cmd) else {
        return ExitCode::from(2);
    };
    if committed.len() > page::MAX_PAGE_BYTES {
        println!("{cmd}: {out} is larger than {} bytes and differs from the generated page", page::MAX_PAGE_BYTES);
        return ExitCode::from(1);
    }
    match page::check(&committed, &generated, &out) {
        None => {
            println!("{out} is up to date");
            ExitCode::SUCCESS
        }
        Some(diff) => {
            print!("{diff}");
            ExitCode::from(1)
        }
    }
}
