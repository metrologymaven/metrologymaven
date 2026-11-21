//! Acceptance criteria of spec node IO.CAD.STEP.HDR (AC-1 .. AC-4), REQ-004 provenance, the
//! bounds, and adversarial inputs. Fixtures and expected results come from
//! `fixtures/io/cad/step/hdr/make_cases.py`, an independent stdlib Python generator and
//! reader; none are produced by this crate. The bound and fuzz cases are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_io_reg::prov::{principal_commit, LengthUnit, OsSalts, PrincipalDraft, PrincipalKind};
use fid_io_reg::{
    BoundedSource, Budget, Detection, Direction, EffectiveOptions, FormatDescriptor, FormatId, HashingSource,
    LimitRequest, Matcher, Origin, Phase, Principal, ReaderId, ReaderInfo, Registry, ReportBuilder, Signature,
    SourceProvenance, UnitSource, UntrustedText, VersionRange, VersionToken,
};
use fid_io_step::header::{HeaderBound, MAX_HEADER_LIST_ITEMS, MAX_UNITS_PER_CONTEXT};
use fid_io_step::{
    parse, resolve_header, AngleUnit, HeaderError, HeaderOptions, StepHeaderInfo, StepLengthUnit, StepSchema, StepText,
};
use std::collections::BTreeMap;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/io/cad/step/hdr")
}

fn read(name: &str) -> Vec<u8> {
    std::fs::read(dir().join(name)).unwrap()
}

fn opts(unit: Option<LengthUnit>, override_unit: bool) -> HeaderOptions {
    HeaderOptions { unit, override_unit, ..HeaderOptions::default() }
}

fn resolve(name: &str, unit: Option<LengthUnit>, override_unit: bool) -> Result<StepHeaderInfo, HeaderError> {
    let file = parse(&read(name)).unwrap();
    resolve_header(&file, &opts(unit, override_unit))
}

fn unit(sym: &str) -> Option<LengthUnit> {
    match sym {
        "none" => None,
        "mm" => Some(LengthUnit::Mm),
        "in" => Some(LengthUnit::In),
        "ft" => Some(LengthUnit::Ft),
        "m" => Some(LengthUnit::M),
        other => panic!("unit {other}"),
    }
}

fn unhex(h: &str) -> String {
    let bytes: Vec<u8> = (0..h.len()).step_by(2).map(|i| u8::from_str_radix(&h[i..i + 2], 16).unwrap()).collect();
    String::from_utf8(bytes).unwrap()
}

/// Renders a result in the oracle's line format. Text the crate offers only as untrusted
/// values (the originating system, a refused identifier) is compared with the oracle's hex
/// form by equality and written back as that hex when it matches.
fn render(result: &Result<StepHeaderInfo, HeaderError>, want: &[String]) -> Vec<String> {
    let want_hex = |tag: &str| -> Option<String> {
        want.iter().find_map(|l| l.split(tag).nth(1).map(|h| h.split(' ').next().unwrap().to_string()))
    };
    let mut out = Vec::new();
    match result {
        Ok(info) => {
            out.push(format!("schema {}", info.schema.as_str()));
            let hex = want_hex("system ").unwrap_or_default();
            let same = StepText::new(&unhex(&hex)) == info.originating_system;
            out.push(format!("system {}", if same { hex } else { "MISMATCH".into() }));
            for c in info.contexts.values() {
                let unc = match &c.uncertainty {
                    Some(u) => format!("{} {:016x}", u.authored, u.length_factor_m.to_bits()),
                    None => "none".into(),
                };
                out.push(format!(
                    "context {} unit {} factor {:016x} angle {} uncertainty {} source {} geometry {}",
                    c.context,
                    c.length_unit.symbol(),
                    c.length_factor_m.to_bits(),
                    c.angle.map_or("none", AngleUnit::as_str),
                    unc,
                    c.source.as_str(),
                    c.owns_geometry
                ));
            }
            for u in &info.unitless {
                out.push(format!("unitless {u}"));
            }
            for w in &info.warnings {
                out.push(format!("warning {} {}", w.entity, w.code));
            }
        }
        Err(e) => {
            let mut line = format!("error {}", e.code());
            if let Some(n) = e.entity() {
                line.push_str(&format!(" entity {n}"));
            }
            if let HeaderError::SchemaUnsupported { raw } = e {
                let hex = want_hex(" raw ").unwrap_or_default();
                let same = *raw == UntrustedText::new(&unhex(&hex));
                line.push_str(&format!(" raw {}", if same { hex } else { "MISMATCH".into() }));
            }
            out.push(line);
        }
    }
    out
}

