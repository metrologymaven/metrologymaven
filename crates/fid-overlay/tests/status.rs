//! Acceptance criteria of spec node VIZ.OVERLAY.STATUS (AC-1 .. AC-5, AC-7) and further cases.
//! Expected results are in `fixtures/viz/status-matrix/expected.json`, written by
//! `make_fixtures.py` from the leaf's hand-labelled tables and the colours of the shipped
//! `design/tokens/` files, with its own CIEDE2000 and colour-vision simulation for the guard;
//! never by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_overlay::status::colour::to_hex;
use fid_overlay::status::{
    from_display_status, out_of_tolerance_cue, pattern_def, provenance_badge, status_glyph, DisplayState, Evidence,
    GdtResultItem, Interpretation, OverlayItem, Palette, PaletteGuardConfig, PatternKind, Provenance, Side, Skin, Slot,
    StatusError, Theme, PATTERNS,
};
use fid_render::scene::proj::DisplayStatus;
use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn viz(rel: &str) -> PathBuf {
    repo().join("fixtures/viz").join(rel)
}

fn expected() -> CValue {
    fid_canon::parse(&std::fs::read(viz("status-matrix/expected.json")).unwrap()).unwrap()
}

fn get<'a>(v: &'a CValue, path: &[&str]) -> &'a CValue {
    let mut cur = v;
    for k in path {
        match cur {
            CValue::Obj(m) => cur = m.get(*k).unwrap_or_else(|| panic!("missing key {k}")),
            _ => panic!("not an object at {k}"),
        }
    }
    cur
}

fn s(v: &CValue) -> &str {
    match v {
        CValue::Str(x) => x,
        other => panic!("not a string: {other:?}"),
    }
}

fn arr(v: &CValue) -> &[CValue] {
    match v {
        CValue::Arr(a) => a,
        other => panic!("not an array: {other:?}"),
    }
}

fn canon(v: &CValue) -> Vec<u8> {
    fid_canon::to_canonical_bytes(v).unwrap()
}

fn state(name: &str) -> DisplayState {
    DisplayState::parse(name).unwrap()
}

fn provenance(name: &str) -> Provenance {
    Provenance::ALL.into_iter().find(|p| p.name() == name).unwrap()
}

fn theme(name: &str) -> Theme {
    Theme::parse(name).unwrap()
}

/// The locked palette equals the shipped status palette and out-of-range colours.
#[test]
fn locked_palette_is_the_shipped_one() {
    let e = expected();
    let p = Palette::locked();
    for t in Theme::ALL {
        for slot in Slot::ALL {
            assert_eq!(to_hex(p.colour(slot, t)), s(get(&e, &["locked", t.name(), slot.name()])), "{t:?} {slot:?}");
        }
    }
    assert!(p.violation(&PaletteGuardConfig::default()).is_none());
}

/// AC-1: the 6 x 5 x 4 matrix renders exactly the oracle's spec; every state has a unique glyph,
/// a unique pattern and a non-empty text, identical across themes except the colour.
#[test]
fn ac1_status_matrix() {
    let e = expected();
    let palette = Palette::locked();
    let matrix = arr(get(&e, &["matrix"]));
    assert_eq!(matrix.len(), 6 * 5 * 4);
    for m in matrix {
        let item = OverlayItem::status(state(s(get(m, &["state"]))))
            .number(1.25)
            .provenance(provenance(s(get(m, &["provenance"]))))
            .build()
            .unwrap();
        let r = item.render(&palette, theme(s(get(m, &["theme"]))));
        assert_eq!(r.to_canonical_bytes().unwrap(), canon(get(m, &["render"])), "{m:?}");
    }
    let mut glyphs = BTreeSet::new();
    let mut patterns = BTreeSet::new();
    for st in DisplayState::ALL {
        let base = status_glyph(st, Theme::Light);
        assert!(!base.text.is_empty() && !base.text_key.is_empty());
        glyphs.insert(base.glyph);
        patterns.insert(base.pattern);
        let mut colours = BTreeSet::new();
        for t in Theme::ALL {
            let g = status_glyph(st, t);
            assert_eq!((g.glyph, g.pattern, g.text_key, g.text), (base.glyph, base.pattern, base.text_key, base.text));
            colours.insert(g.color);
        }
        assert!(!colours.is_empty());
    }
    assert_eq!(glyphs.len(), 6);
    assert_eq!(patterns.len(), 6);
}

