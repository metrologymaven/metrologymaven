//! ALN.BFIT.STOP acceptance criteria AC-1 to AC-5. Every expected value comes from the
//! fixtures `fixtures/aln/bfit/stop-traces.json` and `stop-manifest.json`, written by the
//! independent Python reference `fixtures/aln/bfit/make_stop_traces.py` (whose traces carry
//! hand-written labels), never from running this crate.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

use fid_align_solve::{
    Converged, FitStatus, StopCriteria, StopError, StopMonitor, Stopped, Verdict, DEFAULT_DIVERGENCE_WINDOW,
    DEFAULT_MAX_ITERATIONS, DEFAULT_OBJECTIVE_REL_TOL, DEFAULT_ROTATION_TOL, DEFAULT_TRANSLATION_TOL,
    STOP_CRITERIA_DECL,
};

// ------------------------------------------------------------------ a small JSON reader

#[derive(Debug, Clone, PartialEq)]
enum J {
    Null,
    Bool(bool),
    Num(String),
    Str(String),
    Arr(Vec<J>),
    Obj(Vec<(String, J)>),
}

struct P<'a> {
    b: &'a [u8],
    i: usize,
}

impl P<'_> {
    fn ws(&mut self) {
        while self.i < self.b.len() && matches!(self.b[self.i], b' ' | b'\n' | b'\r' | b'\t') {
            self.i += 1;
        }
    }

    fn eat(&mut self, c: u8) {
        self.ws();
        assert_eq!(self.b[self.i], c, "expected {} at {}", c as char, self.i);
        self.i += 1;
    }

    fn value(&mut self) -> J {
        self.ws();
        match self.b[self.i] {
            b'{' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b'}' {
                    self.i += 1;
                    return J::Obj(out);
                }
                loop {
                    self.ws();
                    let J::Str(k) = self.value() else { panic!("object key") };
                    assert!(out.iter().all(|(o, _)| *o != k), "duplicate key {k}");
                    self.eat(b':');
                    let v = self.value();
                    out.push((k, v));
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b'}' => return J::Obj(out),
                        _ => panic!("bad object at {}", self.i),
                    }
                }
            }
            b'[' => {
                self.i += 1;
                let mut out = Vec::new();
                self.ws();
                if self.b[self.i] == b']' {
                    self.i += 1;
                    return J::Arr(out);
                }
                loop {
                    out.push(self.value());
                    self.ws();
                    let c = self.b[self.i];
                    self.i += 1;
                    match c {
                        b',' => continue,
                        b']' => return J::Arr(out),
                        _ => panic!("bad array at {}", self.i),
                    }
                }
            }
            b'"' => {
                self.i += 1;
                let start = self.i;
                while self.b[self.i] != b'"' {
                    assert_ne!(self.b[self.i], b'\\', "escapes are not used by the fixtures");
                    self.i += 1;
                }
                let s = std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string();
                self.i += 1;
                J::Str(s)
            }
            b't' => {
                self.i += 4;
                J::Bool(true)
            }
            b'f' => {
                self.i += 5;
                J::Bool(false)
            }
            b'n' => {
                self.i += 4;
                J::Null
            }
            _ => {
                let start = self.i;
                while self.i < self.b.len() && matches!(self.b[self.i], b'-' | b'+' | b'.' | b'e' | b'E' | b'0'..=b'9')
                {
                    self.i += 1;
                }
                J::Num(std::str::from_utf8(&self.b[start..self.i]).unwrap().to_string())
            }
        }
    }
}

impl J {
    fn get(&self, key: &str) -> Option<&J> {
        match self {
            J::Obj(m) => m.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => panic!("not an object"),
        }
    }

    fn at(&self, key: &str) -> &J {
        self.get(key).unwrap_or_else(|| panic!("missing key {key}"))
    }

    fn arr(&self) -> &[J] {
        match self {
            J::Arr(v) => v,
            _ => panic!("not an array"),
        }
    }

    fn str(&self) -> &str {
        match self {
            J::Str(s) => s,
            _ => panic!("not a string"),
        }
    }

