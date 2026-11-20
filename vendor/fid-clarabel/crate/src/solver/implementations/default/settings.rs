// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use crate::solver::core::traits::Settings;
use crate::{algebra::*, solver::core::SettingsError};

/// Standard-form solver type implementing the [`Settings`](crate::solver::core::traits::Settings) trait
#[derive(Debug, Clone)]
pub struct DefaultSettings {
    ///maximum number of iterations
    pub max_iter: u32,

    ///maximum interior point step length
    pub max_step_fraction: T,

    ///absolute duality gap tolerance
    pub tol_gap_abs: T,

    ///relative duality gap tolerance
    pub tol_gap_rel: T,

    ///feasibility check tolerance (primal and dual)
    pub tol_feas: T,

    ///absolute infeasibility tolerance (primal and dual)
    pub tol_infeas_abs: T,

    ///relative infeasibility tolerance (primal and dual)
    pub tol_infeas_rel: T,

    ///κ/τ tolerance
    pub tol_ktratio: T,

    ///reduced absolute duality gap tolerance
    // NB: reduced_tol_infeas_abs is *smaller* when relaxed, since
    // we are checking that we are this far into the interior of
    // an inequality when checking.   Smaller for this value means
    // "less margin required"
    pub reduced_tol_gap_abs: T,

    ///reduced relative duality gap tolerance
    pub reduced_tol_gap_rel: T,

    ///reduced feasibility check tolerance (primal and dual)
    pub reduced_tol_feas: T,

    ///reduced absolute infeasibility tolerance (primal and dual)
    pub reduced_tol_infeas_abs: T,

    ///reduced relative infeasibility tolerance (primal and dual)
    pub reduced_tol_infeas_rel: T,

    ///reduced κ/τ tolerance
    pub reduced_tol_ktratio: T,

    ///enable data equilibration pre-scaling
    pub equilibrate_enable: bool,

    /// maximum equilibration scaling iterations
    pub equilibrate_max_iter: u32,

    ///minimum equilibration scaling allowed
    pub equilibrate_min_scaling: T,

    ///maximum equilibration scaling allowed
    pub equilibrate_max_scaling: T,

    ///line search backtracking
    pub linesearch_backtrack_step: T,

    ///minimum step size allowed for asymmetric cones with PrimalDual scaling
    pub min_switch_step_length: T,

    ///minimum step size allowed for symmetric cones & asymmetric cones with Dual scaling
    pub min_terminate_step_length: T,

    ///use a direct linear solver method (required true)
    pub direct_kkt_solver: bool,

    ///direct linear solver method ("qdldl" or "auto")
    pub direct_solve_method: String,

    ///enable KKT static regularization
    pub static_regularization_enable: bool,

    ///KKT static regularization parameter
    pub static_regularization_constant: T,

    ///additional regularization parameter w.r.t. the maximum abs diagonal term
    pub static_regularization_proportional: T,

    ///enable KKT dynamic regularization
    pub dynamic_regularization_enable: bool,

    ///KKT dynamic regularization threshold
    pub dynamic_regularization_eps: T,

    ///KKT dynamic regularization shift
    pub dynamic_regularization_delta: T,

    ///KKT direct solve with iterative refinement
    pub iterative_refinement_enable: bool,

    ///iterative refinement relative tolerance
    pub iterative_refinement_reltol: T,

    ///iterative refinement absolute tolerance
    pub iterative_refinement_abstol: T,

    ///iterative refinement maximum iterations
    pub iterative_refinement_max_iter: u32,

    ///iterative refinement stalling tolerance
    pub iterative_refinement_stop_ratio: T,

    ///enable presolve constraint reduction
    pub presolve_enable: bool,

    ///explicitly drop structural zeros from sparse data inputs
    ///Caution: this will disable parametric updating functionality
    ///See also ['dropzeros'][crate::algebra::CscMatrix::dropzeros]
    ///for dropping structural zeros before passing to the solver
    ///
    pub input_sparse_dropzeros: bool,
}

