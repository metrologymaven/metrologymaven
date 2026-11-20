//! Acceptance criteria of spec node UX.TOKENS.DATAPAL (AC-1 .. AC-4) and further
//! error paths. Expected results are in `fixtures/ux/tokens/datapal/expected.json`, written by
//! `make_fixtures.py`: hand-labelled findings, and the shipped palettes' digests, lightness,
//! CIEDE2000 distances under four vision conditions, contrasts and OKLab distances computed by an
//! independent Python reading of the files, never by this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_canon::CValue;
use fid_design::palettes::{
    chroma, delta_pairs, digest, lstar, min_delta_e2000, Kind, CONFORMS_TOKEN, DEFAULT, MAX_NEUTRAL_CHROMA,
    MAX_WINDOW_RATIO, MIN_OOT_CONTRAST, MIN_OOT_INRANGE_DELTA_E2000, MIN_OOT_NEUTRAL_DELTA_E2000, MIN_STEP_DELTA_E2000,
    NEUTRAL_TOKENS, REQUIRED, SURFACE_TOKEN, WINDOW,
};
use fid_design::status::cvd::{delta_e2000_under, Vision};
use fid_design::tokens::oklab::delta_e_ok;
use fid_design::tokens::wcag::{contrast_ratio, parse_hex, Rgb};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn repo() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn fix(rel: &str) -> PathBuf {
    repo().join("fixtures/ux/tokens/datapal").join(rel)
}

fn design() -> PathBuf {
    repo().join("design")
}

/// The release report of `fid-design color-check` (UX.TOKENS.CHECK) on the shipped tree, which
/// must pass.
fn gate_report() -> CValue {
    let d = design().to_string_lossy().into_owned();
    let o = run(&["color-check", "--root", d.as_str(), "--json"]);
    assert_eq!(o.status.code(), Some(0), "{}", String::from_utf8_lossy(&o.stdout));
    fid_canon::parse(String::from_utf8(o.stdout).unwrap().trim_end().as_bytes()).unwrap()
}

/// A report value (rounded to 0.01) against an oracle value at full precision.
fn close_to_rounded(reported: f64, oracle: f64) -> bool {
    (reported - oracle).abs() <= 0.005 + 1e-9
}

fn run(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_fid-design")).args(args).output().expect("run fid-design")
}

