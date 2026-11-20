// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
use super::*;
use crate::solver::core::{
    cones::{CompositeCone, Cone},
    kktsolvers::{direct::*, *},
    traits::{KKTSystem, Settings},
    StepDirection,
};

use crate::algebra::*;

// We require Send/Sync here to allow pyo3 builds to share
// solver objects between threads.

type BoxedKKTSolver = Box<dyn KKTSolver + Send + Sync>;

/// Standard-form solver type implementing the [`KKTSystem`](crate::solver::core::traits::KKTSystem) trait
pub struct DefaultKKTSystem {
    kktsolver: BoxedKKTSolver,

    // solution vector for constant part of KKT solves
    x1: Vec<T>,
    z1: Vec<T>,

    // solution vector for general KKT solves
    x2: Vec<T>,
    z2: Vec<T>,

    // work vectors for assembling/dissambling vectors
    workx: Vec<T>,
    workz: Vec<T>,
    work_conic: Vec<T>,
}

impl DefaultKKTSystem {
    /// Create a new KKT system solver
    pub fn new(
        data: &DefaultProblemData,
        cones: &CompositeCone,
        settings: &DefaultSettings,
    ) -> Self {
        let (m, n) = (data.m, data.n);

        //only direct / LDL based KKT solvers exist; settings validation rejects
        //direct_kkt_solver = false (upstream panicked here)
        let kktsolver = Box::new(DirectLDLKKTSolver::new(
            &data.P,
            &data.A,
            cones,
            m,
            n,
            settings.core(),
        ));

        //the LHS constant part of the reduced solve
        let x1 = vec![T::zero(); n];
        let z1 = vec![T::zero(); m];

        //the LHS for other solves
        let x2 = vec![T::zero(); n];
        let z2 = vec![T::zero(); m];

        //workspace compatible with (x,z)
        let workx = vec![T::zero(); n];
        let workz = vec![T::zero(); m];

        //additional conic workspace vector compatible with s and z
        let work_conic = vec![T::zero(); m];

        Self {
            kktsolver,
            x1,
            z1,
            x2,
            z2,
            workx,
            workz,
            work_conic,
        }
    }
}

impl HasLinearSolverInfo for DefaultKKTSystem {
    fn linear_solver_info(&self) -> LinearSolverInfo {
        self.kktsolver.linear_solver_info()
    }
}

impl KKTSystem for DefaultKKTSystem {
    type D = DefaultProblemData;
    type V = DefaultVariables;
    type C = CompositeCone;
    type SE = DefaultSettings;

    fn update(
        &mut self,
        data: &DefaultProblemData,
        cones: &CompositeCone,
        settings: &DefaultSettings,
    ) -> bool {
        // update the linear solver with new cones
        let is_success = self.kktsolver.update(cones, settings.core());

        if !is_success {
            return is_success;
        }

        // calculate KKT solution for constant terms
        self.solve_constant_rhs(data, settings.core())

        //PJG is_success should be a Result in rust
    }