impl Default for DefaultSettings {
    fn default() -> DefaultSettings {
        // (KNP-1, patch 0007: hand-written defaults; upstream used derive_builder)
        DefaultSettings {
            max_iter: 200,
            max_step_fraction: (0.99).as_T(),
            tol_gap_abs: (1e-8).as_T(),
            tol_gap_rel: (1e-8).as_T(),
            tol_feas: (1e-8).as_T(),
            tol_infeas_abs: (1e-8).as_T(),
            tol_infeas_rel: (1e-8).as_T(),
            tol_ktratio: (1e-6).as_T(),
            reduced_tol_gap_abs: (5e-5).as_T(),
            reduced_tol_gap_rel: (5e-5).as_T(),
            reduced_tol_feas: (1e-4).as_T(),
            reduced_tol_infeas_abs: (5e-12).as_T(),
            reduced_tol_infeas_rel: (5e-5).as_T(),
            reduced_tol_ktratio: (1e-4).as_T(),
            equilibrate_enable: true,
            equilibrate_max_iter: 10,
            equilibrate_min_scaling: (1e-4).as_T(),
            equilibrate_max_scaling: (1e+4).as_T(),
            linesearch_backtrack_step: (0.8).as_T(),
            min_switch_step_length: (1e-1).as_T(),
            min_terminate_step_length: (1e-4).as_T(),
            direct_kkt_solver: true,
            direct_solve_method: "auto".to_string(),
            static_regularization_enable: true,
            static_regularization_constant: (1e-8).as_T(),
            static_regularization_proportional: T::epsilon() * T::epsilon(),
            dynamic_regularization_enable: true,
            dynamic_regularization_eps: (1e-13).as_T(),
            dynamic_regularization_delta: (2e-7).as_T(),
            iterative_refinement_enable: true,
            iterative_refinement_reltol: (1e-13).as_T(),
            iterative_refinement_abstol: (1e-12).as_T(),
            iterative_refinement_max_iter: 10,
            iterative_refinement_stop_ratio: (5.0).as_T(),
            presolve_enable: true,
            input_sparse_dropzeros: false,
        }
    }
}

impl Settings for DefaultSettings {
    //NB: CoreSettings is typedef'd to DefaultSettings
    fn core(&self) -> &DefaultSettings {
        self
    }
    fn core_mut(&mut self) -> &mut DefaultSettings {
        self
    }

    /// Checks that the settings are valid.  This only ensures that fields specified
    /// by strings contain valid options.   It does not sanity check numerical values
    fn validate(&self) -> Result<(), SettingsError> {
        // this direct check avoids an internal panic since indirect
        // solvers are not yet available at all
        if !self.direct_kkt_solver {
            return Err(SettingsError::BadFieldValue("direct_kkt_solver"));
        }

        //check that the choice of LDL solver (string) is valid
        validate_direct_solve_method(&self.direct_solve_method)?;

        Ok(())
    }

    /// check that a settings object is valid as an updated collection
    /// of settings for a solver that has already been initialized.   This
    /// should reject changed to parameters that are only applicable during
    /// solver initialization.  Calls `validate()` internally to check
    /// that values are also legal.
    fn validate_as_update(&self, prev: &Self) -> Result<(), SettingsError> {
        self.validate()?;

        if self.equilibrate_enable != prev.equilibrate_enable {
            return Err(SettingsError::ImmutableSetting("equilibrate_enable"));
        }
        if self.equilibrate_max_iter != prev.equilibrate_max_iter {
            return Err(SettingsError::ImmutableSetting("equilibrate_max_iter"));
        }
        if self.equilibrate_min_scaling != prev.equilibrate_min_scaling {
            return Err(SettingsError::ImmutableSetting("equilibrate_min_scaling"));
        }
        if self.equilibrate_max_scaling != prev.equilibrate_max_scaling {
            return Err(SettingsError::ImmutableSetting("equilibrate_max_scaling"));
        }
        if self.direct_kkt_solver != prev.direct_kkt_solver {
            return Err(SettingsError::ImmutableSetting("direct_kkt_solver"));
        }
        if self.direct_solve_method != prev.direct_solve_method {
            return Err(SettingsError::ImmutableSetting("direct_solve_method"));
        }
        if self.presolve_enable != prev.presolve_enable {
            return Err(SettingsError::ImmutableSetting("presolve_enable"));
        }
        if self.input_sparse_dropzeros != prev.input_sparse_dropzeros {
            return Err(SettingsError::ImmutableSetting("input_sparse_dropzeros"));
        }

        Ok(())
    }
}

// ---------------------------------------------------------
// individual validation functions go here
// ---------------------------------------------------------

fn validate_direct_solve_method(direct_solve_method: &str) -> Result<(), SettingsError> {
    match direct_solve_method {
        "auto" => Ok(()),
        "qdldl" => Ok(()),
        _ => Err(SettingsError::BadFieldValue("direct_solve_method")),
    }
}