fn check(root: &Path, baseline: Option<&Path>, json: bool) -> Output {
    let r = root.to_string_lossy().into_owned();
    let b = baseline.map(|b| b.to_string_lossy().into_owned());
    let mut args = vec!["palettes-check", "--root", r.as_str()];
    if let Some(b) = &b {
        args.push("--baseline");
        args.push(b.as_str());
    }
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

fn strs(v: &CValue) -> Vec<String> {
    arr(v).iter().map(|x| s(x).to_string()).collect()
}

fn finding_keys(report: &CValue) -> Vec<String> {
    let keys: BTreeSet<String> = arr(get(report, &["findings"]))
        .iter()
        .map(|f| format!("{} {}", s(get(f, &["code"])), s(get(f, &["subject"]))))
        .collect();
    keys.into_iter().collect()
}

fn fixture_case(name: &str) -> CValue {
    let e = get(&expected(), &["fixtures", name]).clone();
    let baseline = match get(&e, &["baseline"]) {
        CValue::Str(b) => Some(fix(name).join(b)),
        _ => None,
    };
    let o = check(&fix(name), baseline.as_deref(), true);
    assert_eq!(o.status.code(), Some(num(get(&e, &["exit"])) as i32), "{name}: {}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), strs(get(&e, &["findings"])), "{name}");
    r
}

fn hex(v: &CValue) -> Rgb {
    parse_hex(s(v)).unwrap()
}

/// The shipped palettes as the oracle read them: ref -> (colors, under, over).
fn shipped_values() -> BTreeMap<String, (Vec<Rgb>, Rgb, Rgb)> {
    let e = expected();
    obj(get(&e, &["shipped", "values"]))
        .iter()
        .map(|(r, p)| {
            let cols = arr(get(p, &["colors"])).iter().map(hex).collect();
            (r.clone(), (cols, hex(get(p, &["under"])), hex(get(p, &["over"]))))
        })
        .collect()
}

/// The semantic roles the oracle resolved: theme -> name -> colour.
fn shipped_sem() -> BTreeMap<String, BTreeMap<String, Rgb>> {
    let e = expected();
    obj(get(&e, &["shipped", "sem"]))
        .iter()
        .map(|(t, row)| (t.clone(), obj(row).iter().map(|(k, v)| (k.clone(), hex(v))).collect()))
        .collect()
}

#[test]
fn ac1_shipped_catalogue_lists_the_four_palettes_with_the_default() {
    let o = check(&design(), None, true);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    let r = json_out(&o);
    assert_eq!(finding_keys(&r), Vec::<String>::new());
    assert_eq!(get(&r, &["ok"]), &CValue::Bool(true));
    assert_eq!(s(get(&r, &["default"])), DEFAULT);
    assert_eq!(DEFAULT, "pal.step.cvd@1");
    let e = expected();
    let pals = arr(get(&r, &["palettes"]));
    let refs: BTreeSet<String> = pals.iter().map(|p| s(get(p, &["ref"])).to_string()).collect();
    assert_eq!(refs, REQUIRED.iter().map(|x| x.to_string()).collect::<BTreeSet<_>>());
    for p in pals {
        let rf = s(get(p, &["ref"]));
        let want = get(&e, &["shipped", "palettes", rf]);
        assert_eq!(s(get(p, &["kind"])), s(get(want, &["kind"])), "{rf}");
        assert_eq!(num(get(p, &["count"])), num(get(want, &["count"])), "{rf}");
        assert_eq!(s(get(p, &["digest"])), s(get(want, &["digest"])), "{rf}: digest differs from the oracle's");
        assert_eq!(get(p, &["default"]), &CValue::Bool(rf == DEFAULT), "{rf}");
    }
    let t = stdout(&check(&design(), None, false));
    assert!(t.contains("4 palettes, default pal.step.cvd@1"), "{t}");
    assert!(t.contains("ok: no findings"), "{t}");
    // The shipped digests as a baseline: nothing changed.
    let o = check(&design(), Some(&fix("shipped-digests.json")), true);
    assert_eq!(o.status.code(), Some(0), "{}", stdout(&o));
    assert_eq!(get(&json_out(&o), &["baseline"]), &CValue::Bool(true));
}

#[test]
fn ac1_valid_fixtures_pass() {
    for name in ["ok", "ok-baseline", "new-major"] {
        fixture_case(name);
    }
}

#[test]
fn ac2_even_step_count() {
    fixture_case("even-steps");
    let t = stdout(&check(&fix("even-steps"), None, false));
    assert!(t.contains("PAL-STEPS pal.step.cvd@1"), "{t}");
    fixture_case("five-steps");
}

#[test]
fn ac3_changed_colour_is_immutable() {
    fixture_case("changed-v1");
    let b = fix("changed-v1/baseline.json");
    let t = stdout(&check(&fix("changed-v1"), Some(&b), false));
    assert!(t.contains("PAL-IMMUTABLE pal.step.tol@1"), "{t}");
    // Without the baseline the changed palette is valid on its own.
    assert_eq!(check(&fix("changed-v1"), None, false).status.code(), Some(0));
    fixture_case("removed-v1");
}

#[test]
fn structural_errors() {
    for name in [
        "digest-mismatch",
        "centre-off",
        "centre-tinted",
        "samples",
        "licence-nc",
        "licence-third-party",
        "default-wrong",
        "default-missing",
        "required-missing",
        "unlisted-file",
        "no-catalogue",
        "bad-hex",
        "extra-key",
        "wrong-kind",
        "id-mismatch",
        "origin-missing",
        "origin-wrong-artefact",
    ] {
        fixture_case(name);
    }
}

#[test]
fn perceptual_errors() {
    for name in [
        "lightness-step",
        "lightness-cont",
        "delta",
        "delta-mirror",
        "delta-under",
        "delta-over",
        "oot-blue",
        "oot-over",
        "contrast",
        "claimed",
        "uniform",
    ] {
        fixture_case(name);
    }
}

/// The digest is SHA-256 of the RFC 8785 form of {colors, under, over}, as the oracle computes it.
#[test]
fn digests_match_the_oracle() {
    let e = expected();
    for (rf, d) in obj(get(&e, &["shipped", "digests"])) {
        let p = get(&e, &["shipped", "values", rf]);
        let cols = strs(get(p, &["colors"]));
        assert_eq!(digest(&cols, s(get(p, &["under"])), s(get(p, &["over"]))).unwrap(), s(d), "{rf}");
    }
}

/// AC-4: the UX.TOKENS.CHECK release gate (`color-check`) passes every stepped palette:
/// strictly monotonic lightness per arm and adjacent-step CIEDE2000 at least 10 under the four
/// vision conditions, with values equal to this leaf's independent oracle (to the report's 0.01).
/// The stricter palettes-check rules hold too: a neutral centre, mirrored steps and the
/// out-of-range colours next to the ends at least 10 apart; this crate's values equal the
/// oracle's.
#[test]
fn ac4_color_check_lightness_and_step_differences() {
    let gate = gate_report();
    let e = expected();
    let gate_pals = arr(get(&gate, &["palettes"]));
    assert_eq!(gate_pals.len(), 2);
    for g in gate_pals {
        let rf = s(get(g, &["ref"]));
        let rec = get(&e, &["shipped", "palettes", rf]);
        assert_eq!(s(get(rec, &["kind"])), "stepped");
        assert_eq!(get(g, &["lstar_monotonic"]), &CValue::Bool(true), "{rf}");
        assert_eq!(get(g, &["default"]), &CValue::Bool(rf == DEFAULT), "{rf}");
        for (a, b) in arr(get(g, &["lstar"])).iter().zip(arr(get(rec, &["lstar"]))) {
            assert!(close_to_rounded(num(a), num(b)), "{rf}: L*");
        }
        let adjacent = arr(get(g, &["adjacent"]));
        assert_eq!(adjacent.len(), (arr(get(rec, &["lstar"])).len() - 1) * 4, "{rf}");
        for x in adjacent {
            let i = num(get(x, &["step"])) as usize;
            let v = s(get(x, &["vision"]));
            let w = arr(get(rec, &["pairs"]))
                .iter()
                .find(|w| s(get(w, &["a"])) == i.to_string() && s(get(w, &["b"])) == (i + 1).to_string())
                .unwrap_or_else(|| panic!("{rf} {i} not in oracle"));
            assert!(close_to_rounded(num(get(x, &["de2000"])), num(get(w, &["de2000", v]))), "{rf} {i} {v}");
            assert_eq!(get(x, &["pass"]), &CValue::Bool(true), "{rf} {i} {v}");
        }
    }
    let values = shipped_values();
    let e = expected();
    for (rf, rec) in obj(get(&e, &["shipped", "palettes"])) {
        let (cols, under, over) = &values[rf];
        let kind = if s(get(rec, &["kind"])) == "stepped" { Kind::Stepped } else { Kind::Continuous };
        let ls: Vec<f64> = cols.iter().map(|c| lstar(*c)).collect();
        for (a, b) in ls.iter().zip(arr(get(rec, &["lstar"]))) {
            assert!((a - num(b)).abs() < 1e-6, "{rf}: L* {a} vs oracle {}", num(b));
        }
        let n = cols.len();
        if kind == Kind::Stepped {
            let c = (n - 1) / 2;
            assert!(n % 2 == 1 && (7..=15).contains(&n), "{rf}");
            assert!(chroma(cols[c]) <= MAX_NEUTRAL_CHROMA);
            assert!((chroma(cols[c]) - num(get(rec, &["centre_chroma"]))).abs() < 1e-6);
            assert!(ls[..c].windows(2).all(|w| w[0] < w[1]), "{rf}: blue arm darkens outwards");
            assert!(ls[c..].windows(2).all(|w| w[0] > w[1]), "{rf}: orange arm darkens outwards");
        } else {
            assert!(ls.windows(2).all(|w| w[1] > w[0]), "{rf}: lightness rises");
        }
        let pairs = arr(get(rec, &["pairs"]));
        assert_eq!(pairs.len(), delta_pairs(kind, n).len(), "{rf}");
        let pick = |l: &str| match l {
            "under" => *under,
            "over" => *over,
            i => cols[i.parse::<usize>().unwrap()],
        };
        for w in pairs {
            let (a, b) = (pick(s(get(w, &["a"]))), pick(s(get(w, &["b"]))));
            for v in Vision::ALL {
                let d = delta_e2000_under(v, a, b);
                let oracle = num(get(w, &["de2000", v.as_str()]));
                assert!((d - oracle).abs() < 1e-6, "{rf} {v:?}: {d} vs oracle {oracle}");
            }
            assert!(min_delta_e2000(a, b).0 >= MIN_STEP_DELTA_E2000, "{rf}");
        }
    }
}

/// REQ-UX.TOKENS-009 for the out-of-tolerance region colours (deferred here by UX.TOKENS.STATUS):
/// every palette's `under` and `over` are at least CIEDE2000 20 from CONFORMS in every theme under
/// the four vision conditions, and at least 3:1 on the base surface; values equal the oracle's.
#[test]
fn out_of_tolerance_colours_keep_away_from_conforms() {
    let values = shipped_values();
    let sem = shipped_sem();
    let tokens = fid_design::tokens::check(&design()).unwrap();
    for (t, row) in &sem {
        for (n, c) in row {
            assert_eq!(parse_hex(&tokens.themes[t].values[n].value).unwrap(), *c, "{t} {n}");
        }
    }
    let e = expected();
    for (rf, rec) in obj(get(&e, &["shipped", "palettes"])) {
        let (_, under, over) = values[rf];
        let side = |x: &str| if x == "under" { under } else { over };
        let oot = arr(get(rec, &["oot"]));
        assert_eq!(oot.len(), 4 * 2 * 4);
        for w in oot {
            let (t, sd, v) = (s(get(w, &["theme"])), s(get(w, &["side"])), s(get(w, &["vision"])));
            let d = delta_e2000_under(Vision::parse(v).unwrap(), side(sd), sem[t][CONFORMS_TOKEN]);
            let oracle = num(get(w, &["de2000"]));
            assert!((d - oracle).abs() < 1e-6, "{rf} {t} {sd} {v}: {d} vs {oracle}");
            assert!(d >= fid_design::status::MIN_DELTA_E2000, "{rf} {t} {sd} {v}: {d}");
        }
        for w in arr(get(rec, &["oot_contrast"])) {
            let (t, sd) = (s(get(w, &["theme"])), s(get(w, &["side"])));
            let ratio = contrast_ratio(side(sd), sem[t][SURFACE_TOKEN]);
            assert!((ratio - num(get(w, &["ratio"]))).abs() < 1e-9, "{rf} {t} {sd}");
            assert!(ratio >= MIN_OOT_CONTRAST, "{rf} {t} {sd}: {ratio}");
        }
    }
}

/// Out-of-range colours never look in range: every palette's `under` and `over` are at least
/// CIEDE2000 10 from every in-range colour of every shipped palette, and from the neutral status
/// colours of every theme, under the four vision conditions; the least distances equal the
/// oracle's.
#[test]
fn out_of_range_colours_differ_from_every_in_range_and_neutral_colour() {
    let values = shipped_values();
    let sem = shipped_sem();
    let all: Vec<Rgb> = values.values().flat_map(|(c, _, _)| c.iter().copied()).collect();
    let e = expected();
    for (rf, rec) in obj(get(&e, &["shipped", "palettes"])) {
        let (_, under, over) = values[rf];
        for (side, c) in [("under", under), ("over", over)] {
            let d = all.iter().map(|x| min_delta_e2000(c, *x).0).fold(f64::INFINITY, f64::min);
            let want = num(get(rec, &["oot_inrange_min", side]));
            assert!((d - want).abs() < 1e-6, "{rf} {side}: {d} vs oracle {want}");
            assert!(d >= MIN_OOT_INRANGE_DELTA_E2000, "{rf} {side}: {d}");
            let mut n = f64::INFINITY;
            for row in sem.values() {
                for t in NEUTRAL_TOKENS {
                    n = n.min(min_delta_e2000(c, row[t]).0);
                }
            }
            let want = num(get(rec, &["oot_neutral_min", side]));
            assert!((n - want).abs() < 1e-6, "{rf} {side}: {n} vs oracle {want}");
            assert!(n >= MIN_OOT_NEUTRAL_DELTA_E2000, "{rf} {side}: {n}");
        }
    }
}

/// Perceptual uniformity of the continuous maps: CIEDE2000 between samples i and i + 16 stays
/// within a factor MAX_WINDOW_RATIO; this crate's minimum and maximum equal the oracle's.
#[test]
fn continuous_maps_are_perceptually_uniform() {
    let values = shipped_values();
    let e = expected();
    let mut seen = 0;
    for (rf, rec) in obj(get(&e, &["shipped", "palettes"])) {
        let CValue::Obj(m) = rec else { panic!() };
        let Some(w) = m.get("window_de2000") else {
            continue;
        };
        seen += 1;
        assert_eq!(num(get(w, &["window"])), WINDOW as f64);
        let d = fid_design::palettes::window_differences(&values[rf].0);
        let lo = d.iter().copied().fold(f64::INFINITY, f64::min);
        let hi = d.iter().copied().fold(0.0, f64::max);
        assert!((lo - num(get(w, &["min"]))).abs() < 1e-6, "{rf}");
        assert!((hi - num(get(w, &["max"]))).abs() < 1e-6, "{rf}");
        assert!(hi / lo <= MAX_WINDOW_RATIO, "{rf}: {}", hi / lo);
    }
    assert_eq!(seen, 2);
}

/// UX.TOKENS.COLOR's claimed-versus-measured floor: no palette colour lies within OKLab 0.10 of a
/// claimed role in any theme; the least distance equals the oracle's.
#[test]
fn palette_colours_stay_away_from_claimed_roles() {
    let values = shipped_values();
    let sem = shipped_sem();
    let e = expected();
    for (rf, rec) in obj(get(&e, &["shipped", "palettes"])) {
        let (cols, under, over) = &values[rf];
        for (t, want) in obj(get(rec, &["claimed_min"])) {
            let claimed: Vec<Rgb> = sem[t].iter().filter(|(k, _)| k.contains(".claimed")).map(|(_, c)| *c).collect();
            assert!(claimed.len() >= 3, "{t}");
            let mut d = f64::INFINITY;
            for x in cols.iter().chain([under, over]) {
                for c in &claimed {
                    d = d.min(delta_e_ok(*x, *c));
                }
            }
            assert!((d - num(want)).abs() < 1e-6, "{rf} {t}: {d} vs {}", num(want));
            assert!(d >= fid_design::tokens::MIN_DELTA_E_OK, "{rf} {t}: {d}");
        }
    }
}

/// Measured, not gated: the WCAG contrast of every stepped colour on the base surface equals the
/// oracle's (the legend and the viewer draw step swatches with a border instead).
#[test]
fn step_contrast_on_the_base_surface_matches_the_oracle() {
    let values = shipped_values();
    let sem = shipped_sem();
    let e = expected();
    for (rf, rec) in obj(get(&e, &["shipped", "palettes"])) {
        let CValue::Obj(m) = rec else { panic!() };
        let Some(table) = m.get("contrast_on_base") else {
            continue;
        };
        for (t, row) in obj(table) {
            for (c, want) in values[rf].0.iter().zip(arr(row)) {
                assert!((contrast_ratio(*c, sem[t][SURFACE_TOKEN]) - num(want)).abs() < 1e-9, "{rf} {t}");
            }
        }
    }
}

#[test]
fn report_is_deterministic_and_canonical() {
    let a = check(&design(), None, true);
    let b = check(&design(), None, true);
    assert_eq!(a.stdout, b.stdout);
    let body = stdout(&a);
    let canon = fid_canon::to_canonical_bytes(&json_out(&a)).unwrap();
    assert_eq!(body.trim_end().as_bytes(), canon.as_slice());
    assert_eq!(s(get(&json_out(&a), &["format"])), fid_design::palettes::REPORT_FORMAT);
}

#[test]
fn bad_usage_and_unreadable_input_exit_2() {
    assert_eq!(run(&["palettes-check"]).status.code(), Some(2));
    assert_eq!(run(&["palettes-check", "--root"]).status.code(), Some(2));
    assert_eq!(run(&["palettes-check", "--root", "x", "--baseline"]).status.code(), Some(2));
    let d = design().to_string_lossy().into_owned();
    // --baseline belongs to palettes-check only.
    assert_eq!(run(&["status-check", "--root", &d, "--baseline", "x"]).status.code(), Some(2));
    assert_eq!(check(&fix("no-such-dir"), None, false).status.code(), Some(2));
    // A missing or invalid baseline file.
    assert_eq!(check(&design(), Some(&fix("no-such-file.json")), false).status.code(), Some(2));
    assert_eq!(check(&design(), Some(&fix("expected.json")), false).status.code(), Some(2));
}

/// Every labelled fixture is exercised by some test above.
#[test]
fn every_fixture_is_labelled() {
    let e = expected();
    let names: BTreeSet<&String> = obj(get(&e, &["fixtures"])).keys().collect();
    assert_eq!(names.len(), 35);
    for n in names {
        assert!(fix(n).join("tokens/palettes").is_dir(), "{n}");
    }
}
