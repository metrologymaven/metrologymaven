//! Iteration limits and tolerances fixed in code (REQ-KRN.SOLV.CONIC.WRAP-003) and their
//! descriptor for the kernel manifest (REQ-KRN.SOLV.CONIC.WRAP-004).

use fid_clarabel::qdldl::{MAX_ORDERING_DIM, MAX_ORDERING_FILL, MAX_ORDERING_NNZ};
use fid_clarabel::solver::DefaultSettings;

/// Name of the limits set in [`limits_descriptor`].
pub const LIMITS_NAME: &str = "fid-solv.conic.limits";

/// The solver limits the wrapper runs with. The values are compiled in ([`ConicLimits::FIXED`]);
/// no entry point takes a `ConicLimits`, so a caller cannot change them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConicLimits {
    /// Version of this limits set; bumped whenever any value of the descriptor changes.
    pub version: u16,
    /// Most interior-point iterations.
    pub max_iter: u32,
    /// Primal and dual feasibility tolerance.
    pub tol_feas: f64,
    /// Absolute duality gap tolerance.
    pub tol_gap_abs: f64,
    /// Relative duality gap tolerance.
    pub tol_gap_rel: f64,
    /// Infeasibility certificate tolerance (absolute and relative).
    pub tol_infeas: f64,
    /// Static regularisation added to the KKT diagonal.
    pub static_reg: f64,
    /// Most iterative refinement steps per KKT solve.
    pub refine_steps: u8,
}

impl ConicLimits {
    /// The limits compiled into this crate. Placeholders until KRN.SOLV.CONIC.XTEST and the
    /// minimum-zone problems of KRN.MZ are run [unverified].
    pub const FIXED: ConicLimits = ConicLimits {
        version: 1,
        max_iter: 200,
        tol_feas: 1e-10,
        tol_gap_abs: 1e-10,
        tol_gap_rel: 1e-10,
        tol_infeas: 1e-10,
        static_reg: 1e-8,
        refine_steps: 10,
    };
}

/// The complete solver settings for `limits`. Every field is written out (no
/// `..Default::default()`), so a field added to the vendored settings fails to compile here
/// instead of silently taking an unrecorded default. Fields that are not part of
/// [`ConicLimits`] are fixed at the values below and listed in the descriptor too.
pub(crate) fn settings(limits: &ConicLimits) -> DefaultSettings {
    DefaultSettings {
        max_iter: limits.max_iter,
        max_step_fraction: 0.99,
        tol_gap_abs: limits.tol_gap_abs,
        tol_gap_rel: limits.tol_gap_rel,
        tol_feas: limits.tol_feas,
        tol_infeas_abs: limits.tol_infeas,
        tol_infeas_rel: limits.tol_infeas,
        tol_ktratio: 1e-6,
        reduced_tol_gap_abs: 5e-5,
        reduced_tol_gap_rel: 5e-5,
        reduced_tol_feas: 1e-4,
        reduced_tol_infeas_abs: 5e-12,
        reduced_tol_infeas_rel: 5e-5,
        reduced_tol_ktratio: 1e-4,
        equilibrate_enable: true,
        equilibrate_max_iter: 10,
        equilibrate_min_scaling: 1e-4,
        equilibrate_max_scaling: 1e4,
        linesearch_backtrack_step: 0.8,
        min_switch_step_length: 1e-1,
        min_terminate_step_length: 1e-4,
        direct_kkt_solver: true,
        direct_solve_method: String::from("qdldl"),
        static_regularization_enable: true,
        static_regularization_constant: limits.static_reg,
        // 2^-104 (f64::EPSILON squared, exact)
        static_regularization_proportional: f64::EPSILON * f64::EPSILON,
        dynamic_regularization_enable: true,
        dynamic_regularization_eps: 1e-13,
        dynamic_regularization_delta: 2e-7,
        iterative_refinement_enable: true,
        iterative_refinement_reltol: 1e-13,
        iterative_refinement_abstol: 1e-12,
        iterative_refinement_max_iter: u32::from(limits.refine_steps),
        iterative_refinement_stop_ratio: 5.0,
        // The builder rejects non-finite b, so presolve (which drops infinite bounds) has
        // nothing to do; it is off so the solver sees exactly the canonical problem.
        presolve_enable: false,
        input_sparse_dropzeros: false,
    }
}

/// One constant of the descriptor.
#[derive(Debug, Clone, PartialEq)]
pub enum LimitValue {
    /// A float, as its IEEE-754 bit pattern (16 lowercase hex digits).
    F64(f64),
    /// An unsigned integer.
    Uint(u64),
    /// A flag.
    Bool(bool),
    /// A fixed string.
    Str(&'static str),
}

/// The limits as data for the kernel manifest (KRN.MAN.MANI).
#[derive(Debug, Clone, PartialEq)]
pub struct LimitsDescriptor {
    /// [`LIMITS_NAME`].
    pub name: &'static str,
    /// [`ConicLimits::version`].
    pub version: u16,
    /// Every constant, sorted by name (byte order).
    pub constants: Vec<(&'static str, LimitValue)>,
}

impl LimitsDescriptor {
    /// RFC 8785 canonical JSON: `{"constants":{...},"name":...,"version":...}`, keys sorted,
    /// no whitespace, floats as hex strings of their bits. The output is a pure function of
    /// the compiled-in constants, so two builds give identical bytes.
    pub fn canonical_json(&self) -> String {
        let mut out = String::from("{\"constants\":{");
        for (i, (k, v)) in self.constants.iter().enumerate() {
            if i > 0 {
                out.push(',');
            }
            push_json_str(&mut out, k);
            out.push(':');
            match v {
                LimitValue::F64(x) => push_json_str(&mut out, &format!("{:016x}", x.to_bits())),
                LimitValue::Uint(u) => out.push_str(&u.to_string()),
                LimitValue::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
                LimitValue::Str(s) => push_json_str(&mut out, s),
            }
        }
        out.push_str("},\"name\":");
        push_json_str(&mut out, self.name);
        out.push_str(",\"version\":");
        out.push_str(&self.version.to_string());
        out.push('}');
        out
    }
}

fn push_json_str(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            c if u32::from(c) < 0x20 => out.push_str(&format!("\\u{:04x}", u32::from(c))),
            c => out.push(c),
        }
    }
    out.push('"');
}