fn skin(name: &str) -> Skin {
    Skin::parse(&std::fs::read(viz(name)).unwrap()).unwrap()
}

fn is_schema_skin(name: &str) -> bool {
    get(&expected(), &["skins", name, "schema"]) == &CValue::Bool(true)
}

fn violation_of(name: &str) -> Option<(String, String)> {
    let e = expected();
    match get(&e, &["skins", name, "violation"]) {
        CValue::Null => None,
        CValue::Arr(a) => Some((s(&a[0]).to_string(), s(&a[1]).to_string())),
        other => panic!("{other:?}"),
    }
}

fn guard_case(name: &str) {
    if is_schema_skin(name) {
        let got = Skin::parse(&std::fs::read(viz(name)).unwrap());
        assert!(matches!(got, Err(StatusError::SkinSchema(_))), "{name}: {got:?}");
        return;
    }
    let mut p = Palette::locked();
    let got = p.apply(&skin(name), &PaletteGuardConfig::default());
    match violation_of(name) {
        None => {
            assert_eq!(got, Ok(()), "{name}");
            assert_ne!(p, Palette::locked(), "{name}: the skin is taken");
        }
        Some((slot, t)) => {
            let Err(StatusError::LockedPaletteViolation { state, theme: th }) = got else { panic!("{name}: {got:?}") };
            assert_eq!(state.to_string(), slot, "{name}");
            assert_eq!(th.name(), t, "{name}");
            assert_eq!(p, Palette::locked(), "{name}: the locked default stays active");
        }
    }
}

/// AC-2: NONCONFORMS set to the conforming azure is refused, and the palette stays the locked
/// default.
#[test]
fn ac2_azure_nonconform_skin_is_refused() {
    guard_case("skin-azure-nonconform.json");
    let mut p = Palette::locked();
    let err = p.apply(&skin("skin-azure-nonconform.json"), &PaletteGuardConfig::default()).unwrap_err();
    assert_eq!(
        err,
        StatusError::LockedPaletteViolation { state: Slot::State(DisplayState::Nonconforms), theme: Theme::Light }
    );
    assert!(err.to_string().starts_with("LockedPaletteViolation { state: NONCONFORMS"), "{err}");
}

/// AC-3: with the threshold 20, INDETERMINATE near the azure CONFORMS is refused.
#[test]
fn ac3_near_azure_indeterminate_is_refused() {
    let cfg = PaletteGuardConfig::default();
    assert_eq!(cfg.min_delta_e2000, 20.0);
    assert_eq!(get(&expected(), &["guard", "min_delta_e2000"]), &CValue::Num(20.0));
    guard_case("skin-near-azure-indeterminate.json");
    // The same skin passes a guard configured below its distance.
    let mut p = Palette::locked();
    let loose = PaletteGuardConfig { min_delta_e2000: 1.0, ..cfg };
    assert_eq!(p.apply(&skin("skin-near-azure-indeterminate.json"), &loose), Ok(()));
}

/// REQ-002, REQ-003 and REQ-VIZ.OVERLAY-006: the other skins.
#[test]
fn guard_cases() {
    for name in [
        "skin-azure-over-tolerance.json",
        "skin-stale-as-nonconforms.json",
        "skin-not-evaluated-as-stale.json",
        "skin-darker-red.json",
        "skin-inversion.json",
        "skin-stale-near-nonconforms.json",
    ] {
        guard_case(name);
    }
    let e = expected();
    let CValue::Obj(m) = get(&e, &["skins"]) else { panic!() };
    assert_eq!(m.len(), 8);
}

