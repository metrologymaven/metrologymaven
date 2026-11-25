//! Acceptance criteria of spec node IO.CAD.PMI.READ (AC-1 .. AC-5), the bounds, and
//! malformed inputs. Fixtures and expected graphs come from
//! `fixtures/io/cad/pmi/read/make_cases.py`, an independent stdlib Python generator and
//! oracle; none are produced by this crate. The malformed and bound cases are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_io_pmi::{
    read_pmi, read_pmi_with, PmiBound, PmiClass, PmiError, PmiGraph, PmiItem, PmiKind, PmiOptions, PmiValue,
    UnsupportedReason, ValueUnit, IDENT_COST, INDEX_COST, ITEM_COST, LINK_COST, PARAM_COST, VALUE_COST,
};
use fid_io_step::{parse, resolve_header, HeaderOptions, Ident, Param, StepLengthUnit};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cad/pmi/read")
}

fn read_bytes(bytes: &[u8], opts: &PmiOptions) -> Result<PmiGraph, PmiError> {
    let file = parse(bytes).unwrap();
    let header = resolve_header(&file, &HeaderOptions::default()).unwrap();
    read_pmi_with(&file, &header, opts)
}

fn read_fixture(name: &str) -> PmiGraph {
    let bytes = std::fs::read(dir().join(name)).unwrap();
    let file = parse(&bytes).unwrap();
    let header = resolve_header(&file, &HeaderOptions::default()).unwrap();
    read_pmi(&file, &header).unwrap()
}

fn list(v: &[u64]) -> String {
    if v.is_empty() {
        "-".to_string()
    } else {
        v.iter().map(u64::to_string).collect::<Vec<_>>().join(",")
    }
}

fn opt<T: ToString>(v: &Option<T>) -> String {
    v.as_ref().map_or("-".to_string(), ToString::to_string)
}

fn value(v: &PmiValue) -> String {
    format!(
        "{}:{}:{}:{}:{}:{}",
        v.measure,
        v.measure_type.as_ref().map_or("-", Ident::as_str),
        v.lexeme.as_deref().unwrap_or("-"),
        opt(&v.unit_entity),
        opt(&v.unit_ctx),
        v.unit.as_str()
    )
}

fn line(i: &PmiItem) -> String {
    let values =
        if i.values.is_empty() { "-".to_string() } else { i.values.iter().map(value).collect::<Vec<_>>().join(";") };
    format!(
        "item {} class={} kind={} reason={} types={} links={} unresolved={} refs={} presented_by={} values={}",
        i.entity,
        i.class.as_str(),
        i.kind.as_str(),
        i.unsupported.map_or("-", UnsupportedReason::as_str),
        i.types.iter().map(Ident::as_str).collect::<Vec<_>>().join("+"),
        list(&i.links),
        list(&i.link_unresolved),
        list(&i.refs),
        list(&i.presented_by),
        values
    )
}

/// `expected.txt` blocks: fixture name to its lines.
fn expected() -> BTreeMap<String, Vec<String>> {
    let text = std::fs::read_to_string(dir().join("expected.txt")).unwrap();
    let mut out: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut cur = None;
    for l in text.lines() {
        if l.starts_with('#') || l.is_empty() {
            continue;
        }
        if let Some(name) = l.strip_prefix("== ") {
            cur = Some(name.to_string());
            out.insert(name.to_string(), Vec::new());
        } else {
            out.get_mut(cur.as_ref().unwrap()).unwrap().push(l.to_string());
        }
    }
    out
}

fn scan_count(lines: &[String]) -> usize {
    lines.iter().find_map(|l| l.strip_prefix("scan ")).unwrap().parse().unwrap()
}

#[test]
fn every_fixture_matches_the_oracle() {
    let exp = expected();
    assert_eq!(exp.len(), 6);
    for (name, lines) in &exp {
        let g = read_fixture(name);
        let mut got: Vec<String> = g.items.values().map(line).collect();
        got.push(format!("count {}", g.items.len()));
        let want: Vec<String> = lines.iter().filter(|l| !l.starts_with("scan ")).cloned().collect();
        assert_eq!(got, want, "{name}");
    }
}

#[test]
fn ac1_flatness_semantic() {
    let g = read_fixture("flatness_semantic.stp");
    let tols: Vec<_> = g.items.values().filter(|i| i.kind == PmiKind::Tolerance).collect();
    assert_eq!(tols.len(), 1);
    let t = tols[0];
    assert_eq!(t.class, PmiClass::Semantic);
    assert_eq!(t.links, vec![30]);
    assert_eq!(t.values.len(), 1);
    assert_eq!(t.values[0].lexeme.as_deref(), Some("0.05"));
    assert_eq!(t.values[0].unit, ValueUnit::Length(StepLengthUnit::Mm));
    assert_eq!(t.values[0].unit_ctx, Some(14));
    assert_eq!(t.presented_by, vec![74]);
    let p = &g.items[&74];
    assert_eq!(p.kind, PmiKind::Presentation);
    assert_eq!(p.class, PmiClass::Semantic);
    assert_eq!(p.refs, vec![t.entity]);
    assert_eq!(g.count(PmiClass::GraphicOnly), 0);
}