    fn solve(
        &mut self,
        lhs: &mut DefaultVariables,
        rhs: &DefaultVariables,
        data: &DefaultProblemData,
        variables: &DefaultVariables,
        cones: &mut CompositeCone,
        step_direction: StepDirection,
        settings: &DefaultSettings,
    ) -> bool {
        let (x1, z1) = (&mut self.x1, &mut self.z1);
        let (x2, z2) = (&self.x2, &self.z2); //from constant solve, so not mut
        let (workx, workz) = (&mut self.workx, &mut self.workz);

        // solve for (x1,z1)
        // -----------
        workx.copy_from(&rhs.x);

        // compute the vector c in the step equation HₛΔz + Δs = -c,
        // with shortcut in affine case
        let Ds_const_term = &mut self.work_conic;

        match step_direction {
            StepDirection::Affine => {
                Ds_const_term.copy_from(&variables.s);
            }
            StepDirection::Combined => {
                cones.Ds_from_Dz_offset(Ds_const_term, &rhs.s, &mut lhs.z, &variables.z);
            }
        }

        workz.waxpby(T::one(), Ds_const_term, -T::one(), &rhs.z);

        // ---------------------------------------------------
        // this solves the variable part of reduced KKT system
        self.kktsolver.setrhs(workx, workz);
        let is_success = self.kktsolver.solve(Some(x1), Some(z1), settings.core());
        if !is_success {
            return false;
        }

        // solve for Δτ.
        // -----------
        // Numerator first
        let xi = workx;
        xi.axpby(T::recip(variables.tau), &variables.x, T::zero());

        let two: T = (2.).as_T();
        let tau_num = rhs.tau - rhs.kappa / variables.tau
            + data.q.dot(x1)
            + data.b.dot(z1)
            + two * data.P.sym_up().quad_form(xi, x1);

        // offset ξ for the quadratic form in the denominator
        let xi_minus_x2 = xi; //alias to ξ, same as workx
        xi_minus_x2.axpby(-T::one(), x2, T::one());

        let mut tau_den = variables.kappa / variables.tau - data.q.dot(x2) - data.b.dot(z2);
        tau_den +=
            data.P.sym_up().quad_form(xi_minus_x2, xi_minus_x2) - data.P.sym_up().quad_form(x2, x2);

        // solve for (Δx,Δz)
        // -----------
        lhs.tau = tau_num / tau_den;
        lhs.x.waxpby(T::one(), x1, lhs.tau, x2);
        lhs.z.waxpby(T::one(), z1, lhs.tau, z2);

        // solve for Δs
        // -------------
        //  compute the linear term HₛΔz, where Hs = WᵀW for symmetric
        //  cones and Hs = μH(z) for asymmetric cones
        cones.mul_Hs(&mut lhs.s, &lhs.z, workz);
        lhs.s.axpby(-T::one(), Ds_const_term, -T::one()); // lhs.s = -(lhs.s+Δs_const_term);

        // solve for Δκ
        // --------------
        lhs.kappa = -(rhs.kappa + variables.kappa * lhs.tau) / variables.tau;

        // we don't check the validity of anything
        // after the KKT solve, so just return is_success
        // without further validation
        is_success
    }

    fn solve_initial_point(
        &mut self,
        variables: &mut DefaultVariables,
        data: &DefaultProblemData,
        settings: &DefaultSettings,
    ) -> bool {
        let mut is_success;

        if data.P.nnz() == 0 {
            // LP initialization
            // solve with [0;b] as a RHS to get (x,-s) initializers
            // zero out any sparse cone variables at end
            self.workx.fill(T::zero());
            self.workz.copy_from(&data.b);
            self.kktsolver.setrhs(&self.workx, &self.workz);
            is_success = self.kktsolver.solve(
                Some(&mut variables.x),
                Some(&mut variables.s),
                settings.core(),
            );
            variables.s.negate();

            if !is_success {
                return is_success;
            }

            // solve with [-q;0] as a RHS to get z initializer
            // zero out any sparse cone variables at end
            self.workx.axpby(-T::one(), &data.q, T::zero());
            self.workz.fill(T::zero());

            self.kktsolver.setrhs(&self.workx, &self.workz);
            is_success = self
                .kktsolver
                .solve(None, Some(&mut variables.z), settings.core());
        } else {
            //QP initialization
            self.workx.scalarop_from(|q| -q, &data.q);
            self.workz.copy_from(&data.b);
            self.kktsolver.setrhs(&self.workx, &self.workz);
            is_success = self.kktsolver.solve(
                Some(&mut variables.x),
                Some(&mut variables.z),
                settings.core(),
            );
            variables.s.scalarop_from(|z| -z, &variables.z);
        }
        is_success
    }
}

impl DefaultKKTSystem {
    fn solve_constant_rhs(
        &mut self,
        data: &DefaultProblemData,
        settings: &DefaultSettings,
    ) -> bool {
        self.workx.axpby(-T::one(), &data.q, T::zero()); //workx .= -q
        self.kktsolver.setrhs(&self.workx, &data.b);
        let is_success =
            self.kktsolver
                .solve(Some(&mut self.x2), Some(&mut self.z2), settings.core());

        is_success
    }

    pub(crate) fn update_P(&mut self, P: &CscMatrix) {
        self.kktsolver.update_P(P);
    }

    pub(crate) fn update_A(&mut self, A: &CscMatrix) {
        self.kktsolver.update_A(A);
    }
}
