use super::*;
use crate::algebra::*;
use crate::solver::core::kktsolvers::LinearSolverInfo;
use crate::solver::core::{cones::CompositeCone, traits::Info, SolverStatus};
use crate::solver::traits::Variables;

/// Standard-form solver type implementing the [`Info`](crate::solver::core::traits::Info) trait
#[derive(Default, Debug, Clone)]
pub struct DefaultInfo {
    /// interior point path parameter μ
    pub mu: T,
    /// interior point path parameter reduction ratio σ
    pub sigma: T,
    /// step length for the current iteration
    pub step_length: T,
    /// number of iterations
    pub iterations: u32,
    /// primal objective value
    pub cost_primal: T,
    /// dual objective value
    pub cost_dual: T,
    /// primal residual
    pub res_primal: T,
    /// dual residual
    pub res_dual: T,
    /// primal infeasibility residual
    pub res_primal_inf: T,
    /// dual infeasibility residual
    pub res_dual_inf: T,
    /// absolute duality gap
    pub gap_abs: T,
    /// relative duality gap
    pub gap_rel: T,
    /// κ/τ ratio
    pub ktratio: T,

    // previous iterate
    /// primal object value from previous iteration
    pub(crate) prev_cost_primal: T,
    /// dual objective value from previous iteration
    pub(crate) prev_cost_dual: T,
    /// primal residual from previous iteration
    pub(crate) prev_res_primal: T,
    /// dual residual from previous iteration
    pub(crate) prev_res_dual: T,
    /// absolute duality gap from previous iteration
    pub(crate) prev_gap_abs: T,
    /// relative duality gap from previous iteration
    pub(crate) prev_gap_rel: T,
    /// solver status
    pub status: SolverStatus,

    /// linear solver information
    pub linsolver: LinearSolverInfo,
}

impl DefaultInfo {
    /// creates a new `DefaultInfo` object
    pub fn new() -> Self {
        Self::default()
    }
}

impl Info for DefaultInfo {
    type D = DefaultProblemData;
    type C = CompositeCone;
    type SE = DefaultSettings;
    type V = DefaultVariables;
    type R = DefaultResiduals;

    fn reset(&mut self) {
        self.status = SolverStatus::Unsolved;
        self.iterations = 0;
    }

    fn post_process(&mut self, residuals: &DefaultResiduals, settings: &DefaultSettings) {
        // if there was an error or we ran out of
        // iterations, check for partial convergence

        if self.status.is_errored() || matches!(self.status, SolverStatus::MaxIterations) {
            self.check_convergence_almost(residuals, settings);
        }
    }

    fn finalize(&mut self) {}

    fn update(
        &mut self,
        data: &mut DefaultProblemData,
        variables: &DefaultVariables,
        residuals: &DefaultResiduals,
    ) {
        // optimality termination check should be computed w.r.t
        // the pre-homogenization x and z variables.
        let tauinv = T::recip(variables.tau);

        // unscaled linear term norms
        let normb = data.get_normb();
        let normq = data.get_normq();

        // shortcuts for the equilibration matrices
        let d = &data.equilibration.d;
        let e = &data.equilibration.e;
        let dinv = &data.equilibration.dinv;
        let einv = &data.equilibration.einv;
        let cinv = T::recip(data.equilibration.c);

        // primal and dual costs. dot products are invariant w.r.t
        // equilibration, but we still need to back out the overall
        // objective scaling term c

        let xPx_tauinvsq_over2 = residuals.dot_xPx * tauinv * tauinv / (2.).as_T();
        self.cost_primal = (residuals.dot_qx * tauinv + xPx_tauinvsq_over2) * cinv;
        self.cost_dual = (-residuals.dot_bz * tauinv - xPx_tauinvsq_over2) * cinv;

        // variables norms, undoing the equilibration.  Do not unscale
        // by τ yet because the infeasibility residuals are ratios of
        // terms that have no affine parts anyway
        let mut normx = variables.x.norm_scaled(d);
        let mut normz = variables.z.norm_scaled(e) * cinv;
        let mut norms = variables.s.norm_scaled(einv);

        // primal and dual infeasibility residuals.
        self.res_primal_inf = (residuals.rx_inf.norm_scaled(dinv) * cinv) / tmax(T::one(), normz);
        self.res_dual_inf = tmax(
            residuals.Px.norm_scaled(dinv) / tmax(T::one(), normx),
            residuals.rz_inf.norm_scaled(einv) / tmax(T::one(), normx + norms),
        );

        // now back out the τ scaling so we can normalize the unscaled primal / dual errors
        normx *= tauinv;
        normz *= tauinv;
        norms *= tauinv;

        // primal and dual relative residuals.
        self.res_primal =
            residuals.rz.norm_scaled(einv) * tauinv / tmax(T::one(), normb + normx + norms);
        self.res_dual =
            residuals.rx.norm_scaled(dinv) * tauinv * cinv / tmax(T::one(), normq + normx + normz);

        // absolute and relative gaps
        self.gap_abs = T::abs(self.cost_primal - self.cost_dual);
        self.gap_rel = self.gap_abs
            / tmax(
                T::one(),
                tmin(T::abs(self.cost_primal), T::abs(self.cost_dual)),
            );

        // κ/τ ratio (scaled)
        self.ktratio = variables.kappa * tauinv;
    }

