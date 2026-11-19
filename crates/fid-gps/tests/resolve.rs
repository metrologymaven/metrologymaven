//! Acceptance criteria of spec node GDT.RES.RESOLVE (AC-1 .. AC-7) on the pure resolver, plus
//! the error paths and limits pinned in the leaf's Design. Inputs and expected output bytes come
//! from the independent stdlib-Python reference `fixtures/gdt/resolve/make_cases.py`
//! (`cases.json`, `expected/*.json`) over the synthetic table of GDT.RES.SCHEMA; the ambiguity
//! and input-limit cases are hand-labelled from the leaf's rules. Synthetic data only.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gps::dmt::{Dialect, DmtTable, RowStatus};
use fid_gps::resolve::{
    get_dmt_row, resolve, Resolution, ResolveError, ResolveInput, VariantSource, MAX_INPUT_EDITIONS,
    MAX_PROFILE_CHOICES,
};
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

fn gdt(sub: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt").join(sub)
}

fn read(sub: &str, name: &str) -> Vec<u8> {
    std::fs::read(gdt(sub).join(name)).unwrap()
}

fn table() -> DmtTable {
    DmtTable::load(&read("dmt", "synthetic_table_v1.json")).unwrap()
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(o) => o,
        _ => panic!("object"),
    }
}

fn st(v: &CValue) -> &str {
    match v {
        CValue::Str(s) => s,
        _ => panic!("string"),
    }
}

fn strs(v: &CValue) -> Vec<&str> {
    match v {
        CValue::Arr(a) => a.iter().map(st).collect(),
        _ => panic!("array"),
    }
}

/// `cases.json` from the Python reference: (name, input).
fn cases() -> Vec<(String, ResolveInput)> {
    let v = fid_canon::parse(&read("resolve", "cases.json")).unwrap();
    let CValue::Arr(list) = &obj(&v)["cases"] else { panic!("cases") };
    list.iter()
        .map(|c| {
            let c = obj(c);
            let dialect = Dialect::parse(st(&c["dialect"])).unwrap();
            let profile = obj(&c["profile"]).iter().map(|(k, v)| (k.clone(), st(v).to_owned())).collect();
            let over = match &c["override"] {
                CValue::Null => None,
                v => Some(st(v)),
            };
            let input = ResolveInput::new(
                dialect,
                st(&c["characteristic"]),
                &strs(&c["modifiers"]),
                &strs(&c["editions"]),
                profile,
                over,
            )
            .unwrap();
            (st(&c["name"]).to_owned(), input)
        })
        .collect()
}

/// What the resolver prints for an input: the canonical result, or `{"error":"<code>"}`.
fn printed(input: &ResolveInput, t: &DmtTable) -> Vec<u8> {
    match resolve(input, t) {
        Ok(r) => r.canonical_bytes().unwrap(),
        Err(e) => format!("{{\"error\":\"{}\"}}", e.code()).into_bytes(),
    }
}

fn input(name: &str) -> ResolveInput {
    cases().into_iter().find(|(n, _)| n == name).unwrap().1
}

// ------------------------------------------------------------------ AC-1 .. AC-6

#[test]
fn every_case_prints_the_reference_bytes() {
    // AC-1 .. AC-5 against the independent reference, byte for byte. Run on Linux, Windows and
    // macOS in CI, the same committed bytes are matched on each (AC-6).
    let t = table();
    let all = cases();
    assert_eq!(all.len(), 16);
    for (name, input) in &all {
        let want = read("resolve", &format!("expected/{name}.json"));
        assert_eq!(String::from_utf8(printed(input, &t)).unwrap(), String::from_utf8(want).unwrap(), "case {name}");
    }
}

#[test]
fn ac1_flatness_names_its_row_default_chain_and_digests() {
    let t = table();
    let Resolution::Resolved(r) = resolve(&input("flatness_asme"), &t).unwrap() else { panic!("resolved") };
    assert_eq!(r.row().as_str(), "dmt:asme/flatness/none@1");
    assert_eq!((r.variant(), r.variant_source()), ("default", VariantSource::RowDefault));
    assert_eq!(r.row_status(), RowStatus::Supported);
    let row = t.get("dmt:asme/flatness/none@1").unwrap();
    assert_eq!(r.chain(), row.default_chain().unwrap());
    // Digests from the GDT.RES.SCHEMA reference files.
    let digest = String::from_utf8(read("dmt", "synthetic_table_v1.digest")).unwrap();
    assert_eq!(r.dmt_digest().as_str(), digest.trim());
    let rows = fid_canon::parse(&read("dmt", "synthetic_table_v1.rows.json")).unwrap();
    let chains = obj(&obj(&obj(&rows)["dmt:asme/flatness/none@1"])["chains"]);
    assert_eq!(r.chain_digest().as_str(), st(&chains["default"]));
    assert!(r.limitations().is_empty());
}