#[test]
fn association_to_a_bare_shape_aspect_stays_graphic_only() {
    // A shape aspect is not a characteristic, so presenting it does not make the
    // presentation semantic.
    let g = read_fixture("aspect_association.stp");
    assert_eq!(g.items[&74].class, PmiClass::GraphicOnly);
    assert!(g.items[&74].refs.is_empty());
    assert!(g.items[&60].presented_by.is_empty());
    assert_eq!(g.items[&60].links, vec![30]);
}

#[test]
fn ac2_graphic_only() {
    let g = read_fixture("graphic_only.stp");
    assert_eq!(g.count(PmiClass::GraphicOnly), 1);
    assert_eq!(g.count(PmiClass::Semantic), 0);
    assert_eq!(g.count(PmiClass::Unsupported), 0);
    let p = g.with_class(PmiClass::GraphicOnly).next().unwrap();
    // REQ-002: no characteristic data.
    assert!(p.values.is_empty() && p.links.is_empty() && p.refs.is_empty());
}

#[test]
fn ac3_unknown_complex() {
    let g = read_fixture("unknown_complex.stp");
    let u: Vec<_> = g.with_class(PmiClass::Unsupported).collect();
    assert_eq!(u.len(), 1);
    let names: Vec<&str> = u[0].types.iter().map(Ident::as_str).collect();
    assert_eq!(
        names,
        [
            "GEOMETRIC_TOLERANCE",
            "GEOMETRIC_TOLERANCE_WITH_MAXIMUM_TOLERANCE",
            "GEOMETRIC_TOLERANCE_WITH_MODIFIERS",
            "STRAIGHTNESS_TOLERANCE"
        ]
    );
    assert_eq!(u[0].unsupported, Some(UnsupportedReason::TypeNotSupported));
    assert!(u[0].values.is_empty());
}

#[test]
fn ac4_dangling_aspect() {
    let g = read_fixture("dangling_aspect.stp");
    let t = &g.items[&63];
    assert_eq!(t.class, PmiClass::Semantic);
    assert_eq!(t.link_unresolved, vec![999]);
    assert!(t.links.is_empty());
}

#[test]
fn ac5_completeness_against_independent_scan() {
    for (name, lines) in &expected() {
        let g = read_fixture(name);
        assert_eq!(g.items.len(), scan_count(lines), "{name}");
    }
}

#[test]
fn deterministic() {
    for name in expected().keys() {
        assert_eq!(read_fixture(name), read_fixture(name));
    }
}

const HEAD: &str = "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('t','',(''),(''),'','','');\n\
FILE_SCHEMA(('AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF'));\nENDSEC;\nDATA;\n\
#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n\
#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('c','s'));\n\
#20=SHAPE_REPRESENTATION('s',(),#14);\n";

fn file(body: &str) -> Vec<u8> {
    format!("{HEAD}{body}ENDSEC;\nEND-ISO-10303-21;\n").into_bytes()
}

fn malformed(body: &str) -> (u64, &'static str) {
    match read_bytes(&file(body), &PmiOptions::default()) {
        Err(PmiError::Malformed { entity, reason }) => (entity, reason),
        other => panic!("{other:?}"),
    }
}

#[test]
fn malformed_references_fail_closed() {
    // Tolerance without its toleranced aspect.
    assert_eq!(malformed("#1=FLATNESS_TOLERANCE('f','',$);\n"), (1, "tolerance_target"));
    // Target is not a reference.
    assert_eq!(malformed("#1=FLATNESS_TOLERANCE('f','',$,5);\n"), (1, "tolerance_target"));
    // Complex tolerance without the root part.
    assert_eq!(
        malformed("#1=(FLATNESS_TOLERANCE()GEOMETRIC_TOLERANCE_WITH_MODIFIERS(()));\n"),
        (1, "tolerance_target")
    );
    // Usage whose identified item is not a reference.
    assert_eq!(
        malformed("#2=SHAPE_ASPECT('a','',$,.T.);\n#3=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#2,#20,'x');\n"),
        (3, "usage_item")
    );
    assert_eq!(
        malformed("#2=SHAPE_ASPECT('a','',$,.T.);\n#3=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#2,#20,(#20,1));\n"),
        (3, "usage_item")
    );
    assert_eq!(malformed("#3=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',$,#20,#20);\n"), (3, "usage_definition"));
    // Relationship whose related side is unset.
    assert_eq!(malformed("#3=SHAPE_ASPECT_RELATIONSHIP('','',#20,$);\n"), (3, "relationship_related"));
    // Measure without a unit reference.
    assert_eq!(
        malformed(
            "#2=SHAPE_ASPECT('a','',$,.T.);\n#4=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.),$);\n\
             #5=FLATNESS_TOLERANCE('f','',#4,#2);\n"
        ),
        (4, "measure_unit")
    );
    // Dimension representation whose items are not a list.
    assert_eq!(
        malformed(
            "#2=SHAPE_ASPECT('a','',$,.T.);\n#6=DIMENSIONAL_SIZE(#2,'d');\n#7=SHAPE_DIMENSION_REPRESENTATION('',#2,#14);\n\
             #8=DIMENSIONAL_CHARACTERISTIC_REPRESENTATION(#6,#7);\n"
        ),
        (7, "representation_items")
    );
    // Linked callout whose contents are not a list.
    assert_eq!(
        malformed(
            "#2=SHAPE_ASPECT('a','',$,.T.);\n#9=DRAUGHTING_CALLOUT('c',#20);\n\
             #5=FLATNESS_TOLERANCE('f','',$,#2);\n#11=DRAUGHTING_MODEL_ITEM_ASSOCIATION('','',#5,#20,#9);\n"
        ),
        (9, "callout_contents")
    );
}