    fn check_termination(
        &mut self,
        residuals: &DefaultResiduals,
        settings: &DefaultSettings,
        iter: u32,
    ) -> bool {
        //  optimality or infeasibility
        // ---------------------
        self.check_convergence_full(residuals, settings);

        //  poor progress
        // ----------------------
        if self.status == SolverStatus::Unsolved
            && iter > 1u32
            && (self.res_dual > self.prev_res_dual || self.res_primal > self.prev_res_primal)
        {
            // Poor progress at high tolerance.
            if self.ktratio < T::epsilon() * (100.).as_T()
                && (self.prev_gap_abs < settings.tol_gap_abs
                    || self.prev_gap_rel < settings.tol_gap_rel)
            {
                self.status = SolverStatus::InsufficientProgress;
            }

            // Going backwards. Stop immediately if residuals diverge out of feasibility tolerance.
            #[allow(clippy::collapsible_if)] // nested if for readability
            if self.ktratio < T::one() {
                if (self.res_dual > settings.tol_feas * (100.).as_T()
                    && self.res_dual > self.prev_res_dual * (100.).as_T())
                    || (self.res_primal > settings.tol_feas * (100.).as_T()
                        && self.res_primal > self.prev_res_primal * (100.).as_T())
                {
                    self.status = SolverStatus::InsufficientProgress;
                }
            }
        }

        // iteration limit
        // ----------------------
        if self.status == SolverStatus::Unsolved && settings.max_iter == self.iterations {
            self.status = SolverStatus::MaxIterations;
        }

        // return TRUE if we settled on a final status
        self.status != SolverStatus::Unsolved
    }

    fn save_prev_iterate(&mut self, variables: &Self::V, prev_variables: &mut Self::V) {
        self.prev_cost_primal = self.cost_primal;
        self.prev_cost_dual = self.cost_dual;
        self.prev_res_primal = self.res_primal;
        self.prev_res_dual = self.res_dual;
        self.prev_gap_abs = self.gap_abs;
        self.prev_gap_rel = self.gap_rel;

        prev_variables.copy_from(variables);
    }

    fn reset_to_prev_iterate(&mut self, variables: &mut Self::V, prev_variables: &Self::V) {
        self.cost_primal = self.prev_cost_primal;
        self.cost_dual = self.prev_cost_dual;
        self.res_primal = self.prev_res_primal;
        self.res_dual = self.prev_res_dual;
        self.gap_abs = self.prev_gap_abs;
        self.gap_rel = self.prev_gap_rel;

        variables.copy_from(prev_variables);
    }

    fn save_scalars(&mut self, mu: T, alpha: T, sigma: T, iter: u32) {
        self.mu = mu;
        self.step_length = alpha;
        self.sigma = sigma;
        self.iterations = iter;
    }

    fn get_status(&self) -> SolverStatus {
        self.status
    }

