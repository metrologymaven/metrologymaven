//! Integration tests for spec node `PLAT.GPU.NOTICE` (AC-1..AC-5, REQ-001..005,
//! reduced/unknown fallback tiers, workaround-only notices, and bounded TOML parsing).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_gpu_policy::policy::{parse_known_bad, parse_policy, Backend, GpuOs, KnownBad, PolicyRow};
use fid_gpu_policy::Arch;
use fid_gpu_select::{
    build_notice, build_notice_for_tier, parse_adapters, parse_gpu_fallback, sanitize_untrusted, select, AdapterInfo,
    EffectKind, GpuFallback, NoticeId, TierName, MAX_FALLBACK_BYTES, NOTICE_LINK,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(root().join("fixtures/plat/gpu").join(name)).unwrap()
}

fn windows_row() -> PolicyRow {
    parse_policy(&read("policy-valid.toml")).unwrap().row(GpuOs::Windows, Arch::X86_64).unwrap().clone()
}

fn no_known_bad() -> KnownBad {
    parse_known_bad(b"schema_version = 1\n").unwrap()
}

fn adapters(name: &str) -> Vec<AdapterInfo> {
    parse_adapters(&read(name)).unwrap()
}

fn fallback_tiers() -> GpuFallback {
    parse_gpu_fallback(&read("fallback-valid.toml")).unwrap()
}

#[test]
fn ac1_software_fallback_matches_golden_notice_and_lists_disabled_features() {
    let sel = select(&adapters("adapters-vm-none.json"), &[], &windows_row(), &no_known_bad(), None);
    let tiers = fallback_tiers();

    let notice = build_notice(&sel, &tiers).expect("software fallback must build a notice");
    assert_eq!(notice.id, NoticeId::FallbackSoftware);
    assert_eq!(notice.tier, Some(TierName::Software));
    assert!(notice.collapsible, "REQ-003: collapsible must be true");
    assert!(!notice.dismissible, "REQ-003: dismissible must be false");
    assert_eq!(notice.link, NOTICE_LINK);
    assert!(notice.features.iter().any(|f| f.id == "shadows" && f.effect == EffectKind::Disabled));
    assert!(notice
        .features
        .iter()
        .any(|f| f.id == "order_independent_transparency" && f.effect == EffectKind::Disabled));

    let got_json = notice.to_json().expect("canonical json");
    let expected_json = read("notice-software.json");
    assert_eq!(got_json, expected_json, "notice JSON must match fixtures/plat/gpu/notice-software.json byte-for-byte");
}

#[test]
fn ac2_avoid_backend_entry_builds_workaround_notice_without_fallback_tier() {
    let kb = parse_known_bad(&read("known-bad-detect.toml")).unwrap();
    let sel = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &kb, None);
    assert_eq!(sel.backend, Backend::Vulkan);
    assert!(!sel.fallback);
    assert_eq!(sel.known_bad, ["detect-avoid-dx12"]);

    let notice = build_notice(&sel, &fallback_tiers()).expect("known-bad entry must produce a notice");
    assert_eq!(notice.id, NoticeId::Workaround);
    assert_eq!(notice.tier, None);
    assert!(notice.collapsible);
    assert!(!notice.dismissible);
    assert_eq!(notice.entries, ["detect-avoid-dx12"]);
    assert!(notice.text.body.contains("detect-avoid-dx12"));
    assert!(!notice.text.title.to_ascii_lowercase().contains("fallback"));
    assert!(!notice.text.body.to_ascii_lowercase().contains("fallback tier"));

    // Also test a `workaround:<name>` entry on a hardware backend (REQ-002)
    let kb_wa = parse_known_bad(&read("known-bad-detect-force.toml")).unwrap();
    let sel_wa = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &kb_wa, None);
    assert!(!sel_wa.fallback);
    assert_eq!(sel_wa.workarounds, ["no_msaa"]);
    let notice_wa = build_notice(&sel_wa, &fallback_tiers()).expect("workaround must produce a notice");
    assert_eq!(notice_wa.id, NoticeId::Workaround);
    assert_eq!(notice_wa.entries, ["detect-workaround-dx12"]);
    assert_eq!(notice_wa.workarounds, ["no_msaa"]);
    assert!(notice_wa.text.body.contains("no_msaa"));
    assert!(notice_wa.text.body.contains("detect-workaround-dx12"));
}

#[test]
fn ac3_clean_hardware_selection_returns_none() {
    let sel = select(&adapters("adapters-win-discrete.json"), &[], &windows_row(), &no_known_bad(), None);
    assert!(!sel.fallback);
    assert!(sel.known_bad.is_empty());
    assert!(sel.workarounds.is_empty());

    assert_eq!(build_notice(&sel, &fallback_tiers()), None);
}

#[test]
fn ac4_ansi_escape_and_newline_in_adapter_name_are_stripped() {
    let mut hostile = adapters("adapters-vm-none.json");
    hostile[0].name = "Evil\x1b[31mRed\x1b[0m\nAdapter\r\t\x07Name".to_string();
    let sel = select(&hostile, &[], &windows_row(), &no_known_bad(), None);

    let notice = build_notice(&sel, &fallback_tiers()).expect("software notice");
    let json_bytes = notice.to_json().expect("json");
    let json_str = std::str::from_utf8(&json_bytes).expect("utf8");

    assert!(
        !notice.reason.bytes().any(|b| b < 0x20 || b == 0x7f),
        "reason must contain no control bytes: {:?}",
        notice.reason
    );
    assert!(
        !notice.text.body.bytes().any(|b| b < 0x20 || b == 0x7f),
        "body must contain no control bytes: {:?}",
        notice.text.body
    );
    assert!(!notice.text.body.contains("[31m"), "ANSI CSI payload must be stripped");
    assert!(!json_str.contains("\\u001b"));
    assert!(!json_str.contains("\\n"));
    assert_eq!(sanitize_untrusted("A\x1b[1;32mB\x1b[0m\nC"), "AB C");
}

#[test]
fn ac5_deterministic_byte_identical_serialization_and_unknown_tier() {
    let sel = select(&adapters("adapters-vm-none.json"), &[], &windows_row(), &no_known_bad(), None);
    let tiers = fallback_tiers();
    let b1 = build_notice(&sel, &tiers).unwrap().to_json().unwrap();
    let b2 = build_notice(&sel, &tiers).unwrap().to_json().unwrap();
    assert_eq!(b1, b2);

    // Reduced tier notice
    let reduced = build_notice_for_tier(&sel, Some(TierName::Reduced), &tiers).expect("reduced notice");
    assert_eq!(reduced.id, NoticeId::FallbackReduced);
    assert_eq!(reduced.tier, Some(TierName::Reduced));
    assert_eq!(reduced.features.len(), 3);

    // Missing tier in fallback table -> generic notice with tier `unknown` (Design)
    let empty_tiers = GpuFallback { schema_version: 1, tiers: BTreeMap::new() };
    let unknown = build_notice(&sel, &empty_tiers).expect("unknown tier still produces notice");
    assert_eq!(unknown.tier, Some(TierName::Unknown));
    assert!(unknown.features.is_empty());

    // Bounded TOML parser rejects oversized input without panicking
    let huge = vec![b' '; MAX_FALLBACK_BYTES + 1];
    assert!(parse_gpu_fallback(&huge).is_err());
}
