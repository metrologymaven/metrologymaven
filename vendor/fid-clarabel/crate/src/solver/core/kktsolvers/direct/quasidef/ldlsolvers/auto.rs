// Modified from Clarabel 0.11.1 by Metrology Maven;
// see vendor/fid-clarabel/patches/
#![allow(non_snake_case)]
use crate::{
    algebra::*,
    solver::core::{
        kktsolvers::direct::{
            ldlsolvers::qdldl::QDLDLDirectLDLSolver, BoxedDirectLDLSolver, DirectLDLSolverReqs,
        },
        CoreSettings,
    },
};

pub struct AutoDirectLDLSolver {}

impl DirectLDLSolverReqs for AutoDirectLDLSolver {
    fn required_matrix_shape() -> MatrixTriangle {
        MatrixTriangle::Triu
    }
}

impl AutoDirectLDLSolver {
    #[allow(clippy::new_ret_no_self)]
    pub fn new(
        KKT: &CscMatrix,
        Dsigns: &[i8],
        settings: &CoreSettings,
        _perm: Option<Vec<usize>>,
    ) -> BoxedDirectLDLSolver {
        let solver = QDLDLDirectLDLSolver::new(KKT, Dsigns, settings, None);
        Box::new(solver)
    }
}