/// PR 355: a skin may not set CONFORMS (no palette inversion), not even when built in code, and
/// distances are measured against the locked CONFORMS.
#[test]
fn conforms_is_locked() {
    let mut s = Skin::default();
    s.colours.insert((Theme::Light, Slot::State(DisplayState::Conforms)), [0xc7, 0x2d, 0x2e]);
    s.colours.insert((Theme::Light, Slot::State(DisplayState::Nonconforms)), [0x00, 0x63, 0x7e]);
    let mut p = Palette::locked();
    assert!(matches!(p.apply(&s, &PaletteGuardConfig::default()), Err(StatusError::SkinSchema(_))));
    assert_eq!(p, Palette::locked());
    assert!(is_schema_skin("skin-inversion.json"));
}

#[test]
fn skin_schema_errors() {
    assert!(matches!(Skin::parse(b"{}"), Err(StatusError::SkinSchema(_))));
    let unknown = br##"{"format":"status-skin@1","themes":{"print":{"stale":"#000000"}}}"##;
    assert_eq!(Skin::parse(unknown), Err(StatusError::UnknownTheme("print".to_string())));
    let slot = br##"{"format":"status-skin@1","themes":{"light":{"error":"#000000"}}}"##;
    assert!(matches!(Skin::parse(slot), Err(StatusError::SkinSchema(_))));
    let upper = br##"{"format":"status-skin@1","themes":{"light":{"stale":"#AABBCC"}}}"##;
    assert!(matches!(Skin::parse(upper), Err(StatusError::SkinSchema(_))));
    let dup = br##"{"format":"status-skin@1","themes":{"light":{"stale":"#000000","stale":"#111111"}}}"##;
    assert!(matches!(Skin::parse(dup), Err(StatusError::SkinSchema(_))));
    assert_eq!(Theme::parse("Light"), Err(StatusError::UnknownTheme("Light".to_string())));
}

/// AC-4: a number without a provenance cannot be built; nor a claimed value badged measured.
#[test]
fn ac4_missing_provenance() {
    let b = OverlayItem::status(DisplayState::Conforms).number(0.5);
    assert_eq!(b.build(), Err(StatusError::MissingProvenance));
    assert_eq!(
        OverlayItem::status(DisplayState::Stale).provenance(Provenance::Measured).build(),
        Err(StatusError::ProvenanceWithoutNumber)
    );
    assert_eq!(
        OverlayItem::status(DisplayState::Conforms).number(f64::NAN).provenance(Provenance::Computed).build(),
        Err(StatusError::NonFiniteNumber)
    );
    assert_eq!(
        OverlayItem::status(DisplayState::Conforms).claimed(true).number(1.0).provenance(Provenance::Measured).build(),
        Err(StatusError::ClaimedAsMeasured)
    );
    for e in [
        Evidence { declared_input: true, ..Evidence::default() },
        Evidence { uncharacterized_input: true, ..Evidence::default() },
    ] {
        assert_eq!(
            OverlayItem::status(DisplayState::Conforms)
                .evidence(e)
                .number(1.0)
                .provenance(Provenance::Measured)
                .build(),
            Err(StatusError::ClaimedAsMeasured)
        );
    }
    assert!(OverlayItem::status(DisplayState::Conforms).build().is_ok());
}

/// AC-5: the same inputs give identical bytes and digests.
#[test]
fn ac5_pure() {
    let item = GdtResultItem::new(DisplayState::Indeterminate, Interpretation::Provisional)
        .number(-0.003)
        .provenance(Provenance::Computed)
        .build()
        .unwrap();
    let a = item.render(&Palette::locked(), Theme::Floor);
    let b = item.render(&Palette::locked(), Theme::Floor);
    assert_eq!(a.to_canonical_bytes().unwrap(), b.to_canonical_bytes().unwrap());
    assert_eq!(a.digest().unwrap(), b.digest().unwrap());
    assert!(a.digest().unwrap().starts_with("sha256:"));
}

fn scene(wire: &str) -> (DisplayState, Evidence) {
    from_display_status(&DisplayStatus::parse(wire).unwrap()).unwrap()
}

