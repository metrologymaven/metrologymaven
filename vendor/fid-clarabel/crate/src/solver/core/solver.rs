// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use self::internal::*;
use crate::algebra::*;
use crate::solver::core::cones::Cone;
use crate::solver::core::{traits::*, SettingsError};

// ---------------------------------
// Solver status type
// ---------------------------------

/// Status of solver at termination
#[repr(C)]
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub enum SolverStatus {
    /// Problem is not solved (solver hasn't run).
    Unsolved = 0,
    /// Solver terminated with a solution.
    Solved,
    /// Problem is primal infeasible.  Solution returned is a certificate of primal infeasibility.
    PrimalInfeasible,
    /// Problem is dual infeasible.  Solution returned is a certificate of dual infeasibility.
    DualInfeasible,
    /// Solver terminated with a solution (reduced accuracy)
    AlmostSolved,
    /// Problem is primal infeasible.  Solution returned is a certificate of primal infeasibility (reduced accuracy).
    AlmostPrimalInfeasible,
    /// Problem is dual infeasible.  Solution returned is a certificate of dual infeasibility (reduced accuracy).
    AlmostDualInfeasible,
    /// Iteration limit reached before solution or infeasibility certificate found.
    MaxIterations,
    /// Solver terminated with a numerical error
    NumericalError,
    /// Solver terminated due to lack of progress.
    InsufficientProgress,
}

impl Default for SolverStatus {
    fn default() -> Self {
        SolverStatus::Unsolved
    }
}

impl SolverStatus {
    pub(crate) fn is_infeasible(&self) -> bool {
        matches!(
            *self,
            |SolverStatus::PrimalInfeasible| SolverStatus::DualInfeasible
                | SolverStatus::AlmostPrimalInfeasible
                | SolverStatus::AlmostDualInfeasible
        )
    }

    pub(crate) fn is_errored(&self) -> bool {
        // status is any of the error codes
        matches!(
            *self,
            SolverStatus::NumericalError | SolverStatus::InsufficientProgress
        )
    }
}

#[repr(u32)]
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub enum StepDirection {
    Affine,
    Combined,
}

/// Scaling strategy used by the solver when
/// linearizing centrality conditions.  
#[repr(u32)]
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
pub enum ScalingStrategy {
    PrimalDual,
    Dual,
}

/// An enum for reporting strategy checkpointing
#[repr(u32)]
#[derive(PartialEq, Eq, Clone, Debug, Copy)]
enum StrategyCheckpoint {
    Update(ScalingStrategy), // Checkpoint is suggesting a new ScalingStrategy
    NoUpdate,                // Checkpoint recommends no change to ScalingStrategy
    Fail,                    // Checkpoint found a problem but no more ScalingStrategies to try
}

impl std::fmt::Display for SolverStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{self:?}")
    }
}

// ---------------------------------
// top level solver container type
// ---------------------------------

// The top-level solver.

// This trait is defined with a collection of mutually interacting associated types.
// See the [`DefaultSolver`](crate::solver::implementations::default) for an example.

pub struct Solver<D, V, R, K, C, I, SO, SE>
where
    SE: Settings,
{
    pub data: D,
    pub variables: V,
    pub residuals: R,
    pub kktsystem: K,
    pub cones: C,
    pub step_lhs: V,
    pub step_rhs: V,
    pub prev_vars: V,
    pub info: I,
    pub solution: SO,
    pub(crate) settings: SE, // not public to avoid unchecked modifications
    pub(crate) phantom: std::marker::PhantomData<T>,
}

impl<D, V, R, K, C, I, SO, SE> Solver<D, V, R, K, C, I, SO, SE>
where
    I: Info<D = D, V = V, R = R, C = C, SE = SE>,
    SE: Settings,
{
    pub fn settings(&self) -> &SE {
        &self.settings
    }

    pub fn update_settings(&mut self, settings: SE) -> Result<(), SettingsError> {
        settings.validate_as_update(&self.settings)?;
        self.settings = settings;
        Ok(())
    }
}

// ---------------------------------
// IPSolver trait and its standard implementation.
// ---------------------------------

/// An interior point solver implementing a predictor-corrector scheme
//
// Only the main solver function lives in IPSolver, since this is the
// only publicly facing trait we want to give the solver.   Additional
// internal functionality for the top level solver object is implemented
// for the IPSolverUtilities trait below, upon which IPSolver depends
pub trait IPSolver<D, V, R, K, C, I, SO, SE> {
    /// Run the solver
    fn solve(&mut self);
}

