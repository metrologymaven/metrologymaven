//! REQ-TRUST.CHAIN.CANON-004: this crate shall not depend on any CORE crate.
//! The workspace layering check (`cargo xtask layers`) cannot express "no CORE
//! crate" (CORE crates sit in lower layers), so this test reads the manifest and
//! fails the build if a normal or build dependency names one. `fid-canon` is
//! allowed only under `[dev-dependencies]`, for the cross-check.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

/// First-party crates this crate may link: the shared vocabulary only.
const ALLOWED_FIRST_PARTY: [&str; 1] = ["fid-types"];

#[test]
fn no_core_crate_in_dependencies() {
    let manifest = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")).unwrap();
    let mut section = String::new();
    let mut checked = 0;
    for line in manifest.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            section = line.to_string();
            continue;
        }
        if line.starts_with('#') || line.is_empty() {
            continue;
        }
        let linking = section == "[dependencies]"
            || section == "[build-dependencies]"
            || (section.starts_with("[target.") && !section.ends_with("dev-dependencies]"));
        if !linking {
            continue;
        }
        let name = line.split(['=', '.']).next().unwrap_or("").trim().trim_matches('"');
        if name.starts_with("fid-") || name.starts_with("fid_") {
            assert!(ALLOWED_FIRST_PARTY.contains(&name), "{section} names first-party crate {name}");
        }
        assert!(!line.contains("fid-canon") && !line.contains("fid-journal") && !line.contains("fid-core"), "{line}");
        checked += 1;
    }
    assert!(checked >= 1, "no dependencies found: manifest layout changed?");
}
