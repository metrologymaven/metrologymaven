//! Acceptance criteria of spec node UNC.MODEL.GRAPH.SCHEMA (AC-1 .. AC-4) plus requirement
//! checks.
//!
//! Fixture bytes, canonical bytes, digests and expected errors come from the independent
//! Python reference `fixtures/unc/model/graph/make_fixtures.py` (stdlib `json`, `struct`,
//! `hashlib`) and its hand-labelled error table; none come from this crate. AC-1's
//! cross-platform part is this same file run by CI on wasm32-wasip1 (wasmtime), x86-64
//! (Linux, Windows) and ARM64 (macOS).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_kci::kci::json::Json;
use fid_kunc::dist::{Distribution, Quantity, UnitCode};
use fid_kunc::model::{
    CorrelationGroup, Edge, Endpoint, InPort, InputNode, InputSourceKind, ModelParts, OperatorNode, OutPort,
    OutputNode, SchemaError, SourceRef, UncertaintyModel, MAX_INPUT_BYTES, MAX_PARAMS_DEPTH, MAX_VALUES, SCHEMA,
};
use fid_kunc::{ModelRef, StatementParts};
use fid_types::Digest;

/// A change applied to model parts.
type Edit = Box<dyn Fn(&mut ModelParts)>;

const FIX: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../fixtures/unc/model/graph/");