impl<D, V, R, K, C, I, SO, SE> IPSolver<D, V, R, K, C, I, SO, SE>
    for Solver<D, V, R, K, C, I, SO, SE>
where
    D: ProblemData<V = V>,
    V: Variables<D = D, R = R, C = C, SE = SE>,
    R: Residuals<D = D, V = V>,
    K: KKTSystem<D = D, V = V, C = C, SE = SE>,
    C: Cone,
    I: Info<D = D, V = V, R = R, C = C, SE = SE>,
    SO: Solution<D = D, V = V, I = I, SE = SE>,
    SE: Settings,
{
    fn solve(&mut self) {
        // various initializations
        let mut iter: u32 = 0;
        let mut sigma = T::one();
        let mut alpha = T::zero();
        let mut mu;

        self.info.reset();

        // initialize variables to some reasonable starting point
        self.default_start();

        // ----------
        // main loop
        // ----------

        let mut scaling = {
            if self.cones.allows_primal_dual_scaling() {
                ScalingStrategy::PrimalDual
            } else {
                ScalingStrategy::Dual
            }
        };

        loop {
            //update the residuals
            //--------------
            self.residuals.update(&self.variables, &self.data);

            //calculate duality gap (scaled)
            //--------------
            mu = self.variables.calc_mu(&self.residuals, &self.cones);

            // record scalar values from most recent iteration.
            // This captures μ at iteration zero.
            self.info.save_scalars(mu, alpha, sigma, iter);

            // KNP-1 (patch 0003): the fid-num facade turns a NaN, an infinity or a domain
            // error into a NaN, which reaches μ; stop with a numerical error status.
            // The "almost solved" check in post_process must not read this iterate: go
            // back to the previous one (whose info and residuals are recomputed), or, at
            // iteration 0, leave the info at its NaN reset values so no check passes.
            if !mu.is_finite() {
                if iter > 0 {
                    self.info
                        .reset_to_prev_iterate(&mut self.variables, &self.prev_vars);
                    self.residuals.update(&self.variables, &self.data);
                    self.info
                        .update(&mut self.data, &self.variables, &self.residuals);
                }
                self.info.set_status(SolverStatus::NumericalError);
                break;
            }

            // convergence check and printing
            // --------------
            self.info
                .update(&mut self.data, &self.variables, &self.residuals);

            // termination checks
            // --------------

            // internal termination checks
            let is_done = self
                .info
                .check_termination(&self.residuals, &self.settings, iter);

            // check for termination due to slow progress and update strategy
            if is_done {
                match self.strategy_checkpoint_insufficient_progress(scaling) {
                    StrategyCheckpoint::NoUpdate | StrategyCheckpoint::Fail => break,
                    StrategyCheckpoint::Update(s) => {
                        scaling = s;
                        continue;
                    }
                }
            } // allows continuation if new strategy provided

            // update the scalings
            // --------------
            let is_scaling_success = self.variables.scale_cones(&mut self.cones, mu, scaling);
            // check whether variables are interior points
            match self.strategy_checkpoint_is_scaling_success(is_scaling_success, scaling) {
                StrategyCheckpoint::Fail => break,
                // we only expect NoUpdate or Fail here (upstream: unreachable!() on Update)
                StrategyCheckpoint::NoUpdate | StrategyCheckpoint::Update(_) => {}
            }

            //increment counter here because we only count
            //iterations that produce a KKT update
            iter += 1;

            // Update the KKT system and the constant parts of its solution.
            // Keep track of the success of each step that calls KKT
            // --------------
            //PJG: This should be a Result in Rust, but needs changes down
            //into the KKT solvers to do that.
            let mut is_kkt_solve_success: bool =
                self.kktsystem
                    .update(&self.data, &self.cones, &self.settings);

            // calculate the affine step
            // --------------
            self.step_rhs
                .affine_step_rhs(&self.residuals, &self.variables, &self.cones);

            is_kkt_solve_success = is_kkt_solve_success
                && self.kktsystem.solve(
                    &mut self.step_lhs,
                    &self.step_rhs,
                    &self.data,
                    &self.variables,
                    &mut self.cones,
                    StepDirection::Affine,
                    &self.settings,
                );

            // combined step only on affine step success
            if is_kkt_solve_success {
                //calculate step length and centering parameter
                // --------------
                alpha = self.get_step_length(StepDirection::Affine, scaling);
                sigma = self.centering_parameter(alpha);

                // make a reduced Mehrotra correction in the first iteration
                // to accommodate badly centred starting points
                let m = if iter > 1 { T::one() } else { alpha };

                // calculate the combined step and length
                // --------------
                self.step_rhs.combined_step_rhs(
                    &self.residuals,
                    &self.variables,
                    &mut self.cones,
                    &mut self.step_lhs,
                    sigma,
                    mu,
                    m,
                );

                is_kkt_solve_success = self.kktsystem.solve(
                    &mut self.step_lhs,
                    &self.step_rhs,
                    &self.data,
                    &self.variables,
                    &mut self.cones,
                    StepDirection::Combined,
                    &self.settings,
                );
            }

            // check for numerical failure and update strategy
            match self.strategy_checkpoint_numerical_error(is_kkt_solve_success, scaling) {
                StrategyCheckpoint::NoUpdate => {}
                StrategyCheckpoint::Update(s) => {
                    alpha = T::zero();
                    scaling = s;
                    continue;
                }
                StrategyCheckpoint::Fail => {
                    alpha = T::zero();
                    break;
                }
            }

            // compute final step length and update the current iterate
            // --------------
            alpha = self.get_step_length(StepDirection::Combined, scaling);

            // check for undersized step and update strategy
            match self.strategy_checkpoint_small_step(alpha, scaling) {
                StrategyCheckpoint::NoUpdate => {}
                StrategyCheckpoint::Update(s) => {
                    alpha = T::zero();
                    scaling = s;
                    continue;
                }
                StrategyCheckpoint::Fail => {
                    alpha = T::zero();
                    break;
                }
            }

            // Copy previous iterate in case the next one is a dud
            self.info
                .save_prev_iterate(&self.variables, &mut self.prev_vars);

            self.variables.add_step(&self.step_lhs, alpha);
        } //end loop
          // ----------
          // ----------

        // Check we if actually took a final step.  If not, we need
        // to recapture the scalars
        if alpha.is_zero() {
            self.info.save_scalars(mu, alpha, sigma, iter);
        }

        //check for "almost" convergence case and then extract solution
        self.info.post_process(&self.residuals, &self.settings);
        self.solution
            .post_process(&self.data, &mut self.variables, &self.info, &self.settings);

        self.info.finalize();
        self.solution.finalize(&self.info);
    }
}