    /// A number, or the strings the reference uses for non-finite values.
    fn f(&self) -> f64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            J::Str(s) if s == "NaN" => f64::NAN,
            J::Str(s) if s == "Infinity" => f64::INFINITY,
            J::Str(s) if s == "-Infinity" => f64::NEG_INFINITY,
            _ => panic!("not a number: {self:?}"),
        }
    }

    fn u(&self) -> u64 {
        match self {
            J::Num(s) => s.parse().unwrap(),
            _ => panic!("not an integer"),
        }
    }
}

fn load(name: &str) -> J {
    let path = format!("{}/fixtures/aln/bfit/{name}", fid_testkit::WORKSPACE_ROOT);
    let text = std::fs::read(path).unwrap();
    let mut p = P { b: &text, i: 0 };
    let v = p.value();
    p.ws();
    assert_eq!(p.i, text.len(), "trailing bytes in {name}");
    v
}

fn settings(case: &J) -> Vec<(String, f64)> {
    case.at("settings").arr().iter().map(|s| (s.arr()[0].str().to_string(), s.arr()[1].f())).collect()
}

fn criteria(case: &J) -> Result<StopCriteria, StopError> {
    let owned = settings(case);
    let refs: Vec<(&str, f64)> = owned.iter().map(|(n, v)| (n.as_str(), *v)).collect();
    StopCriteria::from_settings(&refs)
}

fn steps(case: &J) -> Vec<(f64, f64, f64)> {
    case.at("steps").arr().iter().map(|s| (s.arr()[0].f(), s.arr()[1].f(), s.arr()[2].f())).collect()
}

fn same(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
}

// ------------------------------------------------------------------ AC-1, AC-2, AC-3

/// AC-1 (and AC-2, AC-3 through the diverging and never-converging traces): every scripted
/// trace gives the reference's verdict at the reference's iteration, `continue` before it,
/// and the reference's best pose, status, pose and history criterion.
#[test]
fn ac1_traces_match_reference() {
    let fx = load("stop-traces.json");
    let traces = fx.at("traces").arr();
    assert!(traces.len() >= 15);
    for t in traces {
        let name = t.at("name").str();
        let e = t.at("expect");
        let mut m = StopMonitor::new(criteria(t).unwrap(), t.at("start_objective").f()).unwrap();
        let want_iter = e.at("iteration").u();
        let mut last = Verdict::Continue;
        for (k, (dt, dr, f)) in steps(t).into_iter().enumerate() {
            let k = k as u64 + 1;
            last = m.update(k, dt, dr, f).unwrap();
            if k < want_iter {
                assert_eq!(last, Verdict::Continue, "{name}: iteration {k}");
                assert!(m.outcome().is_none(), "{name}: outcome before the end");
            } else {
                break;
            }
        }
        assert_eq!(last.as_str(), e.at("verdict").str(), "{name}");
        assert_eq!(m.last_iteration(), want_iter, "{name}");
        let (bi, bo) = m.best();
        assert_eq!(bi, e.at("best_iteration").u(), "{name}");
        assert!(same(bo, e.at("best_objective").f()), "{name}: best objective {bo}");
        let out = m.outcome().unwrap();
        assert_eq!(out.verdict(), last, "{name}");
        assert_eq!(out.last_iteration(), want_iter, "{name}");
        assert_eq!(out.status().as_str(), e.at("status").str(), "{name}");
        assert_eq!(out.pose_iteration(), e.at("pose_iteration").u(), "{name}");
        assert!(same(out.pose_objective(), e.at("pose_objective").f()), "{name}");
        assert_eq!(out.stopped_by().unwrap().to_string(), e.at("stopped_by").str(), "{name}");
        // A finished monitor refuses further updates (the run cannot continue past its verdict).
        assert_eq!(m.update(want_iter + 1, 0.0, 0.0, 0.0), Err(StopError::Finished), "{name}");
    }
}

