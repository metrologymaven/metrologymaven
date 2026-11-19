//! Acceptance criteria of spec node CORE.REG.VER.COMPAT (AC-1 .. AC-5).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::{canonicalize, parse, to_canonical_bytes, CValue};
use fid_registry::{classify, diff, CommandDef, Kind, Manifest, Reason};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    let CValue::Obj(o) = v else { panic!("object expected") };
    o
}

fn pairs() -> Vec<(String, BTreeMap<String, CValue>)> {
    let dir = root().join("fixtures/core/reg/ver/compat/pairs");
    let mut files: Vec<PathBuf> = std::fs::read_dir(&dir).unwrap().map(|e| e.unwrap().path()).collect();
    files.sort();
    files
        .into_iter()
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .map(|p| {
            let name = p.file_name().unwrap().to_string_lossy().into_owned();
            let CValue::Obj(o) = parse(&std::fs::read(&p).unwrap()).unwrap() else { panic!("{name}: object") };
            (name, o)
        })
        .collect()
}

fn def(v: &CValue) -> CommandDef {
    CommandDef::from_value(v.clone()).unwrap()
}

fn manifest(name: &str) -> Manifest {
    Manifest::from_json(&std::fs::read(root().join("fixtures/core/reg/manifest").join(name)).unwrap()).unwrap()
}

/// AC-1: every hand-labelled pair classifies to exactly its `expect` list, and the
/// fixture set exercises every reason.
#[test]
fn ac1_pairs() {
    let all = pairs();
    assert!(all.len() >= 40, "only {} pairs", all.len());
    let mut seen = BTreeSet::new();
    for (name, p) in &all {
        let got: Vec<CValue> = classify(&def(&p["old"]), &def(&p["new"])).iter().map(|c| c.to_value()).collect();
        let want = &p["expect"];
        assert_eq!(
            String::from_utf8(to_canonical_bytes(&CValue::Arr(got)).unwrap()).unwrap(),
            String::from_utf8(to_canonical_bytes(want).unwrap()).unwrap(),
            "pair {name}"
        );
        let CValue::Arr(want) = want else { panic!("{name}: expect must be an array") };
        for w in want {
            let CValue::Str(r) = &obj(w)["reason"] else { panic!("reason") };
            seen.insert(r.clone());
        }
    }
    for r in Reason::ALL {
        assert!(seen.contains(r.as_str()), "no pair exercises reason {r}");
    }
}

/// The reason table: additive exactly for the REQ-001 list.
#[test]
fn additive_reasons_are_the_req001_list() {
    let additive: Vec<&str> = Reason::ALL.iter().filter(|r| r.kind() == Kind::Additive).map(|r| r.as_str()).collect();
    assert_eq!(
        additive,
        [
            "doc_changed",
            "ui_hint_changed",
            "web_changed",
            "cli_bare_changed",
            "optional_param_added",
            "result_field_added"
        ]
    );
}

/// AC-2: the five-change manifest pair.
#[test]
fn ac2_five_changes() {
    let d = diff(&manifest("v1.json"), &manifest("v2.json"));
    assert_eq!(d.len(), 5);
    assert_eq!(d.added.len(), 1);
    assert_eq!(d.removed.len(), 1);
    assert_eq!(d.lifecycle_changed.len(), 1);
    let ty = d.changed.iter().find(|c| c.pointer.ends_with("/speed/type")).unwrap();
    assert_eq!((ty.kind, ty.reason), (Kind::Breaking, Reason::TypeChanged));
    let opt = d.changed.iter().find(|c| c.pointer.ends_with("/retries")).unwrap();
    assert_eq!((opt.kind, opt.reason), (Kind::Additive, Reason::OptionalParamAdded));
    assert!(d.is_breaking());
    let file = std::fs::read(root().join("fixtures/core/reg/manifest/v1_v2.diff.json")).unwrap();
    let want = canonicalize(&file).unwrap();
    assert_eq!(String::from_utf8(d.to_canonical_bytes().unwrap()).unwrap(), String::from_utf8(want).unwrap());
}

/// AC-3: every fixture manifest diffed with itself is empty, and so is every pair side
/// classified against itself.
#[test]
fn ac3_self_diff_empty() {
    let dir = root().join("fixtures/core/reg/manifest");
    let mut n = 0;
    for e in std::fs::read_dir(dir).unwrap() {
        let name = e.unwrap().file_name().to_string_lossy().into_owned();
        if !name.ends_with(".json") || name.ends_with(".diff.json") {
            continue;
        }
        let bytes = std::fs::read(root().join("fixtures/core/reg/manifest").join(&name)).unwrap();
        let Ok(m) = Manifest::from_json(&bytes) else {
            continue; // e.g. a tampered manifest of CORE.REG.MAN
        };
        let d = diff(&m, &m);
        assert!(d.is_empty(), "{name}: {d:?}");
        assert_eq!(d.to_canonical_bytes().unwrap(), b"{\"added\":[],\"changed\":[],\"lifecycle_changed\":[],\"removed\":[]}");
        n += 1;
    }
    assert!(n >= 2);
    for (name, p) in pairs() {
        for side in ["old", "new"] {
            let d = def(&p[side]);
            assert!(classify(&d, &d).is_empty(), "{name} {side}");
        }
    }
}

