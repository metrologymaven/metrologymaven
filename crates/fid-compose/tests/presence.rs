//! AC-6 of spec node AUTO.SURF.COMPOSE, the presence-provider scan (REQ-009): the workspace
//! passes; fixture `shipped/` fails naming `AutoYes` and `src/auto.rs`; fixture `tests_only/`
//! passes. The fixtures are hand-written (fixtures/auto/surf/compose/presence_scan/).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_compose::presence::{scan_crate, scan_text, scan_workspace, unlisted};
use fid_compose::PRESENCE_PROVIDERS;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fixture(name: &str) -> PathBuf {
    root().join("fixtures/auto/surf/compose/presence_scan").join(name)
}

#[test]
fn ac6_the_workspace_ships_no_unlisted_presence_provider() {
    let findings = scan_workspace(&root()).unwrap();
    assert_eq!(unlisted(&findings, PRESENCE_PROVIDERS), Vec::new(), "{findings:#?}");
}

#[test]
fn ac6_a_shipped_provider_is_named_with_its_file() {
    let found = unlisted(&scan_crate(&fixture("shipped")).unwrap(), PRESENCE_PROVIDERS);
    assert_eq!(found.len(), 1, "{found:#?}");
    assert_eq!(found[0].ty, "AutoYes");
    assert!(found[0].file.ends_with(Path::new("src").join("auto.rs")), "{}", found[0].file.display());
}

#[test]
fn ac6_providers_in_tests_and_cfg_test_modules_pass() {
    assert_eq!(scan_crate(&fixture("tests_only")).unwrap(), Vec::new());
}

/// The text rules: path prefixes and generics count; comments, strings and `#[cfg(test)]`
/// items do not; a macro whose body names the trait is reported.
#[test]
fn scan_text_rules() {
    assert_eq!(scan_text("impl fid_bus::principal::HumanPresence for a::b::Yes {}"), ["Yes"]);
    assert_eq!(scan_text("impl<T: Fn() -> u8> HumanPresence for Wrap<T> where T: Send {}"), ["Wrap"]);
    assert_eq!(scan_text("impl<'a> crate::HumanPresence for &'a Ref<'a> {}"), ["Ref"]);
    assert!(scan_text("impl Other for HumanPresence {}").is_empty());
    assert!(scan_text("/// impl HumanPresence for Doc {}\nfn f() {}").is_empty());
    assert!(scan_text(r##"const S: &str = r#"impl HumanPresence for Raw {}"#;"##).is_empty());
    assert!(scan_text("#[cfg(test)]\nimpl HumanPresence for T {}\nimpl HumanPresence for U {}") == ["U"]);
    assert!(scan_text("#[cfg(test)]\n#[allow(dead_code)]\nmod m { impl HumanPresence for T {} }").is_empty());
    assert_eq!(scan_text("macro_rules! yes { ($t:ty) => { impl HumanPresence for $t {} } }"), ["macro_rules! yes"]);
    assert_eq!(scan_text("fn f(c: char) -> bool { c == '{' }\nimpl HumanPresence for After {}"), ["After"]);
}