/// The compiled-in limits and every other fixed solver setting, including the LDL ordering
/// size budget of the vendored solver (REQ-KRN.SOLV.CONIC.WRAP-004).
pub fn limits_descriptor() -> LimitsDescriptor {
    descriptor_of(&ConicLimits::FIXED)
}

pub(crate) fn descriptor_of(limits: &ConicLimits) -> LimitsDescriptor {
    // Destructured without `..`: a new settings field fails to compile until listed here.
    let DefaultSettings {
        max_iter,
        max_step_fraction,
        tol_gap_abs,
        tol_gap_rel,
        tol_feas,
        tol_infeas_abs,
        tol_infeas_rel,
        tol_ktratio,
        reduced_tol_gap_abs,
        reduced_tol_gap_rel,
        reduced_tol_feas,
        reduced_tol_infeas_abs,
        reduced_tol_infeas_rel,
        reduced_tol_ktratio,
        equilibrate_enable,
        equilibrate_max_iter,
        equilibrate_min_scaling,
        equilibrate_max_scaling,
        linesearch_backtrack_step,
        min_switch_step_length,
        min_terminate_step_length,
        direct_kkt_solver,
        direct_solve_method: _,
        static_regularization_enable,
        static_regularization_constant,
        static_regularization_proportional,
        dynamic_regularization_enable,
        dynamic_regularization_eps,
        dynamic_regularization_delta,
        iterative_refinement_enable,
        iterative_refinement_reltol,
        iterative_refinement_abstol,
        iterative_refinement_max_iter,
        iterative_refinement_stop_ratio,
        presolve_enable,
        input_sparse_dropzeros,
    } = settings(limits);
    use LimitValue::{Bool, Str, Uint, F64};
    let mut constants = vec![
        ("max_iter", Uint(u64::from(max_iter))),
        ("max_step_fraction", F64(max_step_fraction)),
        ("tol_gap_abs", F64(tol_gap_abs)),
        ("tol_gap_rel", F64(tol_gap_rel)),
        ("tol_feas", F64(tol_feas)),
        ("tol_infeas_abs", F64(tol_infeas_abs)),
        ("tol_infeas_rel", F64(tol_infeas_rel)),
        ("tol_ktratio", F64(tol_ktratio)),
        ("reduced_tol_gap_abs", F64(reduced_tol_gap_abs)),
        ("reduced_tol_gap_rel", F64(reduced_tol_gap_rel)),
        ("reduced_tol_feas", F64(reduced_tol_feas)),
        ("reduced_tol_infeas_abs", F64(reduced_tol_infeas_abs)),
        ("reduced_tol_infeas_rel", F64(reduced_tol_infeas_rel)),
        ("reduced_tol_ktratio", F64(reduced_tol_ktratio)),
        ("equilibrate_enable", Bool(equilibrate_enable)),
        ("equilibrate_max_iter", Uint(u64::from(equilibrate_max_iter))),
        ("equilibrate_min_scaling", F64(equilibrate_min_scaling)),
        ("equilibrate_max_scaling", F64(equilibrate_max_scaling)),
        ("linesearch_backtrack_step", F64(linesearch_backtrack_step)),
        ("min_switch_step_length", F64(min_switch_step_length)),
        ("min_terminate_step_length", F64(min_terminate_step_length)),
        ("direct_kkt_solver", Bool(direct_kkt_solver)),
        // `settings` always sets "qdldl"; the string is not Copy, so it is named here.
        ("direct_solve_method", Str("qdldl")),
        ("static_regularization_enable", Bool(static_regularization_enable)),
        ("static_regularization_constant", F64(static_regularization_constant)),
        ("static_regularization_proportional", F64(static_regularization_proportional)),
        ("dynamic_regularization_enable", Bool(dynamic_regularization_enable)),
        ("dynamic_regularization_eps", F64(dynamic_regularization_eps)),
        ("dynamic_regularization_delta", F64(dynamic_regularization_delta)),
        ("iterative_refinement_enable", Bool(iterative_refinement_enable)),
        ("iterative_refinement_reltol", F64(iterative_refinement_reltol)),
        ("iterative_refinement_abstol", F64(iterative_refinement_abstol)),
        ("iterative_refinement_max_iter", Uint(u64::from(iterative_refinement_max_iter))),
        ("iterative_refinement_stop_ratio", F64(iterative_refinement_stop_ratio)),
        ("presolve_enable", Bool(presolve_enable)),
        ("input_sparse_dropzeros", Bool(input_sparse_dropzeros)),
        ("ordering_max_dim", Uint(MAX_ORDERING_DIM as u64)),
        ("ordering_max_nnz", Uint(MAX_ORDERING_NNZ as u64)),
        ("ordering_max_fill", Uint(MAX_ORDERING_FILL as u64)),
    ];
    constants.sort_by(|x, y| x.0.as_bytes().cmp(y.0.as_bytes()));
    LimitsDescriptor { name: LIMITS_NAME, version: limits.version, constants }
}
