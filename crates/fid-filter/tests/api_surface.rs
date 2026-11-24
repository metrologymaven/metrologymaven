#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
//! KRN.OUT.POL AC-4 (REQ-KRN.OUT.POL-006, REQ-KRN.OUT-004): enumerate the public functions of
//! `fid-fit` and `fid-filter` that take a `PointsView` and return a point set; only the
//! `fid-filter` policy entry point may return a reduced, reweighted or reordered one.
//!
//! The scan reads the crates' sources: every `pub fn` signature (from `pub fn` to the first
//! `{` or `;`) whose parameters mention `PointsView` and whose return type names a point-set
//! type. The expected list is written out below, so a new such function in either crate
//! fails this test until it is reviewed and listed here.

use std::path::{Path, PathBuf};

/// Return types that carry a set of points (as views, owned buffers or per-point weights).
const POINT_SET_TYPES: &[&str] =
    &["PointsView", "ReducedPoints", "Centred", "Vec<V3>", "Vec<[f64; 3]>", "Weights", "ExclusionMask"];

/// The functions found, as `crate/relative/path.rs::name`, and why each is allowed.
const ALLOWED: &[(&str, &str)] = &[
    // KRN.LSQ.TRAIT REQ-003 normalisation: a similarity transform of every point, same count
    // and order, with an exact inverse; it neither drops, reweights nor reorders points.
    ("fid-fit/src/contract/degen.rs::check_points", "normalisation (count and order preserved)"),
    ("fid-fit/src/contract/normalise.rs::new", "normalisation (count and order preserved)"),
    // The policy entry point of this leaf.
    ("fid-filter/src/policy/mod.rs::apply", "the policy entry point"),
];

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let mut entries: Vec<_> = std::fs::read_dir(dir).unwrap().map(|e| e.unwrap().path()).collect();
    entries.sort();
    for p in entries {
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|e| e == "rs") {
            out.push(p);
        }
    }
}

/// Every `pub fn` signature in `text`, with its name.
fn pub_fns(text: &str) -> Vec<(String, String)> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(at) = rest.find("pub fn ") {
        let before = rest[..at].chars().last();
        let start = &rest[at..];
        rest = &start[7..];
        // `pub(crate) fn` and friends never match "pub fn "; skip an identifier ending in "pub".
        if before.is_some_and(|c| c.is_alphanumeric() || c == '_') {
            continue;
        }
        let end = start.find(['{', ';']).unwrap_or(start.len());
        let sig = &start[..end];
        let name: String = sig[7..].chars().take_while(|c| c.is_alphanumeric() || *c == '_').collect();
        out.push((name, sig.to_string()));
    }
    out
}

fn returns_point_set(sig: &str) -> bool {
    let Some((params, ret)) = sig.split_once("->") else {
        return false;
    };
    params.contains("PointsView") && POINT_SET_TYPES.iter().any(|t| ret.contains(t))
}

#[test]
fn ac4_only_policy_entry_points_return_modified_point_sets() {
    let crates = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut found = Vec::new();
    let mut scanned = 0usize;
    for krate in ["fid-fit", "fid-filter"] {
        let mut files = Vec::new();
        rust_files(&crates.join(krate).join("src"), &mut files);
        assert!(!files.is_empty(), "{krate}: no sources");
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap();
            let rel = f.strip_prefix(crates).unwrap().to_string_lossy().replace('\\', "/");
            for (name, sig) in pub_fns(&text) {
                scanned += 1;
                if returns_point_set(&sig) {
                    found.push(format!("{rel}::{name}"));
                }
            }
        }
    }
    assert!(scanned > 50, "the scan found only {scanned} public functions");
    let mut expected: Vec<String> = ALLOWED.iter().map(|(f, _)| f.to_string()).collect();
    expected.sort();
    found.sort();
    assert_eq!(found, expected, "public functions taking a PointsView and returning a point set");
    // Of those, only fid-filter's may reduce, reweight or reorder.
    let reducing: Vec<&String> = found.iter().filter(|f| !f.starts_with("fid-fit/src/contract/")).collect();
    assert_eq!(reducing, vec!["fid-filter/src/policy/mod.rs::apply"]);
}

#[test]
fn scanner_sees_multi_line_signatures() {
    let src = "pub fn a(\n    x: &PointsView<'_>,\n) -> Result<ReducedPoints, E> {\n}\npub(crate) fn b(p: PointsView) -> PointsView;\npub fn c(p: &PointsView) -> f64 { 0.0 }\n";
    let fns = pub_fns(src);
    let names: Vec<&str> = fns.iter().map(|(n, _)| n.as_str()).collect();
    assert_eq!(names, ["a", "c"]);
    assert!(returns_point_set(&fns[0].1));
    assert!(!returns_point_set(&fns[1].1));
}