#[test]
fn ac2_unknown_modifier_combination_is_unresolved_with_the_derived_key() {
    let out = resolve(&input("unknown_modifier"), &table()).unwrap();
    let Resolution::Unresolved(u) = &out else { panic!("unresolved") };
    assert_eq!(u.key(), "dmt:asme/flatness/lmc");
    assert_eq!(out.outcome(), "UNRESOLVED");
    // No chain anywhere in the output.
    assert!(!obj(&out.to_cvalue()).contains_key("chain"));
}

#[test]
fn ac3_unsupported_and_partial_rows() {
    let t = table();
    let Resolution::Unsupported(u) = resolve(&input("unsupported_concentricity"), &t).unwrap() else {
        panic!("unsupported")
    };
    assert_eq!(u.row().as_str(), "dmt:asme/concentricity/none@1");
    let Resolution::Resolved(p) = resolve(&input("partial_position_cz_mmc"), &t).unwrap() else { panic!("partial") };
    assert_eq!(p.row().as_str(), "dmt:asme/position/cz+mmc@1");
    assert_eq!(p.row_status(), RowStatus::Partial);
    assert_eq!(p.limitations(), ["pattern_size_limited", "zone_def_unsupported:nonuniform"]);
    // Modifiers are a set: their order in the input does not matter.
    let sorted =
        ResolveInput::new(Dialect::Asme, "position", &["cz", "mmc"], &["y14.5-2018"], BTreeMap::new(), None).unwrap();
    assert_eq!(sorted, input("partial_position_cz_mmc"));
}

#[test]
fn ac4_profile_choice_then_override() {
    let t = table();
    let Resolution::Resolved(a) = resolve(&input("position_profile_axis"), &t).unwrap() else { panic!("axis") };
    assert_eq!((a.variant(), a.variant_source()), ("axis", VariantSource::Profile));
    let Resolution::Resolved(s) = resolve(&input("position_override_surface"), &t).unwrap() else { panic!("surface") };
    assert_eq!((s.variant(), s.variant_source()), ("surface", VariantSource::Override));
    assert_ne!(a.chain_digest(), s.chain_digest());
    // A profile choice for another row does not apply.
    let Resolution::Resolved(d) = resolve(&input("position_row_default"), &t).unwrap() else { panic!("default") };
    assert_eq!((d.variant(), d.variant_source()), ("surface", VariantSource::RowDefault));
}

#[test]
fn ac5_unknown_variant_from_override_or_profile() {
    let t = table();
    let e = resolve(&input("position_override_median"), &t).unwrap_err();
    assert_eq!(
        e,
        ResolveError::UnknownVariant {
            variant: "median".to_owned(),
            source: VariantSource::Override,
            row: "dmt:asme/position/none@1".to_owned()
        }
    );
    assert_eq!(e.to_string(), "unknown_variant{median}");
    let e = resolve(&input("position_profile_unknown"), &t).unwrap_err();
    assert!(matches!(e, ResolveError::UnknownVariant { source: VariantSource::Profile, .. }));
    // The status branch comes first: an unsupported row ignores the override.
    assert_eq!(resolve(&input("unsupported_with_override"), &t).unwrap().outcome(), "UNSUPPORTED");
}

#[test]
fn ac6_output_is_identical_for_identical_input_and_independent_of_file_order() {
    let a = table();
    let b = DmtTable::load(&read("dmt", "synthetic_table_v1_shuffled.json")).unwrap();
    for (name, input) in cases() {
        let first = printed(&input, &a);
        assert_eq!(first, printed(&input, &a), "{name}");
        assert_eq!(first, printed(&input, &b), "{name}");
    }
}

// ------------------------------------------------------------------ AC-7