/// AC-4: a changed `if`/`then`/`else` is breaking with `unclassified_construct`.
#[test]
fn ac4_if_then_else() {
    let all = pairs();
    let (_, p) = all.iter().find(|(n, _)| n.ends_with("-if-then-else-changed.json")).unwrap();
    let got = classify(&def(&p["old"]), &def(&p["new"]));
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].pointer, "/params_schema/then");
    assert_eq!((got[0].kind, got[0].reason), (Kind::Breaking, Reason::UnclassifiedConstruct));
    assert_eq!(got[0].reason.as_str(), "unclassified_construct");
}

/// AC-5: AC-2 twice gives byte-identical output.
#[test]
fn ac5_byte_identical() {
    let a = diff(&manifest("v1.json"), &manifest("v2.json")).to_canonical_bytes().unwrap();
    let b = diff(&manifest("v1.json"), &manifest("v2.json")).to_canonical_bytes().unwrap();
    assert_eq!(a, b);
}

/// Reversing the direction swaps added and removed; the type change stays breaking and
/// the parameter, now dropped, is breaking too.
#[test]
fn reverse_diff() {
    let d = diff(&manifest("v2.json"), &manifest("v1.json"));
    assert_eq!(d.added[0].id, "demo.legacy");
    assert_eq!(d.removed[0].id, "demo.export");
    assert!(d.changed.iter().all(|c| c.kind == Kind::Breaking));
}

fn manifest_value() -> BTreeMap<String, CValue> {
    let v = parse(&std::fs::read(root().join("fixtures/core/reg/manifest/v1.json")).unwrap()).unwrap();
    obj(&v).clone()
}

/// `manifest_invalid` for a tampered definition digest, a duplicate (ID, major), an
/// unknown field and bad JSON.
#[test]
fn manifest_invalid() {
    let code = |v: BTreeMap<String, CValue>| Manifest::from_value(CValue::Obj(v)).unwrap_err().code();

    let mut m = manifest_value();
    let CValue::Arr(cs) = m.get_mut("commands").unwrap() else { panic!() };
    let CValue::Obj(c) = &mut cs[0] else { panic!() };
    c.insert("title".into(), CValue::Str("tampered".into()));
    assert_eq!(code(m), "manifest_invalid");

    let mut m = manifest_value();
    let CValue::Arr(cs) = m.get_mut("commands").unwrap() else { panic!() };
    let first = cs[0].clone();
    cs.push(first);
    assert_eq!(code(m), "manifest_invalid");

    let mut m = manifest_value();
    m.insert("generated_at".into(), CValue::Str("2026-01-01".into()));
    assert_eq!(code(m), "manifest_invalid");

    assert_eq!(Manifest::from_json(b"{\"format\":1,\"format\":2}").unwrap_err().code(), "manifest_invalid");
}

/// Schemas nested beyond the depth limit are not walked: the change is unclassified.
#[test]
fn depth_limit_fails_closed() {
    let all = pairs();
    let (_, p) = all.iter().find(|(n, _)| n.ends_with("-identical.json")).unwrap();
    let mut old = def(&p["old"]);
    let mut leaf = CValue::Obj(BTreeMap::from([("type".to_string(), CValue::Str("string".into()))]));
    for _ in 0..70 {
        leaf = CValue::Obj(BTreeMap::from([
            ("type".to_string(), CValue::Str("object".into())),
            ("additionalProperties".to_string(), CValue::Bool(false)),
            ("properties".to_string(), CValue::Obj(BTreeMap::from([("a".to_string(), leaf)]))),
        ]));
    }
    old.result_schema = leaf.clone();
    let mut new = old.clone();
    // Add a result field at the deepest level: additive if walked, but it is too deep.
    fn deepest(v: &mut CValue) -> &mut BTreeMap<String, CValue> {
        let CValue::Obj(o) = v else { panic!() };
        let CValue::Obj(p) = o.get_mut("properties").unwrap() else { panic!() };
        let deeper = matches!(p.get("a"), Some(CValue::Obj(n)) if n.contains_key("properties"));
        if deeper {
            deepest(p.get_mut("a").unwrap())
        } else {
            p
        }
    }
    deepest(&mut new.result_schema).insert("b".into(), CValue::Obj(BTreeMap::new()));
    let got = classify(&old, &new);
    assert_eq!(got.len(), 1);
    assert_eq!(got[0].reason, Reason::UnclassifiedConstruct);
}