#[test]
fn bounds_are_enforced() {
    let body = "#2=SHAPE_ASPECT('a','',$,.T.);\n#3=SHAPE_ASPECT('b','',$,.T.);\n";
    let one = PmiOptions { max_items: 1, ..PmiOptions::default() };
    assert_eq!(read_bytes(&file(body), &one), Err(PmiError::LimitExceeded { bound: PmiBound::Items, value: 1 }));
    let two = PmiOptions { max_items: 2, ..PmiOptions::default() };
    assert_eq!(read_bytes(&file(body), &two).unwrap().items.len(), 2);
    let alloc = PmiOptions { max_alloc_bytes: 600, ..PmiOptions::default() };
    assert_eq!(read_bytes(&file(body), &alloc), Err(PmiError::LimitExceeded { bound: PmiBound::Alloc, value: 600 }));
    let work = PmiOptions { max_work: 5, ..PmiOptions::default() };
    assert_eq!(read_bytes(&file(body), &work), Err(PmiError::LimitExceeded { bound: PmiBound::Work, value: 5 }));
}

#[test]
fn self_referencing_composites_terminate() {
    // A composite aspect that is its own member, and a relationship cycle.
    let body = "#30=ADVANCED_FACE('f',(),$,.T.);\n#2=COMPOSITE_SHAPE_ASPECT('a','',$,.T.);\n\
                #3=SHAPE_ASPECT_RELATIONSHIP('','',#2,#2);\n#4=COMPOSITE_SHAPE_ASPECT('b','',$,.T.);\n\
                #5=SHAPE_ASPECT_RELATIONSHIP('','',#2,#4);\n#6=SHAPE_ASPECT_RELATIONSHIP('','',#4,#2);\n\
                #7=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#4,#20,#30);\n";
    let g = read_bytes(&file(body), &PmiOptions::default()).unwrap();
    assert_eq!(g.items[&2].links, vec![30]);
    assert_eq!(g.items[&4].links, vec![30]);
}

#[test]
fn nominal_costs_cover_the_real_sizes() {
    let u = |n: usize| u64::try_from(n).unwrap();
    // An item sits in a BTreeMap entry with its key.
    assert!(ITEM_COST >= u(std::mem::size_of::<PmiItem>() + std::mem::size_of::<u64>()));
    // Raw parameters and values sit in growing Vecs (up to twice the length).
    assert!(PARAM_COST >= u(2 * std::mem::size_of::<Param>()));
    assert!(VALUE_COST >= u(2 * std::mem::size_of::<PmiValue>()));
    assert!(IDENT_COST >= u(2 * std::mem::size_of::<Ident>()));
    assert!(LINK_COST >= u(2 * std::mem::size_of::<u64>()));
    // An index entry: key, a Vec header or a second key, and node share.
    assert!(INDEX_COST >= u(std::mem::size_of::<u64>() + std::mem::size_of::<Vec<u64>>()));
}

#[test]
fn ten_thousand_items_smoke() {
    // Performance budget 10^4 PMI items < 1 s is [unverified until measured]; this only
    // checks that a file of that size reads within the default bounds.
    let mut body = String::new();
    body.push_str("#30=ADVANCED_FACE('f',(),$,.T.);\n#40=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.1),#10);\n");
    for k in 0..5000u64 {
        let a = 1000 + 3 * k;
        body.push_str(&format!(
            "#{a}=SHAPE_ASPECT('a','',$,.T.);\n#{}=GEOMETRIC_ITEM_SPECIFIC_USAGE('','',#{a},#20,#30);\n\
             #{}=POSITION_TOLERANCE('p','',#40,#{a});\n",
            a + 1,
            a + 2
        ));
    }
    let start = std::time::Instant::now();
    let g = read_bytes(&file(&body), &PmiOptions::default()).unwrap();
    eprintln!("10^4 items read in {:?}", start.elapsed());
    assert_eq!(g.items.len(), 10_000);
    assert!(g.items.values().all(|i| i.links == [30]));
}
