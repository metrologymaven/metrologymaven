//! Acceptance criteria of spec node IO.CAD.STEP.HDR (AC-1 .. AC-4), REQ-004 provenance, the
//! bounds, and adversarial inputs. Fixtures and expected results come from
//! `fixtures/io/cad/step/hdr/make_cases.py`, an independent stdlib Python generator and
//! reader; none are produced by this crate. The bound and fuzz cases are hand-labelled.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_io_reg::prov::{principal_commit, LengthUnit, OsSalts, PrincipalDraft, PrincipalKind};
use fid_io_reg::ProvError;
use fid_io_reg::{
    BoundedSource, Budget, Detection, Direction, EffectiveOptions, FormatDescriptor, FormatId, HashingSource,
    LimitRequest, Matcher, Origin, Phase, Principal, ProvenanceBuilder, ReaderId, ReaderInfo, Registry, ReportBuilder,
    Signature, SourceProvenance, UnitResolution, UnitSource, UntrustedText, VersionRange, VersionToken,
};
use fid_io_step::header::{
    HeaderBound, LEXEME_ENTRY_COST, MAX_HEADER_LIST_ITEMS, MAX_NUMBER_LEXEME_BYTES, MAX_NUMBER_TEXT_BYTES,
    MAX_PARTS_READ, MAX_UNCERTAINTIES_PER_CONTEXT, MAX_UNITS_PER_CONTEXT, UNCERTAINTY_REEXPRESSED,
};
use fid_io_step::{
    parse, resolve_header, AngleUnit, HeaderError, HeaderOptions, StepHeaderInfo, StepLengthUnit, StepSchema, StepText,
    WarningDetail,
};
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

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
        "um" => Some(LengthUnit::Um),
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
fn render(
    result: &Result<StepHeaderInfo, HeaderError>,
    want: &[String],
    bytes: &[u8],
    o: &HeaderOptions,
) -> Vec<String> {
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
            out.extend(render_provenance(info, bytes, o));
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

/// The provenance lines of the oracle: the record's unit, source and `unit_overridden`, then
/// its unit items in key order.
fn render_provenance(info: &StepHeaderInfo, bytes: &[u8], o: &HeaderOptions) -> Vec<String> {
    let unit = match info.provenance_unit(o) {
        Ok(u) => u,
        Err(e) => {
            let mut line = format!("prov error {}", e.code());
            if let Some(n) = e.entity() {
                line.push_str(&format!(" entity {n}"));
            }
            return vec![line];
        }
    };
    let mut out = vec![format!(
        "prov unit {} source {} overridden {}",
        unit.unit().symbol(),
        unit.source().as_str(),
        unit.overridden()
    )];
    let mut b = builder(bytes, unit);
    if let Err(e) = info.record_provenance(&mut b) {
        out.push(format!("prov record_error {}", e.code()));
        return out;
    }
    let record = b.build();
    let json = String::from_utf8(record.canonical_bytes().unwrap()).unwrap();
    assert!(json.contains(&format!(r#""unit_overridden":{}"#, unit.overridden())), "{json}");
    let declared = match record.to_cvalue() {
        CValue::Obj(fields) => fields.get("declared").cloned(),
        _ => None,
    };
    let Some(CValue::Obj(declared)) = declared else { panic!("no declared object") };
    for (k, v) in &declared {
        if k == "step.contexts" || k == "step.unitless_contexts" || k.starts_with("step.units.") {
            let CValue::Str(v) = v else { panic!("{k}") };
            out.push(format!("declared {k} {v}"));
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
    assert_eq!(blocks.len(), 37);
    for (head, want) in &blocks {
        let mut it = head.split(' ');
        let name = it.next().unwrap();
        let u = unit(it.next().unwrap().strip_prefix("unit=").unwrap());
        let ov = it.next().unwrap() == "override=true";
        let bytes = read(name);
        let o = opts(u, ov);
        let got = render(&resolve_header(&parse(&bytes).unwrap(), &o), want, &bytes, &o);
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
    let mut b = builder(bytes, info.provenance_unit(o).unwrap());
    info.record_provenance(&mut b).unwrap();
    b.build()
}

fn builder(bytes: &[u8], unit: UnitResolution) -> ProvenanceBuilder {
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
    SourceProvenance::builder(digest, detection, reader, principal, EffectiveOptions::empty(), unit)
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
            "length=mm factor_m=0.001 source=declared declared=mm angle=radian uncertainty=1e-6 mm geometry=true \
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
fn centimetre_is_recorded_in_provenance() {
    // GEO.FRAME.UNITS now has `cm`, so a centimetre file carries its own unit.
    let bytes = read("ap203_cm_deg.stp");
    let o = HeaderOptions::default();
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    assert_eq!(info.contexts[&14].length_factor_m, 0.01);
    assert_eq!(StepLengthUnit::Cm.to_length_unit(), LengthUnit::Cm);
    assert_eq!(StepLengthUnit::from(LengthUnit::Cm), StepLengthUnit::Cm);
    let unit = info.provenance_unit(&o).unwrap();
    assert_eq!((unit.unit(), unit.source()), (LengthUnit::Cm, UnitSource::Declared));
    assert!(!unit.overridden());
    let record = provenance(&bytes, &info, &o);
    let json = String::from_utf8(record.canonical_bytes().unwrap()).unwrap();
    assert!(json.contains(r#""unit":"cm""#), "{json}");
    assert!(json.contains(r#""unit_factor":"0.01""#), "{json}");
    assert!(json.contains(r#""unit_source":"declared""#), "{json}");
    assert_eq!(
        declared(&record, "step.units.group_1"),
        UntrustedText::new(
            "length=cm factor_m=0.01 source=declared declared=cm angle=radian uncertainty=5.0 cm geometry=true \
             contexts=1 first=#14"
        )
    );
    // Overridden to mm: the record carries mm, and the group keeps `declared=cm`.
    let o = opts(Some(LengthUnit::Mm), true);
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    let unit = info.provenance_unit(&o).unwrap();
    assert_eq!((unit.unit(), unit.source()), (LengthUnit::Mm, UnitSource::Override));
    let record = provenance(&bytes, &info, &o);
    assert_eq!(
        declared(&record, "step.units.group_1"),
        UntrustedText::new(
            "length=mm factor_m=0.001 source=override declared=cm angle=radian uncertainty=5.0 mm geometry=true \
             contexts=1 first=#14"
        )
    );
}

/// Review N1 of #377: two contexts overridden to the same unit from different declared
/// units (um and in) are two groups, so the record does not hide that the rescale differed.
#[test]
fn overridden_contexts_with_different_declared_units_are_separate_groups() {
    let bytes = read("override_um_inch.stp");
    let o = opts(Some(LengthUnit::Mm), true);
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    assert_eq!(info.contexts[&30].declared, Some(StepLengthUnit::Um));
    assert_eq!(info.contexts[&31].declared, Some(StepLengthUnit::In));
    for c in [30, 31] {
        let ctx = &info.contexts[&c];
        assert_eq!((ctx.length_unit, ctx.source), (StepLengthUnit::Mm, UnitSource::Override));
    }
    let groups = info.unit_groups();
    let sig = |d: &str| {
        format!("length=mm factor_m=0.001 source=override declared={d} angle=radian uncertainty=none geometry=true")
    };
    assert_eq!(groups, vec![(sig("um"), 1, 30), (sig("in"), 1, 31)]);
    let record = provenance(&bytes, &info, &o);
    assert!(record.declared().contains_key("step.units.group_2"));
    assert!(!record.declared().contains_key("step.units.group_3"));

    // Same declared unit, same override: still one group (300 um contexts).
    let um = "#15=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MICRO.,.METRE.));";
    let mut body = format!("{um}\n");
    for i in 0..300u64 {
        body.push_str(&geo_ctx(1000 + 2 * i, "#15", ""));
    }
    let info = run(&wrap(NAME, &body), &o).unwrap();
    let groups = info.unit_groups();
    assert_eq!(groups, vec![(sig("um").replace("angle=radian", "angle=none"), 300, 1000)]);
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
    assert_eq!(names.len(), 25);
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

// ------------------------------------------------------------------ review of #377

/// A geometry context `#n` over the units `units`, with uncertainties `unc`, and a shape `#n+1`
/// that refers to it.
fn geo_ctx(n: u64, units: &str, unc: &str) -> String {
    let unc = if unc.is_empty() { String::new() } else { format!("GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc}))") };
    format!(
        "#{n}=(GEOMETRIC_REPRESENTATION_CONTEXT(3){unc}GLOBAL_UNIT_ASSIGNED_CONTEXT(({units}))REPRESENTATION_CONTEXT('',''));\n\
         #{}=SHAPE_REPRESENTATION('',(),#{n});\n",
        n + 1
    )
}

const IN: &str = "#11=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\n\
                  #12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254),#11);\n\
                  #13=(CONVERSION_BASED_UNIT('INCH',#12)LENGTH_UNIT()NAMED_UNIT(*));";

#[test]
fn an_override_of_a_non_primary_context_marks_the_record_overridden() {
    // multi_context.stp: #30 declares mm (primary), #31 declares um. With unit=mm and
    // override, #31's geometry is rescaled 1000x, so the record must say so.
    let bytes = read("multi_context.stp");
    let o = opts(Some(LengthUnit::Mm), true);
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    assert_eq!(info.primary_context().unwrap().source, UnitSource::Declared);
    assert_eq!(info.contexts[&31].source, UnitSource::Override);
    let unit = info.provenance_unit(&o).unwrap();
    assert!(unit.overridden());
    assert_eq!(unit.source(), UnitSource::Override);
    assert_eq!(unit.unit(), LengthUnit::Mm);
    let record = provenance(&bytes, &info, &o);
    let json = String::from_utf8(record.canonical_bytes().unwrap()).unwrap();
    assert!(json.contains(r#""unit_overridden":true"#), "{json}");
    assert_eq!(declared(&record, "step.units.not_declared"), UntrustedText::new("contexts=1 override=1 caller=0"));
    assert_eq!(declared(&record, "step.units.not_declared_1"), UntrustedText::new("#31"));
}

#[test]
fn a_caller_filled_non_primary_context_marks_the_record_overridden() {
    // partial_unit.stp: #30 declares mm (primary), #31 owns geometry and declares none.
    let bytes = read("partial_unit.stp");
    let o = opts(Some(LengthUnit::Mm), false);
    let info = resolve_header(&parse(&bytes).unwrap(), &o).unwrap();
    assert_eq!(info.primary_context().unwrap().context, 30);
    assert_eq!(info.contexts[&30].source, UnitSource::Declared);
    assert_eq!(info.contexts[&31].source, UnitSource::Caller);
    let unit = info.provenance_unit(&o).unwrap();
    assert!(unit.overridden());
    assert_eq!(unit.source(), UnitSource::Caller);
    let record = provenance(&bytes, &info, &o);
    let json = String::from_utf8(record.canonical_bytes().unwrap()).unwrap();
    assert!(json.contains(r#""unit_overridden":true"#), "{json}");
    assert!(json.contains(r#""unit_source":"caller""#), "{json}");
    assert_eq!(declared(&record, "step.units.not_declared"), UntrustedText::new("contexts=1 override=0 caller=1"));
    assert_eq!(declared(&record, "step.units.not_declared_1"), UntrustedText::new("#31"));
    // The same file with every unit declared as given: not overridden.
    let ok = resolve("ap242_mm.stp", Some(LengthUnit::Mm), false).unwrap();
    assert!(!ok.provenance_unit(&opts(Some(LengthUnit::Mm), false)).unwrap().overridden());
    assert!(ok.not_declared().is_empty());
}

#[test]
fn not_declared_contexts_are_written_as_runs_and_split_into_pieces() {
    // #101, #103 mm; #105 in (declared as given); #107, #109 mm. Override to inch.
    let mut body = format!("{MM}\n{IN}\n");
    for (n, u) in [(101, "#10"), (103, "#10"), (105, "#13"), (107, "#10"), (109, "#10")] {
        body.push_str(&geo_ctx(n, u, ""));
    }
    let o = opts(Some(LengthUnit::In), true);
    let info = run(&wrap(NAME, &body), &o).unwrap();
    assert_eq!(info.not_declared_runs(), vec!["#101..#103,#107..#109".to_string()]);

    // 3,000 contexts alternating override and declared: 1,500 runs over several pieces.
    let mut body = format!("{MM}\n{IN}\n");
    for i in 0..3000u64 {
        body.push_str(&geo_ctx(1000 + 2 * i, if i % 2 == 0 { "#10" } else { "#13" }, ""));
    }
    let info = run(&wrap(NAME, &body), &o).unwrap();
    let pieces = info.not_declared_runs();
    assert!(pieces.len() > 1, "{}", pieces.len());
    let mut listed = Vec::new();
    for p in &pieces {
        assert!(p.len() <= fid_io_reg::prov::MAX_TEXT_BYTES);
        for run in p.split(',') {
            assert!(!run.contains(".."), "{run}");
            listed.push(run.trim_start_matches('#').parse::<u64>().unwrap());
        }
    }
    let want: Vec<u64> = (0..3000u64).filter(|i| i % 2 == 0).map(|i| 1000 + 2 * i).collect();
    assert_eq!(listed, want);
    let mut b = builder(b"ISO-10303-21;", info.provenance_unit(&o).unwrap());
    info.record_provenance(&mut b).unwrap();
}

#[test]
fn an_override_re_expresses_an_uncertainty_in_the_declared_unit() {
    let info = resolve("ap242_mm.stp", Some(LengthUnit::In), true).unwrap();
    let u = info.contexts[&14].uncertainty.as_ref().unwrap();
    assert_eq!(u.value.to_bits(), 1e-6f64.to_bits(), "the number is kept, as for the geometry");
    assert_eq!(u.unit, StepLengthUnit::In);
    assert_eq!(u.length_factor_m, 0.0254);
    assert_eq!(u.reexpressed_from, Some(StepLengthUnit::Mm));
    assert_eq!(info.warnings.len(), 1);
    let w = &info.warnings[0];
    assert_eq!((w.entity, w.code), (14, UNCERTAINTY_REEXPRESSED));
    assert_eq!(w.detail, WarningDetail::Reexpressed { from: StepLengthUnit::Mm, to: StepLengthUnit::In });
    let mut report = ReportBuilder::new();
    info.record_warnings(&mut report).unwrap();
    assert_eq!(report.warnings_total(), 1);
    // Without an override nothing moves.
    let plain = resolve("ap242_mm.stp", None, false).unwrap();
    assert_eq!(plain.contexts[&14].uncertainty.as_ref().unwrap().reexpressed_from, None);
    // An uncertainty in a unit other than the declared one keeps it (#31's is in nm).
    let multi = resolve("multi_context.stp", Some(LengthUnit::Mm), true).unwrap();
    let u = multi.contexts[&31].uncertainty.as_ref().unwrap();
    assert_eq!((u.unit, u.reexpressed_from), (StepLengthUnit::Nm, None));
    assert!(multi.warnings.is_empty());
}

#[test]
fn a_part_read_twice_is_malformed() {
    let cases: Vec<(String, u64)> = vec![
        // two GLOBAL_UNIT_ASSIGNED_CONTEXT parts
        (
            format!("{MM}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
            14,
        ),
        // two GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT parts
        (
            format!("{MM}\n#13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10,'','');\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
            14,
        ),
        // two GEOMETRIC_REPRESENTATION_CONTEXT parts
        (
            format!("{MM}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GEOMETRIC_REPRESENTATION_CONTEXT(2)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
            14,
        ),
        // two SI_UNIT parts
        (
            "#10=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.)SI_UNIT($,.METRE.));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));".to_string(),
            10,
        ),
        // two LENGTH_UNIT parts
        (
            "#10=(LENGTH_UNIT()LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));".to_string(),
            10,
        ),
        // two CONVERSION_BASED_UNIT parts
        (
            "#11=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.));\n#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254),#11);\n#13=(CONVERSION_BASED_UNIT('INCH',#12)CONVERSION_BASED_UNIT('FOOT',#12)LENGTH_UNIT()NAMED_UNIT(*));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#13))REPRESENTATION_CONTEXT('',''));".to_string(),
            13,
        ),
        // a conversion base with two SI_UNIT parts
        (
            "#11=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT($,.METRE.)SI_UNIT(.MILLI.,.METRE.));\n#12=LENGTH_MEASURE_WITH_UNIT(LENGTH_MEASURE(0.0254),#11);\n#13=(CONVERSION_BASED_UNIT('INCH',#12)LENGTH_UNIT()NAMED_UNIT(*));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#13))REPRESENTATION_CONTEXT('',''));".to_string(),
            11,
        ),
        // a measure part name twice
        (
            format!("{MM}\n#13=(MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10)MEASURE_WITH_UNIT('a','b'));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
            13,
        ),
        // two value-carrying measure parts
        (
            format!("{MM}\n#13=(LENGTH_MEASURE_WITH_UNIT()MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10)UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-03),#10,'',''));\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));"),
            13,
        ),
    ];
    for (body, entity) in &cases {
        let e = run(&wrap(NAME, body), &HeaderOptions::default()).unwrap_err();
        assert_eq!(e, HeaderError::Malformed { entity: Some(*entity), reason: "duplicate_part" }, "{body}");
    }
}

/// `(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT() + fillers)` with `parts` parts: a
/// unit this module classifies as neither length nor angle.
fn fat_unit(n: u64, parts: usize) -> String {
    format!("#{n}=(NAMED_UNIT(*)SI_UNIT($,.STERADIAN.)SOLID_ANGLE_UNIT(){});", "FILLER_PART()".repeat(parts - 3))
}

#[test]
fn a_fat_complex_instance_is_refused_not_scanned() {
    let refs = vec!["#10"; MAX_UNITS_PER_CONTEXT].join(",");
    let body = format!(
        "{}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT(({refs}))REPRESENTATION_CONTEXT('',''));",
        fat_unit(10, 100_000)
    );
    let e = run(&wrap(NAME, &body), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::Parts, value: MAX_PARTS_READ as u64 });
    // One part over the bound is refused, the bound itself is read.
    let over = format!(
        "{}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));",
        fat_unit(10, MAX_PARTS_READ + 1)
    );
    assert_eq!(run(&wrap(NAME, &over), &HeaderOptions::default()).unwrap_err().code(), "limit_exceeded");
    let at = over.replace(&fat_unit(10, MAX_PARTS_READ + 1), &fat_unit(10, MAX_PARTS_READ));
    assert!(run(&wrap(NAME, &at), &HeaderOptions::default()).is_ok());
    // A fat measure and a fat context are refused the same way.
    let measure = format!(
        "{MM}\n#13=(MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10){});\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT((#13))GLOBAL_UNIT_ASSIGNED_CONTEXT((#10))REPRESENTATION_CONTEXT('',''));",
        "FILLER_PART()".repeat(MAX_PARTS_READ)
    );
    let e = run(&wrap(NAME, &measure), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::Parts, value: MAX_PARTS_READ as u64 });
    let context = format!(
        "{MM}\n#14=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNIT_ASSIGNED_CONTEXT((#10)){}REPRESENTATION_CONTEXT('',''));",
        "FILLER_PART()".repeat(MAX_PARTS_READ)
    );
    let e = run(&wrap(NAME, &context), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::Parts, value: MAX_PARTS_READ as u64 });
}

#[test]
fn shared_units_and_measures_are_evaluated_once() {
    // 10,000 contexts, each listing a 64-part unit 63 times (plus mm) and a 64-part
    // uncertainty measure (over a non-length unit, so every entry is read) 64 times. Without
    // memoisation this is ~10^9 part-name comparisons; with it, one evaluation per entity.
    let measure =
        format!("#12=(MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10){});", "FILLER_PART()".repeat(MAX_PARTS_READ - 1));
    let units = std::iter::once("#11")
        .chain(std::iter::repeat_n("#10", MAX_UNITS_PER_CONTEXT - 1))
        .collect::<Vec<_>>()
        .join(",");
    let unc = vec!["#12"; MAX_UNCERTAINTIES_PER_CONTEXT].join(",");
    let mut body = format!(
        "{}\n#11=(LENGTH_UNIT()NAMED_UNIT(*)SI_UNIT(.MILLI.,.METRE.));\n{measure}\n",
        fat_unit(10, MAX_PARTS_READ)
    );
    for i in 0..10_000u64 {
        body.push_str(&format!(
            "#{}=(GEOMETRIC_REPRESENTATION_CONTEXT(3)GLOBAL_UNCERTAINTY_ASSIGNED_CONTEXT(({unc}))GLOBAL_UNIT_ASSIGNED_CONTEXT(({units}))REPRESENTATION_CONTEXT('',''));\n",
            100 + i
        ));
    }
    let file = parse(wrap(NAME, &body).as_bytes()).unwrap();
    let started = std::time::Instant::now();
    let info = resolve_header(&file, &HeaderOptions::default()).unwrap();
    let took = started.elapsed();
    assert_eq!(info.contexts.len(), 10_000);
    assert!(info.contexts.values().all(|c| c.length_unit == StepLengthUnit::Mm && c.uncertainty.is_none()));
    // About 0.4 s in a local debug build with memoisation, about 7 s without it.
    assert!(took < std::time::Duration::from_secs(3), "{took:?}");
}

#[test]
fn authored_tokens_are_shared_capped_and_budgeted() {
    // Two contexts over one measure, and a third over another measure with the same token:
    // one shared copy.
    let mut body = format!(
        "{MM}\n#12=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10,'','');\n\
         #13=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.E-06),#10,'','');\n"
    );
    body.push_str(&geo_ctx(20, "#10", "#12"));
    body.push_str(&geo_ctx(30, "#10", "#12"));
    body.push_str(&geo_ctx(40, "#10", "#13"));
    let info = run(&wrap(NAME, &body), &HeaderOptions::default()).unwrap();
    let a = &info.contexts[&20].uncertainty.as_ref().unwrap().authored;
    for c in [30, 40] {
        assert!(Arc::ptr_eq(a, &info.contexts[&c].uncertainty.as_ref().unwrap().authored));
    }
    // A token over the cap.
    let long = format!("1.{}E-06", "0".repeat(MAX_NUMBER_LEXEME_BYTES));
    let body = format!(
        "{MM}\n#12=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({long}),#10,'','');\n{}",
        geo_ctx(20, "#10", "#12")
    );
    let e = run(&wrap(NAME, &body), &HeaderOptions::default()).unwrap_err();
    assert_eq!(
        e,
        HeaderError::LimitExceeded { bound: HeaderBound::NumberLexeme, value: MAX_NUMBER_LEXEME_BYTES as u64 }
    );
    // Many distinct tokens exhaust the budget.
    let mut body = format!("{MM}\n");
    let each = 100usize;
    let count = MAX_NUMBER_TEXT_BYTES / (each + LEXEME_ENTRY_COST) + 2;
    for i in 0..count {
        let digits = format!("{i:0>width$}", width = each - 5);
        body.push_str(&format!(
            "#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE(1.{digits}E-6),#10,'','');\n",
            100_000 + i
        ));
        body.push_str(&geo_ctx(200_000 + 2 * i as u64, "#10", &format!("#{}", 100_000 + i)));
    }
    let e = run(&wrap(NAME, &body), &HeaderOptions::default()).unwrap_err();
    assert_eq!(e, HeaderError::LimitExceeded { bound: HeaderBound::NumberText, value: MAX_NUMBER_TEXT_BYTES as u64 });
}

#[test]
fn lexeme_entry_cost_covers_its_allocation() {
    assert!(LEXEME_ENTRY_COST >= 2 * std::mem::size_of::<usize>() + std::mem::size_of::<Arc<str>>());
}

#[test]
fn provenance_groups_on_values_not_tokens() {
    // 300 contexts whose equal uncertainties are written five ways: one group.
    let tokens = ["1.E-06", "1.0E-06", "0.000001", "1.00E-6", "10.E-07"];
    let mut body = format!("{MM}\n");
    for (i, t) in tokens.iter().enumerate() {
        body.push_str(&format!("#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({t}),#10,'','');\n", 20 + i));
    }
    for i in 0..300u64 {
        body.push_str(&geo_ctx(1000 + 2 * i, "#10", &format!("#{}", 20 + i % 5)));
    }
    let o = HeaderOptions::default();
    let info = run(&wrap(NAME, &body), &o).unwrap();
    let groups = info.unit_groups();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert_eq!(groups[0].1, 300);
    assert!(groups[0].0.contains("uncertainty=1e-6 mm"), "{}", groups[0].0);
    let mut b = builder(b"ISO-10303-21;", info.provenance_unit(&o).unwrap());
    info.record_provenance(&mut b).unwrap();

    // 300 different values: more groups than IO.REG.PROV allows, a typed refusal before
    // anything is declared.
    let mut body = format!("{MM}\n");
    for i in 0..300u64 {
        body.push_str(&format!(
            "#{}=UNCERTAINTY_MEASURE_WITH_UNIT(LENGTH_MEASURE({}.E-06),#10,'','');\n",
            100_000 + i,
            i + 1
        ));
        body.push_str(&geo_ctx(1000 + 2 * i, "#10", &format!("#{}", 100_000 + i)));
    }
    let info = run(&wrap(NAME, &body), &o).unwrap();
    let mut b = builder(b"ISO-10303-21;", info.provenance_unit(&o).unwrap());
    let e = info.record_provenance(&mut b).unwrap_err();
    assert_eq!(e, ProvError::TooLarge { field: "step.units", limit: fid_io_reg::prov::MAX_DECLARED as u64 });
    assert_eq!(e.code(), "too_large");
    assert!(b.build().declared().is_empty());
}

#[test]
fn a_negative_zero_uncertainty_reads_as_zero() {
    let info = resolve("uncertainty_tokens.stp", None, false).unwrap();
    let u = info.contexts[&32].uncertainty.as_ref().unwrap();
    assert_eq!(&*u.authored, "-0.");
    assert_eq!(u.value.to_bits(), 0);
    assert_eq!(info.unit_groups().len(), 2);
}
