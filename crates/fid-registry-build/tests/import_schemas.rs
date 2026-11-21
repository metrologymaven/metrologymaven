//! The registry definitions of IO.REG.ENV.IMPORT (`import.file@1`, `import.<format>@1`)
//! validated with the bus's own validator (`fid_bus::validate::Compiled`, CORE.BUS.PIPE.VALID):
//! both schemas compile, real parameters pass the params schema, the real reports of the
//! independent reference (`fixtures/io/reg/env/import/make_fixtures.py`) pass the result
//! schema, and shape mistakes fail it. The definitions pass VAL with the workspace's.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_bus::error::{BusError, Detail, ValidationReason};
use fid_bus::unit::Normalised;
use fid_bus::validate::{Compiled, Hooks, RefLookup};
use fid_canon::{parse, CValue};
use fid_io_reg::env::import::command::{format_def, import_file_def};
use fid_io_reg::{Direction, FormatDescriptor, FormatId, Matcher, Origin, Phase, Registry, Signature, VersionRange};
use fid_registry::{validate, CommandDecl};
use std::collections::BTreeMap;

const DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/io/reg/env/import/");

struct NoHooks;

impl Hooks for NoHooks {
    fn literal(&self, _: &str, _: &CValue) -> Result<Normalised, BusError> {
        Err(BusError::Validation(ValidationReason::UnitMismatch, Detail::new("no literals")))
    }
    fn entity(&self, _: &str) -> RefLookup {
        RefLookup::Missing
    }
}

fn registry() -> Registry {
    let mut r = Registry::new();
    for (id, ext, magic) in [("fid-test-points", "ftp", b"FTP1"), ("fid-test-mesh", "ftm", b"FTM1")] {
        r.register(
            FormatDescriptor::new(
                FormatId::new(id).unwrap(),
                VersionRange::new("1", "1").unwrap(),
                vec![Signature::new(0, Matcher::Bytes(magic.to_vec()), 50).unwrap()],
                vec![ext.to_string()],
                CValue::Obj(BTreeMap::new()),
                Phase::new(0).unwrap(),
                Origin::FirstParty,
                Direction::Import,
            )
            .unwrap(),
        )
        .unwrap();
    }
    r
}

fn passes(schema: &CValue, v: &CValue) -> bool {
    Compiled::compile(schema).unwrap().evaluate(v, &NoHooks).is_ok()
}

fn j(s: &str) -> CValue {
    parse(s.as_bytes()).unwrap()
}

#[test]
fn import_schemas_validate_real_params_and_results() {
    let reg = registry();
    let generic = import_file_def(&reg).unwrap();
    let per = format_def(&reg, &FormatId::new("fid-test-points").unwrap(), &["import.file@1"]).unwrap();
    for d in [&generic, &per] {
        Compiled::compile(&d.params_schema).unwrap();
        Compiled::compile(&d.result_schema).unwrap();
    }
    for ok in [
        r#"{"source":"root://in/ok.ftp"}"#,
        r#"{"dry_run":true,"source":"root://in/ok.ftp","stage":"S2","unit":"mm"}"#,
        r#"{"options":{},"source":"root://in/many.ftp","stage":"0190aaaa-0000-7000-8000-000000000001"}"#,
    ] {
        assert!(passes(&generic.params_schema, &j(ok)), "{ok}");
        assert!(passes(&per.params_schema, &j(ok)), "{ok}");
    }
    assert!(passes(&generic.params_schema, &j(r#"{"format":"fid-test-points","source":"root://in/ok.ftp"}"#)));
    for bad in [
        r#"{}"#,
        r#"{"source":""}"#,
        r#"{"source":"root://in/ok.ftp","x":1}"#,
        r#"{"source":"root://in/ok.ftp","unit":"furlong"}"#,
        r#"{"source":"root://in/ok.ftp","dry_run":"yes"}"#,
        r#"{"format":"Bad_Format","source":"root://in/ok.ftp"}"#,
    ] {
        assert!(!passes(&generic.params_schema, &j(bad)), "{bad}");
    }
    assert!(!passes(&per.params_schema, &j(r#"{"format":"fid-test-points","source":"root://in/ok.ftp"}"#)));

    let expected = parse(&std::fs::read(format!("{DIR}expected.json")).unwrap()).unwrap();
    let CValue::Obj(top) = &expected else { panic!() };
    let Some(CValue::Obj(cases)) = top.get("cases") else { panic!() };
    let mut n = 0;
    let mut with_warnings = 0;
    for (name, c) in cases {
        let CValue::Obj(c) = c else { panic!() };
        for key in ["report", "report_dry"] {
            if let Some(r) = c.get(key) {
                assert!(passes(&generic.result_schema, r), "{name} {key}");
                assert!(passes(&per.result_schema, r), "{name} {key}");
                n += 1;
                let CValue::Obj(ro) = r else { panic!() };
                if matches!(ro.get("warnings"), Some(CValue::Arr(w)) if !w.is_empty()) {
                    with_warnings += 1;
                }
            }
        }
    }
    assert!(n >= 6, "{n}");
    // The warnings branch: the reference's renamed-entity warnings (with positions) ...
    assert!(with_warnings >= 2, "{with_warnings}");
    // ... and a hard-linked source's warning (no position), hand-labelled.
    let hard_linked = j(r#"{"dropped":[],"dropped_total":"0","dry_run":false,"entity_counts":{"test.point_set":"1"},
        "format":"fid-test-points","source":"root://in/linked.ftp","warnings":[{"code":"hard_linked",
        "detail":{"text":"2 links","truncated":false},"pos":null}],"warnings_total":"1"}"#);
    assert!(passes(&generic.result_schema, &hard_linked));
    assert!(passes(&per.result_schema, &hard_linked));
    let bad_warning = j(r#"{"dropped":[],"dropped_total":"0","dry_run":false,"entity_counts":{},
        "format":"fid-test-points","source":"root://in/linked.ftp","warnings":[{"code":"Hard-Linked",
        "detail":{"text":"2 links","truncated":false},"pos":null}],"warnings_total":"1"}"#);
    assert!(!passes(&generic.result_schema, &bad_warning));
    assert!(!passes(&generic.result_schema, &j(r#"{"dry_run":false}"#)));

    let decls: Vec<CommandDecl> = [&generic, &per].iter().map(|d| CommandDecl::from_def(d)).collect();
    assert_eq!(validate(&decls), vec![]);
}
