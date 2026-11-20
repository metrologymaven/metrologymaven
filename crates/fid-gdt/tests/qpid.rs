//! Acceptance criteria of spec node GDT.CHAR.IDENT (AC-1 .. AC-5) plus the error paths,
//! uniqueness rules and QIF mapping rules of the leaf's Design and of the #301 review. The QIF
//! fixtures, the imported QPId text and the expected UUIDv7 values come from the independent
//! stdlib-Python reference `fixtures/gdt/ident/make_fixtures.py` (`expected.json`). Error
//! cases are hand-labelled from the leaf's rules.
//!
//! QIF reading and writing belong to `IO.QIF` and the characteristic types to
//! `GDT.CHAR.MODEL`, and neither is built yet. The tests therefore read the identity fields of
//! one element with the project's XML reader (`fid-xml`), write them back with a small
//! test-only serialiser, and edit a test-only stand-in characteristic. The end-to-end chain
//! is an acceptance criterion of `GDT.CHAR.MODEL.CMDS`.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_gdt::chars::qpid::{
    check_edit, check_unchanged, read_identity, write_identity, CharIdentity, CounterRng, IdSource, IdentError,
    QifIdentityView, QifStrAttribute, Qpid, QpidAssigner, QpidSlot, FX_QPID, MAX_UNIX_MS,
};
use fid_io_reg::{Budget, LimitRequest, Principal};
use fid_project::{uuid_v7, Bits, IdError, Observation, ObservationSource, Recording, ReplaySource, Uuid};
use fid_xml::{read_slice, EventKind};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const ITEM: &str = "FlatnessCharacteristicItem";
const NOMINAL: &str = "FlatnessCharacteristicNominal";
const GAMMA: u64 = 0x9e37_79b9_7f4a_7c15;

fn ns() -> Uuid {
    Uuid::parse("6ba7b810-9dad-11d1-80b4-00c04fd430c8").unwrap()
}

fn path(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/gdt/ident").join(name)
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(path(name)).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&read("expected.json")).unwrap()
}

fn get<'a>(v: &'a CValue, key: &str) -> &'a CValue {
    let CValue::Obj(o) = v else { panic!("object for {key}") };
    o.get(key).unwrap_or_else(|| panic!("missing {key}"))
}

fn s<'a>(v: &'a CValue, key: &str) -> &'a str {
    let CValue::Str(t) = get(v, key) else { panic!("string {key}") };
    t
}

fn n(v: &CValue, key: &str) -> u64 {
    s(v, key).parse().unwrap()
}

fn budget() -> Budget {
    Budget::new(Principal::Agent, &LimitRequest::default()).unwrap()
}

/// One assignment through a fresh assigner over `source` and `existing`.
fn assign(
    source: &mut dyn ObservationSource,
    existing: &BTreeSet<Uuid>,
    imported: Option<Qpid>,
) -> Result<CharIdentity, IdentError> {
    QpidAssigner::new(ns(), source, existing).assign(imported)
}

fn assign_on_import(
    source: &mut dyn ObservationSource,
    existing: &BTreeSet<Uuid>,
    slot: QpidSlot,
    view: &QifIdentityView,
) -> Result<CharIdentity, IdentError> {
    QpidAssigner::new(ns(), source, existing).assign_on_import(slot, view)
}

