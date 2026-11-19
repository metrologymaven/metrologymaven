//! Acceptance criteria of spec node GDT.RES.RESOLVE through the command layer
//! (`gdt.resolve_chain@1`, `gdt.get_dmt_row@1`): the governing-spec gate runs first, the
//! characteristic's dialect, editions and defaults profile come from its governing spec, and
//! the printed JSON equals the bytes of the independent stdlib-Python reference
//! `fixtures/gdt/resolve/make_cases.py`. Error paths are hand-labelled from the leaf's rules.
//! Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::dprof::{parse_profile, Branch, DprofState, KeyVocabulary, ProfileRef};
use fid_gdt::chars::spec::{load_project, project_bytes, SetRequest, SpecState};
use fid_gdt::dmt_flow::roster::Dialect;
use fid_gdt::resolve::{
    get_dmt_row, resolve_chain, CharacteristicRef, CommandError, GET_DMT_ROW_COMMAND, RESOLVE_CHAIN_COMMAND,
};
use fid_gps::dmt::DmtTable;
use fid_gps::resolve::{Resolution, VariantSource};
use fid_io_reg::Principal;
use fid_types::Digest;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

const H: Principal = Principal::Human;
const A: Principal = Principal::Agent;
const ASME_STD: &str = "std:asme:y14.5:2018";

fn dir(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt").join(sub)
}

fn read(sub: &str, name: &str) -> Vec<u8> {
    std::fs::read(dir(sub).join(name)).unwrap()
}

fn expected(name: &str) -> String {
    String::from_utf8(read("resolve", &format!("expected/{name}.json"))).unwrap()
}

fn table() -> DmtTable {
    DmtTable::load(&read("dmt", "synthetic_table_v1.json")).unwrap()
}

/// The explicit test vocabulary of GDT.CHAR.DPROF the spec fixtures were published under.
fn test_vocab() -> KeyVocabulary {
    let v = fid_canon::parse(&read("dprof", "vocabulary.json")).unwrap();
    let CValue::Obj(o) = v else { panic!("vocabulary") };
    let Some(CValue::Arr(keys)) = o.get("keys") else { panic!("keys") };
    let keys: Vec<&str> = keys
        .iter()
        .map(|k| match k {
            CValue::Str(s) => s.as_str(),
            _ => panic!("key"),
        })
        .collect();
    KeyVocabulary::new(keys).unwrap()
}

/// The DMT schema's own vocabulary for ASME profiles over the synthetic table.
fn dmt_vocab() -> KeyVocabulary {
    KeyVocabulary::from_dmt_table(&table(), Dialect::Asme).unwrap()
}

/// plan_asme.fidproj (from the GDT.CHAR.SPEC reference) with `flat-1` and `pos-1` created by a
/// human in `plan-1`: ASME Y14.5-2018 and the ASME 1.0.0 placeholder profile.
fn project() -> (SpecState, DprofState) {
    let (mut st, mut dp) = load_project(&read("spec", "plan_asme.fidproj"), &test_vocab()).unwrap();
    st.create_characteristic(&mut dp, H, "plan-1", "flat-1", false).unwrap();
    st.create_characteristic(&mut dp, H, "plan-1", "pos-1", false).unwrap();
    (st, dp)
}

fn char_doc(char: &str, characteristic: &str, mods: &[&str], over: Option<&str>) -> CharacteristicRef {
    let mut m = BTreeMap::new();
    m.insert("schema".to_owned(), CValue::Str("fid.gdt-resolve-char/1".to_owned()));
    m.insert("char".to_owned(), CValue::Str(char.to_owned()));
    m.insert("characteristic".to_owned(), CValue::Str(characteristic.to_owned()));
    m.insert("modifiers".to_owned(), CValue::Arr(mods.iter().map(|s| CValue::Str((*s).to_owned())).collect()));
    m.insert("override".to_owned(), over.map_or(CValue::Null, |o| CValue::Str(o.to_owned())));
    CharacteristicRef::parse(&fid_canon::to_canonical_bytes(&CValue::Obj(m)).unwrap()).unwrap()
}