#[test]
fn every_oracle_case_matches() {
    let text = String::from_utf8(read("expected.txt")).unwrap();
    let mut blocks: Vec<(String, Vec<String>)> = Vec::new();
    for line in text.lines() {
        if let Some(head) = line.strip_prefix("== ") {
            blocks.push((head.to_string(), Vec::new()));
        } else if !line.starts_with('#') && !line.is_empty() {
            blocks.last_mut().unwrap().1.push(line.to_string());
        }
    }
    assert_eq!(blocks.len(), 25);
    for (head, want) in &blocks {
        let mut it = head.split(' ');
        let name = it.next().unwrap();
        let u = unit(it.next().unwrap().strip_prefix("unit=").unwrap());
        let ov = it.next().unwrap() == "override=true";
        let got = render(&resolve(name, u, ov), want);
        assert_eq!(&got, want, "{head}");
    }
}

// ------------------------------------------------------------------ acceptance criteria

#[test]
fn ac1_ap242_millimetre_context_with_uncertainty() {
    let info = resolve("ap242_mm.stp", None, false).unwrap();
    assert_eq!(info.schema, StepSchema::Ap242);
    assert_eq!(info.contexts.len(), 1);
    let c = &info.contexts[&14];
    assert_eq!(c.length_factor_m.to_bits(), 0.001f64.to_bits());
    assert_eq!(c.length_unit, StepLengthUnit::Mm);
    assert_eq!(c.angle, Some(AngleUnit::Radian));
    let u = c.uncertainty.as_ref().unwrap();
    assert_eq!(u.value.to_bits(), 1e-6f64.to_bits());
    assert_eq!(u.unit, StepLengthUnit::Mm, "uncertainty is in file units");
    assert_eq!(c.source, UnitSource::Declared);
    assert!(info.warnings.is_empty());
}

#[test]
fn ac2_conversion_based_inch_is_exactly_0_0254() {
    let info = resolve("ap214_inch.stp", None, false).unwrap();
    assert_eq!(info.schema, StepSchema::Ap214);
    let c = &info.contexts[&30];
    assert_eq!(c.length_unit, StepLengthUnit::In);
    assert_eq!(c.length_factor_m.to_bits(), 0x3F9A_0275_2546_0AA6);
    assert_eq!(c.length_factor_m, 0.0254);
    assert_eq!(c.angle, Some(AngleUnit::Degree));
    // 25.4 * 0.001 agrees with 0.0254 within 1e-12 relative: no warning.
    assert!(info.warnings.is_empty());
}

#[test]
fn ac3_ap209_is_step_schema_unsupported() {
    let e = resolve("ap209.stp", None, false).unwrap_err();
    assert_eq!(e.code(), "step_schema_unsupported");
    match e {
        HeaderError::SchemaUnsupported { raw } => {
            assert_eq!(raw, UntrustedText::new("AP209_MULTIDISCIPLINARY_ANALYSIS_AND_DESIGN_MIM_LF"))
        }
        other => panic!("{other:?}"),
    }
}