// Encapsulate the internal helpers trait in a private module
// so it doesn't get exported
mod internal {
    use super::*;

    pub(super) trait IPSolverInternals<D, V, R, K, C, I, SO, SE> {
        /// Find an initial condition
        fn default_start(&mut self);

        /// Compute a centering parameter
        fn centering_parameter(&self, alpha: T) -> T;

        /// Compute the current step length
        fn get_step_length(&mut self, step_direction: StepDirection, scaling: ScalingStrategy)
            -> T;

        /// backtrack a step direction to the barrier
        fn backtrack_step_to_barrier(&mut self, alphainit: T) -> T;

        /// Scaling strategy checkpointing functions
        fn strategy_checkpoint_insufficient_progress(
            &mut self,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint;

        fn strategy_checkpoint_numerical_error(
            &mut self,
            is_kkt_solve_success: bool,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint;

        fn strategy_checkpoint_small_step(
            &mut self,
            alpha: T,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint;

        fn strategy_checkpoint_is_scaling_success(
            &mut self,
            is_scaling_success: bool,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint;
    }

    impl<D, V, R, K, C, I, SO, SE> IPSolverInternals<D, V, R, K, C, I, SO, SE>
        for Solver<D, V, R, K, C, I, SO, SE>
    where
        D: ProblemData<V = V>,
        V: Variables<D = D, R = R, C = C, SE = SE>,
        R: Residuals<D = D, V = V>,
        K: KKTSystem<D = D, V = V, C = C, SE = SE>,
        C: Cone,
        I: Info<D = D, V = V, R = R, C = C, SE = SE>,
        SO: Solution<D = D, V = V, I = I>,
        SE: Settings,
    {
        fn default_start(&mut self) {
            if self.cones.is_symmetric() {
                // set all scalings to identity (or zero for the zero cone)
                self.cones.set_identity_scaling();
                // Refactor
                self.kktsystem
                    .update(&self.data, &self.cones, &self.settings);
                // solve for primal/dual initial points via KKT
                self.kktsystem
                    .solve_initial_point(&mut self.variables, &self.data, &self.settings);
                // fix up (z,s) so that they are in the cone
                self.variables.symmetric_initialization(&mut self.cones);
            } else {
                // Assigns unit (z,s) and zeros the primal variables
                self.variables.unit_initialization(&self.cones);
            }
        }

        fn centering_parameter(&self, alpha: T) -> T {
            let a = T::one() - alpha;
            a * a * a
        }

        fn get_step_length(
            &mut self,
            step_direction: StepDirection,
            scaling: ScalingStrategy,
        ) -> T {
            //step length to stay within the cones
            let mut alpha = self.variables.calc_step_length(
                &self.step_lhs,
                &mut self.cones,
                &self.settings,
                step_direction,
            );

            // additional barrier function limits for asymmetric cones
            if !self.cones.is_symmetric()
                && step_direction == StepDirection::Combined
                && scaling == ScalingStrategy::Dual
            {
                let alphainit = alpha;
                alpha = self.backtrack_step_to_barrier(alphainit);
            }
            alpha
        }

        fn backtrack_step_to_barrier(&mut self, alphainit: T) -> T {
            let step = self.settings.core().linesearch_backtrack_step;
            let mut alpha = alphainit;

            for _ in 0..50 {
                let barrier = self
                    .variables
                    .barrier(&self.step_lhs, alpha, &mut self.cones);
                if barrier < T::one() {
                    return alpha;
                } else {
                    alpha = step * alpha;
                }
            }
            alpha
        }

        fn strategy_checkpoint_insufficient_progress(
            &mut self,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint {
            let output;
            if self.info.get_status() != SolverStatus::InsufficientProgress {
                // there is no problem, so nothing to do
                output = StrategyCheckpoint::NoUpdate;
            } else {
                // recover old iterate since "insufficient progress" often
                // involves actual degradation of results
                self.info
                    .reset_to_prev_iterate(&mut self.variables, &self.prev_vars);

                // If problem is asymmetric, we can try to continue with the dual-only strategy
                if !self.cones.is_symmetric() && (scaling == ScalingStrategy::PrimalDual) {
                    self.info.set_status(SolverStatus::Unsolved);
                    output = StrategyCheckpoint::Update(ScalingStrategy::Dual);
                } else {
                    output = StrategyCheckpoint::Fail;
                }
            }
            output
        }

        fn strategy_checkpoint_numerical_error(
            &mut self,
            is_kkt_solve_success: bool,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint {
            let output;
            // No update if kkt updates successfully
            if is_kkt_solve_success {
                output = StrategyCheckpoint::NoUpdate;
            }
            // If problem is asymmetric, we can try to continue with the dual-only strategy
            else if !self.cones.is_symmetric() && (scaling == ScalingStrategy::PrimalDual) {
                output = StrategyCheckpoint::Update(ScalingStrategy::Dual);
            } else {
                // out of tricks.  Bail out with an error
                self.info.set_status(SolverStatus::NumericalError);
                output = StrategyCheckpoint::Fail;
            }
            output
        }

        fn strategy_checkpoint_small_step(
            &mut self,
            alpha: T,
            scaling: ScalingStrategy,
        ) -> StrategyCheckpoint {
            let output;

            if !self.cones.is_symmetric()
                && scaling == ScalingStrategy::PrimalDual
                && alpha < self.settings.core().min_switch_step_length
            {
                output = StrategyCheckpoint::Update(ScalingStrategy::Dual);
            } else if alpha <= tmax(T::zero(), self.settings.core().min_terminate_step_length) {
                self.info.set_status(SolverStatus::InsufficientProgress);
                output = StrategyCheckpoint::Fail;
            } else {
                output = StrategyCheckpoint::NoUpdate;
            }

            output
        }

        fn strategy_checkpoint_is_scaling_success(
            &mut self,
            is_scaling_success: bool,
            _scaling: ScalingStrategy,
        ) -> StrategyCheckpoint {
            if is_scaling_success {
                StrategyCheckpoint::NoUpdate
            } else {
                self.info.set_status(SolverStatus::NumericalError);
                StrategyCheckpoint::Fail
            }
        }
    } // end trait impl
} //end internals module
