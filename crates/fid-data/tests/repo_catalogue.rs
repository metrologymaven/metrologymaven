//! The repository's own `datasets.toml` passes the admission check (spec node
//! ECO.DATA.CAT), with the domain list taken from the level-1 nodes of the spec tree.
//! Entries that point into this repository must match the committed files.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_data::cat::{check, parse_domains, read_bounded};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const REPO_BLOB: &str = "https://github.com/metrologymaven/metrologymaven/blob/main/";

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Level-1 domain IDs: `id:` of every `spec/<dir>/NODE.md` whose frontmatter says `level: 1`.
fn spec_domains() -> BTreeSet<String> {
    let mut list = String::new();
    for e in std::fs::read_dir(root().join("spec")).unwrap() {
        let node = e.unwrap().path().join("NODE.md");
        let Ok(text) = std::fs::read_to_string(&node) else { continue };
        let front: Vec<&str> = text.lines().skip(1).take_while(|l| l.trim() != "---").collect();
        let field = |k: &str| front.iter().find_map(|l| l.strip_prefix(k)).map(str::trim);
        if field("level:") == Some("1") {
            list.push_str(field("id:").unwrap());
            list.push('\n');
        }
    }
    let domains = parse_domains(&list).unwrap();
    assert!(domains.len() >= 20 && domains.contains("ECO") && domains.contains("KRN"), "{domains:?}");
    domains
}

#[test]
fn repository_catalogue_is_admitted() {
    let text = read_bounded(&root().join("datasets.toml")).unwrap();
    let findings = check(&text, &spec_domains());
    assert!(findings.is_empty(), "datasets.toml:\n{}", findings.iter().map(|f| format!("{f}\n")).collect::<String>());
}

/// The fixture domain list matches the spec tree, so AC-1 tests what CI would see.
#[test]
fn fixture_domain_list_matches_spec_tree() {
    let text = std::fs::read_to_string(root().join("fixtures/datasets/domains.txt")).unwrap();
    assert_eq!(parse_domains(&text).unwrap(), spec_domains());
}

/// Size and SHA-256 of in-repository entries match the committed files.
#[test]
fn repository_entries_match_files() {
    let text = std::fs::read_to_string(root().join("datasets.toml")).unwrap();
    let mut source: Option<String> = None;
    let mut size: Option<u64> = None;
    let mut checked = 0;
    for line in text.lines() {
        let val = |k: &str| line.strip_prefix(k).map(|v| v.trim().trim_matches('"').to_string());
        if let Some(v) = val("source = ") {
            source = Some(v);
        } else if let Some(v) = val("size_bytes = ") {
            size = Some(v.replace('_', "").parse().unwrap());
        } else if let Some(sha) = val("sha256 = ") {
            let src = source.take().unwrap();
            let Some(rel) = src.strip_prefix(REPO_BLOB) else { continue };
            let bytes = std::fs::read(root().join(rel)).unwrap();
            assert_eq!(Some(bytes.len() as u64), size.take(), "{rel}: size_bytes");
            assert_eq!(fid_testkit::sha256_hex(&bytes), format!("sha256:{sha}"), "{rel}: sha256");
            checked += 1;
        }
    }
    assert!(checked >= 8, "only {checked} in-repository entries checked");
}