#[test]
fn ac4_no_unit_needs_a_caller_unit_which_is_recorded_as_override() {
    assert_eq!(resolve("no_unit.stp", None, false).unwrap_err(), HeaderError::UnitRequired { context: Some(14) });
    let o = opts(Some(LengthUnit::Mm), false);
    let bytes = read("no_unit.stp");
    let file = parse(&bytes).unwrap();
    let info = resolve_header(&file, &o).unwrap();
    let c = &info.contexts[&14];
    assert_eq!(c.source, UnitSource::Caller);
    assert_eq!(c.declared, None);
    assert_eq!(c.length_factor_m, 0.001);
    let unit = info.provenance_unit(&o).unwrap();
    assert!(unit.overridden());
    assert_eq!(unit.unit(), LengthUnit::Mm);
    let record = provenance(&bytes, &info, &o);
    let json = String::from_utf8(record.canonical_bytes().unwrap()).unwrap();
    assert!(json.contains(r#""unit_overridden":true"#), "{json}");
    assert!(json.contains(r#""unit":"mm""#), "{json}");
}

// ------------------------------------------------------------------ REQ-004 provenance

fn provenance(bytes: &[u8], info: &StepHeaderInfo, o: &HeaderOptions) -> SourceProvenance {
    let budget = Budget::new(Principal::Agent, &LimitRequest::default()).unwrap();
    let src = HashingSource::new(BoundedSource::new(std::io::Cursor::new(bytes.to_vec()), budget)).unwrap();
    let (digest, _) = src.finish().unwrap();
    let name = FormatId::new("fid-test-step").unwrap();
    let mut registry = Registry::new();
    let d = FormatDescriptor::new(
        name.clone(),
        VersionRange::new("1", "1").unwrap(),
        vec![Signature::new(0, Matcher::Bytes(b"ISO-10303-21;".to_vec()), 10).unwrap()],
        vec!["stp".to_string()],
        CValue::Obj(BTreeMap::new()),
        Phase::new(0).unwrap(),
        Origin::FirstParty,
        Direction::Import,
    )
    .unwrap();
    registry.register(d).unwrap();
    let detection = Detection::detect(&registry, bytes, bytes.len() as u64, Some("f.stp"), Some(&name)).unwrap();
    let reader = ReaderInfo { id: ReaderId::new("fid-io-step").unwrap(), version: VersionToken::new("0.0.1").unwrap() };
    let draft =
        PrincipalDraft { kind: PrincipalKind::Agent, identity: CValue::Str("agent-7".into()), on_behalf_of: None };
    let principal = principal_commit(&draft, &mut OsSalts).unwrap();
    let mut b = SourceProvenance::builder(
        digest,
        detection,
        reader,
        principal,
        EffectiveOptions::empty(),
        info.provenance_unit(o).unwrap(),
    );
    info.record_provenance(&mut b).unwrap();
    b.build()
}

fn declared(record: &SourceProvenance, key: &str) -> UntrustedText {
    record.declared().get(key).cloned().unwrap_or_else(|| panic!("no {key}"))
}

#[test]
fn req004_schema_units_uncertainty_and_originating_system_are_recorded() {
    let bytes = read("ap242_mm.stp");
    let o = HeaderOptions::default();
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    let record = provenance(&bytes, &info, &o);
    assert_eq!(declared(&record, "step.schema"), UntrustedText::new("ap242"));
    assert_eq!(
        declared(&record, "step.schema_identifier"),
        UntrustedText::new("AP242_MANAGED_MODEL_BASED_3D_ENGINEERING_MIM_LF { 1 0 10303 442 1 1 4 }")
    );
    assert_eq!(declared(&record, "step.originating_system"), UntrustedText::new("fid synthetic generator"));
    assert_eq!(declared(&record, "step.author"), UntrustedText::new(r#"["Fixture Author","Second Author"]"#));
    assert_eq!(declared(&record, "step.contexts"), UntrustedText::new("1"));
    assert_eq!(
        declared(&record, "step.units.group_1"),
        UntrustedText::new(
            "length=mm factor_m=0.001 source=declared angle=radian uncertainty=1.E-06 mm geometry=true \
             contexts=1 first=#14"
        )
    );
    assert!(!record.unit().overridden());
    assert_eq!(record.unit().unit(), LengthUnit::Mm);
}

#[test]
fn mixed_contexts_are_grouped_and_flagged() {
    let bytes = read("multi_context.stp");
    let o = HeaderOptions::default();
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    assert_eq!(info.primary_context().unwrap().context, 30);
    let record = provenance(&bytes, &info, &o);
    assert_eq!(declared(&record, "step.units.mixed"), UntrustedText::new("true"));
    assert_eq!(declared(&record, "step.unitless_contexts"), UntrustedText::new("1"));
    assert!(record.declared().contains_key("step.units.group_2"));
    assert!(!record.declared().contains_key("step.units.group_3"));
}

#[test]
fn override_is_recorded_as_override() {
    let o = opts(Some(LengthUnit::In), true);
    let info = resolve("ap242_mm.stp", Some(LengthUnit::In), true).unwrap();
    let u = info.provenance_unit(&o).unwrap();
    assert_eq!(u.source(), UnitSource::Override);
    assert!(u.overridden());
}

#[test]
fn centimetre_resolves_but_has_no_provenance_unit() {
    let info = resolve("ap203_cm_deg.stp", None, false).unwrap();
    assert_eq!(info.contexts[&14].length_factor_m, 0.01);
    let e = info.provenance_unit(&HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::UnitUnsupported { entity: 14 });
}

#[test]
fn factor_warnings_reach_the_import_report() {
    let info = resolve("ap203e2_foot_warn.stp", None, false).unwrap();
    assert_eq!(info.contexts[&30].length_factor_m, 0.3048, "the exact factor is used");
    assert_eq!(info.warnings.len(), 2);
    let mut report = ReportBuilder::new();
    info.record_warnings(&mut report).unwrap();
    assert_eq!(report.warnings_total(), 2);
}

// ------------------------------------------------------------------ untrusted text

#[test]
fn a_refused_identifier_is_shown_escaped() {
    let e = resolve("evil_schema.stp", None, false).unwrap_err();
    let shown = e.to_string();
    assert!(!shown.contains('\u{202e}'), "{shown}");
    assert!(!shown.contains('\u{7}'), "{shown}");
    assert!(shown.starts_with("step_schema_unsupported: \""), "{shown}");
    assert!(shown.contains("\\u{202e}"), "{shown}");
    let dbg = format!("{e:?}");
    assert!(!dbg.contains('\u{202e}'), "{dbg}");
}

#[test]
fn a_quote_in_a_refused_identifier_cannot_close_the_quotes() {
    let src = wrap_schema("FILE_SCHEMA(('X\" injected: \"Y'));");
    let e = resolve_header(&parse(src.as_bytes()).unwrap(), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e.to_string(), "step_schema_unsupported: \"X\\\" injected: \\\"Y\"");
}

// ------------------------------------------------------------------ bounds and adversarial input

fn wrap_schema(schema_record: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\nFILE_NAME('','',(''),(''),'','','');\n\
         {schema_record}\nENDSEC;\nDATA;\n#1=CARTESIAN_POINT('',(0.,0.,0.));\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

fn wrap(header_extra: &str, body: &str) -> String {
    format!(
        "ISO-10303-21;\nHEADER;\nFILE_DESCRIPTION((''),'2;1');\n{header_extra}\
         FILE_SCHEMA(('CONFIG_CONTROL_DESIGN'));\nENDSEC;\nDATA;\n{body}\nENDSEC;\nEND-ISO-10303-21;\n"
    )
}

const NAME: &str = "FILE_NAME('','',(''),(''),'','','');\n";
const MM: &str = "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));";

fn run(text: &str, o: &HeaderOptions) -> Result<StepHeaderInfo, HeaderError> {
    resolve_header(&parse(text.as_bytes()).unwrap(), o)
}

#[test]
fn schema_matching_is_case_insensitive_and_by_whole_prefix() {
    use fid_io_step::header::match_schema;
    assert_eq!(match_schema("automotive_design"), Some(StepSchema::Ap214));
    assert_eq!(match_schema("AUTOMOTIVE_DESIGN_CC2"), Some(StepSchema::Ap214));
    assert_eq!(match_schema(" CONFIG_CONTROL_DESIGN {1 0}"), Some(StepSchema::Ap203));
    assert_eq!(match_schema("AUTOMOTIVE_DESIGNS"), None);
    assert_eq!(match_schema("AP238_STEP_NC"), None);
    assert_eq!(match_schema(""), None);
    // Non-ASCII case folding is not applied: the Kelvin sign is not a K.
    assert_eq!(match_schema("CONFIG_CONTROL_DESIGN\u{212a}"), None);
}

#[test]
fn context_count_is_bounded() {
    let body = format!("#1=CARTESIAN_POINT('',(0.,0.,0.));\n{MM}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));");
    let o = HeaderOptions { max_contexts: 0, ..HeaderOptions::default() };
    let e = run(&wrap(NAME, &body), &o).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::Contexts, value: 0 });
    assert!(run(&wrap(NAME, &body), &HeaderOptions::default()).is_ok());
}

#[test]
fn header_text_is_bounded() {
    let long = "A".repeat(5000);
    let name = format!("FILE_NAME('{long}','',(''),(''),'','','');\n");
    let o = HeaderOptions { max_header_text_bytes: 4096, ..HeaderOptions::default() };
    let e = run(&wrap(&name, ""), &o).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::HeaderText, value: 4096 });
}

#[test]
fn header_lists_are_bounded() {
    let authors = vec!["'a'"; MAX_HEADER_LIST_ITEMS + 1].join(",");
    let name = format!("FILE_NAME('','',({authors}),(''),'','','');\n");
    let e = run(&wrap(&name, ""), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e.code(), "limit_exceeded");
}

#[test]
fn unit_lists_are_bounded() {
    let refs = vec!["#10"; MAX_UNITS_PER_CONTEXT + 1].join(",");
    let body = format!(
        "{MM}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT(({refs}))REPRESENTATION_CONTEXT('',''));"
    );
    let e = run(&wrap(NAME, &body), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::Units, value: MAX_UNITS_PER_CONTEXT as u64 });
}

#[test]
fn structural_surprises_are_malformed_not_panics() {
    let cases = [
        // nested list instead of references
        format!("{MM}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((((#10))))REPRESENTATION_CONTEXT('',''));"),
        // conversion-based unit whose measure is itself
        "#23=(CONVERSION_BASED_UNIT('INCH',#23)LENGTH_UNIT()NAMED_UNIT(*));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#23))REPRESENTATION_CONTEXT('',''));".to_string(),
        // a unit that is both a length and an angle
        "#10=(LENGTH_UNIT()NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT(.MILLI.,.METRE.));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));".to_string(),
        // an uncertainty whose value is a string
        format!("{MM}\n#13=UNCERTAINTY_MEASURE_WITH_UNIT('tiny',#10,'','');\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
        // a negative uncertainty
        format!("{MM}\n#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(-1.),#10,'','');\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
        // a length unit with no definition
        "#10=(LENGTH_UNIT()NAMED_UNIT(*));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));".to_string(),
    ];
    for body in &cases {
        let text = wrap(NAME, body);
        let e = run(&text, &HeaderOptions::default()).unwrap_err();
        assert_eq!(e.code(), "malformed", "{body}: {e}");
    }
    // A context listing itself as its unit: not a unit, and a self-reference does not make
    // the context own geometry.
    let selfref =
        "#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#14))REPRESENTATION_CONTEXT('',''));";
    let info = run(&wrap(NAME, selfref), &HeaderOptions::default()).unwrap();
    assert_eq!(info.unitless, vec![14]);
    let shaped = format!(
        "{selfref}
#20=SHAPE_REPRESENTATION('',(),#14);"
    );
    let e = run(&wrap(NAME, &shaped), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::UnitRequired { context: Some(14) });
    // A FILE_NAME with the wrong arity, and a duplicated header record.
    let e = run(&wrap("FILE_NAME('');\n", ""), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::Malformed { entity: None, reason: "file_name_arity" });
    let e = run(&wrap(&format!("{NAME}{NAME}"), ""), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::Malformed { entity: None, reason: "duplicate_header_record" });
}

/// Deterministic xorshift64*.
struct Rng(u64);

impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 >> 12;
        self.0 ^= self.0 << 25;
        self.0 ^= self.0 >> 27;
        self.0.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % (n.max(1) as u64)) as usize
    }
}

const CODES: &[&str] = &[
    "step_schema_unsupported",
    "unit_required",
    "unit_conflict",
    "step_unit_unsupported",
    "malformed",
    "limit_exceeded",
];

fn exercise(text: &[u8]) -> usize {
    let Ok(file) = parse(text) else { return 0 };
    let mut ok = 0;
    for o in [
        HeaderOptions::default(),
        opts(Some(LengthUnit::Mm), false),
        opts(Some(LengthUnit::In), true),
        HeaderOptions { max_contexts: 1, max_header_text_bytes: 16, ..HeaderOptions::default() },
    ] {
        match resolve_header(&file, &o) {
            Ok(info) => {
                ok += 1;
                let _ = info.unit_groups();
                let _ = info.provenance_unit(&o).map_err(|e| e.to_string());
                let mut r = ReportBuilder::new();
                info.record_warnings(&mut r).unwrap();
                for c in info.contexts.values() {
                    assert!(c.length_factor_m.is_finite() && c.length_factor_m > 0.0);
                    let _ = c.signature();
                }
            }
            Err(e) => {
                assert!(CODES.contains(&e.code()), "{e:?}");
                let _ = e.to_string();
            }
        }
    }
    ok
}

#[test]
fn fuzz_mutations_of_every_fixture_never_panic() {
    const TOKENS: &[&[u8]] = &[
        b"#14",
        b"#10",
        b"#23",
        b"#999999999999",
        b"(",
        b")",
        b"((",
        b"'",
        b"$",
        b"*",
        b",",
        b".MILLI.",
        b".METRE.",
        b".RADIAN.",
        b"1.E-06",
        b"-0.",
        b"1.E308",
        b"SI_UNIT(",
        b"CONVERSION_BASED_UNIT('INCH',#21)",
        b"LENGTH_UNIT()",
        b"PLANE_ANGLE_UNIT()",
        b"GEOMETRIC_REPRESENTATION_CONTEXT(3)",
        b"\\X2\\202E\\X0\\",
    ];
    let mut names: Vec<String> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .filter(|n| n.ends_with(".stp"))
        .collect();
    names.sort();
    assert_eq!(names.len(), 19);
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);
    let mut resolved = 0;
    for name in &names {
        let base = read(name);
        resolved += exercise(&base);
        for _ in 0..200 {
            let mut m = base.clone();
            for _ in 0..=rng.below(3) {
                let at = rng.below(m.len());
                match rng.below(5) {
                    0 => m[at] = rng.next() as u8,
                    1 => {
                        let end = (at + rng.below(40)).min(m.len());
                        m.drain(at..end);
                    }
                    2 => {
                        let t = TOKENS[rng.below(TOKENS.len())];
                        m.splice(at..at, t.iter().copied());
                    }
                    3 => {
                        let end = (at + rng.below(80)).min(m.len());
                        let piece = m[at..end].to_vec();
                        m.splice(at..at, piece);
                    }
                    _ => m.truncate(at),
                }
                if m.is_empty() {
                    break;
                }
            }
            resolved += exercise(&m);
        }
    }
    // Some mutants still resolve, so the resolver (not only the parser) was exercised.
    assert!(resolved > 100, "{resolved}");
}

