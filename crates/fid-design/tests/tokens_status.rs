//! Acceptance criteria of spec node UX.TOKENS.STATUS (AC-1 .. AC-3, provisional AC-4) and further
//! error paths. Expected results are in `fixtures/ux/tokens/status/expected.json`, written by
//! `make_fixtures.py`: hand-labelled findings, and the shipped palette's values, CIEDE2000
//! distances under four vision conditions, WCAG contrasts and OKLCH hues computed by an
//! independent Python reading of the files, never by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::status::cvd::{delta_e2000_under, Vision};
use fid_design::status::{
    glyph_id, text_id, CUE_KEYS, MIN_DELTA_E2000, MIN_NEUTRAL_DELTA_E2000, NC_COLOURED, NEUTRAL, NONCONFORMING, STATES,
};
use fid_design::tokens::oklab::oklab;
use fid_design::tokens::wcag::{contrast_ratio, parse_hex};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo().join("fixtures/ux/tokens/status").join(rel)
}

fn design() -> PathBuf {
    repo().join("design")
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).args(args).output().expect("run fid-design")
}

fn check(root: &Path, json: bool) -> Output {
    let r = root.to_string_lossy().into_owned();
    let mut args = vec!["status-check", "--root", r.as_str()];
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

fn fixture_case(name: &str) -> CValue {
    let e = get(&expected(), &["fixtures", name]).clone();
    let o = check(&fix(name), true);
    assert_eq!(o.status.code(), Some(num(get(&e, &["exit"])) as i32), "{name}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(&e, &["findings"])), "{name}");
    r
}

/// The shipped palette as the oracle read it: theme -> state -> hex.
fn shipped_values() -> BTreeMap<String, BTreeMap<String, String>> {
    let e = expected();
    obj(get(&e, &["shipped", "values"]))
        .iter()
        .map(|(t, row)| (t.clone(), obj(row).iter().map(|(k, v)| (k.clone(), s(v).to_string())).collect()))
        .collect()
}

#[test]
fn ac1_shipped_palette_lists_6_states_by_4_themes() {
    let o = check(&design(), true);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), Vec::<String>::new());
    assert_eq!(get(&r, &["ok"]), &CValue::Bool(true));
    assert_eq!(get(&r, &["locked"]), &CValue::Bool(true));
    assert_eq!(num(get(&r, &["counts", "states"])), 6.0);
    assert_eq!(num(get(&r, &["counts", "themes"])), 4.0);
    assert_eq!(num(get(&r, &["counts", "colours"])), 24.0);
    assert_eq!(strs(get(&r, &["states"])), STATES.iter().map(|x| x.to_string()).collect::<Vec<_>>());
    let want = shipped_values();
    assert_eq!(want.len(), 4);
    for (t, row) in &want {
        assert_eq!(row.len(), 6, "{t}");
        for (st, hex) in row {
            assert_eq!(s(get(&r, &["themes", t, st])), hex, "{t} {st}");
        }
    }
    let t = stdout(&check(&design(), false));
    assert!(t.contains("6 states x 4 themes, 24 colours"), "{t}");
    assert!(t.contains("ok: no findings"), "{t}");
}

#[test]
fn ac1_valid_fixture_passes() {
    fixture_case("ok");
}

#[test]
fn ac2_missing_hc_stale() {
    fixture_case("missing-hc");
    let t = stdout(&check(&fix("missing-hc"), false));
    assert!(t.contains("STATUS-MISSING hc stale"), "{t}");
    fixture_case("missing-theme");
}

#[test]
fn ac3_unlocked() {
    let r = fixture_case("unlocked");
    assert_eq!(get(&r, &["locked"]), &CValue::Bool(false));
    let t = stdout(&check(&fix("unlocked"), false));
    assert!(t.contains("STATUS-UNLOCKED"), "{t}");
}

#[test]
fn set_membership_and_brand_tier() {
    fixture_case("stale-conforming");
    fixture_case("set-duplicate");
    fixture_case("brand-status");
}

#[test]
fn threshold_fields() {
    fixture_case("threshold");
}

#[test]
fn schema_errors() {
    for name in ["wrong-format", "extra-key", "locked-string", "bad-hex", "unknown-state", "unknown-theme", "no-file"] {
        fixture_case(name);
    }
}