/// The identity fields of the first element named `local`: the text of its direct `QPId`
/// child and the `AttributeStr` entries of its direct `Attributes` child.
fn view_of(xml: &[u8], local: &str) -> QifIdentityView {
    let events = read_slice(xml, budget()).unwrap();
    let mut stack: Vec<String> = Vec::new();
    let mut inside: Option<usize> = None;
    let mut found = false;
    let mut view = QifIdentityView::default();
    for ev in events {
        match ev.kind {
            EventKind::Start(e) => {
                let name = e.name.local.clone();
                if inside.is_none() && !found && name == local {
                    inside = Some(stack.len());
                    found = true;
                }
                if let Some(base) = inside {
                    let rel: Vec<&str> = stack[base..].iter().map(String::as_str).collect();
                    if name == "AttributeStr" && rel == [local, "Attributes"] {
                        let attr = |k: &str| {
                            e.attrs.iter().find(|a| a.name.local == k).map(|a| a.value.raw().to_owned()).unwrap()
                        };
                        view.attributes.push(QifStrAttribute { name: attr("name"), value: attr("val") });
                    }
                }
                stack.push(name);
            }
            EventKind::End { .. } => {
                stack.pop();
                if inside == Some(stack.len()) {
                    inside = None;
                }
            }
            EventKind::Text(t) => {
                if let Some(base) = inside {
                    let rel: Vec<&str> = stack[base..].iter().map(String::as_str).collect();
                    if rel == [local, "QPId"] {
                        view.native_qpid = Some(view.native_qpid.take().unwrap_or_default() + t.raw());
                    }
                }
            }
            _ => {}
        }
    }
    assert!(found, "no element {local}");
    view
}

fn esc(t: &str) -> String {
    t.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

/// Test-only export of one element's identity fields (QIF writing is IO.QIF's job).
fn to_xml(local: &str, view: &QifIdentityView) -> String {
    let mut x = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    x.push_str(&format!("<{local} xmlns=\"http://qifstandards.org/xsd/qif3\" id=\"9\">"));
    if let Some(q) = &view.native_qpid {
        x.push_str(&format!("<QPId>{}</QPId>", esc(q)));
    }
    if !view.attributes.is_empty() {
        x.push_str(&format!("<Attributes n=\"{}\">", view.attributes.len()));
        for a in &view.attributes {
            x.push_str(&format!("<AttributeStr name=\"{}\" val=\"{}\"/>", esc(&a.name), esc(&a.value)));
        }
        x.push_str("</Attributes>");
    }
    x.push_str(&format!("</{local}>\n"));
    x
}

/// A stand-in characteristic: an identity plus one editable field.
#[derive(Debug, Clone)]
struct Characteristic {
    id: CharIdentity,
    tolerance: String,
}

impl Characteristic {
    /// Field-by-field comparison, with the identity compared by value *and* text.
    fn identical(&self, other: &Characteristic) -> bool {
        self.id == other.id
            && self.id.same_text(&other.id)
            && self.id.source() == other.id.source()
            && self.tolerance == other.tolerance
    }
}

struct Edit<'a> {
    identity: Option<&'a str>,
    tolerance: &'a str,
}