    fn set_status(&mut self, status: SolverStatus) {
        self.status = status;
    }
}

// Utility functions for convergence checkiing

impl DefaultInfo {
    fn check_convergence_full(&mut self, residuals: &DefaultResiduals, settings: &DefaultSettings) {
        // "full" tolerances
        let tol_gap_abs = settings.tol_gap_abs;
        let tol_gap_rel = settings.tol_gap_rel;
        let tol_feas = settings.tol_feas;
        let tol_infeas_abs = settings.tol_infeas_abs;
        let tol_infeas_rel = settings.tol_infeas_rel;
        let tol_ktratio = settings.tol_ktratio;

        let solved_status = SolverStatus::Solved;
        let pinf_status = SolverStatus::PrimalInfeasible;
        let dinf_status = SolverStatus::DualInfeasible;

        self.check_convergence(
            residuals,
            tol_gap_abs,
            tol_gap_rel,
            tol_feas,
            tol_infeas_abs,
            tol_infeas_rel,
            tol_ktratio,
            solved_status,
            pinf_status,
            dinf_status,
        );
    }

    fn check_convergence_almost(
        &mut self,
        residuals: &DefaultResiduals,
        settings: &DefaultSettings,
    ) {
        // "almost" tolerances
        let tol_gap_abs = settings.reduced_tol_gap_abs;
        let tol_gap_rel = settings.reduced_tol_gap_rel;
        let tol_feas = settings.reduced_tol_feas;
        let tol_infeas_abs = settings.reduced_tol_infeas_abs;
        let tol_infeas_rel = settings.reduced_tol_infeas_rel;
        let tol_ktratio = settings.reduced_tol_ktratio;

        let solved_status = SolverStatus::AlmostSolved;
        let pinf_status = SolverStatus::AlmostPrimalInfeasible;
        let dinf_status = SolverStatus::AlmostDualInfeasible;

        self.check_convergence(
            residuals,
            tol_gap_abs,
            tol_gap_rel,
            tol_feas,
            tol_infeas_abs,
            tol_infeas_rel,
            tol_ktratio,
            solved_status,
            pinf_status,
            dinf_status,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn check_convergence(
        &mut self,
        residuals: &DefaultResiduals,
        tol_gap_abs: T,
        tol_gap_rel: T,
        tol_feas: T,
        tol_infeas_abs: T,
        tol_infeas_rel: T,
        tol_ktratio: T,
        solved_status: SolverStatus,
        pinf_status: SolverStatus,
        dinf_status: SolverStatus,
    ) {
        if self.ktratio <= T::one() && self.is_solved(tol_gap_abs, tol_gap_rel, tol_feas) {
            self.status = solved_status;
        //PJG hardcoded factor 1000 here should be fixed
        } else if self.ktratio > tol_ktratio.recip() * (1000.0).as_T() {
            if self.is_primal_infeasible(residuals, tol_infeas_abs, tol_infeas_rel) {
                self.status = pinf_status;
            } else if self.is_dual_infeasible(residuals, tol_infeas_abs, tol_infeas_rel) {
                self.status = dinf_status;
            }
        }
    }

    fn is_solved(&self, tol_gap_abs: T, tol_gap_rel: T, tol_feas: T) -> bool {
        ((self.gap_abs < tol_gap_abs) || (self.gap_rel < tol_gap_rel))
            && (self.res_primal < tol_feas)
            && (self.res_dual < tol_feas)
    }

    fn is_primal_infeasible(
        &self,
        residuals: &DefaultResiduals,
        tol_infeas_abs: T,
        tol_infeas_rel: T,
    ) -> bool {
        (residuals.dot_bz < -tol_infeas_abs)
            && (self.res_primal_inf < -tol_infeas_rel * residuals.dot_bz)
    }

    fn is_dual_infeasible(
        &self,
        residuals: &DefaultResiduals,
        tol_infeas_abs: T,
        tol_infeas_rel: T,
    ) -> bool {
        (residuals.dot_qx < -tol_infeas_abs)
            && (self.res_dual_inf < -tol_infeas_rel * residuals.dot_qx)
    }
}