fn printed(r: &Result<Resolution, CommandError>) -> String {
    match r {
        Ok(r) => String::from_utf8(r.canonical_bytes().unwrap()).unwrap(),
        Err(e) => format!("{{\"error\":\"{}\"}}", e.code()),
    }
}

/// Publishes `profile_position_axis.json` (checked against the DMT vocabulary) on `p`'s branch
/// and points `pos-1`'s governing spec at it there.
fn point_pos_at_axis_profile(st: &mut SpecState, dp: &mut DprofState, p: Principal) -> ProfileRef {
    let doc = parse_profile(&read("resolve", "profile_position_axis.json"), &dmt_vocab()).unwrap();
    let r = dp.publish(doc, p, "root://imports/profile_position_axis.json", false).unwrap().reference().clone();
    let req = SetRequest { char: "pos-1", dialect: "ASME", standards: &[ASME_STD], defaults_profile: &r.to_string() };
    st.set_governing_spec(dp, p, &req, false).unwrap();
    r
}

#[test]
fn command_ids() {
    assert_eq!(RESOLVE_CHAIN_COMMAND, "gdt.resolve_chain@1");
    assert_eq!(GET_DMT_ROW_COMMAND, "gdt.get_dmt_row@1");
}

#[test]
fn ac1_resolve_chain_by_handle_prints_the_reference_bytes() {
    let (st, dp) = project();
    let t = table();
    let ch = CharacteristicRef::parse(&read("resolve", "flatness_asme.json")).unwrap();
    assert_eq!((ch.char().as_str(), ch.characteristic(), ch.variant_override()), ("flat-1", "flatness", None));
    let out = resolve_chain(&st, &dp, H, &ch, Some(&t));
    assert_eq!(printed(&out), expected("flatness_asme"));
    let Ok(Resolution::Resolved(r)) = out else { panic!("resolved") };
    assert_eq!(r.row().as_str(), "dmt:asme/flatness/none@1");
    // AC-6 locally: the same input prints the same bytes again.
    assert_eq!(printed(&resolve_chain(&st, &dp, H, &ch, Some(&t))), expected("flatness_asme"));
}

#[test]
fn ac2_ac3_through_the_command_layer() {
    let (st, dp) = project();
    let t = table();
    let run = |c: CharacteristicRef| printed(&resolve_chain(&st, &dp, H, &c, Some(&t)));
    assert_eq!(run(char_doc("flat-1", "flatness", &["lmc"], None)), expected("unknown_modifier"));
    assert_eq!(run(char_doc("flat-1", "concentricity", &[], None)), expected("unsupported_concentricity"));
    assert_eq!(run(char_doc("pos-1", "position", &["mmc", "cz"], None)), expected("partial_position_cz_mmc"));
    assert_eq!(run(char_doc("pos-1", "perpendicularity", &[], None)), expected("partial_perpendicularity"));
}

#[test]
fn ac4_ac5_profile_and_override_through_the_command_layer() {
    let (mut st, mut dp) = project();
    let t = table();
    // The ASME placeholder profile names a variant the position row lacks: refused, no chain.
    let pos = char_doc("pos-1", "position", &[], None);
    let e = resolve_chain(&st, &dp, H, &pos, Some(&t)).unwrap_err();
    assert_eq!(e.code(), "unknown_variant");
    assert!(matches!(
        e,
        CommandError::Resolve(fid_gps::resolve::ResolveError::UnknownVariant { source: VariantSource::Profile, .. })
    ));
    point_pos_at_axis_profile(&mut st, &mut dp, H);
    assert_eq!(printed(&resolve_chain(&st, &dp, H, &pos, Some(&t))), expected("position_profile_axis"));
    let surface = char_doc("pos-1", "position", &[], Some("surface"));
    assert_eq!(printed(&resolve_chain(&st, &dp, H, &surface, Some(&t))), expected("position_override_surface"));
    let median = char_doc("pos-1", "position", &[], Some("median"));
    assert_eq!(printed(&resolve_chain(&st, &dp, H, &median, Some(&t))), expected("position_override_median"));
}