/// Every terminal verdict maps to its status: converged runs are `ok`, stopped runs are
/// `not_converged`, never the other way round (REQ-004, REQ-ALN.BFIT-007).
#[test]
fn stopped_is_never_ok() {
    let fx = load("stop-traces.json");
    let mut seen = [false; 4];
    for t in fx.at("traces").arr() {
        let mut m = StopMonitor::new(criteria(t).unwrap(), t.at("start_objective").f()).unwrap();
        for (k, (dt, dr, f)) in steps(t).into_iter().enumerate() {
            if m.update(k as u64 + 1, dt, dr, f).unwrap().is_terminal() {
                break;
            }
        }
        let out = m.outcome().unwrap();
        let (i, status) = match out.verdict() {
            Verdict::Converged(Converged::Increment) => (0, FitStatus::Ok),
            Verdict::Converged(Converged::Objective) => (1, FitStatus::Ok),
            Verdict::Stopped(Stopped::MaxIterations) => (2, FitStatus::NotConverged),
            Verdict::Stopped(Stopped::Diverged) => (3, FitStatus::NotConverged),
            Verdict::Continue => panic!("non-terminal outcome"),
        };
        seen[i] = true;
        assert_eq!(out.status(), status);
        if status == FitStatus::NotConverged {
            assert_eq!(out.pose_iteration(), m.best().0, "stopped runs use the best pose");
        }
    }
    assert_eq!(seen, [true; 4], "the fixture covers every verdict");
}

/// AC-2: the objective rises on 5 iterations from the start: `stopped(diverged)` at iteration
/// 5, and the pose used is the lowest-objective one (the start pose, iteration 0). After an
/// improving phase the best pose is the improving phase's minimum.
#[test]
fn ac2_divergence_uses_best_pose() {
    let fx = load("stop-traces.json");
    let t = fx.at("traces").arr().iter().find(|t| t.at("name").str() == "diverges_from_start").unwrap();
    let mut m = StopMonitor::new(StopCriteria::default(), t.at("start_objective").f()).unwrap();
    let mut v = Verdict::Continue;
    for (k, (dt, dr, f)) in steps(t).into_iter().take(5).enumerate() {
        v = m.update(k as u64 + 1, dt, dr, f).unwrap();
    }
    assert_eq!(v, Verdict::Stopped(Stopped::Diverged));
    assert_eq!(m.last_iteration(), 5);
    let out = m.outcome().unwrap();
    assert_eq!(out.status(), FitStatus::NotConverged);
    assert_eq!(out.pose_iteration(), 0);
    assert_eq!(out.pose_objective(), 1.0);

    let t = fx.at("traces").arr().iter().find(|t| t.at("name").str() == "diverges_after_improving").unwrap();
    assert_eq!(t.at("expect").at("iteration").u(), 8);
    assert_eq!(t.at("expect").at("pose_iteration").u(), 3);
}

/// AC-3: a trace that never converges stops at iteration 100 with `stopped(max_iterations)`
/// and status `not_converged`, although the trace has more steps.
#[test]
fn ac3_never_converges() {
    let fx = load("stop-traces.json");
    let t = fx.at("traces").arr().iter().find(|t| t.at("name").str() == "oscillating_never_converges").unwrap();
    let s = steps(t);
    assert!(s.len() > 100);
    let mut m = StopMonitor::new(StopCriteria::default(), t.at("start_objective").f()).unwrap();
    let mut stopped_at = None;
    for (k, (dt, dr, f)) in s.into_iter().enumerate() {
        let v = m.update(k as u64 + 1, dt, dr, f).unwrap();
        if v.is_terminal() {
            assert_eq!(v, Verdict::Stopped(Stopped::MaxIterations));
            stopped_at = Some(k as u64 + 1);
            break;
        }
    }
    assert_eq!(stopped_at, Some(100));
    let out = m.outcome().unwrap();
    assert_eq!(out.status().as_str(), "not_converged");
    assert_eq!(out.stopped_by().unwrap().to_string(), "limit(max_iterations)");
}

// ------------------------------------------------------------------ errors