/// AC-7 (structure), the claimed rule and the kernel evidence facts: the hand-labelled cases
/// render exactly.
#[test]
fn ac7_cases_render_as_labelled() {
    let e = expected();
    let palette = Palette::locked();
    let theme_of = |name: &str| theme(s(get(get(&e, &["cases", name]), &["theme"])));
    let mut got: BTreeMap<&str, Vec<u8>> = BTreeMap::new();
    let item = |b: fid_overlay::status::ItemBuilder, name: &str| {
        b.build().unwrap().render(&palette, theme_of(name)).to_canonical_bytes().unwrap()
    };
    got.insert(
        "claimed-computed",
        item(
            OverlayItem::status(DisplayState::Conforms).claimed(true).number(1.25).provenance(Provenance::Computed),
            "claimed-computed",
        ),
    );
    got.insert("status-only", item(OverlayItem::status(DisplayState::Stale), "status-only"));
    let (st, ev) = scene("kernel:INDETERMINATE:-du");
    assert_eq!(ev, Evidence { claimed: false, declared_input: true, uncharacterized_input: true });
    got.insert(
        "evidence-indeterminate-du",
        item(
            OverlayItem::status(st).evidence(ev).number(1.25).provenance(Provenance::Computed),
            "evidence-indeterminate-du",
        ),
    );
    let (st, ev) = scene("kernel:CONFORMS:-d-");
    got.insert(
        "evidence-declared-status-only",
        item(OverlayItem::status(st).evidence(ev), "evidence-declared-status-only"),
    );
    let (st, ev) = scene("kernel:NOT_DECIDABLE(u_missing):---");
    got.insert(
        "not-decidable-entered",
        item(
            OverlayItem::status(st).evidence(ev).number(1.25).provenance(Provenance::Entered),
            "not-decidable-entered",
        ),
    );
    let gdt = |b: fid_overlay::status::GdtResultBuilder, name: &str| {
        b.build().unwrap().render(&palette, theme_of(name)).to_canonical_bytes().unwrap()
    };
    got.insert(
        "gdt-provisional",
        gdt(
            GdtResultItem::new(DisplayState::Nonconforms, Interpretation::Provisional)
                .number(1.25)
                .provenance(Provenance::Computed),
            "gdt-provisional",
        ),
    );
    got.insert(
        "gdt-signed-off",
        gdt(
            GdtResultItem::new(DisplayState::Conforms, Interpretation::SignedOff)
                .number(1.25)
                .provenance(Provenance::Measured),
            "gdt-signed-off",
        ),
    );
    let CValue::Obj(cases) = get(&e, &["cases"]) else { panic!() };
    assert_eq!(cases.len(), got.len());
    for (name, bytes) in &got {
        assert_eq!(bytes, &canon(get(&e, &["cases", name])), "{name}");
    }
    // A value with the claimed styling never gets the measured role, whatever its provenance.
    for p in Provenance::ALL.into_iter().filter(|p| *p != Provenance::Measured) {
        for ev in [
            Evidence { claimed: true, ..Evidence::default() },
            Evidence { declared_input: true, ..Evidence::default() },
            Evidence { uncharacterized_input: true, ..Evidence::default() },
        ] {
            let it =
                OverlayItem::status(DisplayState::Conforms).evidence(ev).number(2.0).provenance(p).build().unwrap();
            let r = it.render(&palette, Theme::Light);
            assert!(r.claimed_styling());
            let v = r.to_cvalue();
            assert_eq!(s(get(&v, &["value", "role"])), "sem.text.value.claimed", "{p:?} {ev:?}");
            let labels: Vec<&str> = arr(get(&v, &["labels"])).iter().map(|l| s(get(l, &["text"]))).collect();
            assert!(labels.contains(&"claimed"), "{p:?} {ev:?}");
        }
    }
}

/// The oracle's `sha256:` digests of three matrix rows are pinned.
#[test]
fn pinned_digests() {
    let e = expected();
    let rows = arr(get(&e, &["digests"]));
    assert_eq!(rows.len(), 3);
    for w in rows {
        let item = OverlayItem::status(state(s(get(w, &["state"]))))
            .number(1.25)
            .provenance(provenance(s(get(w, &["provenance"]))))
            .build()
            .unwrap();
        let r = item.render(&Palette::locked(), theme(s(get(w, &["theme"]))));
        assert_eq!(r.digest().unwrap(), s(get(w, &["digest"])));
    }
}