fn read(rel: &str) -> Vec<u8> {
    std::fs::read(format!("{FIX}{rel}")).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

fn text(rel: &str) -> String {
    String::from_utf8(read(rel)).unwrap().trim().to_owned()
}

fn parse(rel: &str) -> UncertaintyModel {
    UncertaintyModel::parse(&read(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

// ------------------------------------------------------------------ AC-1

#[test]
fn ac1_digest_and_canonical_bytes_equal_the_reference() {
    let m = parse("bore_ls.json");
    assert_eq!(m.canonical_json(), read("bore_ls.canon.json").as_slice());
    assert_eq!(m.digest().as_str(), text("bore_ls.sha256"));
    assert_eq!(m.id(), "um:bore-ls@1.0.0");
    // The canonical bytes are themselves a document that parses to the same model.
    let again = UncertaintyModel::parse(m.canonical_json()).unwrap();
    assert_eq!(again, m);
}

#[test]
fn ac1_canonical_order_is_the_documented_one() {
    let m = parse("bore_ls.json");
    let ids = |v: Vec<&str>| v.into_iter().map(str::to_owned).collect::<Vec<_>>();
    assert_eq!(
        m.inputs().iter().map(|n| n.id.clone()).collect::<Vec<_>>(),
        ids(vec!["form", "probe_r", "pts_noise", "scale_err", "thermal_strain", "tilt"])
    );
    assert_eq!(m.operators().iter().map(|n| n.id.clone()).collect::<Vec<_>>(), ids(vec!["fit", "thermal"]));
    assert_eq!(m.outputs().iter().map(|n| n.id.clone()).collect::<Vec<_>>(), ids(vec!["d_bore", "fit_residual"]));
    let keys: Vec<String> =
        m.edges().iter().map(|e| format!("{}.{}<-{}.{}", e.to.node, e.to.port, e.from.node, e.from.port)).collect();
    assert_eq!(
        keys,
        [
            "d_bore.value<-thermal.value",
            "fit.form<-form.value",
            "fit.noise<-pts_noise.value",
            "fit.probe_r<-probe_r.value",
            "fit.tilt<-tilt.value",
            "fit_residual.value<-fit.residual",
            "thermal.strain<-scale_err.value",
            "thermal.strain<-thermal_strain.value",
            "thermal.value<-fit.diameter",
        ]
    );
    let g = &m.correlations()[0];
    assert_eq!(g.inputs, ["form", "pts_noise", "tilt"]);
    // form-pts_noise 0.3, form-tilt 0.1, pts_noise-tilt 0.2 (authored as tilt, form, pts_noise).
    assert_eq!(g.matrix, vec![vec![1.0, 0.3, 0.1], vec![0.3, 1.0, 0.2], vec![0.1, 0.2, 1.0]]);
    assert_eq!(m.correlations()[1].inputs, ["scale_err", "thermal_strain"]);
    // Port lists keep their declared (argument) order.
    let fit = &m.operators()[0];
    let names: Vec<&str> = fit.ports_in.iter().map(|p| p.name.as_str()).collect();
    assert_eq!(names, ["probe_r", "noise", "form", "tilt"]);
    assert_eq!(fit.algorithm().unwrap().to_string(), "fit.circle.ls@1.0.0");
}

// ------------------------------------------------------------------ AC-2

#[test]
fn ac2_shuffled_document_has_the_same_digest() {
    let a = parse("bore_ls.json");
    let b = parse("bore_ls_shuffled.json");
    assert_ne!(read("bore_ls.json"), read("bore_ls_shuffled.json"));
    assert_eq!(b.digest(), a.digest());
    assert_eq!(b.canonical_json(), a.canonical_json());
}

#[test]
fn req002_every_permutation_of_parts_gives_one_digest() {
    let base = parse("bore_ls.json");
    let p = base.parts().clone();
    for k in 0..12usize {
        let mut q = p.clone();
        let n_in = q.inputs.len();
        q.inputs.rotate_left(k % n_in);
        q.inputs.reverse();
        q.operators.rotate_left(k % 2);
        q.outputs.rotate_left((k / 2) % 2);
        let n_e = q.edges.len();
        q.edges.rotate_left(k % n_e);
        if k % 3 == 0 {
            q.edges.reverse();
        }
        q.correlations.rotate_left(k % 2);
        let g = &mut q.correlations[0];
        let n = g.inputs.len();
        let order: Vec<usize> = (0..n).map(|i| (i + k) % n).collect();
        g.inputs = order.iter().map(|&i| g.inputs[i].clone()).collect();
        g.matrix = order.iter().map(|&i| order.iter().map(|&j| g.matrix[i][j]).collect()).collect();
        let m = UncertaintyModel::new(q).unwrap();
        assert_eq!(m.digest(), base.digest(), "permutation {k}");
    }
}

// ------------------------------------------------------------------ AC-3

#[test]
fn ac3_changed_parameter_changes_the_digest() {
    let a = parse("bore_ls.json");
    let b = parse("bore_ls_param.json");
    assert_ne!(a.digest(), b.digest());
    assert_eq!(b.digest().as_str(), text("bore_ls_param.sha256"));
    // Only the fit operator's embedded params differ, and with them its derived digest.
    let (fa, fb) = (&a.operators()[0], &b.operators()[0]);
    assert_ne!(fa.params_digest().unwrap(), fb.params_digest().unwrap());
    let mut pa = a.parts().clone();
    pa.operators[0].params = fb.params.clone();
    assert_eq!(&pa, b.parts());
    // The digest is SHA-256 of the params object's own RFC 8785 bytes, and the canonical
    // form writes it next to the params.
    let bytes = fid_kci::kci::json::to_canonical(&fa.params).unwrap();
    assert_eq!(fa.params_digest().unwrap(), fid_kci::kci::sha256_digest(&bytes));
    let canon = std::str::from_utf8(a.canonical_json()).unwrap();
    assert!(canon.contains(&format!("\"params_digest\":\"{}\"", fa.params_digest().unwrap())));
}

#[test]
fn req003_any_field_change_changes_the_digest() {
    let base = parse("bore_ls.json");
    let p = base.parts().clone();
    let variants: Vec<Edit> = vec![
        Box::new(|q| q.semver = "1.0.1".into()),
        Box::new(|q| q.operators[1].algorithm_version = "1.0.1".into()),
        Box::new(|q| q.operators[1].params = Json::Obj(vec![("mode".into(), Json::Str("additive".into()))])),
        Box::new(|q| q.operators[1].ports_in[1].arity = 3),
        Box::new(|q| q.operators[0].ports_in.swap(0, 1)),
        Box::new(|q| q.outputs[0].characteristic = None),
        Box::new(|q| q.inputs[0].source = None),
        Box::new(|q| q.correlations[1].matrix[0][1] = 0.5),
        Box::new(|q| {
            q.edges.pop();
        }),
    ];
    for (i, f) in variants.iter().enumerate() {
        let mut q = p.clone();
        f(&mut q);
        let m = UncertaintyModel::new(q).unwrap();
        assert_ne!(m.digest(), base.digest(), "variant {i}");
    }
}

// ------------------------------------------------------------------ AC-4

#[test]
fn ac4_duplicate_node_bad_name_and_too_large() {
    assert_eq!(
        UncertaintyModel::parse(&read("dup_node.json")),
        Err(SchemaError::DuplicateNode { id: "probe_r".into() })
    );
    let e = UncertaintyModel::parse(&read("bad_name.json")).unwrap_err();
    assert_eq!((e.code(), e.field()), ("schema_invalid", Some("name")));
    let big = vec![b' '; 17 * 1024 * 1024];
    assert_eq!(UncertaintyModel::parse(&big), Err(SchemaError::TooLarge));
    assert_eq!(SchemaError::TooLarge.code(), "too_large");
    assert_eq!(SchemaError::DuplicateNode { id: "x".into() }.code(), "duplicate_node");
}

#[test]
fn req004_size_bound_is_exactly_16_mib() {
    let mut doc = read("bore_ls.json");
    doc.resize(MAX_INPUT_BYTES, b' ');
    assert!(UncertaintyModel::parse(&doc).is_ok());
    doc.push(b' ');
    assert_eq!(UncertaintyModel::parse(&doc), Err(SchemaError::TooLarge));
}

#[test]
fn req004_every_invalid_fixture_fails_with_its_labelled_error() {
    let rows = text("invalid/expected.txt");
    let mut n = 0;
    for line in rows.lines() {
        let cols: Vec<&str> = line.split_whitespace().collect();
        let (file, code, what) = (cols[0], cols[1], cols[2]);
        let err = UncertaintyModel::parse(&read(&format!("invalid/{file}"))).expect_err(file);
        assert_eq!(err.code(), code, "{file}: {err}");
        match &err {
            SchemaError::SchemaInvalid { field } => assert_eq!(*field, what, "{file}"),
            SchemaError::DuplicateNode { id } => assert_eq!(id, what, "{file}"),
            SchemaError::TooLarge => assert_eq!(what, "-", "{file}"),
        }
        n += 1;
    }
    assert!(n >= 40, "{n} cases");
}

// ------------------------------------------------------------------ constructor and ModelRef

fn q(v: f64, u: UnitCode) -> Quantity {
    Quantity::new(v, u)
}

fn d(label: &str) -> Digest {
    fid_kci::kci::sha256_digest(label.as_bytes())
}

fn small() -> ModelParts {
    let dist = Distribution::normal(q(1.0, UnitCode::Metre), q(0.5, UnitCode::Metre)).unwrap();
    ModelParts {
        name: "small".into(),
        semver: "0.1.0".into(),
        inputs: vec![
            InputNode {
                id: "b".into(),
                quantity_kind: "length".into(),
                unit: UnitCode::Metre,
                distribution: dist,
                source: Some(SourceRef { kind: InputSourceKind::Declared, digest: d("b"), author: "A. B".into() }),
            },
            InputNode {
                id: "a".into(),
                quantity_kind: "length".into(),
                unit: UnitCode::Metre,
                distribution: dist,
                source: None,
            },
        ],
        operators: vec![OperatorNode {
            id: "op".into(),
            algorithm_id: "unc.op.sum".into(),
            algorithm_version: "1.0.0".into(),
            params: Json::Obj(vec![("mode".into(), Json::Str("plain".into()))]),
            ports_in: vec![InPort {
                name: "x".into(),
                quantity_kind: "length".into(),
                unit: UnitCode::Metre,
                arity: 2,
            }],
            ports_out: vec![OutPort { name: "y".into(), quantity_kind: "length".into(), unit: UnitCode::Metre }],
        }],
        outputs: vec![OutputNode {
            id: "out".into(),
            quantity_kind: "length".into(),
            unit: UnitCode::Metre,
            characteristic: None,
        }],
        edges: vec![
            Edge {
                from: Endpoint { node: "op".into(), port: "y".into() },
                to: Endpoint { node: "out".into(), port: "value".into() },
            },
            Edge {
                from: Endpoint { node: "b".into(), port: "value".into() },
                to: Endpoint { node: "op".into(), port: "x".into() },
            },
            Edge {
                from: Endpoint { node: "a".into(), port: "value".into() },
                to: Endpoint { node: "op".into(), port: "x".into() },
            },
        ],
        correlations: vec![CorrelationGroup {
            inputs: vec!["b".into(), "a".into()],
            matrix: vec![vec![1.0, 0.5], vec![0.5, 1.0]],
        }],
    }
}

#[test]
fn constructor_round_trips_through_the_document() {
    let m = UncertaintyModel::new(small()).unwrap();
    let text = std::str::from_utf8(m.canonical_json()).unwrap();
    assert!(text.starts_with("{\"correlations\":[{\"inputs\":[\"a\",\"b\"]"), "{text}");
    assert!(text.contains(&format!("\"schema\":\"{SCHEMA}\"")));
    // The unsourced input has no `source` member (UNC.MODEL.GRAPH.CHECK reports it).
    assert_eq!(text.matches("\"source\"").count(), 1);
    assert_eq!(UncertaintyModel::parse(m.canonical_json()).unwrap(), m);
}

#[test]
fn model_ref_is_accepted_by_the_statement_type() {
    let m = parse("bore_ls.json");
    let r = m.model_ref();
    assert_eq!(r, ModelRef { id: "um:bore-ls".into(), semver: "1.0.0".into(), digest: m.digest() });
    // A UEC-0 statement carrying it passes the UNC.UEC.STMT text checks for model.id/semver.
    let mut p = StatementParts::new(fid_kunc::EvidenceClass::Uec0);
    p.uec0_reason = Some(fid_kunc::Uec0Reason::UMissing);
    p.unit = Some("si:m".into());
    p.model = Some(r);
    assert!(fid_kunc::UncertaintyStatement::new(p).is_ok());
}

#[test]
fn constructor_refuses_what_the_document_cannot_hold() {
    let cases: Vec<(&str, Edit)> = vec![
        ("inputs.unit", Box::new(|p| p.inputs[0].unit = UnitCode::Radian)),
        (
            "inputs.distribution.params",
            Box::new(|p| {
                p.inputs[0].distribution =
                    Distribution::normal(q(-0.0, UnitCode::Metre), q(1.0, UnitCode::Metre)).unwrap()
            }),
        ),
        ("inputs.source.author", Box::new(|p| p.inputs[0].source.as_mut().unwrap().author = "x\u{202e}y".into())),
        ("inputs.source.author", Box::new(|p| p.inputs[0].source.as_mut().unwrap().author = "e\u{301}".into())),
        ("inputs.source.author", Box::new(|p| p.inputs[0].source.as_mut().unwrap().author = "a\nb".into())),
        ("inputs.id", Box::new(|p| p.inputs[0].id = "\u{430}".into())),
        ("operators.ports_in.arity", Box::new(|p| p.operators[0].ports_in[0].arity = 0)),
        (
            "operators.ports_out",
            Box::new(|p| {
                let port = p.operators[0].ports_out[0].clone();
                p.operators[0].ports_out.push(port);
            }),
        ),
        ("operators.algorithm", Box::new(|p| p.operators[0].algorithm_version = "1.0".into())),
        ("correlations.matrix", Box::new(|p| p.correlations[0].matrix[0][1] = f64::NAN)),
        ("correlations.matrix", Box::new(|p| p.correlations[0].matrix[0][1] = -0.0)),
        ("correlations.matrix", Box::new(|p| p.correlations[0].matrix[1].push(0.0))),
        (
            "edges",
            Box::new(|p| {
                let e = p.edges[0].clone();
                p.edges.push(e);
            }),
        ),
        ("name", Box::new(|p| p.name = String::new())),
        ("operators.algorithm_id", Box::new(|p| p.operators[0].algorithm_id = format!("unc.op.{}", "s".repeat(58)))),
        ("operators.params", Box::new(|p| p.operators[0].params = Json::Arr(vec![]))),
        (
            "operators.params",
            Box::new(|p| {
                let k = |v: &str| ("k".to_owned(), Json::Str(v.into()));
                p.operators[0].params = Json::Obj(vec![k("1"), k("2")]);
            }),
        ),
        (
            "operators.params",
            Box::new(|p| p.operators[0].params = Json::Obj(vec![("k".into(), Json::Str("a\"b".into()))])),
        ),
        (
            "operators.params",
            Box::new(|p| p.operators[0].params = Json::Obj(vec![("k\u{202e}".into(), Json::Str(String::new()))])),
        ),
        (
            "operators.params",
            Box::new(|p| {
                let mut v = Json::Str("x".into());
                for _ in 0..MAX_PARAMS_DEPTH {
                    v = Json::Arr(vec![v]);
                }
                p.operators[0].params = Json::Obj(vec![("k".into(), v)]);
            }),
        ),
        ("semver", Box::new(|p| p.semver = "1.0.0+build".into())),
    ];
    for (field, f) in cases {
        let mut p = small();
        f(&mut p);
        assert_eq!(UncertaintyModel::new(p), Err(SchemaError::SchemaInvalid { field }), "{field}");
    }
    let mut p = small();
    p.outputs[0].id = "a".into();
    assert_eq!(UncertaintyModel::new(p), Err(SchemaError::DuplicateNode { id: "a".into() }));
}

#[test]
fn first_error_does_not_depend_on_input_order() {
    // Two errors: input "z" has a bad quantity kind, input "b" a bad author. Nodes are
    // checked in sorted order, so "b" reports first whatever the given order.
    let mut p = small();
    p.inputs[1].id = "z".into();
    p.inputs[1].quantity_kind = "Bad".into();
    p.inputs[0].source.as_mut().unwrap().author = " bad".into();
    let want = Err(SchemaError::SchemaInvalid { field: "inputs.source.author" });
    let mut r = p.clone();
    r.inputs.reverse();
    assert_eq!(UncertaintyModel::new(p), want);
    assert_eq!(UncertaintyModel::new(r), want);
}

#[test]
fn params_at_the_depth_limit_round_trip() {
    let mut p = small();
    let mut v = Json::Str("x".into());
    for _ in 0..MAX_PARAMS_DEPTH - 1 {
        v = Json::Arr(vec![v]);
    }
    p.operators[0].params = Json::Obj(vec![("k".into(), v)]);
    let m = UncertaintyModel::new(p).unwrap();
    assert_eq!(UncertaintyModel::parse(m.canonical_json()).unwrap(), m);
}

#[test]
fn a_model_too_big_for_the_reader_is_refused_in_memory() {
    let mut p = small();
    let many = vec![Json::Str(String::new()); MAX_VALUES];
    p.operators[0].params = Json::Obj(vec![("k".into(), Json::Arr(many))]);
    assert_eq!(UncertaintyModel::new(p), Err(SchemaError::TooLarge));
}

#[test]
fn repeated_ids_are_reported_before_field_errors_in_any_order() {
    // Two inputs share the ID "b"; one copy also has a bad quantity kind. Whichever copy
    // comes first, the error is duplicate_node.
    let mut p = small();
    let mut copy = p.inputs[0].clone();
    copy.quantity_kind = "Bad".into();
    p.inputs.push(copy);
    let mut r = p.clone();
    r.inputs.reverse();
    let want = Err(SchemaError::DuplicateNode { id: "b".into() });
    assert_eq!(UncertaintyModel::new(p), want);
    assert_eq!(UncertaintyModel::new(r), want);

    // An input in two groups, one of which also has a bad matrix: always correlations.inputs.
    let mut p = small();
    p.correlations.push(CorrelationGroup { inputs: vec!["a".into(), "c".into()], matrix: vec![vec![1.0]] });
    let mut r = p.clone();
    r.correlations.reverse();
    let want = Err(SchemaError::SchemaInvalid { field: "correlations.inputs" });
    assert_eq!(UncertaintyModel::new(p), want);
    assert_eq!(UncertaintyModel::new(r), want);
}