/// NaN, infinite and negative inputs fail with the reference's code, input and iteration;
/// a refused update leaves the monitor unchanged.
#[test]
fn error_traces_match_reference() {
    let fx = load("stop-traces.json");
    for t in fx.at("error_traces").arr() {
        let name = t.at("name").str();
        let e = t.at("expect");
        let want_iter = e.at("iteration").u();
        let field = e.at("field").str();
        let check = |err: StopError| {
            assert_eq!(err.code(), e.at("error").str(), "{name}");
            assert_eq!(err.field(), Some(field), "{name}");
            match err {
                StopError::NanInIteration { iteration, .. } | StopError::InvalidIterationValue { iteration, .. } => {
                    assert_eq!(iteration, want_iter, "{name}")
                }
                other => panic!("{name}: {other:?}"),
            }
        };
        let mut m = match StopMonitor::new(StopCriteria::default(), t.at("start_objective").f()) {
            Ok(m) => m,
            Err(err) => {
                assert_eq!(want_iter, 0, "{name}");
                check(err);
                continue;
            }
        };
        let mut failed = false;
        for (k, (dt, dr, f)) in steps(t).into_iter().enumerate() {
            let k = k as u64 + 1;
            let before = (m.last_iteration(), m.best(), m.verdict());
            match m.update(k, dt, dr, f) {
                Ok(v) => assert_eq!(v, Verdict::Continue, "{name}"),
                Err(err) => {
                    assert_eq!(k, want_iter, "{name}");
                    check(err);
                    assert_eq!((m.last_iteration(), m.best(), m.verdict()), before, "{name}: state changed");
                    // The run can go on with a valid value for the same iteration.
                    assert!(m.update(k, 1.0, 1.0, 1.0).is_ok(), "{name}");
                    failed = true;
                    break;
                }
            }
        }
        assert!(failed, "{name}: no error");
    }
}

/// Iterations must run 1, 2, 3, ... with no gap or repeat, so the iteration limit cannot be
/// skipped.
#[test]
fn iteration_numbers_are_sequential() {
    let mut m = StopMonitor::new(StopCriteria::default(), 1.0).unwrap();
    assert_eq!(m.update(0, 1.0, 1.0, 1.0), Err(StopError::BadIteration { expected: 1, found: 0 }));
    assert_eq!(m.update(2, 1.0, 1.0, 1.0), Err(StopError::BadIteration { expected: 1, found: 2 }));
    assert_eq!(m.update(1, 1.0, 1.0, 0.5), Ok(Verdict::Continue));
    assert_eq!(m.update(1, 1.0, 1.0, 0.5).unwrap_err().code(), "stop_bad_iteration");
    assert_eq!(m.update(100, 1.0, 1.0, 0.5).unwrap_err().code(), "stop_bad_iteration");
    assert_eq!(m.last_iteration(), 1);
}

// ------------------------------------------------------------------ AC-4: criteria

/// AC-4 and REQ-005: criteria cases give the reference's error and field, or its criteria.
#[test]
fn ac4_criteria_cases_match_reference() {
    let fx = load("stop-traces.json");
    let cases = fx.at("criteria_cases").arr();
    let ac4 = cases.iter().find(|c| c.at("name").str() == "translation_tol_negative").unwrap();
    assert_eq!(settings(ac4), vec![("translation_tol".to_string(), -1.0)]);
    for c in cases {
        let name = c.at("name").str();
        let e = c.at("expect");
        match (criteria(c), e.get("error")) {
            (Err(err), Some(code)) => {
                assert_eq!(err.code(), code.str(), "{name}");
                assert_eq!(err.field(), e.get("field").map(J::str), "{name}");
            }
            (Ok(got), None) => {
                let w = e.at("criteria");
                assert_eq!(got.max_iterations, w.at("max_iterations").u(), "{name}");
                assert!(same(got.translation_tol, w.at("translation_tol").f()), "{name}");
                assert!(same(got.rotation_tol, w.at("rotation_tol").f()), "{name}");
                assert!(same(got.objective_rel_tol, w.at("objective_rel_tol").f()), "{name}");
                assert_eq!(got.divergence_window, w.at("divergence_window").u(), "{name}");
                assert!(got.validate().is_ok());
            }
            (got, _) => panic!("{name}: {got:?}"),
        }
    }
}