#[test]
fn agent_profile_choices_do_not_reach_the_human_branch_before_adoption() {
    let (mut st, mut dp) = project();
    let t = table();
    let pos = char_doc("pos-1", "position", &[], None);
    let human_before = printed(&resolve_chain(&st, &dp, H, &pos, Some(&t)));
    let r = point_pos_at_axis_profile(&mut st, &mut dp, A);
    // The agent sees its own overlay ...
    assert_eq!(printed(&resolve_chain(&st, &dp, A, &pos, Some(&t))), expected("position_profile_axis"));
    // ... the human branch is unchanged: same spec, same profile, same outcome.
    assert_eq!(printed(&resolve_chain(&st, &dp, H, &pos, Some(&t))), human_before);
    assert!(dp.resolve(Branch::Working, &r).is_err());
    // Adoption alone does not re-point the human's characteristic either.
    dp.adopt(H, &r).unwrap();
    assert_eq!(printed(&resolve_chain(&st, &dp, H, &pos, Some(&t))), human_before);
}

#[test]
fn the_gate_runs_first_and_the_table_must_be_loaded() {
    let (st, dp) = project();
    let t = table();
    let unknown = char_doc("nope", "flatness", &[], None);
    assert_eq!(resolve_chain(&st, &dp, H, &unknown, Some(&t)).unwrap_err().code(), "char_unknown");
    // Gate before table: a missing spec is reported even when no table is loaded.
    let (ns, ndp) = load_project(&read("spec", "no_spec.fidproj"), &test_vocab()).unwrap();
    let c = char_doc("c-flat-1", "flatness", &[], None);
    let e = resolve_chain(&ns, &ndp, H, &c, None).unwrap_err();
    assert_eq!((e.code(), e.reason()), ("governing_spec_missing", Some("dialect")));
    // A spec whose profile digest is not in the store (GDT.CHAR.SPEC AC-5 fixture).
    let (ms, mdp) = load_project(&read("spec", "digest_mismatch.fidproj"), &test_vocab()).unwrap();
    assert_eq!(resolve_chain(&ms, &mdp, H, &c, Some(&t)).unwrap_err().code(), "defaults_profile_digest_mismatch");
    let flat = CharacteristicRef::parse(&read("resolve", "flatness_asme.json")).unwrap();
    let e = resolve_chain(&st, &dp, H, &flat, None).unwrap_err();
    assert_eq!((e.code(), e.to_string().as_str()), ("dmt_not_loaded", "dmt_not_loaded"));
}

#[test]
fn a_non_text_row_choice_in_the_profile_is_refused() {
    let (mut st, mut dp) = project();
    let t = table();
    let vocab = KeyVocabulary::new(["dmt:asme/position/none@1"]).unwrap();
    let doc = br#"{"schema":"fid.defaults-profile/1","dialect":"asme","edition":"y14.5-2018","version":"3.0.0",
        "keys":{"dmt:asme/position/none@1":true}}"#;
    let doc = parse_profile(doc, &vocab).unwrap();
    let r = dp.publish(doc, H, "root://imports/x.json", false).unwrap().reference().clone();
    let req = SetRequest { char: "pos-1", dialect: "ASME", standards: &[ASME_STD], defaults_profile: &r.to_string() };
    st.set_governing_spec(&mut dp, H, &req, false).unwrap();
    let e = resolve_chain(&st, &dp, H, &char_doc("pos-1", "flatness", &[], None), Some(&t)).unwrap_err();
    assert_eq!(e, CommandError::BadProfileChoice { key: "dmt:asme/position/none@1".to_owned() });
    assert_eq!(e.to_string(), "bad_profile_choice{dmt:asme/position/none@1}");
}

#[test]
fn bad_characteristic_fields_reach_the_resolver_checks() {
    let (st, dp) = project();
    let t = table();
    let run = |c: CharacteristicRef| resolve_chain(&st, &dp, H, &c, Some(&t)).unwrap_err();
    assert_eq!(run(char_doc("flat-1", "Flatness", &[], None)).reason(), Some("bad_characteristic"));
    assert_eq!(run(char_doc("flat-1", "flatness", &["mmc", "mmc"], None)).reason(), Some("duplicate_modifier"));
    assert_eq!(run(char_doc("flat-1", "flatness", &[], Some("Axis"))).reason(), Some("bad_override"));
}