#[test]
fn ac7_get_dmt_row_returns_the_canonical_row() {
    let t = table();
    let row = get_dmt_row(&t, "dmt:asme/flatness/none@1").unwrap();
    assert_eq!(row.canonical_bytes(), read("resolve", "expected/get_row_flatness.json").as_slice());
    let miss = get_dmt_row(&t, "dmt:asme/flatness/lmc@1").unwrap_err();
    assert_eq!(
        (miss.code(), miss.to_string().as_str()),
        ("dmt_row_unknown", "dmt_row_unknown{dmt:asme/flatness/lmc@1}")
    );
    // Exact keys only: no version, unsorted modifiers, upper case and junk are refused.
    for bad in [
        "dmt:asme/flatness/none",
        "dmt:asme/position/mmc+cz@1",
        "DMT:asme/flatness/none@1",
        "",
        "x".repeat(400).as_str(),
    ] {
        assert_eq!(get_dmt_row(&t, bad).unwrap_err().code(), "bad_dmt_key", "{bad}");
    }
    let long = get_dmt_row(&t, &"é".repeat(400)).unwrap_err();
    let ResolveError::BadDmtKey { key } = long else { panic!("bad key") };
    assert!(key.len() <= 160);
}

// ------------------------------------------------------------------ fail closed

/// The synthetic table plus a second flatness row (`@2`) whose editions are `eds`.
fn with_second_flatness(eds: &[&str]) -> DmtTable {
    let mut v = fid_canon::parse(&read("dmt", "synthetic_table_v1.json")).unwrap();
    let CValue::Obj(top) = &mut v else { panic!("table") };
    let CValue::Arr(rows) = top.get_mut("rows").unwrap() else { panic!("rows") };
    let mut extra = rows.iter().find(|r| st(&obj(r)["key"]) == "dmt:asme/flatness/none@1").unwrap().clone();
    let CValue::Obj(o) = &mut extra else { panic!("row") };
    o.insert("key".to_owned(), CValue::Str("dmt:asme/flatness/none@2".to_owned()));
    o.insert("editions".to_owned(), CValue::Arr(eds.iter().map(|e| CValue::Str((*e).to_owned())).collect()));
    rows.push(extra);
    DmtTable::from_value(&v).unwrap()
}

#[test]
fn two_matching_rows_are_refused_not_chosen() {
    // Hand-labelled: both @1 and @2 cover y14.5-2018, so the resolver must not pick one.
    let t = with_second_flatness(&["y14.5-2018"]);
    let e = resolve(&input("flatness_asme"), &t).unwrap_err();
    assert_eq!(e, ResolveError::Ambiguous { key: "dmt:asme/flatness/none".to_owned(), count: 2 });
    assert_eq!(e.to_string(), "ambiguous_resolution{dmt:asme/flatness/none}");
    // Disjoint editions select exactly one version.
    let t = with_second_flatness(&["y14.5-2009"]);
    let Resolution::Resolved(r) = resolve(&input("flatness_asme"), &t).unwrap() else { panic!("resolved") };
    assert_eq!(r.row().as_str(), "dmt:asme/flatness/none@1");
    let old = ResolveInput::new(Dialect::Asme, "flatness", &[], &["y14.5-2009"], BTreeMap::new(), None).unwrap();
    let Resolution::Resolved(r) = resolve(&old, &t).unwrap() else { panic!("resolved") };
    assert_eq!(r.row().as_str(), "dmt:asme/flatness/none@2");
    // The table digest in the output is the table's own.
    assert_eq!(r.dmt_digest(), t.digest());
}

#[test]
fn neighbouring_keys_are_not_near_matches() {
    // `position` must not match `position_x` rows, nor `position/none` match `position/mmc`.
    let t = table();
    let px = ResolveInput::new(Dialect::Asme, "positio", &[], &["y14.5-2018"], BTreeMap::new(), None).unwrap();
    assert_eq!(resolve(&px, &t).unwrap().outcome(), "UNRESOLVED");
    let mmc = ResolveInput::new(Dialect::Asme, "position", &["mmc"], &["y14.5-2018"], BTreeMap::new(), None).unwrap();
    let Resolution::Resolved(r) = resolve(&mmc, &t).unwrap() else { panic!("resolved") };
    assert_eq!(r.row().as_str(), "dmt:asme/position/mmc@1");
    let cz = ResolveInput::new(Dialect::Asme, "position", &["cz"], &["y14.5-2018"], BTreeMap::new(), None).unwrap();
    assert_eq!(resolve(&cz, &t).unwrap().outcome(), "UNRESOLVED");
    // A characteristic name so long that no key could hold it.
    let long = "a".repeat(32);
    let mods = [
        "aaaaaaaaaaaaaaa1",
        "aaaaaaaaaaaaaaa2",
        "aaaaaaaaaaaaaaa3",
        "aaaaaaaaaaaaaaa4",
        "aaaaaaaaaaaaaaa5",
        "aaaaaaaaaaaaaaa6",
    ];
    let huge = ResolveInput::new(Dialect::Asme, &long, &mods, &["y14.5-2018"], BTreeMap::new(), None).unwrap();
    assert!(huge.derived_key().len() > 128);
    assert_eq!(resolve(&huge, &t).unwrap().outcome(), "UNRESOLVED");
}

