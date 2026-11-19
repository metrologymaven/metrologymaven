//! `fid-platform plat-matrix validate <file>` (spec node `PLAT.MATRIX.FILE`) and
//! `fid-platform plat-hw validate [--file <path>]` (spec node `PLAT.HW.PROFILE`),
//! `fid-platform plat-hw validate-suite [--file <path>]` and
//! `fid-platform plat-hw result-schema` (spec node `PLAT.HW.SCHEMA`) and
//! `fid-platform plat-kiosk display [--file <path>]` (spec node `PLAT.DEPLOY.KIOSK.DISPLAY`) and
//! `fid-platform plat-gpu validate [--policy <file>] [--known-bad <file>] [--matrix <file>]
//! [--phase <P>]` (spec node `PLAT.GPU.POLICY`) and
//! `fid-platform plat-deploy validate-layout [--file <file>] [--matrix <file>] [--phase <P>]`
//! (spec node `PLAT.DEPLOY.NOADMIN.LAYOUT`) and
//! `fid-platform plat-matrix page [--check] [--matrix <file>] --out <file>` (spec node `PLAT.MATRIX.PAGE`)
//! and `fid-platform plat-deploy check-bundle --bundle <dir> --plan <file>`,
//! `fid-platform plat-deploy check-bundles --plan <file> --bundle <dir> [--bundle <dir> ...]`,
//! `fid-platform plat-deploy make-install-manifest --root <dir> --kind <per_user|machine> --release <v>
//! --os <os> --arch <arch> [--out <file>]` and `fid-platform plat-deploy bundle-schemas` (spec node
//! `PLAT.DEPLOY.OFFLINE.BUNDLE`).
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
//! `plat-deploy check-bundle` prints one finding per line (e.g. `MissingRole { role: cli }`) and
//! exits 1 when there are any, or prints `check-bundle: ok` and exits 0; bad usage or
//! an unreadable or invalid package plan exits 2. `plat-deploy check-bundles` checks every
//! bundle the same way and also requires one bundle per Tier 1 combination of the plan; each
//! finding is prefixed with its bundle directory (coverage findings have none); `check-bundles:
//! ok` and exit 0, or exit 1 on findings, 2 on bad usage. `plat-deploy make-install-manifest` writes the
//! canonical manifest of the tree to `--out` (or standard output) and exits 0; findings in the
//! tree (a symbolic link, a bad name) are printed and exit 1; bad usage exits 2.
//! `plat-deploy bundle-schemas` writes `platform/offline-bundle.schema.json` and
//! `platform/install-manifest.schema.json` (exit 0, or 2 if it cannot).

use fid_platform::{
    bench_schema, gpu_policy, hw_profile, install_manifest, kiosk_display, matrix, page, peruser_layout,
};
use std::io::{Read, Write};
use std::path::Path;
use std::process::ExitCode;

const USAGE: &str = "usage: fid-platform plat-matrix validate <file>\n       \
                     fid-platform plat-matrix page [--check] [--matrix <file>] --out <file>\n       \
                     fid-platform plat-hw validate [--file <path>]\n       \
                     fid-platform plat-hw validate-suite [--file <path>]\n       \
                     fid-platform plat-hw result-schema\n       \
                     fid-platform plat-kiosk display [--file <path>]\n       \
                     fid-platform plat-gpu validate [--policy <file>] [--known-bad <file>] [--matrix <file>] \
                     [--phase <P>]\n       \
                     fid-platform plat-deploy validate-layout [--file <file>] [--matrix <file>] [--phase <P>]\n       \
                     fid-platform plat-deploy check-bundle --bundle <dir> --plan <file>\n       \
                     fid-platform plat-deploy check-bundles --plan <file> --bundle <dir> [--bundle <dir> ...]\n       \
                     fid-platform plat-deploy make-install-manifest --root <dir> --kind <per_user|machine> \
                     --release <v> --os <os> --arch <arch> [--out <file>]\n       \
                     fid-platform plat-deploy bundle-schemas";

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
        [group, cmd, rest @ ..] if group == "plat-deploy" && cmd == "check-bundle" => check_bundle(rest),
        [group, cmd, rest @ ..] if group == "plat-deploy" && cmd == "check-bundles" => check_bundles(rest),
        [group, cmd, rest @ ..] if group == "plat-deploy" && cmd == "make-install-manifest" => make_manifest(rest),
        [group, cmd] if group == "plat-deploy" && cmd == "bundle-schemas" => bundle_schemas(),
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
                let Some(value) = it.next().filter(|v| !v.starts_with("--")) else {
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

/// Parses `--flag value` pairs: each flag at most once and only from `allowed`.
fn flag_pairs<'a>(
    args: &'a [String],
    allowed: &[&str],
    cmd: &str,
) -> Option<std::collections::BTreeMap<&'a str, &'a str>> {
    let mut out = std::collections::BTreeMap::new();
    for pair in args.chunks(2) {
        let [flag, value] = pair else {
            eprintln!("{cmd}: every flag needs a value\n{USAGE}");
            return None;
        };
        if !allowed.contains(&flag.as_str()) {
            eprintln!("{cmd}: unknown flag {flag}\n{USAGE}");
            return None;
        }
        if out.insert(flag.as_str(), value.as_str()).is_some() {
            eprintln!("{cmd}: {flag} given twice\n{USAGE}");
            return None;
        }
    }
    Some(out)
}