#[test]
fn project_with_profiles_under_two_vocabularies_reloads_with_both() {
    let (mut st, mut dp) = project();
    point_pos_at_axis_profile(&mut st, &mut dp, H);
    let bytes = project_bytes(&st, &dp).unwrap();
    let e = load_project(&bytes, &test_vocab()).map(|_| ()).unwrap_err();
    assert_eq!(e.reason(), Some("vocabulary_unknown"));
    let both: BTreeMap<Digest, KeyVocabulary> =
        [test_vocab(), dmt_vocab()].into_iter().map(|v| (*v.digest(), v)).collect();
    let (st2, dp2) = load_project(&bytes, &both).unwrap();
    assert_eq!((&st2, &dp2), (&st, &dp));
    let pos = char_doc("pos-1", "position", &[], None);
    assert_eq!(printed(&resolve_chain(&st2, &dp2, H, &pos, Some(&table()))), expected("position_profile_axis"));
}

#[test]
fn ac7_get_dmt_row() {
    let t = table();
    let row = get_dmt_row(Some(&t), "dmt:asme/flatness/none@1").unwrap();
    assert_eq!(row.canonical_bytes(), read("resolve", "expected/get_row_flatness.json").as_slice());
    assert_eq!(get_dmt_row(None, "dmt:asme/flatness/none@1").unwrap_err().code(), "dmt_not_loaded");
    assert_eq!(get_dmt_row(Some(&t), "dmt:asme/flatness").unwrap_err().code(), "bad_dmt_key");
    assert_eq!(get_dmt_row(Some(&t), "dmt:asme/flatness/lmc@1").unwrap_err().code(), "dmt_row_unknown");
}

#[test]
fn characteristic_documents_are_strict() {
    let base = String::from_utf8(read("resolve", "flatness_asme.json")).unwrap();
    let bad = |t: &str| CharacteristicRef::parse(t.as_bytes()).unwrap_err().reason().unwrap();
    assert_eq!(bad(&base.replace("fid.gdt-resolve-char/1", "fid.gdt-resolve-char/2")), "bad_schema");
    assert_eq!(bad(&base.replace("\"override\"", "\"variant\"")), "bad_keys");
    assert_eq!(bad(&base.replace("\"flat-1\"", "\"flat 1\"")), "bad_char_id");
    assert_eq!(bad(&base.replace("\"modifiers\": []", "\"modifiers\": [1]")), "wrong_type");
    assert_eq!(bad(&base.replace("\"modifiers\": []", "\"modifiers\": {}")), "wrong_type");
    assert_eq!(bad(&base.replace("\"override\": null", "\"override\": 3")), "wrong_type");
    assert_eq!(bad(&base.replace("\"modifiers\": []", "\"modifiers\": [[\"a\"]]")), "json");
    let nine = r#""modifiers": ["a","b","c","d","e","f","g","h","i"]"#;
    assert_eq!(bad(&base.replace("\"modifiers\": []", nine)), "too_many_modifiers");
    let long = format!("\"characteristic\": \"{}\"", "a".repeat(129));
    assert_eq!(bad(&base.replace("\"characteristic\": \"flatness\"", &long)), "too_long");
    let dup = base.replacen("{", "{\"char\": \"x\",", 1);
    assert_eq!(bad(&dup), "json");
    assert_eq!(bad("[]"), "wrong_type");
    assert_eq!(bad(&" ".repeat(64 * 1024 + 1)), "too_large");
    let e = CharacteristicRef::parse(b"[]").unwrap_err();
    assert_eq!(
        (e.code(), e.to_string().as_str()),
        ("malformed_characteristic", "malformed_characteristic{wrong_type}")
    );
    // Truncations and byte flips never panic.
    let bytes = base.as_bytes();
    for i in 0..bytes.len() {
        let _ = CharacteristicRef::parse(&bytes[..i]);
        let mut b = bytes.to_vec();
        b[i] ^= 0x20;
        let _ = CharacteristicRef::parse(&b);
    }
}