/// REQ-UX.TOKENS.STATUS-001: `sem.status.<state>` aliases exactly the palette values.
#[test]
fn semantic_tokens_alias_the_palette() {
    for name in ["alias-mismatch", "alias-unbound", "alias-extra"] {
        fixture_case(name);
    }
    let tokens = fid_design::tokens::check(&design()).unwrap();
    for (t, row) in shipped_values() {
        for (st, hex) in row {
            assert_eq!(tokens.themes[&t].values[&format!("sem.status.{st}")].value, hex, "{t} {st}");
        }
    }
}

/// Never colour alone: every state carries the glyph and text IDs of UX.ICONS.CLASS, distinct per
/// state, and the semantic token says so.
#[test]
fn every_state_has_its_glyph_and_text() {
    for name in ["cue-missing", "cue-wrong-glyph", "cue-no-text", "cue-no-oot"] {
        fixture_case(name);
    }
    let pal = fid_canon::parse(&std::fs::read(design().join("tokens/status.json")).unwrap()).unwrap();
    let cues = obj(get(&pal, &["cues"]));
    assert_eq!(cues.len(), CUE_KEYS.len());
    let mut glyphs = BTreeSet::new();
    let mut texts = BTreeSet::new();
    for st in CUE_KEYS {
        let g = s(get(&cues[st], &["glyph"]));
        let x = s(get(&cues[st], &["text"]));
        assert_eq!(g, glyph_id(st));
        assert_eq!(x, text_id(st));
        glyphs.insert(g.to_string());
        texts.insert(x.to_string());
    }
    assert_eq!(glyphs.len(), 7);
    assert_eq!(texts.len(), 7);
    assert_eq!(glyph_id("out-of-tolerance"), "pattern.out-of-tolerance");
    let sem = fid_canon::parse(&std::fs::read(design().join("tokens/semantic/color.tokens.json")).unwrap()).unwrap();
    for st in STATES {
        let d = s(get(&sem, &["sem", "status", st, "$description"]));
        assert!(d.contains("P-NOCOLOR") && d.contains(&glyph_id(st)) && d.contains(&text_id(st)), "{st}: {d}");
    }
}

/// Provisional AC-4 (until UX.TOKENS.CHECK exists): this crate's CIEDE2000 under the four vision
/// conditions equals the independent oracle's, and every nonconforming-set colour is at least 20
/// from CONFORMS in every theme. The other states are held to the same floor (the runtime guard
/// of VIZ.OVERLAY.STATUS applies it to them).
#[test]
fn ac4_provisional_cvd_distances() {
    let values = shipped_values();
    let e = expected();
    let want = arr(get(&e, &["shipped", "de2000"]));
    assert_eq!(want.len(), 4 * 5 * 4);
    let mut nc_seen = 0;
    for w in want {
        let (t, st, v) = (s(get(w, &["theme"])), s(get(w, &["state"])), s(get(w, &["vision"])));
        let vision = Vision::parse(v).unwrap();
        let c = parse_hex(&values[t]["conforms"]).unwrap();
        let x = parse_hex(&values[t][st]).unwrap();
        let d = delta_e2000_under(vision, c, x);
        let oracle = num(get(w, &["de2000"]));
        assert!((d - oracle).abs() < 1e-6, "{t} {st} {v}: {d} vs oracle {oracle}");
        assert!(d >= MIN_DELTA_E2000, "{t} {st} {v}: {d}");
        if NONCONFORMING.contains(&st) {
            nc_seen += 1;
        }
    }
    assert_eq!(nc_seen, 4 * 2 * 4, "nonconforms and stale in every theme under every vision");
}

