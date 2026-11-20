//! Acceptance criteria of spec node UX.TOKENS.COLOR (AC-1 .. AC-4, provisional AC-5) and further
//! error paths. Expected results are in `fixtures/ux/tokens/color/expected.json`, written by
//! `make_fixtures.py`: hand-labelled findings, and per-theme values and WCAG contrast ratios
//! computed by an independent Python reading of the files, never by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo().join("fixtures/ux/tokens/color").join(rel)
}

fn design() -> PathBuf {
    repo().join("design")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).args(args).output().expect("run fid-design")
}

fn check(root: &Path, json: bool) -> Output {
    let r = root.to_string_lossy().into_owned();
    let mut args = vec!["tokens-check", "--root", r.as_str()];
    if json {
        args.push("--json");
    }
    run(&args)
}

fn stdout(o: &Output) -> String {
    String::from_utf8(o.stdout.clone()).unwrap()
}

fn json_out(o: &Output) -> CValue {
    fid_canon::parse(stdout(o).trim_end().as_bytes()).unwrap()
}

fn expected() -> CValue {
    fid_canon::parse(&std::fs::read(fix("expected.json")).unwrap()).unwrap()
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

fn num(v: &CValue) -> f64 {
    match v {
        CValue::Num(n) => *n,
        other => panic!("not a number: {other:?}"),
    }
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

fn obj(v: &CValue) -> &BTreeMap<String, CValue> {
    match v {
        CValue::Obj(m) => m,
        other => panic!("not an object: {other:?}"),
    }
}

/// Unique `CODE subject` keys of a JSON report.
fn finding_keys(report: &CValue) -> Vec<String> {
    let keys: BTreeSet<String> = arr(get(report, &["findings"]))
        .iter()
        .map(|f| format!("{} {}", s(get(f, &["code"])), s(get(f, &["subject"]))))
        .collect();
    keys.into_iter().collect()
}

fn strs(v: &CValue) -> Vec<String> {
    arr(v).iter().map(|x| s(x).to_string()).collect()
}

/// Per-theme `[resolved, unresolved]` counts must match the oracle.
fn assert_counts(report: &CValue, want: &CValue, what: &str) {
    for theme in ["light", "dark", "hc", "floor"] {
        for (tier, key) in [("semantic", "semantic"), ("component", "component")] {
            let w = arr(get(want, &[theme, key]));
            let got = get(report, &["themes", theme, tier]);
            assert_eq!(num(get(got, &["resolved"])), num(&w[0]), "{what} {theme} {tier} resolved");
            assert_eq!(num(get(got, &["unresolved"])), num(&w[1]), "{what} {theme} {tier} unresolved");
        }
    }
}

fn fixture_case(name: &str) {
    let e = get(&expected(), &["fixtures", name]).clone();
    let o = check(&fix(name), true);
    assert_eq!(o.status.code(), Some(num(get(&e, &["exit"])) as i32), "{name}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(&e, &["findings"])), "{name}");
    assert_counts(&r, get(&e, &["counts"]), name);
}

#[test]
fn ac1_shipped_tokens_resolve_in_every_theme() {
    let o = check(&design(), true);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let r = json_out(&o);
    let e = expected();
    let shipped = get(&e, &["shipped"]);
    assert_eq!(finding_keys(&r), Vec::<String>::new());
    assert_eq!(get(&r, &["ok"]), &CValue::Bool(true));
    assert_counts(&r, get(shipped, &["counts"]), "shipped");
    for theme in ["light", "dark", "hc", "floor"] {
        assert_eq!(num(get(&r, &["themes", theme, "semantic", "unresolved"])), 0.0);
        assert_eq!(num(get(&r, &["themes", theme, "component", "unresolved"])), 0.0);
        // Every resolved value equals the oracle's independent resolution.
        let want = obj(get(shipped, &["values", theme]));
        let got = obj(get(&r, &["themes", theme, "values"]));
        assert_eq!(got.len(), want.len(), "{theme}");
        for (n, v) in want {
            assert_eq!(s(get(&got[n], &["value"])), s(v), "{theme} {n}");
        }
    }
    let t = stdout(&check(&design(), false));
    assert!(t.contains("dark: semantic 29 resolved, 0 unresolved"), "{t}");
    assert!(t.contains("ok: no findings"), "{t}");
}

#[test]
fn ac1_valid_fixture_passes() {
    fixture_case("ok");
}

#[test]
fn ac2_missing_dark_binding_is_unresolved() {
    fixture_case("missing-dark");
    let t = stdout(&check(&fix("missing-dark"), false));
    assert!(t.contains("TOK-UNRESOLVED dark sem.text.muted"), "{t}");
    fixture_case("unknown-target");
    fixture_case("missing-theme");
}

#[test]
fn ac3_component_aliasing_a_primitive_fails() {
    fixture_case("comp-to-prim");
    let t = stdout(&check(&fix("comp-to-prim"), false));
    assert!(t.contains("TOK-COMP-SKIPS-SEM comp.button.border.primary"), "{t}");
    fixture_case("comp-literal");
    // Review of PR 330: untyped and non-colour tokens do not escape the rule.
    fixture_case("untyped-comp-to-prim");
    fixture_case("comp-to-noncolour-sem");
}

#[test]
fn ac4_brand_tier_aliasing_status_fails() {
    fixture_case("brand-alias-status");
    let o = check(&fix("brand-alias-status"), false);
    assert_eq!(o.status.code(), Some(1));
    assert!(stdout(&o).contains("TOK-BRAND-STATUS"), "{}", stdout(&o));
    fixture_case("status-alias-brand");
}

#[test]
fn brand_tier_bounds() {
    fixture_case("brand-size");
    fixture_case("brand-scope");
}

#[test]
fn other_structural_errors() {
    fixture_case("alias-cycle");
    fixture_case("name-syntax");
    fixture_case("pair-unknown");
    fixture_case("schema-default");
}

#[test]
fn claimed_values_never_look_measured() {
    for name in [
        "claimed-same-ramp",
        "claimed-via-measured",
        "claimed-via-brand",
        "comp-claimed-measured",
        "claimed-missing",
        "claimed-near-status",
        "claimed-brand-ramp",
        "claimed-prefix",
    ] {
        fixture_case(name);
    }
}

/// AC-5 depends on UX.TOKENS.CHECK (not built yet). Until then this is the provisional evidence:
/// the WCAG contrast of every declared pair in every theme, computed by this crate's formula,
/// equals the independent Python oracle and meets the threshold for its tag and theme.
#[test]
fn ac5_provisional_every_pair_meets_its_threshold() {
    let report = fid_design::tokens::check(&design()).unwrap();
    let got = report.pair_contrasts();
    let e = expected();
    let want = arr(get(&e, &["shipped", "pairs"]));
    assert_eq!(got.len(), want.len());
    assert_eq!(got.len(), 4 * report.pairs.len());
    let mut by_key = BTreeMap::new();
    for w in want {
        let k = format!("{} {}/{}", s(get(w, &["theme"])), s(get(w, &["fg"])), s(get(w, &["bg"])));
        by_key.insert(k, (s(get(w, &["tag"])).to_string(), num(get(w, &["ratio"])), num(get(w, &["required"]))));
    }
    for g in &got {
        let k = format!("{} {}/{}", g.theme, g.pair.fg, g.pair.bg);
        let (tag, ratio, required) = by_key.get(&k).unwrap_or_else(|| panic!("{k} not in oracle"));
        let r = g.ratio.unwrap_or_else(|| panic!("{k}: unresolved"));
        assert_eq!(&g.pair.tag, tag, "{k}");
        assert!((r - ratio).abs() < 1e-8, "{k}: {r} vs oracle {ratio}");
        assert_eq!(g.required, Some(*required), "{k}");
        assert!(r >= *required, "{k}: {r} < {required}");
    }
}

#[test]
fn status_and_claimed_roles_carry_the_no_colour_rule() {
    let v = fid_canon::parse(&std::fs::read(design().join("tokens/semantic/color.tokens.json")).unwrap()).unwrap();
    let status = obj(get(&v, &["sem", "status"]));
    assert_eq!(status.len(), 6);
    for (n, t) in status.iter().filter(|(k, _)| !k.starts_with('$')) {
        assert!(s(get(t, &["$description"])).contains("P-NOCOLOR"), "{n}");
    }
    for path in [["sem", "text", "value", "claimed"], ["sem", "border", "value", "claimed"]] {
        assert!(s(get(&v, &[path[0], path[1], path[2], path[3], "$description"])).contains("never looks measured"));
    }
}

#[test]
fn report_is_deterministic_and_canonical() {
    let a = check(&design(), true);
    let b = check(&design(), true);
    assert_eq!(a.stdout, b.stdout);
    let body = stdout(&a);
    let canon = fid_canon::to_canonical_bytes(&json_out(&a)).unwrap();
    assert_eq!(body.trim_end().as_bytes(), canon.as_slice());
    assert_eq!(s(get(&json_out(&a), &["format"])), fid_design::tokens::REPORT_FORMAT);
}

#[test]
fn bad_usage_and_unreadable_root_exit_2() {
    assert_eq!(run(&["tokens-check"]).status.code(), Some(2));
    assert_eq!(run(&["tokens-check", "--root"]).status.code(), Some(2));
    let missing = repo().join("fixtures/ux/tokens/color/no-such-dir");
    assert_eq!(check(&missing, false).status.code(), Some(2));
}

/// The OKLab distance floor between claimed roles and measured, brand-tier and status roles:
/// this crate's OKLab equals the oracle's independent one, and every distance clears the floor.
#[test]
fn claimed_roles_keep_their_oklab_distance() {
    use fid_design::tokens::{oklab::delta_e_ok, wcag::parse_hex, MIN_DELTA_E_OK};
    let report = fid_design::tokens::check(&design()).unwrap();
    let e = expected();
    let want = arr(get(&e, &["shipped", "claimed_distances"]));
    assert!(want.len() >= 4 * 3 * 11, "{}", want.len());
    for w in want {
        let theme = s(get(w, &["theme"]));
        let values = &report.themes[theme].values;
        let (c, o) = (s(get(w, &["claimed"])), s(get(w, &["other"])));
        let d = delta_e_ok(parse_hex(&values[c].value).unwrap(), parse_hex(&values[o].value).unwrap());
        let oracle = num(get(w, &["de_ok"]));
        assert!((d - oracle).abs() < 1e-8, "{theme} {c}/{o}: {d} vs oracle {oracle}");
        assert!(d >= MIN_DELTA_E_OK, "{theme} {c}/{o}: {d}");
    }
}