#[test]
fn input_is_checked() {
    let ed = ["y14.5-2018"];
    let p = BTreeMap::new;
    let bad = |r: Result<ResolveInput, ResolveError>| r.unwrap_err().reason().unwrap();
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "Flatness", &[], &ed, p(), None)), "bad_characteristic");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "", &[], &ed, p(), None)), "bad_characteristic");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &["none"], &ed, p(), None)), "bad_modifier");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &["MMC"], &ed, p(), None)), "bad_modifier");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &["a+b"], &ed, p(), None)), "bad_modifier");
    assert_eq!(
        bad(ResolveInput::new(Dialect::Asme, "flatness", &["mmc", "mmc"], &ed, p(), None)),
        "duplicate_modifier"
    );
    let nine = ["a", "b", "c", "d", "e", "f", "g", "h", "i"];
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &nine, &ed, p(), None)), "too_many_modifiers");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &[], p(), None)), "no_editions");
    let many: Vec<String> = (0..=MAX_INPUT_EDITIONS).map(|i| format!("e{i}")).collect();
    let many: Vec<&str> = many.iter().map(String::as_str).collect();
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &many, p(), None)), "too_many_editions");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &["Y14.5"], p(), None)), "bad_edition");
    let twice = ["y14.5-2018", "y14.5-2018"];
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &twice, p(), None)), "duplicate_edition");
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &ed, p(), Some("Axis"))), "bad_override");
    let big: BTreeMap<String, String> = (0..=MAX_PROFILE_CHOICES).map(|i| (format!("k{i}"), "v".to_owned())).collect();
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &ed, big, None)), "too_many_profile_keys");
    let empty_key = [(String::new(), "v".to_owned())].into_iter().collect();
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &ed, empty_key, None)), "bad_profile_key");
    let long_val = [("k".to_owned(), "é".repeat(281))].into_iter().collect();
    assert_eq!(bad(ResolveInput::new(Dialect::Asme, "flatness", &[], &ed, long_val, None)), "bad_profile_value");
    let e = ResolveInput::new(Dialect::Asme, "Flatness", &[], &ed, p(), None).unwrap_err();
    assert_eq!((e.code(), e.to_string().as_str()), ("bad_input", "bad_input{bad_characteristic}"));
    // A valid input keeps its sets sorted.
    let ok = ResolveInput::new(Dialect::Asme, "flatness", &["mmc", "cz"], &["y2", "y1"], p(), Some("axis")).unwrap();
    assert_eq!(
        (ok.modifiers(), ok.editions(), ok.variant_override()),
        (&["cz".to_owned(), "mmc".to_owned()][..], &["y1".to_owned(), "y2".to_owned()][..], Some("axis"))
    );
    assert_eq!(ok.derived_key(), "dmt:asme/flatness/cz+mmc");
}

#[test]
fn resolution_reports_every_field() {
    let t = table();
    let out = resolve(&input("partial_perpendicularity"), &t).unwrap();
    let v = out.to_cvalue();
    let keys: Vec<&str> = obj(&v).keys().map(String::as_str).collect();
    assert_eq!(
        keys,
        [
            "chain",
            "chain_digest",
            "dmt_digest",
            "limitations",
            "outcome",
            "row",
            "row_status",
            "variant",
            "variant_source"
        ]
    );
    assert_eq!(st(&obj(&v)["row_status"]), "partial");
    assert_eq!(strs(&obj(&v)["limitations"]), ["bestfit_disallowed:axis"]);
}