/// AC-4 on the struct itself, and the monitor refuses invalid criteria.
#[test]
fn ac4_validate_names_the_field() {
    let mut c = StopCriteria { translation_tol: -1.0, ..StopCriteria::default() };
    let err = c.validate().unwrap_err();
    assert_eq!(err, StopError::InvalidStopCriteria { field: "translation_tol" });
    assert_eq!(err.to_string(), "invalid_stop_criteria: translation_tol");
    assert_eq!(StopMonitor::new(c, 1.0).unwrap_err(), err);
    c.translation_tol = -0.0;
    assert!(c.validate().is_ok());
    for (bad, field) in [
        (StopCriteria { max_iterations: 0, ..StopCriteria::default() }, "max_iterations"),
        (StopCriteria { max_iterations: 100_001, ..StopCriteria::default() }, "max_iterations"),
        (StopCriteria { rotation_tol: f64::NAN, ..StopCriteria::default() }, "rotation_tol"),
        (StopCriteria { objective_rel_tol: f64::INFINITY, ..StopCriteria::default() }, "objective_rel_tol"),
        (StopCriteria { divergence_window: 0, ..StopCriteria::default() }, "divergence_window"),
        // Declaration order: the first bad field is named.
        (StopCriteria { max_iterations: 0, translation_tol: -1.0, ..StopCriteria::default() }, "max_iterations"),
    ] {
        assert_eq!(bad.validate(), Err(StopError::InvalidStopCriteria { field }));
    }
    assert!(StopCriteria { max_iterations: 100_000, ..StopCriteria::default() }.validate().is_ok());
}

/// REQ-001: the defaults.
#[test]
fn defaults() {
    let fx = load("stop-traces.json");
    let d = fx.at("defaults");
    let c = StopCriteria::default();
    assert_eq!(c.max_iterations, d.at("max_iterations").u());
    assert!(same(c.translation_tol, d.at("translation_tol").f()));
    assert!(same(c.rotation_tol, d.at("rotation_tol").f()));
    assert!(same(c.objective_rel_tol, d.at("objective_rel_tol").f()));
    assert_eq!(c.divergence_window, d.at("divergence_window").u());
    assert_eq!(
        (DEFAULT_MAX_ITERATIONS, DEFAULT_TRANSLATION_TOL, DEFAULT_ROTATION_TOL, DEFAULT_OBJECTIVE_REL_TOL),
        (100, 1e-9, 1e-9, 1e-12)
    );
    assert_eq!(DEFAULT_DIVERGENCE_WINDOW, 5);
    assert!(c.validate().is_ok());
}

// ------------------------------------------------------------------ AC-5: manifest

/// AC-5 and REQ-006: the manifest entry of `align.run_bestfit@1` (reference fixture, checked
/// in CI by `cargo xtask convergence-check`) lists the five criteria with units and defaults,
/// equal to this crate's declaration, whose defaults are the values `StopCriteria` uses.
#[test]
fn ac5_manifest_declares_the_five_criteria() {
    let fx = load("stop-manifest.json");
    let cmds = fx.at("commands").arr();
    let cmd = cmds.iter().find(|c| c.at("id").str() == "align.run_bestfit" && c.at("major").u() == 1).unwrap();
    assert_eq!(cmd.at("iterative"), &J::Bool(true));
    let block = cmd.at("convergence").arr();
    assert_eq!(block.len(), 5);
    assert_eq!(STOP_CRITERIA_DECL.len(), 5);
    for (got, want) in STOP_CRITERIA_DECL.iter().zip(block) {
        assert_eq!(got.name, want.at("name").str());
        assert_eq!(got.unit, want.at("unit").str());
        assert_eq!(got.default, want.at("default").str());
        assert_eq!(got.meaning, want.at("meaning").str());
        // fid-xtask convergence-check: at most 120 characters, no control character.
        assert!(got.meaning.chars().count() <= 120 && !got.meaning.chars().any(char::is_control), "{}", got.name);
    }
    // The declared defaults are the defaults in force: setting each to its declared text
    // gives the default criteria.
    let parsed: Vec<(&str, f64)> = STOP_CRITERIA_DECL.iter().map(|d| (d.name, d.default.parse().unwrap())).collect();
    assert_eq!(StopCriteria::from_settings(&parsed).unwrap(), StopCriteria::default());
    let units: Vec<(&str, &str)> = STOP_CRITERIA_DECL.iter().map(|d| (d.name, d.unit)).collect();
    assert_eq!(
        units,
        [
            ("max_iterations", "1"),
            ("translation_tol", "m"),
            ("rotation_tol", "rad"),
            ("objective_rel_tol", "1"),
            ("divergence_window", "1")
        ]
    );
}
