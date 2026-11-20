// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
#![allow(non_snake_case)]
use crate::algebra::*;
use crate::qdldl::*;
use crate::solver::core::kktsolvers::direct::{DirectLDLSolver, DirectLDLSolverReqs};
use crate::solver::core::kktsolvers::HasLinearSolverInfo;
use crate::solver::core::kktsolvers::LinearSolverInfo;
use crate::solver::core::CoreSettings;

pub struct QDLDLDirectLDLSolver {
    //KKT matrix and its QDLDL factorization; `None` if the logical factorization
    //failed (upstream panicked), in which case every refactor reports failure
    factors: Option<QDLDLFactorisation>,
}

impl QDLDLDirectLDLSolver {
    pub fn new(
        KKT: &CscMatrix,
        Dsigns: &[i8],
        settings: &CoreSettings,
        perm: Option<Vec<usize>>,
    ) -> Self {
        // (upstream tuned AMD's dense-row threshold here; patch 0006 replaces AMD with the
        // pattern-only minimum-degree ordering, which has no such parameter)

        //make a logical factorization to determine memory allocations

        let opts = QDLDLSettings {
            logical: true, //allocate memory only on init
            Dsigns: Some(Dsigns.to_vec()),
            regularize_enable: true,
            regularize_eps: settings.dynamic_regularization_eps,
            regularize_delta: settings.dynamic_regularization_delta,
            perm,
        };

        let factors = QDLDLFactorisation::new(KKT, Some(opts)).ok();

        Self { factors }
    }
}

impl DirectLDLSolverReqs for QDLDLDirectLDLSolver {
    fn required_matrix_shape() -> MatrixTriangle {
        MatrixTriangle::Triu
    }
}

impl HasLinearSolverInfo for QDLDLDirectLDLSolver {
    fn linear_solver_info(&self) -> LinearSolverInfo {
        LinearSolverInfo {
            name: "qdldl".to_string(),
            threads: 1,
            direct: true,
            nnzA: self.factors.as_ref().map_or(0, |f| f.nnzA()),
            nnzL: self.factors.as_ref().map_or(0, |f| f.nnzL()),
        }
    }
}

impl DirectLDLSolver for QDLDLDirectLDLSolver {
    fn update_values(&mut self, index: &[usize], values: &[T]) {
        //Update values that are stored within
        //the reordered copy held internally by QDLDL.
        if let Some(f) = self.factors.as_mut() {
            f.update_values(index, values);
        }
    }

    fn scale_values(&mut self, index: &[usize], scale: T) {
        if let Some(f) = self.factors.as_mut() {
            f.scale_values(index, scale);
        }
    }

    fn offset_values(&mut self, index: &[usize], offset: T, signs: &[i8]) {
        if let Some(f) = self.factors.as_mut() {
            f.offset_values(index, offset, signs);
        }
    }

    fn solve(&mut self, _kkt: &CscMatrix, x: &mut [T], b: &mut [T]) {
        // NB: QDLDL solves in place
        x.copy_from(b);
        match self.factors.as_mut() {
            Some(f) => f.solve(x),
            None => x.fill(T::NAN), // fails the solver's finiteness check
        }
    }

    fn refactor(&mut self, _kkt: &CscMatrix) -> bool {
        //QDLDL has maintained its own version of the permuted
        //KKT matrix through custom update/scale/offset methods,
        //so we ignore the KKT matrix provided by the caller
        match self.factors.as_mut() {
            Some(f) => f.refactor().is_ok() && f.Dinv.is_finite(),
            None => false,
        }
    }
}