/// WCAG 2.2 contrast of every status colour on the plain surfaces equals the oracle's and meets
/// 4.5:1 (7:1 in hc). The same pairs are declared in tokens/pairs.json (UX.TOKENS.COLOR).
#[test]
fn status_colours_meet_text_contrast() {
    let values = shipped_values();
    let tokens = fid_design::tokens::check(&design()).unwrap();
    let e = expected();
    let want = arr(get(&e, &["shipped", "contrasts"]));
    assert_eq!(want.len(), 4 * 6 * 3);
    for w in want {
        let (t, st, bg) = (s(get(w, &["theme"])), s(get(w, &["state"])), s(get(w, &["bg"])));
        let a = parse_hex(&values[t][st]).unwrap();
        let b = parse_hex(&tokens.themes[t].values[bg].value).unwrap();
        let r = contrast_ratio(a, b);
        assert!((r - num(get(w, &["ratio"]))).abs() < 1e-9, "{t} {st}/{bg}");
        let need = if t == "hc" { 7.0 } else { 4.5 };
        assert_eq!(num(get(w, &["required"])), need);
        assert!(r >= need, "{t} {st}/{bg}: {r}");
    }
}

/// REQ-UX.TOKENS.STATUS-002, gated on distance: indeterminate, not-decidable and not-evaluated are
/// at least 10 (CIEDE2000) from nonconforms and stale under all four vision conditions in every
/// theme; this crate's values equal the oracle's. The error path is `neutral-near-stale`.
#[test]
fn neutral_states_keep_their_distance_from_the_nonconforming_set() {
    let r = fixture_case("neutral-near-stale");
    assert!(stdout(&check(&fix("neutral-near-stale"), false)).contains("STATUS-DISTANCE dark indeterminate stale"));
    assert_eq!(get(&r, &["ok"]), &CValue::Bool(false));
    let values = shipped_values();
    let e = expected();
    let want = arr(get(&e, &["shipped", "neutral_de2000"]));
    assert_eq!(want.len(), 4 * NEUTRAL.len() * NC_COLOURED.len() * 4);
    for w in want {
        let (t, st, x, v) =
            (s(get(w, &["theme"])), s(get(w, &["state"])), s(get(w, &["other"])), s(get(w, &["vision"])));
        let d = delta_e2000_under(
            Vision::parse(v).unwrap(),
            parse_hex(&values[t][st]).unwrap(),
            parse_hex(&values[t][x]).unwrap(),
        );
        let oracle = num(get(w, &["de2000"]));
        assert!((d - oracle).abs() < 1e-6, "{t} {st}/{x} {v}: {d} vs oracle {oracle}");
        assert!(d >= MIN_NEUTRAL_DELTA_E2000, "{t} {st}/{x} {v}: {d}");
    }
}

/// The +/-30 degree OKLCH hue-family rule is advice only (not gating): this crate's hue and chroma
/// equal the oracle's, and the oracle records any advice.
#[test]
fn hue_family_advice_matches_the_oracle() {
    let values = shipped_values();
    let e = expected();
    let _advice = arr(get(&e, &["shipped", "hue_advice"]));
    for w in arr(get(&e, &["shipped", "hues"])) {
        let (t, st) = (s(get(w, &["theme"])), s(get(w, &["state"])));
        let [_, a, b] = oklab(parse_hex(&values[t][st]).unwrap());
        let (h, c) = (b.atan2(a).to_degrees().rem_euclid(360.0), (a * a + b * b).sqrt());
        assert!((c - num(get(w, &["chroma"]))).abs() < 1e-6, "{t} {st}");
        if c >= 0.03 {
            let d = (h - num(get(w, &["hue"]))).abs() % 360.0;
            assert!(d.min(360.0 - d) < 1e-4, "{t} {st}");
        }
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
    assert_eq!(s(get(&json_out(&a), &["format"])), fid_design::status::REPORT_FORMAT);
}

#[test]
fn bad_usage_and_unreadable_root_exit_2() {
    assert_eq!(run(&["status-check"]).status.code(), Some(2));
    assert_eq!(run(&["status-check", "--root"]).status.code(), Some(2));
    let missing = repo().join("fixtures/ux/tokens/status/no-such-dir");
    assert_eq!(check(&missing, false).status.code(), Some(2));
}

/// Every labelled fixture is exercised by some test above.
#[test]
fn every_fixture_is_labelled() {
    let e = expected();
    let names: BTreeSet<&String> = obj(get(&e, &["fixtures"])).keys().collect();
    assert_eq!(names.len(), 23);
    for n in names {
        assert!(fix(n).join("tokens").is_dir(), "{n}");
    }
}