/// `plat-deploy check-bundle`: see the module documentation for the output and exit codes.
fn check_bundle(args: &[String]) -> ExitCode {
    let cmd = "plat-deploy check-bundle";
    let Some(flags) = flag_pairs(args, &["--bundle", "--plan"], cmd) else {
        return ExitCode::from(2);
    };
    let (Some(bundle), Some(plan_path)) = (flags.get("--bundle"), flags.get("--plan")) else {
        eprintln!("{cmd}: --bundle and --plan are required\n{USAGE}");
        return ExitCode::from(2);
    };
    let Some(plan_bytes) = read_capped(plan_path, install_manifest::MAX_BYTES, cmd) else {
        return ExitCode::from(2);
    };
    let plan = match install_manifest::parse_plan(&plan_bytes, plan_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{cmd}: {e}");
            return ExitCode::from(2);
        }
    };
    let findings = install_manifest::check_bundle(Path::new(bundle), &plan);
    if findings.is_empty() {
        println!("check-bundle: ok");
        return ExitCode::SUCCESS;
    }
    for f in &findings {
        println!("{f}");
    }
    ExitCode::from(1)
}

/// `plat-deploy make-install-manifest`: see the module documentation for the output and exit codes.
fn make_manifest(args: &[String]) -> ExitCode {
    let cmd = "plat-deploy make-install-manifest";
    let allowed = ["--root", "--kind", "--release", "--os", "--arch", "--out"];
    let Some(flags) = flag_pairs(args, &allowed, cmd) else {
        return ExitCode::from(2);
    };
    let (Some(root), Some(kind), Some(release), Some(os), Some(arch)) = (
        flags.get("--root"),
        flags.get("--kind").and_then(|k| install_manifest::InstallKind::from_name(k)),
        flags.get("--release"),
        flags.get("--os").and_then(|o| matrix::OsFamily::from_name(o)),
        flags.get("--arch").and_then(|a| matrix::Arch::from_name(a)),
    ) else {
        eprintln!("{cmd}: --root, --kind (per_user|machine), --release, --os and --arch are required\n{USAGE}");
        return ExitCode::from(2);
    };
    let manifest = match install_manifest::make_install_manifest(Path::new(root), kind, release, os, arch) {
        Ok(m) => m,
        Err(findings) => {
            for f in &findings {
                println!("{f}");
            }
            return ExitCode::from(1);
        }
    };
    let bytes = match manifest.to_json() {
        Ok(b) => b,
        Err(e) => {
            println!("{e}");
            return ExitCode::from(1);
        }
    };
    let written = match flags.get("--out") {
        Some(out) => write_atomically(Path::new(out), &bytes),
        None => std::io::stdout().write_all(&bytes),
    };
    match written {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("{cmd}: cannot write the manifest: {e}");
            ExitCode::from(2)
        }
    }
}

/// `plat-deploy bundle-schemas`: writes the two exported JSON Schemas.
fn bundle_schemas() -> ExitCode {
    let files = match install_manifest::json_schema_files() {
        Ok(f) => f,
        Err(e) => {
            eprintln!("plat-deploy bundle-schemas: cannot serialise: {}", e.code());
            return ExitCode::from(2);
        }
    };
    for (path, bytes) in files {
        if let Err(e) = std::fs::write(path, bytes) {
            eprintln!("plat-deploy bundle-schemas: cannot write {path}: {e}");
            return ExitCode::from(2);
        }
        println!("wrote {path}");
    }
    ExitCode::SUCCESS
}

/// Writes `bytes` to a temporary file next to `out` and renames it over `out`, so a reader never
/// sees a half-written manifest; the temporary file is removed on failure.
fn write_atomically(out: &Path, bytes: &[u8]) -> std::io::Result<()> {
    let name = out.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    let tmp = out.with_file_name(format!(".{name}.tmp-{}", std::process::id()));
    let result = std::fs::File::create(&tmp)
        .and_then(|mut f| f.write_all(bytes).and_then(|()| f.sync_all()))
        .and_then(|()| std::fs::rename(&tmp, out));
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// `plat-deploy check-bundles`: see the module documentation for the output and exit codes.
fn check_bundles(args: &[String]) -> ExitCode {
    let cmd = "plat-deploy check-bundles";
    let mut plan_path: Option<&str> = None;
    let mut dirs: Vec<std::path::PathBuf> = Vec::new();
    for pair in args.chunks(2) {
        match pair {
            [flag, value] if flag == "--bundle" => dirs.push(value.into()),
            [flag, value] if flag == "--plan" && plan_path.is_none() => plan_path = Some(value),
            _ => {
                eprintln!("{cmd}: bad arguments\n{USAGE}");
                return ExitCode::from(2);
            }
        }
    }
    let Some(plan_path) = plan_path else {
        eprintln!("{cmd}: --plan is required\n{USAGE}");
        return ExitCode::from(2);
    };
    let Some(plan_bytes) = read_capped(plan_path, install_manifest::MAX_BYTES, cmd) else {
        return ExitCode::from(2);
    };
    let plan = match install_manifest::parse_plan(&plan_bytes, plan_path) {
        Ok(p) => p,
        Err(e) => {
            eprintln!("{cmd}: {e}");
            return ExitCode::from(2);
        }
    };
    let findings = install_manifest::check_bundles(&dirs, &plan);
    if findings.is_empty() {
        println!("check-bundles: ok");
        return ExitCode::SUCCESS;
    }
    for f in &findings {
        println!("{f}");
    }
    ExitCode::from(1)
}