/// The pattern definitions equal the design's, are distinct, and the two out-of-tolerance sides
/// refine the generic cue; the region cues equal the oracle's.
#[test]
fn patterns_and_region_cues() {
    let e = expected();
    let CValue::Obj(want) = get(&e, &["patterns"]) else { panic!() };
    assert_eq!(want.len(), PATTERNS.len());
    let num = |v: &CValue, k: &str| match v {
        CValue::Obj(m) => match m.get(k) {
            Some(CValue::Num(n)) => *n as u16,
            _ => panic!("{k}"),
        },
        _ => panic!(),
    };
    for p in PATTERNS {
        let w = &want[p.id];
        let kind = s(get(w, &["kind"]));
        match p.kind {
            PatternKind::Solid => assert_eq!(kind, "solid"),
            PatternKind::Hatch { angle, period, width, dash } => {
                assert_eq!(kind, "hatch");
                assert_eq!(
                    (angle, period.into(), width.into(), dash.into()),
                    (num(w, "angle"), num(w, "period"), num(w, "width"), num(w, "dash"))
                );
            }
            PatternKind::Cross { period, width } => {
                assert_eq!(kind, "cross");
                assert_eq!((u16::from(period), u16::from(width)), (num(w, "period"), num(w, "width")));
            }
            PatternKind::Dots { period, radius } => {
                assert_eq!(kind, "dots");
                assert_eq!((u16::from(period), u16::from(radius)), (num(w, "period"), num(w, "radius")));
            }
        }
        let refines = match w {
            CValue::Obj(m) => m.get("refines").map(|r| s(r).to_string()),
            _ => None,
        };
        assert_eq!(p.refines.map(str::to_string), refines, "{}", p.id);
    }
    assert_ne!(
        pattern_def("pattern.under-tolerance").unwrap().kind,
        pattern_def("pattern.over-tolerance").unwrap().kind
    );
    for t in Theme::ALL {
        for (side, k) in [(Side::Under, "under"), (Side::Over, "over")] {
            let c = out_of_tolerance_cue(side, t);
            let w = get(&e, &["regions", t.name(), k]);
            assert_eq!(c.pattern, s(get(w, &["pattern"])));
            assert_eq!(c.text_key, s(get(w, &["text_key"])));
            assert_eq!(c.text, s(get(w, &["text"])));
            assert_eq!(to_hex(c.color), s(get(w, &["color"])));
        }
    }
}

/// The scene's display status (VIZ.RENDER.SCENE.PROJ) maps to a display state, keeping the
/// claimed flag; a kernel error has no state.
#[test]
fn from_scene_display_status() {
    let cases = [
        ("kernel:CONFORMS:---", DisplayState::Conforms, false),
        ("kernel:NONCONFORMS:c--", DisplayState::Nonconforms, true),
        ("kernel:INDETERMINATE:-du", DisplayState::Indeterminate, false),
        ("kernel:NOT_DECIDABLE(u_missing):c--", DisplayState::NotDecidable, true),
        ("not_evaluated", DisplayState::NotEvaluated, false),
    ];
    for (wire, want, claimed) in cases {
        let d = DisplayStatus::parse(wire).unwrap();
        let (st, ev) = from_display_status(&d).unwrap();
        assert_eq!((st, ev.claimed), (want, claimed), "{wire}");
    }
    let (_, ev) = scene("kernel:INDETERMINATE:-du");
    assert!(ev.declared_input && ev.uncharacterized_input && !ev.claimed);
    let err = DisplayStatus::parse("error:kdec.overflow").unwrap();
    assert_eq!(from_display_status(&err), Err(StatusError::KernelError("kdec.overflow".to_string())));
}

#[test]
fn badges_are_distinct() {
    let glyphs: BTreeSet<&str> = Provenance::ALL.iter().map(|p| provenance_badge(*p).glyph).collect();
    assert_eq!(glyphs.len(), 5);
}
