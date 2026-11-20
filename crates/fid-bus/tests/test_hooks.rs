//! The `permit-test-hooks` feature of `fid-bus` mints network permits outside the pipeline and
//! builds egress destinations that do not come from the admin setting (CORE.BUS.POL.EGRESS,
//! maintainer decision on #336). Only `[dev-dependencies]` may enable it. This test reads every
//! manifest of the workspace and fails if any other section names the feature: a
//! `[dependencies]` or `[build-dependencies]` entry (also target-specific or in table form), a
//! `[workspace.dependencies]` entry, or a `[features]` entry of another crate forwarding it.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use std::path::{Path, PathBuf};

const FEATURE: &str = "permit-test-hooks";

/// The table a TOML line opens, without brackets (`[[bin]]` gives `bin`).
fn header(line: &str) -> Option<String> {
    let t = line.trim();
    if !t.starts_with('[') {
        return None;
    }
    let inner = t.trim_start_matches('[');
    let end = inner.find(']')?;
    Some(inner[..end].trim().replace([' ', '"', '\''], ""))
}

fn is_dev_section(section: &str) -> bool {
    // `dev-dependencies`, `dev-dependencies.fid-bus`,
    // `target.cfg(unix).dev-dependencies` and `target.cfg(unix).dev-dependencies.fid-bus`.
    section == "dev-dependencies"
        || section.starts_with("dev-dependencies.")
        || (section.starts_with("target.") && section.contains(".dev-dependencies"))
}

/// The lines of `text` that name the feature outside a dev-dependency table. `own` is true
/// for fid-bus's manifest, whose `[features]` table declares it.
fn violations(text: &str, own: bool) -> Vec<String> {
    let mut section = String::new();
    let mut out = Vec::new();
    for (i, line) in text.lines().enumerate() {
        if let Some(h) = header(line) {
            section = h;
            continue;
        }
        let code = line.split('#').next().unwrap_or("");
        if !code.contains(FEATURE) {
            continue;
        }
        let declaration = own && section == "features" && code.trim_start().starts_with(FEATURE);
        if !(is_dev_section(&section) || declaration) {
            out.push(format!("line {}: [{section}] {}", i + 1, line.trim()));
        }
    }
    out
}

fn manifests(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        let name = p.file_name().unwrap().to_string_lossy().to_string();
        if p.is_dir() {
            if name != "target" && !name.starts_with('.') {
                manifests(&p, out);
            }
        } else if name == "Cargo.toml" {
            out.push(p);
        }
    }
}

#[test]
fn only_dev_dependencies_enable_the_permit_test_hooks() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let mut files = vec![root.join("Cargo.toml")];
    manifests(&root.join("crates"), &mut files);
    manifests(&root.join("vendor"), &mut files);
    assert!(files.len() > 50, "found only {} manifests", files.len());
    let mut bad = Vec::new();
    let mut seen_dev_use = 0;
    for f in &files {
        let text = std::fs::read_to_string(f).unwrap();
        let own = f.parent().and_then(|d| d.file_name()).is_some_and(|n| n == "fid-bus");
        for v in violations(&text, own) {
            bad.push(format!("{}: {v}", f.display()));
        }
        seen_dev_use += text.lines().filter(|l| l.contains(FEATURE) && l.contains("features")).count();
    }
    assert!(bad.is_empty(), "{FEATURE} enabled outside [dev-dependencies]:\n{}", bad.join("\n"));
    assert!(seen_dev_use >= 2, "the scan saw the known dev-dependency uses");
}

#[test]
fn the_scan_catches_every_non_dev_form() {
    let bad = [
        "[dependencies]\nfid-bus = { path = \"../fid-bus\", features = [\"permit-test-hooks\"] }\n",
        "[dependencies.fid-bus]\npath = \"../fid-bus\"\nfeatures = [\"permit-test-hooks\"]\n",
        "[target.'cfg(unix)'.dependencies]\nfid-bus = { path = \"x\", features = [\"permit-test-hooks\"] }\n",
        "[build-dependencies]\nfid-bus = { path = \"x\", features = [\"permit-test-hooks\"] }\n",
        "[workspace.dependencies]\nfid-bus = { path = \"x\", features = [\"permit-test-hooks\"] }\n",
        "[features]\ndefault = [\"fid-bus/permit-test-hooks\"]\n",
        "[features]\nhooks = [\"fid-bus/permit-test-hooks\"]\n",
    ];
    for text in bad {
        assert_eq!(violations(text, false).len(), 1, "{text}");
    }
    let good = [
        "[dev-dependencies]\nfid-bus = { path = \"x\", features = [\"permit-test-hooks\"] }\n",
        "[dev-dependencies.fid-bus]\nfeatures = [\"permit-test-hooks\"]\n",
        "[target.'cfg(unix)'.dev-dependencies]\nfid-bus = { path = \"x\", features = [\"permit-test-hooks\"] }\n",
        "[dependencies]\n# a comment naming permit-test-hooks\nfid-bus = { path = \"x\" }\n",
    ];
    for text in good {
        assert!(violations(text, false).is_empty(), "{text}");
    }
    assert!(violations("[features]\npermit-test-hooks = []\n", true).is_empty());
    assert_eq!(violations("[features]\npermit-test-hooks = []\n", false).len(), 1);
    assert_eq!(violations("[features]\ndefault = [\"permit-test-hooks\"]\n", true).len(), 1);
}