/// Applies an edit the way the edit command must: identity check first, then the change.
fn apply(c: &mut Characteristic, e: &Edit) -> Result<(), IdentError> {
    check_edit(&c.id, e.identity)?;
    c.tolerance = e.tolerance.to_owned();
    Ok(())
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_imported_qpid_survives_edit_and_export_byte_for_byte() {
    let exp = expected();
    let want = s(&exp, "imported_qpid");
    let src = read("imported_qpid.qif");
    assert!(String::from_utf8(src.clone()).unwrap().contains(&format!("<QPId>{want}</QPId>")));

    let view = view_of(&src, ITEM);
    let mut rng = CounterRng::new(1, 1);
    let none = BTreeSet::new();
    let id = assign_on_import(&mut rng, &none, QpidSlot::Native, &view).unwrap();
    assert_eq!(id.source(), IdSource::Imported);
    assert_eq!(id.as_str(), want);
    assert_eq!(rng.counter(), 0, "an imported QPId draws nothing");

    let mut c = Characteristic { id, tolerance: "0.05".into() };
    apply(&mut c, &Edit { identity: None, tolerance: "0.10" }).unwrap();
    assert_eq!(c.tolerance, "0.10");

    let mut out = view.clone();
    write_identity(QpidSlot::Native, &c.id, &mut out);
    let xml = to_xml(ITEM, &out);
    assert!(xml.contains(&format!("<QPId>{want}</QPId>")), "{xml}");
    let back = view_of(xml.as_bytes(), ITEM);
    assert_eq!(back.native_qpid.as_deref().unwrap().as_bytes(), want.as_bytes());
    let again = assign_on_import(&mut rng, &none, QpidSlot::Native, &back).unwrap();
    assert_eq!(again, c.id);
    assert!(again.same_text(&c.id), "export wrote the imported spelling");
}

// ------------------------------------------------------------------ AC-2

fn ac2_round(slot: QpidSlot) {
    let mut rng = CounterRng::new(99, 1_763_500_000_123);
    let mut project = BTreeSet::new();
    let id = assign(&mut rng, &project, None).unwrap();
    project.insert(id.uuid());
    assert_eq!(id.source(), IdSource::Assigned);
    assert_eq!(id.uuid().version(), 7);
    assert_eq!(id.uuid().variant_bits(), 0b10);
    assert_eq!(rng.counter(), 2);
    let first = id.clone();

    let mut c = Characteristic { id, tolerance: "0.05".into() };
    let same = first.as_str().to_uppercase();
    let edits = [
        Edit { identity: None, tolerance: "0.06" },
        Edit { identity: Some(first.as_str()), tolerance: "0.07" },
        Edit { identity: Some(same.as_str()), tolerance: "0.08" },
    ];
    for e in &edits {
        apply(&mut c, e).unwrap();
        assert_eq!(c.id, first, "edit changed the identity");
        assert!(c.id.same_text(&first), "edit changed the identity text");
    }
    assert_eq!(c.tolerance, "0.08");
    assert_eq!(rng.counter(), 2, "edits draw nothing");

    let mut out = QifIdentityView::default();
    write_identity(slot, &c.id, &mut out);
    let xml = to_xml(ITEM, &out);
    let back = view_of(xml.as_bytes(), ITEM);
    // Re-import into another project (the exporting one already holds the ID).
    let mut other_rng = CounterRng::new(12345, 5);
    let re = assign_on_import(&mut other_rng, &BTreeSet::new(), slot, &back).unwrap();
    assert_eq!(re.source(), IdSource::Imported);
    assert_eq!(re, first);
    assert!(re.same_text(&first));
    assert_eq!(re.uuid().version(), 7);
    assert_eq!(other_rng.counter(), 0);
    check_unchanged(&first, &re).unwrap();
    // Re-import into the exporting project: the ID is already there.
    let err = assign_on_import(&mut other_rng, &project, slot, &back).unwrap_err();
    assert_eq!(err.code(), "id_collision");
}

#[test]
fn ac2_assigned_uuidv7_survives_three_edits_and_round_trip_native() {
    ac2_round(QpidSlot::Native);
}

#[test]
fn ac2_assigned_uuidv7_survives_three_edits_and_round_trip_fx() {
    ac2_round(QpidSlot::FxAttribute);
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_no_slot_element_gets_fx_qpid_and_round_trips() {
    let exp = expected();
    let nsl = get(&exp, "no_slot");
    let kept = get(nsl, "kept_attribute");
    assert!(!s(kept, "name").starts_with("fx:"), "the kept attribute is a foreign one");
    let view = view_of(&read("no_qpid_slot.qif"), NOMINAL);
    assert_eq!(view.native_qpid, None);
    assert_eq!(view.attributes, vec![QifStrAttribute { name: s(kept, "name").into(), value: s(kept, "value").into() }]);
    assert_eq!(read_identity(QpidSlot::FxAttribute, &view).unwrap(), None);

    let mut rng = CounterRng::new(n(nsl, "seed"), n(nsl, "clock_ms"));
    let id = assign_on_import(&mut rng, &BTreeSet::new(), QpidSlot::FxAttribute, &view).unwrap();
    assert_eq!(id.source(), IdSource::Assigned);
    assert_eq!(id.as_str(), s(nsl, "identity"));

    let mut out = view.clone();
    write_identity(QpidSlot::FxAttribute, &id, &mut out);
    assert_eq!(out.native_qpid, None);
    assert_eq!(out.attributes.len(), 2);
    assert_eq!(out.attributes[0], view.attributes[0], "other attributes kept, in order");
    assert_eq!(out.attributes[1], QifStrAttribute { name: FX_QPID.into(), value: s(nsl, "identity").into() });

    let xml = to_xml(NOMINAL, &out);
    assert!(xml.contains(&format!("<AttributeStr name=\"fx:qpid\" val=\"{}\"/>", s(nsl, "identity"))), "{xml}");
    let back = view_of(xml.as_bytes(), NOMINAL);
    let mut fresh = CounterRng::new(0, 0);
    let re = assign_on_import(&mut fresh, &BTreeSet::new(), QpidSlot::FxAttribute, &back).unwrap();
    assert_eq!(re, id);
    assert!(re.same_text(&id));
    assert_eq!(fresh.counter(), 0);

    // A second export of the re-imported identity replaces the attribute, never duplicates it.
    let mut twice = back.clone();
    write_identity(QpidSlot::FxAttribute, &re, &mut twice);
    assert_eq!(twice, back);
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_edit_with_other_identity_fails_and_leaves_characteristic_unchanged() {
    let mut rng = CounterRng::new(3, 1_700_000_000_000);
    let id = assign(&mut rng, &BTreeSet::new(), None).unwrap();
    let mut c = Characteristic { id, tolerance: "0.05".into() };
    let before = c.clone();

    let other = assign(&mut rng, &BTreeSet::new(), None).unwrap();
    let err = apply(&mut c, &Edit { identity: Some(other.as_str()), tolerance: "0.50" }).unwrap_err();
    assert_eq!(err.code(), "identity_immutable");
    assert_eq!(err, IdentError::IdentityImmutable { current: before.id.uuid(), attempted: other.uuid() });
    assert!(c.identical(&before));

    let err = apply(&mut c, &Edit { identity: Some("not-a-uuid"), tolerance: "0.50" }).unwrap_err();
    assert_eq!(err.code(), "malformed_qpid");
    assert!(c.identical(&before));

    let err = check_unchanged(&before.id, &other).unwrap_err();
    assert_eq!(err.code(), "identity_immutable");
    check_unchanged(&before.id, &before.id).unwrap();
}

// ------------------------------------------------------------------ AC-5

fn draw(ms: u64, seed: u64, k: usize) -> Vec<String> {
    let mut rng = CounterRng::new(seed, ms);
    (0..k).map(|_| assign(&mut rng, &BTreeSet::new(), None).unwrap().as_str().to_owned()).collect()
}

#[test]
fn ac5_same_clock_and_seed_give_the_reference_uuids() {
    let exp = expected();
    let CValue::Arr(cases) = get(&exp, "v7_cases") else { panic!("v7_cases") };
    assert!(cases.len() >= 5);
    for case in cases {
        let CValue::Arr(want) = get(case, "uuids") else { panic!("uuids") };
        let want: Vec<String> = want
            .iter()
            .map(|u| match u {
                CValue::Str(t) => t.clone(),
                _ => panic!("uuid"),
            })
            .collect();
        let (ms, seed) = (n(case, "clock_ms"), n(case, "seed"));
        let got = draw(ms, seed, want.len());
        assert_eq!(got, want, "case {}", s(case, "name"));
        assert_eq!(draw(ms, seed, want.len()), got, "repeat of case {}", s(case, "name"));
    }
}

#[test]
fn ac5_replay_of_recorded_observations_gives_identical_ids() {
    // Record what the source handed out (as a live session does), then replay it.
    let mut live = Recording::new(CounterRng::new(77, 1_763_500_000_000));
    let mut project = BTreeSet::new();
    let mut minted = Vec::new();
    for _ in 0..3 {
        let id = assign(&mut live, &project, None).unwrap();
        project.insert(id.uuid());
        minted.push(id);
    }
    let log = live.into_observations();
    assert_eq!(log.len(), 6, "one now_ms and one random_bits per ID");

    let mut replay = ReplaySource::new(log);
    let mut replayed_project = BTreeSet::new();
    for want in &minted {
        let got = assign(&mut replay, &replayed_project, None).unwrap();
        assert_eq!(&got, want);
        assert!(got.same_text(want));
        replayed_project.insert(got.uuid());
    }
    assert_eq!(replay.remaining(), 0);
    assert_eq!(assign(&mut replay, &replayed_project, None).unwrap_err().code(), "replay_exhausted");
}

#[test]
fn counter_rng_matches_splitmix64_reference_value() {
    // Published SplitMix64 reference: seed 1234567, first output.
    assert_eq!(CounterRng::new(1_234_567, 0).next_u64(), 6_457_827_717_110_365_317);
}

// ------------------------------------------------------------------ uniqueness (#301 review)

#[test]
fn shifted_seed_collision_is_caught_not_redrawn() {
    // Seeds s and s + 2γ give the same IDs shifted by one (two outputs per UUIDv7).
    let ms = 1_763_500_000_000;
    let a = draw(ms, 5, 2);
    let b = draw(ms, 5u64.wrapping_add(GAMMA.wrapping_mul(2)), 1);
    assert_eq!(a[1], b[0], "the reviewer's shifted-seed case");

    let mut project = BTreeSet::new();
    for t in &a {
        project.insert(Uuid::parse(t).unwrap());
    }
    let mut shifted = CounterRng::new(5u64.wrapping_add(GAMMA.wrapping_mul(2)), ms);
    let err = assign(&mut shifted, &project, None).unwrap_err();
    assert_eq!(err, IdentError::Id(IdError::IdCollision(Uuid::parse(&a[1]).unwrap())));
    assert_eq!(err.code(), "id_collision");
    assert_eq!(shifted.counter(), 2, "exactly one draw: nothing is redrawn");
}

#[test]
fn restart_with_same_seed_collides_on_first_id() {
    let ms = 1_700_000_000_000;
    let mut first_run = CounterRng::new(42, ms);
    let mut project = BTreeSet::new();
    let id = assign(&mut first_run, &project, None).unwrap();
    project.insert(id.uuid());

    let mut restarted = CounterRng::new(42, ms);
    let err = assign(&mut restarted, &project, None).unwrap_err();
    assert_eq!(err, IdentError::Id(IdError::IdCollision(id.uuid())));
    assert_eq!(restarted.counter(), 2);
}

#[test]
fn import_of_an_id_the_project_holds_collides() {
    let mut rng = CounterRng::new(1, 1);
    let held = Uuid::parse("018bcfe5-6800-7a54-a8ef-e333b266f103").unwrap();
    let project: BTreeSet<Uuid> = [held].into_iter().collect();
    let err =
        assign(&mut rng, &project, Some(Qpid::parse("018BCFE5-6800-7A54-A8EF-E333B266F103").unwrap())).unwrap_err();
    assert_eq!(err, IdentError::Id(IdError::IdCollision(held)));
    assert_eq!(rng.counter(), 0);
    // Through a QIF element too.
    let v = view(Some("018bcfe5-6800-7a54-a8ef-e333b266f103"), &[]);
    assert_eq!(assign_on_import(&mut rng, &project, QpidSlot::Native, &v).unwrap_err().code(), "id_collision");
}

// ------------------------------------------------------------------ errors and rules

#[test]
fn clock_past_48_bits_fails_before_drawing() {
    let mut rng = CounterRng::new(1, MAX_UNIX_MS + 1);
    assert_eq!(assign(&mut rng, &BTreeSet::new(), None).unwrap_err().code(), "bad_observation");
    assert_eq!(rng.counter(), 0);
    rng.set_now_ms(MAX_UNIX_MS);
    assert!(assign(&mut rng, &BTreeSet::new(), None).is_ok());
}

#[test]
fn counter_rng_bit_widths() {
    let mut a = CounterRng::new(9, 0);
    let mut b = CounterRng::new(9, 0);
    let x = b.next_u64();
    assert_eq!(a.random_bits(12).unwrap().value(), u128::from(x & 0xfff));
    let (p, q) = (b.next_u64(), b.next_u64());
    assert_eq!(a.random_bits(128).unwrap().value(), (u128::from(p) << 64) | u128::from(q));
    assert_eq!(a.random_bits(129).unwrap_err(), IdError::BadObservation);
}

#[test]
fn qpid_parse_rules() {
    let q = Qpid::parse(" \t4F2A9C3E-1B7D-4E8A-9C21-7D3B5E6F8A10\r\n").unwrap();
    assert_eq!(q.as_str(), "4F2A9C3E-1B7D-4E8A-9C21-7D3B5E6F8A10");
    assert_eq!(q.uuid(), Uuid::parse("4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a10").unwrap());
    let reason = |t: &str| match Qpid::parse(t) {
        Err(IdentError::MalformedQpid { reason }) => reason,
        other => panic!("{t:?}: {other:?}"),
    };
    assert_eq!(reason(""), "not_uuid");
    assert_eq!(reason("4f2a9c3e1b7d4e8a9c217d3b5e6f8a10"), "not_uuid");
    assert_eq!(reason("4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a1g"), "not_uuid");
    assert_eq!(reason("4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a10x"), "not_uuid");
    assert_eq!(reason("4f2a9c3e-1b7d-4e8a-9c21 7d3b5e6f8a10"), "not_uuid");
    assert_eq!(reason("\u{a0}4f2a9c3e-1b7d-4e8a-9c21-7d3b5e6f8a10"), "not_uuid");
    assert_eq!(reason("00000000-0000-0000-0000-000000000000"), "reserved_uuid");
    assert_eq!(reason("FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF"), "reserved_uuid");
    assert_eq!(Qpid::parse("x").unwrap_err().code(), "malformed_qpid");
}

#[test]
fn fx_qpid_accepts_only_the_exact_written_form() {
    let fx_reason = |t: &str| match Qpid::parse_fx(t) {
        Err(IdentError::MalformedQpid { reason }) => reason,
        other => panic!("{t:?}: {other:?}"),
    };
    assert_eq!(Qpid::parse_fx(U1).unwrap().as_str(), U1);
    assert_eq!(fx_reason(&U1.to_uppercase()), "fx_qpid_form");
    assert_eq!(fx_reason(&format!(" {U1}")), "fx_qpid_form");
    assert_eq!(fx_reason(&format!("{U1}\n")), "fx_qpid_form");
    assert_eq!(fx_reason("4f2a9c3e-1B7d-4e8a-9c21-7d3b5e6f8a10"), "fx_qpid_form");
    assert_eq!(fx_reason("nope"), "not_uuid");
    assert_eq!(fx_reason("00000000-0000-0000-0000-000000000000"), "reserved_uuid");
    // Through the element reader.
    let upper = U1.to_uppercase();
    let r = read_identity(QpidSlot::FxAttribute, &view(None, &[(FX_QPID, &upper)]));
    assert_eq!(r.unwrap_err(), IdentError::MalformedQpid { reason: "fx_qpid_form" });
}

#[test]
fn equality_order_and_hash_use_the_uuid_only() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let h = |q: &Qpid| {
        let mut s = DefaultHasher::new();
        q.hash(&mut s);
        s.finish()
    };
    let lower = Qpid::parse(U1).unwrap();
    let upper = Qpid::parse(&U1.to_uppercase()).unwrap();
    assert_eq!(lower, upper);
    assert_eq!(lower.cmp(&upper), std::cmp::Ordering::Equal);
    assert_eq!(h(&lower), h(&upper));
    assert!(!lower.same_text(&upper));
    assert!(lower.same_text(&lower.clone()));

    let mut rng = CounterRng::new(0, 0);
    let a = assign(&mut rng, &BTreeSet::new(), Some(lower)).unwrap();
    let b = assign(&mut rng, &BTreeSet::new(), Some(upper)).unwrap();
    assert_eq!(a, b);
    assert!(!a.same_text(&b));
    let set: BTreeSet<CharIdentity> = [a.clone(), b.clone()].into_iter().collect();
    assert_eq!(set.len(), 1);
    let other = Qpid::parse(U2).unwrap();
    assert!(Qpid::parse(U1).unwrap() < other);
}

fn view(native: Option<&str>, attrs: &[(&str, &str)]) -> QifIdentityView {
    QifIdentityView {
        native_qpid: native.map(str::to_owned),
        attributes: attrs.iter().map(|(n, v)| QifStrAttribute { name: (*n).into(), value: (*v).into() }).collect(),
    }
}

const U1: &str = "018bcfe5-6800-7a54-a8ef-e333b266f103";
const U2: &str = "018bcfe5-6800-7d49-981c-e1ff0e4ae394";

#[test]
fn read_identity_rules() {
    let reason = |slot, v: &QifIdentityView| match read_identity(slot, v) {
        Err(IdentError::MalformedQpid { reason }) => reason,
        other => panic!("{other:?}"),
    };
    // Native slot.
    assert_eq!(read_identity(QpidSlot::Native, &view(None, &[])).unwrap(), None);
    assert_eq!(read_identity(QpidSlot::Native, &view(Some(U1), &[])).unwrap().unwrap().as_str(), U1);
    assert_eq!(read_identity(QpidSlot::Native, &view(None, &[(FX_QPID, U1)])).unwrap().unwrap().as_str(), U1);
    let upper = U1.to_uppercase();
    assert_eq!(
        read_identity(QpidSlot::Native, &view(Some(&upper), &[(FX_QPID, U1)])).unwrap().unwrap().as_str(),
        upper
    );
    assert_eq!(reason(QpidSlot::Native, &view(Some(U1), &[(FX_QPID, U2)])), "conflicting_qpid");
    assert_eq!(reason(QpidSlot::Native, &view(Some("bad"), &[])), "not_uuid");
    // No native slot.
    assert_eq!(read_identity(QpidSlot::FxAttribute, &view(None, &[("inspectionNote", "x")])).unwrap(), None);
    assert_eq!(
        read_identity(QpidSlot::FxAttribute, &view(None, &[("inspectionNote", "x"), (FX_QPID, U2)]))
            .unwrap()
            .unwrap()
            .as_str(),
        U2
    );
    assert_eq!(reason(QpidSlot::FxAttribute, &view(Some(U1), &[])), "qpid_without_slot");
    assert_eq!(reason(QpidSlot::FxAttribute, &view(None, &[(FX_QPID, U1), (FX_QPID, U1)])), "duplicate_fx_qpid");
    assert_eq!(reason(QpidSlot::Native, &view(Some(U1), &[(FX_QPID, U1), (FX_QPID, U1)])), "duplicate_fx_qpid");
    assert_eq!(reason(QpidSlot::FxAttribute, &view(None, &[(FX_QPID, "nope")])), "not_uuid");
    // Attribute names are matched exactly.
    assert_eq!(read_identity(QpidSlot::FxAttribute, &view(None, &[("FX:QPID", U1), ("qpid", U1)])).unwrap(), None);
}

#[test]
fn write_identity_rules() {
    let upper = U1.to_uppercase();
    let id = assign(&mut CounterRng::new(0, 0), &BTreeSet::new(), Some(Qpid::parse(&upper).unwrap())).unwrap();
    // Native: writes the imported spelling and drops fx:qpid attributes, keeping the rest in order.
    let mut v = view(Some(U2), &[("a", "1"), (FX_QPID, U2), ("b", "2")]);
    write_identity(QpidSlot::Native, &id, &mut v);
    assert_eq!(v, view(Some(&upper), &[("a", "1"), ("b", "2")]));
    // FxAttribute: the lowercase form replaces the first fx:qpid in place, others dropped.
    let mut v = view(Some(U2), &[("a", "1"), (FX_QPID, U2), ("b", "2"), (FX_QPID, U2)]);
    write_identity(QpidSlot::FxAttribute, &id, &mut v);
    assert_eq!(v, view(None, &[("a", "1"), (FX_QPID, U1), ("b", "2")]));
    // FxAttribute without one: appended at the end.
    let mut v = view(None, &[("a", "1")]);
    write_identity(QpidSlot::FxAttribute, &id, &mut v);
    assert_eq!(v, view(None, &[("a", "1"), (FX_QPID, U1)]));
}

#[test]
fn replay_source_assignment_matches_uuid_v7() {
    let bits = Bits::new(74, (0x2a5u128 << 64) | 0x0123_4567_89ab_cdef).unwrap();
    let mut src = ReplaySource::new(vec![Observation::NowMs(1_700_000_000_000), Observation::RandomBits(bits)]);
    let id = assign(&mut src, &BTreeSet::new(), None).unwrap();
    assert_eq!(id.uuid(), uuid_v7(1_700_000_000_000, &bits).unwrap());
    assert_eq!(id.source(), IdSource::Assigned);
    assert_eq!(src.remaining(), 0);
    // Imported QPId: the source is not touched.
    let mut empty = ReplaySource::new(vec![]);
    let q = Qpid::parse(U1).unwrap();
    assert_eq!(assign(&mut empty, &BTreeSet::new(), Some(q)).unwrap().source(), IdSource::Imported);
}

#[test]
fn both_spellings_map_to_one_lowercase_char_id() {
    let mut rng = CounterRng::new(8, 1);
    let id = assign(&mut rng, &BTreeSet::new(), None).unwrap();
    assert_eq!(id.to_char_id().unwrap().as_str(), id.as_str());
    assert_eq!(id.to_string(), id.as_str());
    assert_eq!(id.as_str(), id.uuid().to_string(), "assigned identities are lowercase");

    let upper = U1.to_uppercase();
    let imported_upper = assign(&mut rng, &BTreeSet::new(), Some(Qpid::parse(&upper).unwrap())).unwrap();
    let imported_lower = assign(&mut rng, &BTreeSet::new(), Some(Qpid::parse(U1).unwrap())).unwrap();
    assert_eq!(imported_upper.to_char_id().unwrap().as_str(), U1);
    assert_eq!(imported_upper.to_char_id(), imported_lower.to_char_id());
    // The verbatim text stays available for export only.
    assert_eq!(imported_upper.as_str(), upper);
    assert_eq!(imported_upper.qpid().to_string(), upper);
}

#[test]
fn error_display_and_codes_are_stable() {
    let u = Uuid::parse(U1).unwrap();
    let v = Uuid::parse(U2).unwrap();
    let e = IdentError::IdentityImmutable { current: u, attempted: v };
    assert_eq!(e.code(), "identity_immutable");
    assert_eq!(e.to_string(), format!("identity_immutable: {U1} cannot become {U2}"));
    assert_eq!(IdentError::MalformedQpid { reason: "not_uuid" }.to_string(), "malformed_qpid: not_uuid");
    assert_eq!(IdentError::Id(IdError::IdCollision(u)).code(), "id_collision");
    assert_eq!(IdentError::Id(IdError::IdCollision(u)).to_string(), format!("id_collision: {U1}"));
}