#[test]
fn random_unit_graphs_never_panic_or_loop() {
    let templates: &[&str] = &[
        "(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.))",
        "(NAMED_UNIT(*)PLANE_ANGLE_UNIT()SI_UNIT($,.RADIAN.))",
        "(CONVERSION_BASED_UNIT('INCH',#A)LENGTH_UNIT()NAMED_UNIT(#B))",
        "(CONVERSION_BASED_UNIT('DEGREE',#A)NAMED_UNIT(#B)PLANE_ANGLE_UNIT())",
        "LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(25.4),#A)",
        "UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-03),#A,'','')",
        "(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#A))GLOBAL_UNIT_ASSIGNED_CONTEXT((#A,#B))REPRESENTATION_CONTEXT('',''))",
        "SHAPE_REPRESENTATION('',(#A),#B)",
    ];
    let mut rng = Rng(0xD1B5_4A32_D192_ED03);
    let mut resolved = 0;
    for _ in 0..400 {
        let n = 2 + rng.below(12);
        let mut body = String::new();
        for i in 1..=n {
            let t = templates[rng.below(templates.len())];
            let a = 1 + rng.below(n);
            let b = 1 + rng.below(n);
            body.push_str(&format!("#{i}={};\n", t.replace("#A", &format!("#{a}")).replace("#B", &format!("#{b}"))));
        }
        resolved += exercise(wrap(NAME, &body).as_bytes());
    }
    assert!(resolved > 0);
}
